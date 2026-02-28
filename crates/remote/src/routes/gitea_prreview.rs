use std::{fs::File, io, path::Path};

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use chrono::{DateTime, Duration, Utc};
use flate2::{Compression, write::GzEncoder};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use tar::Builder;
use tokio::process::Command;
use tracing::{error, info, warn};
use url::Url;
use uuid::Uuid;

use crate::{
    AppState,
    config::GiteaPrReviewConfig,
    db::{
        gitea_prreview::GiteaPrReviewRepository,
        reviews::{CreateWebhookReviewParams, ReviewRepository},
    },
    gitea_prreview::verify_webhook_signature,
};

pub fn public_router() -> Router<AppState> {
    Router::new().route("/gitea/prreview", post(handle_gitea_prreview))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct WebhookAck {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Debug)]
struct ValidatedWebhook {
    delivery_id: String,
    event_type: String,
    payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct DeliveryRecord<'a> {
    delivery_id: &'a str,
    event_type: &'a str,
    received_at: String,
    payload: &'a serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JobStateSnapshot {
    job_id: String,
    status: String,
    delivery_id: String,
    event_type: String,
    repo_data_path: String,
    payload_path: String,
    cleanup_after: String,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct PrContext {
    owner: String,
    repo: String,
    clone_url: String,
    pr_number: u64,
    pr_url: String,
    pr_api_url: Option<String>,
    title: Option<String>,
    body: Option<String>,
    head_sha: Option<String>,
    base_ref: Option<String>,
}

pub async fn handle_gitea_prreview(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(config) = state.config.gitea_prreview.as_ref() else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(WebhookAck {
                status: "disabled",
                job_id: None,
                reason: Some("gitea_prreview_not_configured"),
            }),
        );
    };

    let validated = match validate_webhook_request(config, &headers, &body) {
        Ok(validated) => validated,
        Err((status, ack)) => return (status, Json(ack)),
    };
    let delivery_id = validated.delivery_id.as_str();
    let event_type = validated.event_type.as_str();
    let payload = validated.payload;

    if let Err(reason) = check_storage_thresholds(config) {
        warn!(
            reason,
            repo_data_dir = %config.repo_data_dir.display(),
            "Rejecting gitea webhook due to storage threshold guard"
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(WebhookAck {
                status: "rejected",
                job_id: None,
                reason: Some("storage_threshold_guard"),
            }),
        );
    }

    let delivery_repo = GiteaPrReviewRepository::new(state.pool());
    let inserted = match delivery_repo
        .insert_delivery_if_new(delivery_id, event_type)
        .await
    {
        Ok(inserted) => inserted,
        Err(e) => {
            error!(?e, delivery_id, event_type, "Failed to persist delivery id");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookAck {
                    status: "rejected",
                    job_id: None,
                    reason: Some("delivery_store_error"),
                }),
            );
        }
    };

    if let Some((status, ack)) = duplicate_delivery_response(inserted) {
        return (status, Json(ack));
    }

    let job_id = Uuid::new_v4();
    if let Err(e) = provision_job_workspace(config, delivery_id, event_type, &payload, job_id) {
        error!(
            ?e,
            delivery_id,
            event_type,
            job_id = %job_id,
            "Failed to create gitea job workspace"
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WebhookAck {
                status: "rejected",
                job_id: None,
                reason: Some("workspace_create_failed"),
            }),
        );
    }

    let state_clone = state.clone();
    let config_clone = config.clone();
    let payload_for_job = payload.clone();
    let event_type_owned = event_type.to_string();
    let delivery_id_owned = delivery_id.to_string();
    tokio::spawn(async move {
        if let Err(e) = process_gitea_job(
            state_clone,
            config_clone.clone(),
            job_id,
            &delivery_id_owned,
            &event_type_owned,
            &payload_for_job,
        )
        .await
        {
            error!(
                ?e,
                job_id = %job_id,
                delivery_id = %delivery_id_owned,
                event_type = %event_type_owned,
                "Gitea review orchestration failed"
            );
            let _ = mark_job_failed(&config_clone, job_id, e);
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(WebhookAck {
            status: "accepted",
            job_id: Some(job_id.to_string()),
            reason: None,
        }),
    )
}

fn required_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn rejected_ack(reason: &'static str) -> WebhookAck {
    WebhookAck {
        status: "rejected",
        job_id: None,
        reason: Some(reason),
    }
}

fn validate_webhook_request(
    config: &GiteaPrReviewConfig,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<ValidatedWebhook, (StatusCode, WebhookAck)> {
    let delivery_id = required_header(headers, "X-Gitea-Delivery").ok_or((
        StatusCode::BAD_REQUEST,
        rejected_ack("missing_delivery_header"),
    ))?;
    let event_type = required_header(headers, "X-Gitea-Event").ok_or((
        StatusCode::BAD_REQUEST,
        rejected_ack("missing_event_header"),
    ))?;
    let signature = required_header(headers, "X-Gitea-Signature").ok_or((
        StatusCode::BAD_REQUEST,
        rejected_ack("missing_signature_header"),
    ))?;

    if !verify_webhook_signature(
        config.webhook_secret.expose_secret().as_bytes(),
        signature,
        body,
    ) {
        return Err((StatusCode::UNAUTHORIZED, rejected_ack("invalid_signature")));
    }

    let payload: serde_json::Value = match serde_json::from_slice(body) {
        Ok(payload) => payload,
        Err(e) => {
            warn!(?e, "Failed to parse gitea webhook payload");
            return Err((StatusCode::BAD_REQUEST, rejected_ack("invalid_json")));
        }
    };

    if !is_supported_event(event_type, &payload) {
        return Err((
            StatusCode::OK,
            WebhookAck {
                status: "ignored",
                job_id: None,
                reason: Some("unsupported_event"),
            },
        ));
    }

    Ok(ValidatedWebhook {
        delivery_id: delivery_id.to_string(),
        event_type: event_type.to_string(),
        payload,
    })
}

fn duplicate_delivery_response(inserted: bool) -> Option<(StatusCode, WebhookAck)> {
    if inserted {
        None
    } else {
        Some((
            StatusCode::OK,
            WebhookAck {
                status: "deduplicated",
                job_id: None,
                reason: Some("delivery_already_processed"),
            },
        ))
    }
}

fn is_supported_event(event_type: &str, payload: &serde_json::Value) -> bool {
    match event_type {
        "pull_request" => matches!(
            payload["action"].as_str(),
            Some("opened" | "reopened" | "synchronized")
        ),
        "issue_comment" => {
            payload["action"].as_str() == Some("created")
                && !payload["issue"]["pull_request"].is_null()
                && payload["comment"]["body"]
                    .as_str()
                    .is_some_and(|body| body.trim() == "!reviewfast")
        }
        _ => false,
    }
}

fn extract_pr_context(event_type: &str, payload: &serde_json::Value) -> Result<PrContext, String> {
    let owner = payload["repository"]["owner"]["login"]
        .as_str()
        .or_else(|| payload["repository"]["owner"]["username"].as_str())
        .unwrap_or("")
        .to_string();
    let repo = payload["repository"]["name"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let clone_url = payload["repository"]["clone_url"]
        .as_str()
        .unwrap_or("")
        .to_string();

    if owner.is_empty() || repo.is_empty() || clone_url.is_empty() {
        return Err("missing repository owner/name/clone_url in webhook payload".to_string());
    }

    if event_type == "pull_request" {
        let pr_number = payload["pull_request"]["number"]
            .as_u64()
            .or_else(|| payload["number"].as_u64())
            .unwrap_or_default();
        let pr_url = payload["pull_request"]["html_url"]
            .as_str()
            .or_else(|| payload["pull_request"]["url"].as_str())
            .unwrap_or("")
            .to_string();

        return Ok(PrContext {
            owner,
            repo,
            clone_url,
            pr_number,
            pr_url,
            pr_api_url: payload["pull_request"]["url"]
                .as_str()
                .map(ToString::to_string),
            title: payload["pull_request"]["title"]
                .as_str()
                .map(ToString::to_string),
            body: payload["pull_request"]["body"]
                .as_str()
                .map(ToString::to_string),
            head_sha: payload["pull_request"]["head"]["sha"]
                .as_str()
                .map(ToString::to_string),
            base_ref: payload["pull_request"]["base"]["ref"]
                .as_str()
                .map(ToString::to_string),
        });
    }

    let pr_number = payload["issue"]["number"].as_u64().unwrap_or_default();
    let pr_url = payload["issue"]["html_url"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let pr_api_url = payload["issue"]["pull_request"]["url"]
        .as_str()
        .map(ToString::to_string);

    Ok(PrContext {
        owner,
        repo,
        clone_url,
        pr_number,
        pr_url,
        pr_api_url,
        title: None,
        body: None,
        head_sha: None,
        base_ref: None,
    })
}

async fn fill_pr_context_from_api(
    state: &AppState,
    config: &GiteaPrReviewConfig,
    ctx: &mut PrContext,
) -> Result<(), String> {
    if ctx.title.is_some() && ctx.head_sha.is_some() && ctx.base_ref.is_some() {
        return Ok(());
    }

    let pr_api_url = ctx
        .pr_api_url
        .clone()
        .ok_or_else(|| "missing PR API URL for metadata fetch".to_string())?;
    let response = state
        .http_client
        .get(&pr_api_url)
        .header(
            "Authorization",
            format!("token {}", config.token.expose_secret()),
        )
        .send()
        .await
        .map_err(|e| format!("failed to fetch PR details from gitea: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "gitea PR details request failed with status {status}: {body}"
        ));
    }

    let details: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("failed to decode gitea PR details: {e}"))?;

    if ctx.title.is_none() {
        ctx.title = details["title"].as_str().map(ToString::to_string);
    }
    if ctx.body.is_none() {
        ctx.body = details["body"].as_str().map(ToString::to_string);
    }
    if ctx.head_sha.is_none() {
        ctx.head_sha = details["head"]["sha"].as_str().map(ToString::to_string);
    }
    if ctx.base_ref.is_none() {
        ctx.base_ref = details["base"]["ref"].as_str().map(ToString::to_string);
    }
    if ctx.pr_url.is_empty() {
        ctx.pr_url = details["html_url"].as_str().unwrap_or("").to_string();
    }

    Ok(())
}

async fn process_gitea_job(
    state: AppState,
    config: GiteaPrReviewConfig,
    job_id: Uuid,
    delivery_id: &str,
    event_type: &str,
    payload: &serde_json::Value,
) -> Result<(), String> {
    let job_root = config.repo_data_dir.join("jobs").join(job_id.to_string());
    transition_job_state(&job_root, "running", None, None, None)
        .map_err(|e| format!("failed to update job state to running: {e}"))?;

    let mut pr_ctx = extract_pr_context(event_type, payload)?;
    fill_pr_context_from_api(&state, &config, &mut pr_ctx).await?;

    let pr_title = pr_ctx
        .title
        .clone()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "missing PR title".to_string())?;
    let head_sha = pr_ctx
        .head_sha
        .clone()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "missing PR head SHA".to_string())?;
    let base_ref = pr_ctx
        .base_ref
        .clone()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "main".to_string());

    let repo_path = job_root.join("repo");
    clone_repo_to_path(
        &repo_path,
        &pr_ctx.clone_url,
        config.token.expose_secret(),
        &head_sha,
    )
    .await?;
    let base_commit = get_merge_base(&repo_path, &base_ref).await?;

    let tarball = tokio::task::spawn_blocking({
        let repo_path = repo_path.clone();
        move || create_tarball(&repo_path)
    })
    .await
    .map_err(|e| format!("tarball task failed: {e}"))?
    .map_err(|e| format!("failed to create tarball: {e}"))?;

    let payload_dir = job_root.join("payload");
    std::fs::write(payload_dir.join("payload.tar.gz"), &tarball)
        .map_err(|e| format!("failed to store local payload archive: {e}"))?;

    let r2 = state
        .r2()
        .cloned()
        .ok_or_else(|| "R2 storage is not configured".to_string())?;
    let worker_base_url = state
        .config
        .review_worker_base_url
        .clone()
        .ok_or_else(|| "review worker URL is not configured".to_string())?;

    let review_id = Uuid::new_v4();
    let r2_path = r2
        .upload_bytes(review_id, tarball)
        .await
        .map_err(|e| format!("failed to upload payload to R2: {e}"))?;

    let review_repo = ReviewRepository::new(state.pool());
    review_repo
        .create_webhook_review(CreateWebhookReviewParams {
            id: review_id,
            gh_pr_url: &pr_ctx.pr_url,
            r2_path: &r2_path,
            pr_title: &pr_title,
            github_installation_id: 0,
            pr_owner: &pr_ctx.owner,
            pr_repo: &pr_ctx.repo,
            pr_number: pr_ctx.pr_number as i32,
        })
        .await
        .map_err(|e| format!("failed to create review record: {e}"))?;

    let codebase_url = format!(
        "{}/reviews/{}/payload.tar.gz",
        worker_base_url.trim_end_matches('/'),
        review_id
    );
    let callback_url = format!("{}/review/{}", state.server_public_base_url, review_id);

    let start_request = serde_json::json!({
        "id": review_id.to_string(),
        "title": pr_title,
        "description": pr_ctx.body.unwrap_or_default(),
        "org": pr_ctx.owner,
        "repo": pr_ctx.repo,
        "codebaseUrl": codebase_url,
        "baseCommit": base_commit,
        "callbackUrl": callback_url,
    });

    let response = state
        .http_client
        .post(format!(
            "{}/review/start",
            worker_base_url.trim_end_matches('/')
        ))
        .json(&start_request)
        .send()
        .await
        .map_err(|e| format!("failed to call review worker: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "review worker returned non-success status {status}: {body}"
        ));
    }

    transition_job_state(&job_root, "worker_started", Some(review_id), None, None)
        .map_err(|e| format!("failed to update job state to worker_started: {e}"))?;

    info!(
        job_id = %job_id,
        review_id = %review_id,
        delivery_id,
        event_type,
        "Gitea review worker started"
    );

    Ok(())
}

fn mark_job_failed(
    config: &GiteaPrReviewConfig,
    job_id: Uuid,
    error: String,
) -> Result<(), String> {
    let job_root = config.repo_data_dir.join("jobs").join(job_id.to_string());
    transition_job_state(
        &job_root,
        "failed",
        None,
        Some(error),
        Some(Utc::now() + Duration::hours(config.keep_failed_hours as i64)),
    )
    .map_err(|e| format!("failed to update job state to failed: {e}"))
}

fn transition_job_state(
    job_root: &Path,
    status: &str,
    review_id: Option<Uuid>,
    error_message: Option<String>,
    cleanup_after_override: Option<DateTime<Utc>>,
) -> io::Result<()> {
    let state_path = job_root.join("state.json");
    let raw = std::fs::read(&state_path)?;
    let mut state: JobStateSnapshot = serde_json::from_slice(&raw).map_err(io::Error::other)?;

    state.status = status.to_string();
    state.updated_at = Utc::now().to_rfc3339();
    if let Some(review_id) = review_id {
        state.review_id = Some(review_id.to_string());
    }
    if let Some(cleanup_after) = cleanup_after_override {
        state.cleanup_after = cleanup_after.to_rfc3339();
    }
    state.error = error_message.map(|msg| {
        if msg.len() > 2048 {
            msg[..2048].to_string()
        } else {
            msg
        }
    });

    std::fs::write(
        state_path,
        serde_json::to_vec_pretty(&state).map_err(io::Error::other)?,
    )?;
    Ok(())
}

fn provision_job_workspace(
    config: &GiteaPrReviewConfig,
    delivery_id: &str,
    event_type: &str,
    payload: &serde_json::Value,
    job_id: Uuid,
) -> Result<(), std::io::Error> {
    let received_at = Utc::now();
    let date_prefix = received_at.format("%Y/%m/%d").to_string();
    let safe_delivery_id = sanitize_path_component(delivery_id);
    let delivery_path = config
        .repo_data_dir
        .join("deliveries")
        .join(date_prefix)
        .join(format!("{safe_delivery_id}.json"));

    if let Some(parent) = delivery_path.parent() {
        std::fs::create_dir_all(parent)?;
        ensure_within_root(&config.repo_data_dir, parent)?;
    }
    let delivery_record = DeliveryRecord {
        delivery_id,
        event_type,
        received_at: received_at.to_rfc3339(),
        payload,
    };
    std::fs::write(
        &delivery_path,
        serde_json::to_vec_pretty(&delivery_record).map_err(std::io::Error::other)?,
    )?;

    let job_root = config.repo_data_dir.join("jobs").join(job_id.to_string());
    let repo_path = job_root.join("repo");
    let payload_path = job_root.join("payload");
    std::fs::create_dir_all(&repo_path)?;
    std::fs::create_dir_all(&payload_path)?;
    ensure_within_root(&config.repo_data_dir, &repo_path)?;
    ensure_within_root(&config.repo_data_dir, &payload_path)?;

    std::fs::write(
        payload_path.join("webhook_payload.json"),
        serde_json::to_vec_pretty(payload).map_err(std::io::Error::other)?,
    )?;

    let cleanup_after = received_at + Duration::hours(config.retention_hours as i64);
    let state = JobStateSnapshot {
        job_id: job_id.to_string(),
        status: "accepted".to_string(),
        delivery_id: delivery_id.to_string(),
        event_type: event_type.to_string(),
        repo_data_path: repo_path.display().to_string(),
        payload_path: payload_path.display().to_string(),
        cleanup_after: cleanup_after.to_rfc3339(),
        created_at: received_at.to_rfc3339(),
        updated_at: received_at.to_rfc3339(),
        review_id: None,
        error: None,
    };
    std::fs::write(
        job_root.join("state.json"),
        serde_json::to_vec_pretty(&state).map_err(std::io::Error::other)?,
    )?;

    Ok(())
}

async fn clone_repo_to_path(
    repo_path: &Path,
    clone_url: &str,
    token: &str,
    head_sha: &str,
) -> Result<(), String> {
    let auth_clone_url = build_authenticated_clone_url(clone_url, token)?;

    if repo_path.exists() {
        std::fs::remove_dir_all(repo_path).map_err(|e| {
            format!(
                "failed to reset repo directory {}: {e}",
                repo_path.display()
            )
        })?;
    }
    std::fs::create_dir_all(repo_path).map_err(|e| {
        format!(
            "failed to prepare repo directory {}: {e}",
            repo_path.display()
        )
    })?;

    let clone_output = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.symlinks=false",
            "clone",
            &auth_clone_url,
            ".",
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(repo_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("failed to execute git clone: {e}"))?;

    if !clone_output.status.success() {
        let stderr = String::from_utf8_lossy(&clone_output.stderr).replace(token, "[REDACTED]");
        return Err(format!("git clone failed: {stderr}"));
    }

    let fetch_output = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "fetch",
            "origin",
            head_sha,
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(repo_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("failed to execute git fetch: {e}"))?;

    if !fetch_output.status.success() {
        let stderr = String::from_utf8_lossy(&fetch_output.stderr).replace(token, "[REDACTED]");
        return Err(format!("git fetch failed: {stderr}"));
    }

    let checkout_output = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null", "checkout", head_sha])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(repo_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("failed to execute git checkout: {e}"))?;

    if !checkout_output.status.success() {
        let stderr = String::from_utf8_lossy(&checkout_output.stderr);
        return Err(format!("git checkout failed: {stderr}"));
    }

    Ok(())
}

fn build_authenticated_clone_url(clone_url: &str, token: &str) -> Result<String, String> {
    let mut parsed = Url::parse(clone_url).map_err(|e| format!("invalid clone url: {e}"))?;
    parsed
        .set_username("oauth2")
        .map_err(|_| "failed to set clone URL username".to_string())?;
    parsed
        .set_password(Some(token))
        .map_err(|_| "failed to set clone URL password".to_string())?;
    Ok(parsed.to_string())
}

async fn get_merge_base(repo_dir: &Path, base_ref: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["merge-base", &format!("origin/{base_ref}"), "HEAD"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .current_dir(repo_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("failed to execute git merge-base: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git merge-base failed: {stderr}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn create_tarball(source_dir: &Path) -> Result<Vec<u8>, io::Error> {
    let mut buffer = Vec::new();
    {
        let encoder = GzEncoder::new(&mut buffer, Compression::default());
        let mut archive = Builder::new(encoder);
        add_directory_to_archive(&mut archive, source_dir, source_dir)?;
        let encoder = archive.into_inner()?;
        encoder.finish()?;
    }
    Ok(buffer)
}

fn add_directory_to_archive<W: io::Write>(
    archive: &mut Builder<W>,
    base_dir: &Path,
    current_dir: &Path,
) -> Result<(), io::Error> {
    let entries = std::fs::read_dir(current_dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let relative_path = path.strip_prefix(base_dir).map_err(io::Error::other)?;
        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            add_directory_to_archive(archive, base_dir, &path)?;
        } else if metadata.is_file() {
            let mut file = File::open(&path)?;
            archive.append_file(relative_path, &mut file)?;
        }
    }

    Ok(())
}

fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .collect()
}

fn ensure_within_root(root: &Path, path: &Path) -> io::Result<()> {
    let canonical_root = root.canonicalize()?;
    let canonical_path = path.canonicalize()?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(io::Error::other(
            "computed path escaped configured data root",
        ));
    }
    Ok(())
}

fn check_storage_thresholds(config: &GiteaPrReviewConfig) -> Result<(), &'static str> {
    let total_space =
        fs2::total_space(&config.repo_data_dir).map_err(|_| "failed_to_read_total_space")?;
    let free_space =
        fs2::available_space(&config.repo_data_dir).map_err(|_| "failed_to_read_free_space")?;
    let used_space = total_space.saturating_sub(free_space);

    let min_free_bytes = config.min_free_gb.saturating_mul(1024_u64.pow(3));
    if free_space < min_free_bytes {
        return Err("free_space_below_minimum");
    }

    let max_total_bytes = config.max_total_gb.saturating_mul(1024_u64.pow(3));
    if used_space > max_total_bytes {
        return Err("used_space_above_cap");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        duplicate_delivery_response, is_supported_event, mark_job_failed, provision_job_workspace,
        validate_webhook_request,
    };
    use crate::config::GiteaPrReviewConfig;
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use hmac::{Hmac, Mac};
    use secrecy::SecretString;
    use sha2::Sha256;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn supports_pull_request_opened() {
        let payload = serde_json::json!({ "action": "opened" });
        assert!(is_supported_event("pull_request", &payload));
    }

    #[test]
    fn rejects_pull_request_closed() {
        let payload = serde_json::json!({ "action": "closed" });
        assert!(!is_supported_event("pull_request", &payload));
    }

    #[test]
    fn supports_issue_comment_reviewfast() {
        let payload = serde_json::json!({
            "action": "created",
            "issue": { "pull_request": { "url": "https://gitea/pr/1" }, "number": 1 },
            "comment": { "body": " !reviewfast " }
        });
        assert!(is_supported_event("issue_comment", &payload));
    }

    #[test]
    fn rejects_issue_comment_without_pr() {
        let payload = serde_json::json!({
            "action": "created",
            "issue": { "number": 1 },
            "comment": { "body": "!reviewfast" }
        });
        assert!(!is_supported_event("issue_comment", &payload));
    }

    #[test]
    fn validates_valid_pull_request_webhook() {
        let config = test_config();
        let payload = serde_json::json!({
            "action": "opened",
            "repository": {
                "owner": { "login": "acme" },
                "name": "repo",
                "clone_url": "https://gitea.example/acme/repo.git"
            },
            "pull_request": {
                "number": 1,
                "title": "feat: test",
                "body": "body",
                "html_url": "https://gitea.example/acme/repo/pulls/1",
                "url": "https://gitea.example/api/v1/repos/acme/repo/pulls/1",
                "head": { "sha": "abc123" },
                "base": { "ref": "main" }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let headers = signed_headers("delivery-1", "pull_request", &body, "test-secret");

        let validated = validate_webhook_request(&config, &headers, &body).unwrap();
        assert_eq!(validated.delivery_id, "delivery-1");
        assert_eq!(validated.event_type, "pull_request");
        assert_eq!(validated.payload["action"], "opened");
    }

    #[test]
    fn rejects_missing_required_headers() {
        let config = test_config();
        let payload = serde_json::json!({ "action": "opened" });
        let body = serde_json::to_vec(&payload).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("X-Gitea-Delivery", HeaderValue::from_static("delivery-1"));

        let err = validate_webhook_request(&config, &headers, &body).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        assert_eq!(err.1.reason, Some("missing_event_header"));
    }

    #[test]
    fn rejects_invalid_signature() {
        let config = test_config();
        let payload = serde_json::json!({
            "action": "opened",
            "repository": {
                "owner": { "login": "acme" },
                "name": "repo",
                "clone_url": "https://gitea.example/acme/repo.git"
            },
            "pull_request": {
                "number": 1,
                "title": "feat: test",
                "body": "body",
                "html_url": "https://gitea.example/acme/repo/pulls/1",
                "url": "https://gitea.example/api/v1/repos/acme/repo/pulls/1",
                "head": { "sha": "abc123" },
                "base": { "ref": "main" }
            }
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("X-Gitea-Delivery", HeaderValue::from_static("delivery-1"));
        headers.insert("X-Gitea-Event", HeaderValue::from_static("pull_request"));
        headers.insert(
            "X-Gitea-Signature",
            HeaderValue::from_static(
                "0000000000000000000000000000000000000000000000000000000000000000",
            ),
        );

        let err = validate_webhook_request(&config, &headers, &body).unwrap_err();
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
        assert_eq!(err.1.reason, Some("invalid_signature"));
    }

    #[test]
    fn ignores_unsupported_event() {
        let config = test_config();
        let payload = serde_json::json!({ "action": "opened" });
        let body = serde_json::to_vec(&payload).unwrap();
        let headers = signed_headers("delivery-1", "push", &body, "test-secret");

        let err = validate_webhook_request(&config, &headers, &body).unwrap_err();
        assert_eq!(err.0, StatusCode::OK);
        assert_eq!(err.1.status, "ignored");
        assert_eq!(err.1.reason, Some("unsupported_event"));
    }

    #[test]
    fn duplicate_delivery_short_circuits() {
        let response = duplicate_delivery_response(false).unwrap();
        assert_eq!(response.0, StatusCode::OK);
        assert_eq!(response.1.status, "deduplicated");
        assert_eq!(response.1.reason, Some("delivery_already_processed"));
    }

    #[test]
    fn clone_failure_marks_job_terminal_failed_state() {
        let config = test_config();
        let payload = serde_json::json!({
            "action": "opened",
            "repository": {
                "owner": { "login": "acme" },
                "name": "repo",
                "clone_url": "https://gitea.example/acme/repo.git"
            },
            "pull_request": {
                "number": 1,
                "title": "feat: test",
                "body": "body",
                "html_url": "https://gitea.example/acme/repo/pulls/1",
                "url": "https://gitea.example/api/v1/repos/acme/repo/pulls/1",
                "head": { "sha": "abc123" },
                "base": { "ref": "main" }
            }
        });
        let job_id = Uuid::new_v4();
        provision_job_workspace(&config, "delivery-1", "pull_request", &payload, job_id).unwrap();

        mark_job_failed(&config, job_id, "git clone failed: auth denied".to_string()).unwrap();

        let state_path = config
            .repo_data_dir
            .join("jobs")
            .join(job_id.to_string())
            .join("state.json");
        let state: serde_json::Value =
            serde_json::from_slice(&std::fs::read(state_path).unwrap()).unwrap();
        assert_eq!(state["status"], "failed");
        assert_eq!(state["error"], "git clone failed: auth denied");
        assert!(state["cleanup_after"].as_str().unwrap().len() > 10);
    }

    fn test_config() -> GiteaPrReviewConfig {
        let dir = tempdir().unwrap();
        let repo_data_dir = dir.keep();

        std::fs::create_dir_all(repo_data_dir.join("jobs")).unwrap();
        std::fs::create_dir_all(repo_data_dir.join("deliveries")).unwrap();
        std::fs::create_dir_all(repo_data_dir.join("locks")).unwrap();
        std::fs::create_dir_all(repo_data_dir.join("tmp")).unwrap();
        std::fs::create_dir_all(repo_data_dir.join("quarantine")).unwrap();

        GiteaPrReviewConfig {
            webhook_secret: SecretString::new("test-secret".into()),
            token: SecretString::new("test-token".into()),
            repo_data_dir,
            retention_hours: 24,
            delivery_retention_hours: 72,
            max_total_gb: 100,
            min_free_gb: 0,
            max_repo_size_mb: 2048,
            cleanup_interval_seconds: 300,
            keep_failed_hours: 12,
        }
    }

    fn signed_headers(delivery_id: &str, event_type: &str, body: &[u8], secret: &str) -> HeaderMap {
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = hex::encode(mac.finalize().into_bytes());

        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Gitea-Delivery",
            HeaderValue::from_str(delivery_id).unwrap(),
        );
        headers.insert("X-Gitea-Event", HeaderValue::from_str(event_type).unwrap());
        headers.insert(
            "X-Gitea-Signature",
            HeaderValue::from_str(&signature).unwrap(),
        );
        headers
    }
}
