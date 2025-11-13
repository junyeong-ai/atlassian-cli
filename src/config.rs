use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(skip)]
    pub domain: Option<String>,
    #[serde(skip)]
    pub email: Option<String>,
    #[serde(skip)]
    pub token: Option<String>,

    #[serde(default)]
    pub jira: JiraConfig,

    #[serde(default)]
    pub confluence: ConfluenceConfig,

    #[serde(default)]
    pub performance: PerformanceConfig,

    #[serde(default)]
    pub optimization: OptimizationConfig,

    #[serde(skip)]
    pub(crate) base_url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JiraConfig {
    #[serde(default)]
    pub projects_filter: Vec<String>,

    pub search_default_fields: Option<Vec<String>>,

    #[serde(default)]
    pub search_custom_fields: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfluenceConfig {
    #[serde(default)]
    pub spaces_filter: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    #[serde(default = "default_timeout")]
    pub request_timeout_ms: u64,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            request_timeout_ms: default_timeout(),
        }
    }
}

fn default_timeout() -> u64 {
    30000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
struct ConfigProfile {
    domain: Option<String>,
    email: Option<String>,
    token: Option<String>,

    #[serde(default)]
    jira: JiraConfig,

    #[serde(default)]
    confluence: ConfluenceConfig,

    #[serde(default)]
    performance: PerformanceConfig,

    #[serde(default)]
    optimization: OptimizationConfig,
}

impl Config {
    pub fn load(
        config_path: Option<&PathBuf>,
        profile: Option<&String>,
        domain: Option<&String>,
        email: Option<&String>,
        token: Option<&String>,
    ) -> Result<Self> {
        let mut config = Self::default();

        // 1. Load global config
        if let Some(global_path) = Self::global_config_path()
            && global_path.exists()
        {
            tracing::debug!("Loading global config: {:?}", global_path);
            let profile_config = Self::load_from_file(&global_path, profile)?;
            config.merge(profile_config);
        }

        // 2. Load project config
        if let Some(project_path) = Self::project_config_path() {
            tracing::debug!("Loading project config: {:?}", project_path);
            let profile_config = Self::load_from_file(&project_path, profile)?;
            config.merge(profile_config);
        }

        // 3. Load custom config file
        if let Some(path) = config_path {
            tracing::debug!("Loading custom config: {:?}", path);
            let profile_config = Self::load_from_file(path, profile)?;
            config.merge(profile_config);
        }

        // 4. Environment variables override
        if let Ok(val) = std::env::var("ATLASSIAN_DOMAIN") {
            config.domain = Some(val);
        }
        if let Ok(val) = std::env::var("ATLASSIAN_EMAIL") {
            config.email = Some(val);
        }
        if let Ok(val) = std::env::var("ATLASSIAN_API_TOKEN") {
            config.token = Some(val);
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

        // 5. CLI flags override (highest priority)
        if let Some(val) = domain {
            config.domain = Some(val.clone());
        }
        if let Some(val) = email {
            config.email = Some(val.clone());
        }
        if let Some(val) = token {
            config.token = Some(val.clone());
        }

        // 6. Validate and normalize
        config.validate()?;
        config.normalize_base_url();

        Ok(config)
    }

    fn load_from_file(path: &Path, profile: Option<&String>) -> Result<ConfigProfile> {
        #[cfg(unix)]
        Self::check_permissions(path)?;

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        let config_file: ConfigFile = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {:?}", path))?;

        if let Some(profile_name) = profile {
            config_file
                .profiles
                .get(profile_name)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("Profile '{}' not found in {:?}", profile_name, path)
                })
        } else {
            Ok(config_file.default)
        }
    }

    #[cfg(unix)]
    fn check_permissions(path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::metadata(path)?;
        let permissions = metadata.permissions();
        let mode = permissions.mode();

        // Check if file is readable by group or others (0o077)
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
        if other.email.is_some() {
            self.email = other.email;
        }
        if other.token.is_some() {
            self.token = other.token;
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

        self.performance.request_timeout_ms = other.performance.request_timeout_ms;

        if other.optimization.response_exclude_fields.is_some() {
            self.optimization.response_exclude_fields = other.optimization.response_exclude_fields;
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.domain.is_none() {
            anyhow::bail!(
                "ATLASSIAN_DOMAIN not configured. Set via:\n\
                 1. --domain flag\n\
                 2. ATLASSIAN_DOMAIN env var\n\
                 3. Config file: atlassian config init"
            );
        }

        if self.email.is_none() {
            anyhow::bail!(
                "ATLASSIAN_EMAIL not configured. Set via:\n\
                 1. --email flag\n\
                 2. ATLASSIAN_EMAIL env var\n\
                 3. Config file"
            );
        }

        if self.token.is_none() {
            anyhow::bail!(
                "ATLASSIAN_API_TOKEN not configured. Set via:\n\
                 1. --token flag\n\
                 2. ATLASSIAN_API_TOKEN env var\n\
                 3. NOT recommended: config file (use env var instead)"
            );
        }

        let domain = self.domain.as_ref().unwrap();
        let clean_domain = domain
            .trim_start_matches("https://")
            .trim_start_matches("http://");

        if !clean_domain.contains(".atlassian.net") {
            anyhow::bail!("Invalid Atlassian domain format: {}", domain);
        }

        let email = self.email.as_ref().unwrap();
        if !email.contains('@') {
            anyhow::bail!("Invalid email format: {}", email);
        }

        if self.performance.request_timeout_ms < 100 || self.performance.request_timeout_ms > 60000
        {
            anyhow::bail!("Request timeout must be between 100ms and 60000ms");
        }

        Ok(())
    }

    fn normalize_base_url(&mut self) {
        let domain = self.domain.as_ref().unwrap();
        self.base_url = if domain.starts_with("https://") {
            domain.clone()
        } else if domain.starts_with("http://") {
            domain.replace("http://", "https://")
        } else {
            format!("https://{}", domain)
        };
    }

    #[inline]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[inline]
    pub fn domain(&self) -> &str {
        self.domain.as_ref().unwrap()
    }

    #[inline]
    pub fn email(&self) -> &str {
        self.email.as_ref().unwrap()
    }

    #[inline]
    pub fn token(&self) -> &str {
        self.token.as_ref().unwrap()
    }

    pub fn global_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("atlassian/config.toml"))
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
            anyhow::bail!("Config file already exists: {:?}", path);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let template = r#"[default]
domain = "company.atlassian.net"
email = "user@example.com"
# token = "..." # NOT recommended, use ATLASSIAN_API_TOKEN env var instead

[default.jira]
projects_filter = []
# search_default_fields = ["key", "summary", "status", "assignee"]
# search_custom_fields = ["customfield_10015"]

[default.confluence]
spaces_filter = []

[default.performance]
request_timeout_ms = 30000

# [default.optimization]
# response_exclude_fields = ["avatarUrls", "iconUrl"]

# Additional profiles (multi-tenant support)
# [work]
# domain = "work.atlassian.net"
# email = "me@work.com"

# [personal]
# domain = "personal.atlassian.net"
# email = "me@personal.com"
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

    fn create_test_config() -> Config {
        let mut config = Config::default();
        config.domain = Some("test.atlassian.net".to_string());
        config.email = Some("test@example.com".to_string());
        config.token = Some("token123".to_string());
        config.normalize_base_url();
        config
    }

    #[test]
    fn test_config_validation() {
        let config = create_test_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_domain_normalization() {
        let mut config = create_test_config();
        assert_eq!(config.base_url(), "https://test.atlassian.net");

        config.domain = Some("http://test.atlassian.net".to_string());
        config.normalize_base_url();
        assert_eq!(config.base_url(), "https://test.atlassian.net");

        config.domain = Some("https://test.atlassian.net".to_string());
        config.normalize_base_url();
        assert_eq!(config.base_url(), "https://test.atlassian.net");
    }

    #[test]
    fn test_missing_domain_fails() {
        let mut config = create_test_config();
        config.domain = None;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_domain_fails() {
        let mut config = create_test_config();
        config.domain = Some("invalid-domain.com".to_string());
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_email_fails() {
        let mut config = create_test_config();
        config.email = Some("invalid-email".to_string());
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_timeout_bounds() {
        let mut config = create_test_config();

        config.performance.request_timeout_ms = 50;
        assert!(config.validate().is_err());

        config.performance.request_timeout_ms = 100;
        assert!(config.validate().is_ok());

        config.performance.request_timeout_ms = 60000;
        assert!(config.validate().is_ok());

        config.performance.request_timeout_ms = 60001;
        assert!(config.validate().is_err());
    }
}
