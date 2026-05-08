use crate::auth::AuthConfig;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// Environment variable names — centralized to keep env/CLI wiring in sync.
const ENV_DOMAIN: &str = "ATLASSIAN_DOMAIN";
const ENV_AUTH_METHOD: &str = "ATLASSIAN_AUTH_METHOD";
const ENV_EMAIL: &str = "ATLASSIAN_EMAIL";
const ENV_API_TOKEN: &str = "ATLASSIAN_API_TOKEN";
const ENV_CLIENT_ID: &str = "ATLASSIAN_CLIENT_ID";
const ENV_CLIENT_SECRET: &str = "ATLASSIAN_CLIENT_SECRET";
const ENV_CLOUD_ID: &str = "ATLASSIAN_CLOUD_ID";

/// Resolves the final `AuthConfig` from three sources in strict precedence:
/// CLI flags > environment variables > config file.
///
/// Method precedence:
///   - `ATLASSIAN_AUTH_METHOD` env, when set, selects the method (validated).
///   - Otherwise the method is inferred from the config file's auth section.
///   - If neither is present, returns `Ok(None)`.
///
/// Field precedence (per method):
///   - Each field is picked as `cli.or(env).or(file)`, yielding missing-field
///     errors that name all three sources.
///
/// All logic that used to live in separate `apply_env_*` / `apply_cli_*`
/// helpers plus the ad-hoc method-switch branch is unified here.
struct AuthResolver<'a> {
    file_auth: Option<&'a AuthConfig>,
    cli: &'a CliOverrides,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthMethod {
    Basic,
    ServiceAccount,
}

impl AuthMethod {
    fn parse(s: &str) -> Result<Self> {
        let normalized = s.trim().to_lowercase();
        match normalized.as_str() {
            "basic" => Ok(AuthMethod::Basic),
            "service_account" => Ok(AuthMethod::ServiceAccount),
            other => bail!(
                "Unknown {ENV_AUTH_METHOD}: '{}'. Use 'basic' or 'service_account'",
                other
            ),
        }
    }

    fn of(auth: &AuthConfig) -> Self {
        match auth {
            AuthConfig::Basic { .. } => AuthMethod::Basic,
            AuthConfig::ServiceAccount { .. } => AuthMethod::ServiceAccount,
        }
    }
}

impl AuthResolver<'_> {
    /// Consumes the resolver. It's a one-shot builder — reusing it would be
    /// meaningless since env vars are read at resolve time.
    fn resolve(self) -> Result<Option<AuthConfig>> {
        // Step 1: determine the effective auth method.
        let method = match (std::env::var(ENV_AUTH_METHOD).ok(), self.file_auth) {
            (Some(m), _) => AuthMethod::parse(&m)?,
            (None, Some(auth)) => AuthMethod::of(auth),
            (None, None) => return Ok(None),
        };

        // Step 2: assemble fields using CLI > env > file precedence.
        // File values contribute only when their method matches the effective
        // method — otherwise they'd belong to a different variant.
        let file_matches = self.file_auth.is_some_and(|a| AuthMethod::of(a) == method);

        Ok(Some(match method {
            AuthMethod::Basic => AuthConfig::Basic {
                email: self
                    .pick(self.cli.email.clone(), ENV_EMAIL, |a| match a {
                        AuthConfig::Basic { email, .. } if file_matches => Some(email.clone()),
                        _ => None,
                    })
                    .with_context(|| {
                        format!("email required for basic auth (set via --email, {ENV_EMAIL}, or config)")
                    })?,
                token: self
                    .pick(self.cli.token.clone(), ENV_API_TOKEN, |a| match a {
                        AuthConfig::Basic { token, .. } if file_matches => Some(token.clone()),
                        _ => None,
                    })
                    .with_context(|| {
                        format!("API token required (set via --token, {ENV_API_TOKEN}, or config)")
                    })?,
            },
            AuthMethod::ServiceAccount => AuthConfig::ServiceAccount {
                client_id: self
                    .pick(self.cli.client_id.clone(), ENV_CLIENT_ID, |a| match a {
                        AuthConfig::ServiceAccount { client_id, .. } if file_matches => {
                            Some(client_id.clone())
                        }
                        _ => None,
                    })
                    .with_context(|| {
                        format!(
                            "Service account client_id required (set via --client-id, {ENV_CLIENT_ID}, or config)"
                        )
                    })?,
                client_secret: self
                    .pick(self.cli.client_secret.clone(), ENV_CLIENT_SECRET, |a| {
                        match a {
                            AuthConfig::ServiceAccount { client_secret, .. } if file_matches => {
                                Some(client_secret.clone())
                            }
                            _ => None,
                        }
                    })
                    .with_context(|| {
                        format!(
                            "Service account client_secret required (set via --client-secret, {ENV_CLIENT_SECRET}, or config)"
                        )
                    })?,
                cloud_id: self.pick(self.cli.cloud_id.clone(), ENV_CLOUD_ID, |a| match a {
                    AuthConfig::ServiceAccount { cloud_id, .. } if file_matches => cloud_id.clone(),
                    _ => None,
                }),
            },
        }))
    }

    /// CLI > env > file precedence, returning the first `Some` or `None`.
    fn pick<F>(&self, cli: Option<String>, env_name: &str, from_file: F) -> Option<String>
    where
        F: FnOnce(&AuthConfig) -> Option<String>,
    {
        cli.or_else(|| std::env::var(env_name).ok())
            .or_else(|| self.file_auth.and_then(from_file))
    }
}

/// CLI flag/env var overrides passed to Config::load.
#[derive(Debug, Default)]
pub struct CliOverrides {
    pub domain: Option<String>,
    pub email: Option<String>,
    pub token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub cloud_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Site domain (e.g. "company.atlassian.net").
    /// Required for Basic auth. Ignored for Service account (cloud_id is used instead).
    pub domain: Option<String>,

    /// Authentication configuration (Basic or Service account).
    #[serde(default)]
    pub auth: Option<AuthConfig>,

    #[serde(default)]
    pub jira: JiraConfig,

    #[serde(default)]
    pub confluence: ConfluenceConfig,

    #[serde(default)]
    pub performance: PerformanceConfig,

    #[serde(default)]
    pub optimization: OptimizationConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraConfig {
    #[serde(default)]
    pub projects_filter: Vec<String>,

    pub search_default_fields: Option<Vec<String>>,

    #[serde(default)]
    pub search_custom_fields: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfluenceConfig {
    #[serde(default)]
    pub spaces_filter: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceConfig {
    #[serde(default = "default_timeout")]
    pub request_timeout_ms: u64,

    #[serde(default = "default_rate_limit_delay")]
    pub rate_limit_delay_ms: u64,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            request_timeout_ms: default_timeout(),
            rate_limit_delay_ms: default_rate_limit_delay(),
        }
    }
}

fn default_timeout() -> u64 {
    30000
}

fn default_rate_limit_delay() -> u64 {
    200
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptimizationConfig {
    pub response_exclude_fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    default: ConfigProfile,

    #[serde(flatten)]
    profiles: HashMap<String, ConfigProfile>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigProfile {
    domain: Option<String>,
    auth: Option<AuthConfig>,

    #[serde(default)]
    jira: JiraConfig,

    #[serde(default)]
    confluence: ConfluenceConfig,

    /// Option distinguishes "section absent" from "section present with defaults".
    /// Without Option, a child config with no [performance] section would silently
    /// overwrite a parent's explicit timeout settings with defaults.
    performance: Option<PerformanceConfig>,

    #[serde(default)]
    optimization: OptimizationConfig,
}

impl Config {
    pub fn load(
        config_path: Option<&PathBuf>,
        profile: Option<&String>,
        overrides: CliOverrides,
    ) -> Result<Self> {
        Self::load_with_validation(config_path, profile, overrides, true)
    }

    pub fn load_without_validation(
        config_path: Option<&PathBuf>,
        profile: Option<&String>,
        overrides: CliOverrides,
    ) -> Result<Self> {
        Self::load_with_validation(config_path, profile, overrides, false)
    }

    fn load_with_validation(
        config_path: Option<&PathBuf>,
        profile: Option<&String>,
        overrides: CliOverrides,
        validate: bool,
    ) -> Result<Self> {
        let mut config = Self::default();
        // Track whether the requested profile was found in any config file.
        // A profile must exist in at least one file to be usable.
        let mut profile_found = profile.is_none(); // "default" is always considered found

        // 1. Load global config
        if let Some(global_path) = Self::global_config_path()
            && global_path.exists()
        {
            tracing::debug!("Loading global config: {:?}", global_path);
            if let Some(profile_config) = Self::load_from_file(&global_path, profile)? {
                config.merge(profile_config);
                profile_found = true;
            }
        }

        // 2. Load project config
        if let Some(project_path) = Self::project_config_path() {
            tracing::debug!("Loading project config: {:?}", project_path);
            if let Some(profile_config) = Self::load_from_file(&project_path, profile)? {
                config.merge(profile_config);
                profile_found = true;
            }
        }

        // 3. Load custom config file
        if let Some(path) = config_path {
            tracing::debug!("Loading custom config: {:?}", path);
            if let Some(profile_config) = Self::load_from_file(path, profile)? {
                config.merge(profile_config);
                profile_found = true;
            }
        }

        if !profile_found {
            bail!(
                "Profile '{}' not found in any loaded config file",
                profile.map(String::as_str).unwrap_or("default")
            );
        }

        // 4. Environment variables override (domain + operational settings).
        // Auth resolution is handled separately via AuthResolver at step 6.
        if let Ok(val) = std::env::var(ENV_DOMAIN) {
            config.domain = Some(val);
        }

        // Load additional env vars for filters and settings
        if let Ok(val) = std::env::var("JIRA_PROJECTS_FILTER") {
            config.jira.projects_filter = val
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .collect();
        }

        if let Ok(val) = std::env::var("CONFLUENCE_SPACES_FILTER") {
            config.confluence.spaces_filter = val
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .collect();
        }

        if let Ok(val) = std::env::var("JIRA_SEARCH_DEFAULT_FIELDS") {
            config.jira.search_default_fields = Some(
                val.split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.trim().to_string())
                    .collect(),
            );
        }

        if let Ok(val) = std::env::var("JIRA_SEARCH_CUSTOM_FIELDS") {
            config.jira.search_custom_fields = val
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .collect();
        }

        if let Ok(val) = std::env::var("RESPONSE_EXCLUDE_FIELDS") {
            config.optimization.response_exclude_fields = Some(
                val.split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.trim().to_string())
                    .collect(),
            );
        }

        if let Ok(val) = std::env::var("REQUEST_TIMEOUT_MS") {
            config.performance.request_timeout_ms =
                val.parse().context("Invalid REQUEST_TIMEOUT_MS")?;
        }

        // 5. Resolve auth from file + env + CLI in a single pass.
        //    Precedence: CLI > env > file, per field. See AuthResolver docs.
        config.auth = AuthResolver {
            file_auth: config.auth.as_ref(),
            cli: &overrides,
        }
        .resolve()?;

        // 6. Domain CLI override (highest priority, after env at step 4).
        if let Some(d) = overrides.domain {
            config.domain = Some(d);
        }

        // 7. Validate
        if validate {
            config.validate()?;
        }

        Ok(config)
    }

    /// Load a profile from a config file.
    /// Returns `Ok(None)` if the named profile doesn't exist in this file
    /// (other config files may still have it).
    /// Returns `Ok(Some(default))` when no profile is specified.
    /// Returns `Err` only for parse/IO errors.
    fn load_from_file(path: &Path, profile: Option<&String>) -> Result<Option<ConfigProfile>> {
        #[cfg(unix)]
        Self::check_permissions(path)?;

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        let config_file: ConfigFile = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", path))?;

        match profile {
            Some(profile_name) => Ok(config_file.profiles.get(profile_name).cloned()),
            None => Ok(Some(config_file.default)),
        }
    }

    #[cfg(unix)]
    fn check_permissions(path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::metadata(path)?;
        let permissions = metadata.permissions();
        let mode = permissions.mode();

        if mode & 0o077 != 0 {
            tracing::warn!(
                "Config file {:?} has too permissive permissions: {:o}. \
                 Recommend: chmod 600 {:?}",
                path,
                mode,
                path
            );
        }

        Ok(())
    }

    fn merge(&mut self, other: ConfigProfile) {
        if other.domain.is_some() {
            self.domain = other.domain;
        }
        if other.auth.is_some() {
            self.auth = other.auth;
        }

        if !other.jira.projects_filter.is_empty() {
            self.jira.projects_filter = other.jira.projects_filter;
        }
        if other.jira.search_default_fields.is_some() {
            self.jira.search_default_fields = other.jira.search_default_fields;
        }
        if !other.jira.search_custom_fields.is_empty() {
            self.jira.search_custom_fields = other.jira.search_custom_fields;
        }

        if !other.confluence.spaces_filter.is_empty() {
            self.confluence.spaces_filter = other.confluence.spaces_filter;
        }

        // Only overwrite performance if the child profile explicitly specified it.
        // This prevents silent data loss where a child without [performance] would
        // reset the parent's settings to defaults.
        if let Some(perf) = other.performance {
            self.performance = perf;
        }

        if other.optimization.response_exclude_fields.is_some() {
            self.optimization.response_exclude_fields = other.optimization.response_exclude_fields;
        }
    }

    pub fn validate(&self) -> Result<()> {
        let auth = self
            .auth
            .as_ref()
            .context("Authentication not configured. Set ATLASSIAN_AUTH_METHOD env var or add [default.auth] to config")?;

        match auth {
            AuthConfig::Basic { email, token } => {
                let domain = self.domain.as_ref().context(
                    "ATLASSIAN_DOMAIN not configured. Set via:\n\
                     1. --domain flag\n\
                     2. ATLASSIAN_DOMAIN env var\n\
                     3. Config file: atlassian-cli config init",
                )?;

                let clean_domain = domain
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_end_matches('/');

                // Domain must end with `.atlassian.net` (not just contain it).
                // Prevents spoofing like "attacker.atlassian.net.evil.com"
                if !clean_domain.ends_with(".atlassian.net") {
                    bail!(
                        "Invalid Atlassian domain format: {} (must end with .atlassian.net)",
                        domain
                    );
                }

                if !email.contains('@') {
                    bail!("Invalid email format: {}", email);
                }

                if token.is_empty() {
                    bail!("API token is empty");
                }
            }
            AuthConfig::ServiceAccount {
                client_id,
                client_secret,
                ..
            } => {
                if client_id.is_empty() {
                    bail!("Service account client_id is empty");
                }
                if client_secret.is_empty() {
                    bail!("Service account client_secret is empty");
                }
            }
        }

        if self.performance.request_timeout_ms < 100 || self.performance.request_timeout_ms > 60000
        {
            bail!("Request timeout must be between 100ms and 60000ms");
        }

        Ok(())
    }

    pub fn global_config_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".config/atlassian-cli/config.toml"))
    }

    pub fn project_config_path() -> Option<PathBuf> {
        let current = std::env::current_dir().ok()?;
        let mut dir = current.as_path();

        loop {
            let candidate = dir.join(".atlassian.toml");
            if candidate.exists() {
                return Some(candidate);
            }

            let alt = dir.join(".atlassian/config.toml");
            if alt.exists() {
                return Some(alt);
            }

            dir = dir.parent()?;
        }
    }

    pub fn init_config(global: bool) -> Result<PathBuf> {
        let path = if global {
            Self::global_config_path().context("Failed to determine global config path")?
        } else {
            PathBuf::from(".atlassian.toml")
        };

        if path.exists() {
            bail!("Config file already exists: {:?}", path);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let template = r#"[default]
# domain = "company.atlassian.net"  # Required for basic auth

# === Basic Auth ===
# [default.auth]
# method = "basic"
# email = "user@example.com"
# token = "..."  # Prefer ATLASSIAN_API_TOKEN env var

# === OAuth 2.0 Service Account ===
# [default.auth]
# method = "service_account"
# client_id = "your-client-id"
# client_secret = "..."  # Prefer ATLASSIAN_CLIENT_SECRET env var
# cloud_id = "..."  # Optional, auto-discovered if omitted

[default.jira]
projects_filter = []
# search_default_fields = ["key", "summary", "status", "assignee"]
# search_custom_fields = ["customfield_10015"]

[default.confluence]
spaces_filter = []

[default.performance]
request_timeout_ms = 30000
rate_limit_delay_ms = 200

# [default.optimization]
# response_exclude_fields = ["avatarUrls", "iconUrl"]

# Additional profiles (multi-tenant support)
# [work]
# domain = "work.atlassian.net"
# [work.auth]
# method = "basic"
# email = "me@work.com"
# token = "..."

# [service]
# [service.auth]
# method = "service_account"
# client_id = "..."
# client_secret = "..."
"#;

        fs::write(&path, template)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&path, perms)?;
        }

        Ok(path)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    // ---------- AuthResolver tests ----------
    // These tests mutate process-global environment variables, so they must
    // not run in parallel. The static Mutex serializes them while still
    // letting the rest of the test suite run concurrently.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clear_auth_env() {
        for k in [
            ENV_AUTH_METHOD,
            ENV_EMAIL,
            ENV_API_TOKEN,
            ENV_CLIENT_ID,
            ENV_CLIENT_SECRET,
            ENV_CLOUD_ID,
        ] {
            // SAFETY: callers hold `ENV_LOCK`, serializing env access with all
            // other resolver tests. No other code mutates these during tests.
            unsafe { std::env::remove_var(k) };
        }
    }

    #[test]
    fn test_resolver_no_sources_returns_none() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: None,
            cli: &overrides,
        };
        let result = r.resolve().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_resolver_file_basic_passthrough() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        let file = AuthConfig::Basic {
            email: "a@b.c".into(),
            token: "tk".into(),
        };
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        };
        let result = r.resolve().unwrap().unwrap();
        match result {
            AuthConfig::Basic { email, token } => {
                assert_eq!(email, "a@b.c");
                assert_eq!(token, "tk");
            }
            _ => panic!("expected Basic"),
        }
    }

    #[test]
    fn test_resolver_cli_overrides_file() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        let file = AuthConfig::Basic {
            email: "file@x.com".into(),
            token: "file-tk".into(),
        };
        let overrides = CliOverrides {
            email: Some("cli@x.com".into()),
            ..Default::default()
        };
        let r = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        };
        let result = r.resolve().unwrap().unwrap();
        match result {
            AuthConfig::Basic { email, token } => {
                assert_eq!(email, "cli@x.com"); // CLI wins
                assert_eq!(token, "file-tk"); // file fallback
            }
            _ => panic!("expected Basic"),
        }
    }

    #[test]
    fn test_resolver_method_switch_drops_file_fields() {
        // File has Service account; CLI has basic credentials; env selects basic method.
        // File fields belong to a different method → must not leak into Basic.
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        unsafe { std::env::set_var(ENV_AUTH_METHOD, "basic") };
        let file = AuthConfig::ServiceAccount {
            client_id: "fid".into(),
            client_secret: "fsec".into(),
            cloud_id: None,
        };
        let overrides = CliOverrides {
            email: Some("new@user.com".into()),
            token: Some("new-tk".into()),
            ..Default::default()
        };
        let r = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        };
        let result = r.resolve().unwrap().unwrap();
        match result {
            AuthConfig::Basic { email, token } => {
                assert_eq!(email, "new@user.com");
                assert_eq!(token, "new-tk");
            }
            _ => panic!("method switch should yield Basic"),
        }
        clear_auth_env();
    }

    #[test]
    fn test_resolver_invalid_method_errors() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        unsafe { std::env::set_var(ENV_AUTH_METHOD, "saml") };
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: None,
            cli: &overrides,
        };
        let err = r.resolve().unwrap_err();
        assert!(err.to_string().contains("Unknown"));
        clear_auth_env();
    }

    #[test]
    fn test_resolver_env_method_is_trimmed() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        unsafe {
            std::env::set_var(ENV_AUTH_METHOD, " service_account ");
            std::env::set_var(ENV_CLIENT_ID, "cid");
            std::env::set_var(ENV_CLIENT_SECRET, "secret");
        }
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: None,
            cli: &overrides,
        };
        let result = r.resolve().unwrap().unwrap();
        match result {
            AuthConfig::ServiceAccount {
                client_id,
                client_secret,
                ..
            } => {
                assert_eq!(client_id, "cid");
                assert_eq!(client_secret, "secret");
            }
            _ => panic!("expected service account auth"),
        }
        clear_auth_env();
    }

    #[test]
    fn test_resolver_missing_field_reports_all_sources() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        // method selected but no fields anywhere
        unsafe { std::env::set_var(ENV_AUTH_METHOD, "basic") };
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: None,
            cli: &overrides,
        };
        let err = r.resolve().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--email"),
            "error should mention CLI flag: {}",
            msg
        );
        assert!(
            msg.contains(ENV_EMAIL),
            "error should mention env var: {}",
            msg
        );
        clear_auth_env();
    }

    fn create_basic_config() -> Config {
        Config {
            domain: Some("test.atlassian.net".to_string()),
            auth: Some(AuthConfig::Basic {
                email: "test@example.com".to_string(),
                token: "token123".to_string(),
            }),
            ..Default::default()
        }
    }

    fn create_service_account_config() -> Config {
        Config {
            domain: None,
            auth: Some(AuthConfig::ServiceAccount {
                client_id: "test-cid".to_string(),
                client_secret: "test-secret".to_string(),
                cloud_id: Some("cloud-123".to_string()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_basic_auth_validation() {
        let config = create_basic_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_service_account_validation() {
        let config = create_service_account_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_missing_auth_fails() {
        let config = Config {
            domain: Some("test.atlassian.net".to_string()),
            auth: None,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_basic_missing_domain_fails() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::Basic {
                email: "test@example.com".to_string(),
                token: "token123".to_string(),
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_basic_invalid_domain_fails() {
        let config = Config {
            domain: Some("invalid-domain.com".to_string()),
            auth: Some(AuthConfig::Basic {
                email: "test@example.com".to_string(),
                token: "token123".to_string(),
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_basic_spoofed_domain_fails() {
        // Domain spoofing attempt: ".atlassian.net" appears but not as suffix
        let config = Config {
            domain: Some("attacker.atlassian.net.evil.com".to_string()),
            auth: Some(AuthConfig::Basic {
                email: "test@example.com".to_string(),
                token: "token123".to_string(),
            }),
            ..Default::default()
        };
        assert!(
            config.validate().is_err(),
            "Spoofed domain should be rejected"
        );
    }

    #[test]
    fn test_basic_domain_with_trailing_slash_ok() {
        let config = Config {
            domain: Some("https://test.atlassian.net/".to_string()),
            auth: Some(AuthConfig::Basic {
                email: "test@example.com".to_string(),
                token: "token123".to_string(),
            }),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_basic_invalid_email_fails() {
        let config = Config {
            domain: Some("test.atlassian.net".to_string()),
            auth: Some(AuthConfig::Basic {
                email: "invalid-email".to_string(),
                token: "token123".to_string(),
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_service_account_empty_client_id_fails() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::ServiceAccount {
                client_id: String::new(),
                client_secret: "secret".to_string(),
                cloud_id: None,
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_service_account_empty_secret_fails() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::ServiceAccount {
                client_id: "cid".to_string(),
                client_secret: String::new(),
                cloud_id: None,
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_service_account_without_domain_passes() {
        let config = create_service_account_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_timeout_bounds() {
        let mut config = create_basic_config();

        config.performance.request_timeout_ms = 50;
        assert!(config.validate().is_err());

        config.performance.request_timeout_ms = 100;
        assert!(config.validate().is_ok());

        config.performance.request_timeout_ms = 60000;
        assert!(config.validate().is_ok());

        config.performance.request_timeout_ms = 60001;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_merge_auth() {
        let mut config = Config::default();
        let profile = ConfigProfile {
            auth: Some(AuthConfig::Basic {
                email: "merged@example.com".to_string(),
                token: "merged-token".to_string(),
            }),
            ..Default::default()
        };

        config.merge(profile);
        assert!(config.auth.is_some());
        match config.auth.unwrap() {
            AuthConfig::Basic { email, .. } => assert_eq!(email, "merged@example.com"),
            _ => panic!("Expected Basic auth"),
        }
    }

    #[test]
    fn test_merge_preserves_existing_when_none() {
        let mut config = create_basic_config();
        let profile = ConfigProfile {
            auth: None,
            ..Default::default()
        };

        config.merge(profile);
        assert!(config.auth.is_some());
    }

    #[test]
    fn test_merge_performance_preserved_when_child_not_specified() {
        // Regression test: child profile without [performance] must not overwrite
        // parent's explicit timeout with defaults.
        let mut config = create_basic_config();
        config.performance.request_timeout_ms = 5000;
        config.performance.rate_limit_delay_ms = 100;

        let profile = ConfigProfile {
            performance: None, // [performance] section absent in child TOML
            ..Default::default()
        };

        config.merge(profile);
        assert_eq!(
            config.performance.request_timeout_ms, 5000,
            "Parent's explicit timeout must survive merge of child without [performance]"
        );
        assert_eq!(config.performance.rate_limit_delay_ms, 100);
    }

    #[test]
    fn test_merge_performance_overrides_when_child_specifies() {
        // When child explicitly sets [performance], it should win.
        let mut config = create_basic_config();
        config.performance.request_timeout_ms = 5000;

        let profile = ConfigProfile {
            performance: Some(PerformanceConfig {
                request_timeout_ms: 15000,
                rate_limit_delay_ms: 500,
            }),
            ..Default::default()
        };

        config.merge(profile);
        assert_eq!(config.performance.request_timeout_ms, 15000);
        assert_eq!(config.performance.rate_limit_delay_ms, 500);
    }

    #[test]
    fn test_load_from_file_returns_none_for_missing_profile() {
        use std::io::Write;
        let tmp =
            std::env::temp_dir().join(format!("atlassian-cli-test-{}.toml", std::process::id()));
        let mut f = fs::File::create(&tmp).unwrap();
        writeln!(f, "[default]").unwrap();
        writeln!(f, "domain = \"test.atlassian.net\"").unwrap();
        drop(f);

        // Profile doesn't exist — should return Ok(None), not Err.
        let missing_profile = "work".to_string();
        let result = Config::load_from_file(&tmp, Some(&missing_profile));
        assert!(matches!(result, Ok(None)));

        // Default profile — should return Ok(Some(_)).
        let default = Config::load_from_file(&tmp, None);
        assert!(matches!(default, Ok(Some(_))));

        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_domain_normalization_not_needed_for_service_account() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::ServiceAccount {
                client_id: "cid".to_string(),
                client_secret: "secret".to_string(),
                cloud_id: Some("cloud-123".to_string()),
            }),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }
}
