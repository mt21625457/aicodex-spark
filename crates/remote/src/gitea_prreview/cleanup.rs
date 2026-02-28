use std::{fs, path::Path};

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use tracing::{error, info, warn};

use crate::config::GiteaPrReviewConfig;

#[derive(Debug, Deserialize)]
struct JobCleanupState {
    status: String,
    cleanup_after: String,
}

pub fn spawn_cleanup_task(config: GiteaPrReviewConfig) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            config.cleanup_interval_seconds.max(1),
        ));

        loop {
            interval.tick().await;

            let config_for_run = config.clone();
            match tokio::task::spawn_blocking(move || run_cleanup_once(&config_for_run)).await {
                Ok(Ok((jobs_removed, deliveries_removed))) => {
                    if jobs_removed > 0 || deliveries_removed > 0 {
                        info!(
                            jobs_removed,
                            deliveries_removed, "Completed gitea PR review cleanup cycle"
                        );
                    }
                }
                Ok(Err(err)) => {
                    warn!(error = %err, "Gitea PR review cleanup cycle failed");
                }
                Err(join_err) => {
                    error!(?join_err, "Gitea PR review cleanup task join failed");
                }
            }
        }
    });
}

fn run_cleanup_once(config: &GiteaPrReviewConfig) -> Result<(usize, usize), String> {
    let jobs_removed = cleanup_expired_jobs(config)?;
    let deliveries_removed = cleanup_expired_deliveries(config)?;
    Ok((jobs_removed, deliveries_removed))
}

fn cleanup_expired_jobs(config: &GiteaPrReviewConfig) -> Result<usize, String> {
    let jobs_root = config.repo_data_dir.join("jobs");
    if !jobs_root.exists() {
        return Ok(0);
    }

    let mut removed = 0usize;
    let now = Utc::now();

    for entry in fs::read_dir(&jobs_root).map_err(|e| format!("read jobs dir failed: {e}"))? {
        let entry = entry.map_err(|e| format!("read jobs entry failed: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let state_path = path.join("state.json");
        if !state_path.exists() {
            continue;
        }

        let raw = fs::read(&state_path).map_err(|e| format!("read state.json failed: {e}"))?;
        let state: JobCleanupState =
            serde_json::from_slice(&raw).map_err(|e| format!("parse state.json failed: {e}"))?;

        if !is_terminal_status(&state.status) {
            continue;
        }

        let cleanup_after = DateTime::parse_from_rfc3339(&state.cleanup_after)
            .map_err(|e| format!("invalid cleanup_after in state.json: {e}"))?
            .with_timezone(&Utc);
        if now < cleanup_after {
            continue;
        }

        let quarantine_name = format!(
            "{}-{}",
            entry.file_name().to_string_lossy(),
            now.timestamp_millis()
        );
        let quarantine_path = config
            .repo_data_dir
            .join("quarantine")
            .join(quarantine_name);

        fs::rename(&path, &quarantine_path)
            .map_err(|e| format!("move to quarantine failed for {}: {e}", path.display()))?;

        match fs::remove_dir_all(&quarantine_path) {
            Ok(()) => removed += 1,
            Err(e) => warn!(
                error = %e,
                path = %quarantine_path.display(),
                "Failed to remove quarantined job directory"
            ),
        }
    }

    Ok(removed)
}

fn cleanup_expired_deliveries(config: &GiteaPrReviewConfig) -> Result<usize, String> {
    let deliveries_root = config.repo_data_dir.join("deliveries");
    if !deliveries_root.exists() {
        return Ok(0);
    }

    let cutoff = Utc::now() - Duration::hours(config.delivery_retention_hours as i64);
    let mut removed_files = 0usize;
    cleanup_delivery_dir(&deliveries_root, cutoff, &mut removed_files)?;
    Ok(removed_files)
}

fn cleanup_delivery_dir(
    path: &Path,
    cutoff: DateTime<Utc>,
    removed_files: &mut usize,
) -> Result<(), String> {
    for entry in fs::read_dir(path).map_err(|e| format!("read delivery dir failed: {e}"))? {
        let entry = entry.map_err(|e| format!("read delivery entry failed: {e}"))?;
        let entry_path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|e| format!("read delivery metadata failed: {e}"))?;

        if metadata.is_dir() {
            cleanup_delivery_dir(&entry_path, cutoff, removed_files)?;
            if is_directory_empty(&entry_path)
                .map_err(|e| format!("check delivery dir emptiness failed: {e}"))?
            {
                let _ = fs::remove_dir(&entry_path);
            }
            continue;
        }

        if metadata.is_file() {
            let modified = metadata
                .modified()
                .map_err(|e| format!("read delivery file modified time failed: {e}"))?;
            let modified: DateTime<Utc> = modified.into();
            if modified < cutoff {
                fs::remove_file(&entry_path).map_err(|e| {
                    format!(
                        "remove expired delivery file {} failed: {e}",
                        entry_path.display()
                    )
                })?;
                *removed_files += 1;
            }
        }
    }

    Ok(())
}

fn is_directory_empty(path: &Path) -> Result<bool, std::io::Error> {
    Ok(fs::read_dir(path)?.next().is_none())
}

fn is_terminal_status(status: &str) -> bool {
    matches!(status, "worker_started" | "completed" | "failed")
}
