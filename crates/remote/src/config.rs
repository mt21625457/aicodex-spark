use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use secrecy::SecretString;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct RemoteServerConfig {
    pub database_url: String,
    pub listen_addr: String,
    pub server_public_base_url: Option<String>,
    pub auth: AuthConfig,
    pub electric_url: String,
    pub electric_secret: Option<SecretString>,
    pub electric_role_password: Option<SecretString>,
    pub electric_publication_names: Vec<String>,
    pub r2: Option<R2Config>,
    pub azure_blob: Option<AzureBlobConfig>,
    pub review_worker_base_url: Option<String>,
    pub review_disabled: bool,
    pub github_app: Option<GitHubAppConfig>,
    pub gitea_prreview: Option<GiteaPrReviewConfig>,
}

#[derive(Debug, Clone)]
pub struct GiteaPrReviewConfig {
    pub webhook_secret: SecretString,
    pub token: SecretString,
    pub repo_data_dir: PathBuf,
    pub retention_hours: u64,
    pub delivery_retention_hours: u64,
    pub max_total_gb: u64,
    pub min_free_gb: u64,
    pub max_repo_size_mb: u64,
    pub cleanup_interval_seconds: u64,
    pub keep_failed_hours: u64,
}

#[derive(Debug, Deserialize, Default)]
struct StartupConfigFile {
    #[serde(default)]
    gitea_prreview: Option<GiteaPrReviewConfigFile>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct GiteaPrReviewConfigFile {
    repo_data_dir: Option<String>,
    webhook_secret: Option<String>,
    token: Option<String>,
    retention_hours: Option<u64>,
    delivery_retention_hours: Option<u64>,
    max_total_gb: Option<u64>,
    min_free_gb: Option<u64>,
    max_repo_size_mb: Option<u64>,
    cleanup_interval_seconds: Option<u64>,
    keep_failed_hours: Option<u64>,
}

const DEFAULT_GITEA_RETENTION_HOURS: u64 = 24;
const DEFAULT_GITEA_DELIVERY_RETENTION_HOURS: u64 = 72;
const DEFAULT_GITEA_MAX_TOTAL_GB: u64 = 20;
const DEFAULT_GITEA_MIN_FREE_GB: u64 = 5;
const DEFAULT_GITEA_MAX_REPO_SIZE_MB: u64 = 2048;
const DEFAULT_GITEA_CLEANUP_INTERVAL_SECONDS: u64 = 300;
const DEFAULT_GITEA_KEEP_FAILED_HOURS: u64 = 12;

#[derive(Debug, Clone)]
pub struct R2Config {
    pub access_key_id: String,
    pub secret_access_key: SecretString,
    pub endpoint: String,
    pub bucket: String,
    pub presign_expiry_secs: u64,
}

impl R2Config {
    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        let access_key_id = match env::var("R2_ACCESS_KEY_ID") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                tracing::info!("R2_ACCESS_KEY_ID not set, R2 storage disabled");
                return Ok(None);
            }
        };

        tracing::info!("R2_ACCESS_KEY_ID is set, checking other R2 env vars");

        let secret_access_key = env::var("R2_SECRET_ACCESS_KEY")
            .map_err(|_| ConfigError::MissingVar("R2_SECRET_ACCESS_KEY"))?;

        let endpoint = env::var("R2_REVIEW_ENDPOINT")
            .map_err(|_| ConfigError::MissingVar("R2_REVIEW_ENDPOINT"))?;

        let bucket = env::var("R2_REVIEW_BUCKET")
            .map_err(|_| ConfigError::MissingVar("R2_REVIEW_BUCKET"))?;

        let presign_expiry_secs = env::var("R2_PRESIGN_EXPIRY_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);

        tracing::info!(endpoint = %endpoint, bucket = %bucket, "R2 config loaded successfully");

        Ok(Some(Self {
            access_key_id,
            secret_access_key: SecretString::new(secret_access_key.into()),
            endpoint,
            bucket,
            presign_expiry_secs,
        }))
    }
}

#[derive(Debug, Clone)]
pub enum AzureAuthMode {
    /// Entra ID via user-assigned managed identity (production).
    EntraId { client_id: String },
    /// Shared Key via custom HMAC policy (local Azurite).
    SharedKey,
}

#[derive(Debug, Clone)]
pub struct AzureBlobConfig {
    pub account_name: String,
    /// Account key is always required for SAS token generation.
    pub account_key: SecretString,
    pub container_name: String,
    pub endpoint_url: Option<String>,
    pub public_endpoint_url: Option<String>,
    pub presign_expiry_secs: u64,
    pub auth_mode: AzureAuthMode,
}

impl AzureBlobConfig {
    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        let account_name = match env::var("AZURE_STORAGE_ACCOUNT_NAME") {
            Ok(v) => v,
            Err(_) => {
                tracing::info!("AZURE_STORAGE_ACCOUNT_NAME not set, Azure Blob storage disabled");
                return Ok(None);
            }
        };

        tracing::info!("AZURE_STORAGE_ACCOUNT_NAME is set, checking other Azure Blob env vars");

        let account_key = env::var("AZURE_STORAGE_ACCOUNT_KEY")
            .map_err(|_| ConfigError::MissingVar("AZURE_STORAGE_ACCOUNT_KEY"))?;

        let container_name = env::var("AZURE_STORAGE_CONTAINER_NAME")
            .unwrap_or_else(|_| "issue-attachments".to_string());

        let endpoint_url = env::var("AZURE_STORAGE_ENDPOINT_URL").ok();
        let public_endpoint_url = env::var("AZURE_STORAGE_PUBLIC_ENDPOINT_URL").ok();

        let auth_mode = match env::var("AZURE_MANAGED_IDENTITY_CLIENT_ID") {
            Ok(client_id) => AzureAuthMode::EntraId { client_id },
            Err(_) => AzureAuthMode::SharedKey,
        };

        let presign_expiry_secs = env::var("AZURE_BLOB_PRESIGN_EXPIRY_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);

        tracing::info!(
            account_name = %account_name,
            container_name = %container_name,
            endpoint_url = ?endpoint_url,
            auth_mode = ?auth_mode,
            "Azure Blob config loaded successfully"
        );

        Ok(Some(Self {
            account_name,
            account_key: SecretString::new(account_key.into()),
            container_name,
            endpoint_url,
            public_endpoint_url,
            presign_expiry_secs,
            auth_mode,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct GitHubAppConfig {
    pub app_id: u64,
    pub private_key: SecretString, // Base64-encoded PEM
    pub webhook_secret: SecretString,
    pub app_slug: String,
}

impl GitHubAppConfig {
    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        let app_id = match env::var("GITHUB_APP_ID") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                tracing::info!("GITHUB_APP_ID not set, GitHub App integration disabled");
                return Ok(None);
            }
        };

        let app_id: u64 = app_id
            .parse()
            .map_err(|_| ConfigError::InvalidVar("GITHUB_APP_ID"))?;

        tracing::info!("GITHUB_APP_ID is set, checking other GitHub App env vars");

        let private_key = env::var("GITHUB_APP_PRIVATE_KEY")
            .map_err(|_| ConfigError::MissingVar("GITHUB_APP_PRIVATE_KEY"))?;

        // Validate that the private key is valid base64
        BASE64_STANDARD
            .decode(private_key.as_bytes())
            .map_err(|_| ConfigError::InvalidVar("GITHUB_APP_PRIVATE_KEY"))?;

        let webhook_secret = env::var("GITHUB_APP_WEBHOOK_SECRET")
            .map_err(|_| ConfigError::MissingVar("GITHUB_APP_WEBHOOK_SECRET"))?;

        let app_slug =
            env::var("GITHUB_APP_SLUG").map_err(|_| ConfigError::MissingVar("GITHUB_APP_SLUG"))?;

        tracing::info!(app_id = %app_id, app_slug = %app_slug, "GitHub App config loaded successfully");

        Ok(Some(Self {
            app_id,
            private_key: SecretString::new(private_key.into()),
            webhook_secret: SecretString::new(webhook_secret.into()),
            app_slug,
        }))
    }
}

impl GiteaPrReviewConfig {
    pub fn from_env_and_file() -> Result<Option<Self>, ConfigError> {
        let file_config = load_gitea_prreview_file_config()?;
        let has_any_env = has_any_gitea_env_var();
        let has_file_config = file_config.is_some();

        if !has_any_env && !has_file_config {
            tracing::info!("Gitea PR review integration not configured");
            return Ok(None);
        }

        let file_config = file_config.unwrap_or_default();

        let repo_data_dir = env_non_empty("GITEA_PRREVIEW_REPO_DATA_DIR")
            .or(file_config.repo_data_dir)
            .ok_or(ConfigError::MissingVar("GITEA_PRREVIEW_REPO_DATA_DIR"))?;
        let webhook_secret = env_non_empty("GITEA_PRREVIEW_WEBHOOK_SECRET")
            .or(file_config.webhook_secret)
            .ok_or(ConfigError::MissingVar("GITEA_PRREVIEW_WEBHOOK_SECRET"))?;
        let token = env_non_empty("GITEA_PRREVIEW_TOKEN")
            .or(file_config.token)
            .ok_or(ConfigError::MissingVar("GITEA_PRREVIEW_TOKEN"))?;

        let retention_hours = env_u64("GITEA_PRREVIEW_RETENTION_HOURS")?
            .or(file_config.retention_hours)
            .unwrap_or(DEFAULT_GITEA_RETENTION_HOURS);
        let delivery_retention_hours = env_u64("GITEA_PRREVIEW_DELIVERY_RETENTION_HOURS")?
            .or(file_config.delivery_retention_hours)
            .unwrap_or(DEFAULT_GITEA_DELIVERY_RETENTION_HOURS);
        let max_total_gb = env_u64("GITEA_PRREVIEW_MAX_TOTAL_GB")?
            .or(file_config.max_total_gb)
            .unwrap_or(DEFAULT_GITEA_MAX_TOTAL_GB);
        let min_free_gb = env_u64("GITEA_PRREVIEW_MIN_FREE_GB")?
            .or(file_config.min_free_gb)
            .unwrap_or(DEFAULT_GITEA_MIN_FREE_GB);
        let max_repo_size_mb = env_u64("GITEA_PRREVIEW_MAX_REPO_SIZE_MB")?
            .or(file_config.max_repo_size_mb)
            .unwrap_or(DEFAULT_GITEA_MAX_REPO_SIZE_MB);
        let cleanup_interval_seconds = env_u64("GITEA_PRREVIEW_CLEANUP_INTERVAL_SECONDS")?
            .or(file_config.cleanup_interval_seconds)
            .unwrap_or(DEFAULT_GITEA_CLEANUP_INTERVAL_SECONDS);
        let keep_failed_hours = env_u64("GITEA_PRREVIEW_KEEP_FAILED_HOURS")?
            .or(file_config.keep_failed_hours)
            .unwrap_or(DEFAULT_GITEA_KEEP_FAILED_HOURS);

        let repo_data_dir_path = PathBuf::from(repo_data_dir);
        let repo_data_dir = ensure_gitea_repo_data_dir(&repo_data_dir_path)?;

        tracing::info!(
            repo_data_dir = %repo_data_dir.display(),
            retention_hours,
            delivery_retention_hours,
            max_total_gb,
            min_free_gb,
            max_repo_size_mb,
            cleanup_interval_seconds,
            keep_failed_hours,
            "Gitea PR review config loaded"
        );

        Ok(Some(Self {
            webhook_secret: SecretString::new(webhook_secret.into()),
            token: SecretString::new(token.into()),
            repo_data_dir,
            retention_hours,
            delivery_retention_hours,
            max_total_gb,
            min_free_gb,
            max_repo_size_mb,
            cleanup_interval_seconds,
            keep_failed_hours,
        }))
    }
}

fn has_any_gitea_env_var() -> bool {
    const VARS: &[&str] = &[
        "GITEA_PRREVIEW_REPO_DATA_DIR",
        "GITEA_PRREVIEW_WEBHOOK_SECRET",
        "GITEA_PRREVIEW_TOKEN",
        "GITEA_PRREVIEW_RETENTION_HOURS",
        "GITEA_PRREVIEW_DELIVERY_RETENTION_HOURS",
        "GITEA_PRREVIEW_MAX_TOTAL_GB",
        "GITEA_PRREVIEW_MIN_FREE_GB",
        "GITEA_PRREVIEW_MAX_REPO_SIZE_MB",
        "GITEA_PRREVIEW_CLEANUP_INTERVAL_SECONDS",
        "GITEA_PRREVIEW_KEEP_FAILED_HOURS",
    ];

    VARS.iter().any(|key| env_non_empty(key).is_some())
}

fn load_gitea_prreview_file_config() -> Result<Option<GiteaPrReviewConfigFile>, ConfigError> {
    let Some(config_path) = env_non_empty("GITEA_PRREVIEW_CONFIG_FILE") else {
        return Ok(None);
    };

    let raw = fs::read_to_string(&config_path)
        .map_err(|e| ConfigError::ConfigFileRead(format!("{config_path}: {e}")))?;
    let parsed: StartupConfigFile = serde_yaml::from_str(&raw)
        .map_err(|e| ConfigError::ConfigFileParse(format!("{config_path}: {e}")))?;

    Ok(parsed.gitea_prreview)
}

fn env_non_empty(var: &str) -> Option<String> {
    env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_u64(var: &'static str) -> Result<Option<u64>, ConfigError> {
    let Some(raw) = env_non_empty(var) else {
        return Ok(None);
    };

    let parsed = raw
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidVar(var))?;
    Ok(Some(parsed))
}

fn ensure_gitea_repo_data_dir(path: &Path) -> Result<PathBuf, ConfigError> {
    if !path.is_absolute() {
        return Err(ConfigError::InvalidVar("GITEA_PRREVIEW_REPO_DATA_DIR"));
    }

    fs::create_dir_all(path).map_err(|e| {
        ConfigError::InvalidConfig(format!(
            "failed to create GITEA_PRREVIEW_REPO_DATA_DIR {}: {e}",
            path.display()
        ))
    })?;

    let canonical = path.canonicalize().map_err(|e| {
        ConfigError::InvalidConfig(format!(
            "failed to canonicalize GITEA_PRREVIEW_REPO_DATA_DIR {}: {e}",
            path.display()
        ))
    })?;

    for dir in ["deliveries", "jobs", "locks", "tmp", "quarantine"] {
        let subdir = canonical.join(dir);
        fs::create_dir_all(&subdir).map_err(|e| {
            ConfigError::InvalidConfig(format!(
                "failed to create data subdir {}: {e}",
                subdir.display()
            ))
        })?;
    }

    let probe_path = canonical.join("tmp").join(".write_probe");
    let mut file = fs::File::create(&probe_path).map_err(|e| {
        ConfigError::InvalidConfig(format!(
            "repo data dir is not writable ({}): {e}",
            canonical.display()
        ))
    })?;
    file.write_all(b"ok").map_err(|e| {
        ConfigError::InvalidConfig(format!(
            "repo data dir write probe failed ({}): {e}",
            canonical.display()
        ))
    })?;
    let _ = fs::remove_file(&probe_path);

    Ok(canonical)
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("environment variable `{0}` is not set")]
    MissingVar(&'static str),
    #[error("invalid value for environment variable `{0}`")]
    InvalidVar(&'static str),
    #[error("failed to read config file: {0}")]
    ConfigFileRead(String),
    #[error("failed to parse config file: {0}")]
    ConfigFileParse(String),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("no OAuth providers configured")]
    NoOAuthProviders,
}

impl RemoteServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("SERVER_DATABASE_URL")
            .or_else(|_| env::var("DATABASE_URL"))
            .map_err(|_| ConfigError::MissingVar("SERVER_DATABASE_URL"))?;

        let listen_addr =
            env::var("SERVER_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8081".to_string());

        let server_public_base_url = env::var("SERVER_PUBLIC_BASE_URL").ok();

        let auth = AuthConfig::from_env()?;

        let electric_url =
            env::var("ELECTRIC_URL").map_err(|_| ConfigError::MissingVar("ELECTRIC_URL"))?;

        let electric_secret = env::var("ELECTRIC_SECRET")
            .map(|s| SecretString::new(s.into()))
            .ok();

        let electric_role_password = env::var("ELECTRIC_ROLE_PASSWORD")
            .ok()
            .map(|s| SecretString::new(s.into()));
        let electric_publication_names = match env::var("ELECTRIC_PUBLICATION_NAMES") {
            Ok(value) => parse_publication_names(&value)?,
            Err(_) => Vec::new(),
        };

        let r2 = R2Config::from_env()?;
        let azure_blob = AzureBlobConfig::from_env()?;

        let review_worker_base_url = env::var("REVIEW_WORKER_BASE_URL").ok();

        let review_disabled = env::var("REVIEW_DISABLED")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let github_app = GitHubAppConfig::from_env()?;
        let gitea_prreview = GiteaPrReviewConfig::from_env_and_file()?;

        Ok(Self {
            database_url,
            listen_addr,
            server_public_base_url,
            auth,
            electric_url,
            electric_secret,
            electric_role_password,
            electric_publication_names,
            r2,
            azure_blob,
            review_worker_base_url,
            review_disabled,
            github_app,
            gitea_prreview,
        })
    }
}

fn parse_publication_names(value: &str) -> Result<Vec<String>, ConfigError> {
    let mut names = Vec::new();

    for raw in value.split(',') {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        if !is_valid_identifier(name) {
            return Err(ConfigError::InvalidVar("ELECTRIC_PUBLICATION_NAMES"));
        }
        names.push(name.to_string());
    }

    Ok(names)
}

fn is_valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Clone)]
pub struct OAuthProviderConfig {
    client_id: String,
    client_secret: SecretString,
}

impl OAuthProviderConfig {
    fn new(client_id: String, client_secret: SecretString) -> Self {
        Self {
            client_id,
            client_secret,
        }
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn client_secret(&self) -> &SecretString {
        &self.client_secret
    }
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    github: Option<OAuthProviderConfig>,
    google: Option<OAuthProviderConfig>,
    jwt_secret: SecretString,
    public_base_url: String,
}

impl AuthConfig {
    fn from_env() -> Result<Self, ConfigError> {
        let jwt_secret = env::var("VIBEKANBAN_REMOTE_JWT_SECRET")
            .map_err(|_| ConfigError::MissingVar("VIBEKANBAN_REMOTE_JWT_SECRET"))?;
        validate_jwt_secret(&jwt_secret)?;
        let jwt_secret = SecretString::new(jwt_secret.into());

        let github = match env::var("GITHUB_OAUTH_CLIENT_ID") {
            Ok(client_id) if !client_id.is_empty() => {
                let client_secret = env::var("GITHUB_OAUTH_CLIENT_SECRET")
                    .map_err(|_| ConfigError::MissingVar("GITHUB_OAUTH_CLIENT_SECRET"))?;
                Some(OAuthProviderConfig::new(
                    client_id,
                    SecretString::new(client_secret.into()),
                ))
            }
            _ => None,
        };

        let google = match env::var("GOOGLE_OAUTH_CLIENT_ID") {
            Ok(client_id) if !client_id.is_empty() => {
                let client_secret = env::var("GOOGLE_OAUTH_CLIENT_SECRET")
                    .map_err(|_| ConfigError::MissingVar("GOOGLE_OAUTH_CLIENT_SECRET"))?;
                Some(OAuthProviderConfig::new(
                    client_id,
                    SecretString::new(client_secret.into()),
                ))
            }
            _ => None,
        };

        if github.is_none() && google.is_none() {
            return Err(ConfigError::NoOAuthProviders);
        }

        let public_base_url =
            env::var("SERVER_PUBLIC_BASE_URL").unwrap_or_else(|_| "http://localhost:8081".into());

        Ok(Self {
            github,
            google,
            jwt_secret,
            public_base_url,
        })
    }

    pub fn github(&self) -> Option<&OAuthProviderConfig> {
        self.github.as_ref()
    }

    pub fn google(&self) -> Option<&OAuthProviderConfig> {
        self.google.as_ref()
    }

    pub fn jwt_secret(&self) -> &SecretString {
        &self.jwt_secret
    }

    pub fn public_base_url(&self) -> &str {
        &self.public_base_url
    }
}

fn validate_jwt_secret(secret: &str) -> Result<(), ConfigError> {
    let decoded = BASE64_STANDARD
        .decode(secret.as_bytes())
        .map_err(|_| ConfigError::InvalidVar("VIBEKANBAN_REMOTE_JWT_SECRET"))?;

    if decoded.len() < 32 {
        return Err(ConfigError::InvalidVar("VIBEKANBAN_REMOTE_JWT_SECRET"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use secrecy::ExposeSecret;

    use super::*;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    const GITEA_ENV_KEYS: &[&str] = &[
        "GITEA_PRREVIEW_CONFIG_FILE",
        "GITEA_PRREVIEW_REPO_DATA_DIR",
        "GITEA_PRREVIEW_WEBHOOK_SECRET",
        "GITEA_PRREVIEW_TOKEN",
        "GITEA_PRREVIEW_RETENTION_HOURS",
        "GITEA_PRREVIEW_DELIVERY_RETENTION_HOURS",
        "GITEA_PRREVIEW_MAX_TOTAL_GB",
        "GITEA_PRREVIEW_MIN_FREE_GB",
        "GITEA_PRREVIEW_MAX_REPO_SIZE_MB",
        "GITEA_PRREVIEW_CLEANUP_INTERVAL_SECONDS",
        "GITEA_PRREVIEW_KEEP_FAILED_HOURS",
    ];

    fn clear_gitea_env() {
        for key in GITEA_ENV_KEYS {
            // SAFETY: tests serialize env mutations via `ENV_LOCK`.
            unsafe { env::remove_var(key) };
        }
    }

    #[test]
    fn gitea_config_loads_from_file_and_env_overrides() {
        let _guard = env_lock().lock().unwrap();
        clear_gitea_env();

        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("gitea.yaml");
        let data_dir = temp.path().join("repo-data");

        let yaml = format!(
            "gitea_prreview:\n  repo_data_dir: {}\n  webhook_secret: file-secret\n  token: file-token\n  retention_hours: 36\n",
            data_dir.display()
        );
        std::fs::write(&config_path, yaml).unwrap();

        // SAFETY: tests serialize env mutations via `ENV_LOCK`.
        unsafe {
            env::set_var("GITEA_PRREVIEW_CONFIG_FILE", &config_path);
            env::set_var("GITEA_PRREVIEW_TOKEN", "env-token");
        }

        let config = GiteaPrReviewConfig::from_env_and_file().unwrap().unwrap();
        assert_eq!(config.token.expose_secret(), "env-token");
        assert_eq!(config.webhook_secret.expose_secret(), "file-secret");
        assert_eq!(config.retention_hours, 36);
        assert!(config.repo_data_dir.is_absolute());

        clear_gitea_env();
    }

    #[test]
    fn gitea_config_rejects_relative_repo_data_dir() {
        let _guard = env_lock().lock().unwrap();
        clear_gitea_env();

        // SAFETY: tests serialize env mutations via `ENV_LOCK`.
        unsafe {
            env::set_var("GITEA_PRREVIEW_REPO_DATA_DIR", "relative/path");
            env::set_var("GITEA_PRREVIEW_WEBHOOK_SECRET", "secret");
            env::set_var("GITEA_PRREVIEW_TOKEN", "token");
        }

        let err = GiteaPrReviewConfig::from_env_and_file().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidVar("GITEA_PRREVIEW_REPO_DATA_DIR")
        ));

        clear_gitea_env();
    }

    #[test]
    fn gitea_config_fails_when_required_fields_missing_after_merge() {
        let _guard = env_lock().lock().unwrap();
        clear_gitea_env();

        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("gitea.yaml");
        let data_dir = temp.path().join("repo-data");

        // Missing token on purpose.
        let yaml = format!(
            "gitea_prreview:\n  repo_data_dir: {}\n  webhook_secret: file-secret\n",
            data_dir.display()
        );
        std::fs::write(&config_path, yaml).unwrap();

        // SAFETY: tests serialize env mutations via `ENV_LOCK`.
        unsafe {
            env::set_var("GITEA_PRREVIEW_CONFIG_FILE", &config_path);
        }

        let err = GiteaPrReviewConfig::from_env_and_file().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::MissingVar("GITEA_PRREVIEW_TOKEN")
        ));

        clear_gitea_env();
    }
}
