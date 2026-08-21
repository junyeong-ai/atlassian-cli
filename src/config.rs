use crate::auth::{
    AuthConfig, AuthMethod, DEFAULT_OAUTH_REDIRECT_PORT, OAuthParams, default_oauth_scopes,
};
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

impl AuthResolver<'_> {
    /// Consumes the resolver. It's a one-shot builder — reusing it would be
    /// meaningless since env vars are read at resolve time.
    fn resolve(self) -> Result<Option<AuthConfig>> {
        // Step 1: determine the effective auth method.
        // Blank values follow the same "blank == absent" rule as every other
        // env/CLI source so that an empty `export ATLASSIAN_AUTH_METHOD=""`
        // (a common shell-rc / CI foot-gun) falls back to the file's method
        // instead of bailing with "Unknown auth method ''".
        let method = match (non_blank_env(ENV_AUTH_METHOD), self.file_auth) {
            (Some(m), _) => AuthMethod::parse(&m)
                .with_context(|| format!("Invalid {} environment variable", ENV_AUTH_METHOD))?,
            (None, Some(auth)) => auth.method(),
            (None, None) => return Ok(None),
        };

        // Step 2: zero out file fields that belong to a different variant —
        // they must NOT leak into the resolved config when the method differs.
        let file_for_method = self.file_auth.filter(|a| a.method() == method);

        Ok(Some(match method {
            AuthMethod::Basic => self.resolve_basic(file_for_method)?,
            AuthMethod::ScopedToken => self.resolve_scoped_token(file_for_method)?,
            AuthMethod::ServiceAccount => self.resolve_service_account(file_for_method)?,
            AuthMethod::OAuth => self.resolve_oauth(file_for_method)?,
        }))
    }

    fn resolve_basic(&self, file: Option<&AuthConfig>) -> Result<AuthConfig> {
        Ok(AuthConfig::Basic {
            email: pick(self.cli.email.as_deref(), ENV_EMAIL, file, |a| match a {
                AuthConfig::Basic { email, .. } => Some(email.as_str()),
                _ => None,
            })
            .with_context(|| {
                format!("email required for basic auth (set via --email, {ENV_EMAIL}, or config)")
            })?,
            token: pick(
                self.cli.token.as_deref(),
                ENV_API_TOKEN,
                file,
                |a| match a {
                    AuthConfig::Basic { token, .. } => Some(token.as_str()),
                    _ => None,
                },
            )
            .with_context(|| {
                format!("API token required (set via --token, {ENV_API_TOKEN}, or config)")
            })?,
        })
    }

    fn resolve_scoped_token(&self, file: Option<&AuthConfig>) -> Result<AuthConfig> {
        Ok(AuthConfig::ScopedToken {
            email: pick(self.cli.email.as_deref(), ENV_EMAIL, file, |a| match a {
                AuthConfig::ScopedToken { email, .. } => Some(email.as_str()),
                _ => None,
            })
            .with_context(|| {
                format!(
                    "email required for scoped_token auth (set via --email, {ENV_EMAIL}, or config)"
                )
            })?,
            token: pick(
                self.cli.token.as_deref(),
                ENV_API_TOKEN,
                file,
                |a| match a {
                    AuthConfig::ScopedToken { token, .. } => Some(token.as_str()),
                    _ => None,
                },
            )
            .with_context(|| {
                format!("API token required (set via --token, {ENV_API_TOKEN}, or config)")
            })?,
            cloud_id: pick(
                self.cli.cloud_id.as_deref(),
                ENV_CLOUD_ID,
                file,
                |a| match a {
                    AuthConfig::ScopedToken { cloud_id, .. } => cloud_id.as_deref(),
                    _ => None,
                },
            ),
        })
    }

    fn resolve_service_account(&self, file: Option<&AuthConfig>) -> Result<AuthConfig> {
        Ok(AuthConfig::ServiceAccount {
            client_id: pick(self.cli.client_id.as_deref(), ENV_CLIENT_ID, file, |a| {
                match a {
                    AuthConfig::ServiceAccount { client_id, .. } => Some(client_id.as_str()),
                    _ => None,
                }
            })
            .with_context(|| {
                format!(
                    "Service account client_id required (set via --client-id, {ENV_CLIENT_ID}, or config)"
                )
            })?,
            client_secret: pick(self.cli.client_secret.as_deref(), ENV_CLIENT_SECRET, file, |a| {
                match a {
                    AuthConfig::ServiceAccount { client_secret, .. } => Some(client_secret.as_str()),
                    _ => None,
                }
            })
            .with_context(|| {
                format!(
                    "Service account client_secret required (set via --client-secret, {ENV_CLIENT_SECRET}, or config)"
                )
            })?,
            cloud_id: pick(self.cli.cloud_id.as_deref(), ENV_CLOUD_ID, file, |a| {
                match a {
                    AuthConfig::ServiceAccount { cloud_id, .. } => cloud_id.as_deref(),
                    _ => None,
                }
            }),
        })
    }

    fn resolve_oauth(&self, file: Option<&AuthConfig>) -> Result<AuthConfig> {
        // Pull file-side OAuth-only fields (port + scopes); fall back to library
        // defaults from `auth.rs`.
        let (file_port, file_scopes) = file
            .and_then(|a| match a {
                AuthConfig::OAuth {
                    redirect_port,
                    scopes,
                    ..
                } => Some((*redirect_port, scopes.clone())),
                _ => None,
            })
            .unzip();
        Ok(AuthConfig::OAuth {
            client_id: pick(self.cli.client_id.as_deref(), ENV_CLIENT_ID, file, |a| {
                match a {
                    AuthConfig::OAuth { client_id, .. } => Some(client_id.as_str()),
                    _ => None,
                }
            })
            .with_context(|| {
                format!(
                    "OAuth client_id required (set via --client-id, {ENV_CLIENT_ID}, or config)"
                )
            })?,
            client_secret: pick(self.cli.client_secret.as_deref(), ENV_CLIENT_SECRET, file, |a| {
                match a {
                    AuthConfig::OAuth { client_secret, .. } => Some(client_secret.as_str()),
                    _ => None,
                }
            })
            .with_context(|| {
                format!(
                    "OAuth client_secret required (set via --client-secret, {ENV_CLIENT_SECRET}, or config)"
                )
            })?,
            redirect_port: file_port.unwrap_or(DEFAULT_OAUTH_REDIRECT_PORT),
            scopes: file_scopes.unwrap_or_else(default_oauth_scopes),
            cloud_id: pick(self.cli.cloud_id.as_deref(), ENV_CLOUD_ID, file, |a| {
                match a {
                    AuthConfig::OAuth { cloud_id, .. } => cloud_id.as_deref(),
                    _ => None,
                }
            }),
        })
    }
}

/// CLI > env > file precedence, returning the first non-blank source or `None`.
///
/// Blanks (empty / whitespace-only) are treated as **absent**, not as
/// "explicitly set to empty". Otherwise a shell with `export VAR=""` or a
/// CI environment injecting empty values would silently shadow valid config
/// — the kind of silent override that's the worst class of credential bug.
fn pick<F>(
    cli: Option<&str>,
    env_name: &str,
    file: Option<&AuthConfig>,
    from_file: F,
) -> Option<String>
where
    F: FnOnce(&AuthConfig) -> Option<&str>,
{
    non_blank(cli)
        .map(str::to_string)
        .or_else(|| non_blank_env(env_name))
        .or_else(|| {
            file.and_then(from_file)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
        })
}

fn non_blank(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.trim().is_empty())
}

fn non_blank_owned(s: String) -> Option<String> {
    if s.trim().is_empty() { None } else { Some(s) }
}

/// An env override, or `None` where there is nothing usable to override with.
///
/// `.ok()` folds `VarError::NotUnicode` in with `NotPresent` deliberately: a
/// value whose bytes are not UTF-8 cannot be any of the things these variables
/// hold, and reading it as unset lands on the same side as the blank-value
/// policy above — malformed input does not shadow the file.
fn non_blank_env(env_name: &str) -> Option<String> {
    std::env::var(env_name).ok().and_then(non_blank_owned)
}

/// Parse a comma-separated list, trimming each item and dropping blanks.
fn parse_csv_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// CLI flag/env var overrides passed to Config::load.
#[derive(Default)]
pub struct CliOverrides {
    pub domain: Option<String>,
    pub email: Option<String>,
    pub token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub cloud_id: Option<String>,
}

impl std::fmt::Debug for CliOverrides {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliOverrides")
            .field("domain", &self.domain)
            .field("email", &self.email)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("cloud_id", &self.cloud_id)
            .finish()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Profile name used to load this config. Runtime-only (not serialized).
    /// Keys per-profile OAuth token storage.
    #[serde(skip)]
    pub profile: String,

    /// Site domain (e.g. "company.atlassian.net").
    /// Required for Basic auth, and for Scoped token auth unless `cloud_id` is
    /// pinned. Ignored for Service account and OAuth, which address the site
    /// by `cloud_id`.
    pub domain: Option<String>,

    /// Authentication configuration (Basic, Scoped token, Service account, or OAuth).
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

/// Strip the scheme and trailing slash from a configured Atlassian domain and
/// validate it as a bare host under `.atlassian.net`.
///
/// Returns the cleaned host on success. Rejecting any character outside
/// `[A-Za-z0-9.-]` blocks every URL-structure injection vector — path
/// (`/`), query (`?`), fragment (`#`), userinfo (`@`), and port (`:`) — so a
/// value like `https://evil.com/foo.atlassian.net` cannot pass the suffix
/// check and then send Basic credentials to `evil.com` via
/// `BasicStrategy::build_url`. Single source of truth shared by
/// `Config::validate` and `BasicStrategy::new`.
pub(crate) fn validate_atlassian_domain(raw: &str) -> Result<String> {
    // Schemes and hostnames are case-insensitive (RFC 3986 §3.1, §3.2.2).
    // Normalise to lowercase so `HTTPS://Foo.ATLASSIAN.NET/` is accepted and
    // the returned host is canonical for use in request URLs.
    let lower = raw.to_lowercase();
    let host = lower
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');

    if host.is_empty() {
        bail!("Atlassian domain is empty");
    }

    // A real host is only ASCII alphanumerics, dots, and hyphens. Anything
    // else means the value carries URL structure (path/query/userinfo/port)
    // and must be rejected before it reaches a request URL.
    if !host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        bail!(
            "Invalid Atlassian domain: {} (must be a bare host like your-site.atlassian.net)",
            raw
        );
    }

    // Require a non-empty site label in front of `.atlassian.net` — the
    // suffix alone (`.atlassian.net`) or a bare match is not a real site.
    if !host.ends_with(".atlassian.net") || host.len() <= ".atlassian.net".len() {
        bail!(
            "Invalid Atlassian domain: {} (must end with .atlassian.net)",
            raw
        );
    }

    Ok(host.to_string())
}

/// Validate a `cloud_id` before it is interpolated into a proxy path
/// (`/ex/{service}/{cloud_id}{path}`). Rejecting anything outside
/// `[A-Za-z0-9-]` prevents a value containing `/`, `?`, or `#` from rewriting
/// the proxy path or query on `api.atlassian.com`.
///
/// **Every** cloud_id passes through here — pinned by the user or discovered
/// over the network. A discovered one is not exempt merely because it came
/// from an API: `scoped_token` reads it from the site host, so trusting it
/// would let that response steer the proxy path. Do not drop the validation
/// on the discovery branch as redundant; a test pins it.
pub(crate) fn validate_cloud_id(raw: &str) -> Result<()> {
    if raw.is_empty() {
        bail!("cloud_id is empty");
    }
    if !raw.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        bail!(
            "Invalid cloud_id: {} (expected an Atlassian site identifier — letters, digits, hyphens)",
            raw
        );
    }
    Ok(())
}

/// A TOML parse failure named by position and reason, never by content.
///
/// `toml::de::Error` renders the offending source line, and a config file is
/// exactly where the secrets are: a malformed `client_secret = "…" x` would put
/// that value into the single-line error object this CLI prints on stderr, and
/// from there into a shell history or a CI log. The line and column are derived
/// from the span instead, so the diagnosis keeps its location and loses only the
/// text.
fn parse_failure(path: &Path, content: &str, mut error: toml::de::Error) -> anyhow::Error {
    let position = error
        .span()
        .map(|span| {
            let before = &content[..span.start.min(content.len())];
            let line = before.matches('\n').count() + 1;
            let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            format!(" at line {line}, column {column}")
        })
        .unwrap_or_default();
    error.set_input(None);
    anyhow::anyhow!("Failed to parse config file {path:?}{position}: {error}")
}

impl Config {
    /// Extract OAuth flow parameters for this profile.
    /// Errors with an actionable message when the profile is not OAuth-configured.
    pub fn oauth_params(&self) -> Result<OAuthParams> {
        let profile = &self.profile;
        self.auth
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Profile '{profile}' has no [auth] section. Add `method = \"oauth\"` (see `config init`)."
                )
            })?
            .oauth_params(profile)
    }

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
        let mut config = Self {
            profile: profile.map(String::as_str).unwrap_or("default").to_string(),
            ..Self::default()
        };
        // Track whether the requested profile was found in any config file.
        // A profile must exist in at least one file to be usable.
        let mut profile_found = profile.is_none(); // "default" is always considered found

        // 1. Load global config
        if let Some(global_path) = Self::global_config_path()
            && crate::path_present(&global_path)?
        {
            tracing::debug!("Loading global config: {:?}", global_path);
            if let Some(profile_config) = Self::load_from_file(&global_path, profile)? {
                config.merge(profile_config);
                profile_found = true;
            }
        }

        // 2. Load project config
        if let Some(project_path) = Self::project_config_path()? {
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
        // Blank values are treated as absent — see `pick` for rationale.
        // Auth resolution is handled separately via AuthResolver at step 6.
        if let Some(val) = non_blank_env(ENV_DOMAIN) {
            config.domain = Some(val);
        }

        // List-style env vars: a non-empty value supplies the entire list.
        // A blank env var leaves the file-provided list intact (matches the
        // "blank == absent" rule above).
        if let Some(val) = non_blank_env("JIRA_PROJECTS_FILTER") {
            config.jira.projects_filter = parse_csv_list(&val);
        }

        if let Some(val) = non_blank_env("CONFLUENCE_SPACES_FILTER") {
            config.confluence.spaces_filter = parse_csv_list(&val);
        }

        if let Some(val) = non_blank_env("JIRA_SEARCH_DEFAULT_FIELDS") {
            config.jira.search_default_fields = Some(parse_csv_list(&val));
        }

        if let Some(val) = non_blank_env("JIRA_SEARCH_CUSTOM_FIELDS") {
            config.jira.search_custom_fields = parse_csv_list(&val);
        }

        if let Some(val) = non_blank_env("RESPONSE_EXCLUDE_FIELDS") {
            config.optimization.response_exclude_fields = Some(parse_csv_list(&val));
        }

        if let Some(val) = non_blank_env("REQUEST_TIMEOUT_MS") {
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
        //    Blank CLI value = unset; do not override.
        if let Some(d) = overrides.domain.as_deref().and_then(|s| non_blank(Some(s))) {
            config.domain = Some(d.to_string());
        }

        // 7. Validate
        if validate {
            config.validate()?;
        }

        Ok(config)
    }

    /// Profile names defined in a config file, sorted, with `default` first
    /// when present. Names only — no profile contents are exposed, so callers
    /// (e.g. `config list`) can enumerate without touching secrets.
    pub fn profile_names(path: &Path) -> Result<Vec<String>> {
        #[cfg(unix)]
        Self::check_permissions(path)?;

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        let config_file: ConfigFile =
            toml::from_str(&content).map_err(|e| parse_failure(path, &content, e))?;

        let mut names: Vec<String> = config_file.profiles.keys().cloned().collect();
        names.sort();
        names.insert(0, "default".to_string());
        Ok(names)
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

        let config_file: ConfigFile =
            toml::from_str(&content).map_err(|e| parse_failure(path, &content, e))?;

        match profile {
            Some(profile_name) => Ok(config_file.profiles.get(profile_name).cloned()),
            None => Ok(Some(config_file.default)),
        }
    }

    #[cfg(unix)]
    fn check_permissions(path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        // Reached for a path discovery found, which includes a link with
        // nothing at the other end — so this is where that is named.
        let metadata =
            fs::metadata(path).with_context(|| format!("Failed to read config file: {path:?}"))?;
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

                // Strict host-grammar validation — rejects path/userinfo/port
                // spoofs like `https://evil.com/foo.atlassian.net`.
                validate_atlassian_domain(domain)?;

                if !email.contains('@') {
                    bail!("Invalid email format: {}", email);
                }

                if token.is_empty() {
                    bail!("API token is empty");
                }
            }
            AuthConfig::ScopedToken {
                email,
                token,
                cloud_id,
            } => {
                if !email.contains('@') {
                    bail!("Invalid email format: {}", email);
                }
                if token.is_empty() {
                    bail!("API token is empty");
                }
                // The gateway path carries the cloud_id, so one must exist by
                // the time a request is built. Either it is pinned here — and
                // must not carry URL structure — or it is resolved from the
                // site host, which then has to be a real one.
                match cloud_id.as_deref() {
                    Some(cloud_id) => validate_cloud_id(cloud_id)?,
                    None => {
                        let domain = self.domain.as_ref().context(
                            "scoped_token auth needs a cloud_id, or a domain to resolve one from. Set via:\n\
                             1. --cloud-id flag or ATLASSIAN_CLOUD_ID env var\n\
                             2. --domain flag or ATLASSIAN_DOMAIN env var\n\
                             3. Config file: atlassian-cli config init",
                        )?;
                        validate_atlassian_domain(domain)?;
                    }
                }
            }
            AuthConfig::ServiceAccount {
                client_id,
                client_secret,
                cloud_id,
            } => {
                if client_id.is_empty() {
                    bail!("Service account client_id is empty");
                }
                if client_secret.is_empty() {
                    bail!("Service account client_secret is empty");
                }
                // A user-pinned cloud_id is interpolated into the proxy path,
                // so it must not carry URL structure. Auto-discovered IDs are
                // fetched from the API after this check and never flow here.
                if let Some(cloud_id) = cloud_id.as_deref() {
                    validate_cloud_id(cloud_id)?;
                }
            }
            AuthConfig::OAuth {
                client_id,
                client_secret,
                redirect_port,
                scopes,
                cloud_id,
            } => {
                if client_id.is_empty() {
                    bail!("OAuth client_id is empty");
                }
                if client_secret.is_empty() {
                    bail!("OAuth client_secret is empty");
                }
                if let Some(cloud_id) = cloud_id.as_deref() {
                    validate_cloud_id(cloud_id)?;
                }
                if *redirect_port == 0 {
                    bail!("OAuth redirect_port must be a non-zero TCP port (commonly 8976)");
                }
                if scopes.is_empty() {
                    bail!(
                        "OAuth scopes must not be empty. At minimum: ['read:jira-user', 'offline_access']"
                    );
                }
                if !scopes.iter().any(|s| s == "offline_access") {
                    tracing::warn!(
                        "OAuth scopes do not include `offline_access` — refresh tokens will not be issued, requiring re-login on every expiry."
                    );
                }
            }
        }

        if self.performance.request_timeout_ms < 100
            || self.performance.request_timeout_ms > 600_000
        {
            bail!("Request timeout must be between 100ms and 600000ms");
        }

        Ok(())
    }

    /// The name of the global config inside [`global_config_dir`](Self::global_config_dir).
    pub const GLOBAL_CONFIG_FILE: &'static str = "config.toml";

    /// The directory holding everything this tool stores for the user — the
    /// global config and, beside it, the fallback credentials file.
    ///
    /// Every path under it is derived from here rather than reassembled, so a
    /// caller cannot end up reading one directory while another writes a
    /// different one.
    pub fn global_config_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|home| Self::global_config_dir_in(&home))
    }

    /// The same directory relative to a given home, so a caller holding one can
    /// derive it without reassembling the layout.
    pub fn global_config_dir_in(home: &Path) -> PathBuf {
        home.join(".config").join("atlassian-cli")
    }

    pub fn global_config_path() -> Option<PathBuf> {
        Self::global_config_dir().map(|dir| dir.join(Self::GLOBAL_CONFIG_FILE))
    }

    /// The nearest project config at or above the working directory.
    ///
    /// Absence is concluded from `NotFound` alone. The walk continues upward
    /// past a candidate it does not find, so reading a file it merely could not
    /// stat as absent runs the command against a parent directory's site
    /// instead of the one the user put in this one.
    pub fn project_config_path() -> Result<Option<PathBuf>> {
        // A working directory that cannot be read is not a directory tree with
        // no project config in it — and answering `None` would run the command
        // against the global configuration instead.
        let current = std::env::current_dir()
            .context("Failed to read the working directory while looking for a project config")?;
        let mut dir = current.as_path();

        loop {
            for candidate in [
                dir.join(".atlassian.toml"),
                dir.join(".atlassian/config.toml"),
            ] {
                if crate::path_present(&candidate)? {
                    return Ok(Some(candidate));
                }
            }

            match dir.parent() {
                Some(parent) => dir = parent,
                None => return Ok(None),
            }
        }
    }

    pub fn init_config(global: bool) -> Result<PathBuf> {
        let path = if global {
            Self::global_config_path().context("Failed to determine global config path")?
        } else {
            PathBuf::from(".atlassian.toml")
        };

        if crate::path_present(&path)? {
            bail!("Config file already exists: {:?}", path);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let template = r#"[default]
# domain = "company.atlassian.net"  # Required for basic auth

# === Method 1: Basic auth (classic, unscoped API token) ===
# Identity: yourself. Audit logs show your name.
# Requests go to the site host, which accepts only an unscoped token.
# [default.auth]
# method = "basic"
# email = "user@example.com"
# token = "..."  # Prefer ATLASSIAN_API_TOKEN env var

# === Method 2: Scoped API token (token with scopes) ===
# Identity: yourself, limited to the scopes granted to the token.
# Requests go through api.atlassian.com, the only host that honours scopes.
# [default.auth]
# method = "scoped_token"
# email = "user@example.com"
# token = "..."  # Prefer ATLASSIAN_API_TOKEN env var
# cloud_id = "..."  # Optional, resolved from the domain above if omitted

# === Method 3: Service account (OAuth 2.0 client_credentials) ===
# Identity: a non-human service account principal.
# [default.auth]
# method = "service_account"
# client_id = "your-client-id"
# client_secret = "..."  # Prefer ATLASSIAN_CLIENT_SECRET env var
# cloud_id = "..."  # Optional, auto-discovered if omitted

# === Method 4: OAuth 2.0 (3LO — user-delegated) ===
# Identity: yourself (via interactive browser sign-in).
# Tokens stored in OS keychain (file fallback) and refreshed transparently.
# After configuring, run: atlassian-cli auth login
# [default.auth]
# method = "oauth"
# client_id = "your-oauth-app-client-id"
# client_secret = "..."  # Prefer ATLASSIAN_CLIENT_SECRET env var
# redirect_port = 8976   # MUST match the URI registered at developer.atlassian.com
# scopes = ["read:jira-user", "read:jira-work", "write:jira-work",
#           "read:confluence-content.all", "read:confluence-space.summary",
#           "write:confluence-content", "offline_access"]
# cloud_id = "..."  # Pin to one site if the user has access to many

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

# Additional profiles (multi-tenant / multi-method support)
# [work]
# domain = "work.atlassian.net"
# [work.auth]
# method = "basic"
# email = "me@work.com"
# token = "..."

# Classic and granular OAuth scopes CANNOT mix in one token (Atlassian rule),
# so use a separate profile per scope model and select it with --profile.
# `scopes` is a free list — put whatever your OAuth app grants. Classic
# `read:jira-work`/`write:jira-work` covers core Jira; Jira Software (agile:
# board/sprint/epic) and granular setups need their own scope strings, which
# you copy from your app's Permissions page at developer.atlassian.com.
# [agile]
# [agile.auth]
# method = "oauth"
# client_id = "..."
# scopes = [  # granular example — must be a COMPLETE set; a missing scope 401s that command
#   "read:issue:jira", "write:issue:jira", "read:jql:jira",
#   "read:board-scope:jira-software", "read:sprint:jira-software",
#   "write:sprint:jira-software", "read:epic:jira-software", "offline_access",
# ]
# Then: atlassian-cli --profile agile auth login   (each profile stores its own token)
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

    #[test]
    fn profile_names_lists_default_first_then_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[default]

[zeta.auth]
method = "basic"
email = "a@b.c"
token = "t"

[alpha]
domain = "x.atlassian.net"
"#,
        )
        .unwrap();

        assert_eq!(
            Config::profile_names(&path).unwrap(),
            vec!["default", "alpha", "zeta"]
        );
    }

    #[test]
    fn profile_names_reports_default_even_when_implicit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "[only.auth]\nmethod = \"basic\"\nemail = \"a@b.c\"\ntoken = \"t\"\n",
        )
        .unwrap();

        assert_eq!(
            Config::profile_names(&path).unwrap(),
            vec!["default", "only"]
        );
    }

    #[test]
    fn profile_names_errors_on_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "not [valid toml").unwrap();
        assert!(Config::profile_names(&path).is_err());
    }

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
        // anyhow chains: outer "Invalid ATLASSIAN_AUTH_METHOD" + inner "Unknown auth method".
        let chain = format!("{:#}", err);
        assert!(
            chain.contains("Unknown auth method") || chain.contains("ATLASSIAN_AUTH_METHOD"),
            "chain was: {}",
            chain
        );
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

    #[test]
    fn test_resolver_oauth_method_from_env() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        unsafe {
            std::env::set_var(ENV_AUTH_METHOD, "oauth");
            std::env::set_var(ENV_CLIENT_ID, "oauth-cid");
            std::env::set_var(ENV_CLIENT_SECRET, "oauth-sec");
        }
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: None,
            cli: &overrides,
        };
        let result = r.resolve().unwrap().unwrap();
        match result {
            AuthConfig::OAuth {
                client_id,
                client_secret,
                redirect_port,
                scopes,
                ..
            } => {
                assert_eq!(client_id, "oauth-cid");
                assert_eq!(client_secret, "oauth-sec");
                assert_eq!(redirect_port, 8976, "default port when not pinned");
                assert!(scopes.contains(&"offline_access".to_string()));
            }
            _ => panic!("expected OAuth"),
        }
        clear_auth_env();
    }

    #[test]
    fn test_resolver_oauth_inherits_port_and_scopes_from_file() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        let file = AuthConfig::OAuth {
            client_id: "f-cid".into(),
            client_secret: "f-sec".into(),
            redirect_port: 12345,
            scopes: vec!["read:jira-work".into(), "offline_access".into()],
            cloud_id: Some("pinned-cloud".into()),
        };
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        };
        let result = r.resolve().unwrap().unwrap();
        match result {
            AuthConfig::OAuth {
                client_id,
                redirect_port,
                scopes,
                cloud_id,
                ..
            } => {
                assert_eq!(client_id, "f-cid");
                assert_eq!(redirect_port, 12345);
                assert_eq!(scopes, vec!["read:jira-work", "offline_access"]);
                assert_eq!(cloud_id.as_deref(), Some("pinned-cloud"));
            }
            _ => panic!("expected OAuth"),
        }
    }

    #[test]
    fn test_validate_oauth_rejects_empty_secrets() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::OAuth {
                client_id: String::new(),
                client_secret: "s".into(),
                redirect_port: 8976,
                scopes: vec!["offline_access".into()],
                cloud_id: None,
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = Config {
            domain: None,
            auth: Some(AuthConfig::OAuth {
                client_id: "c".into(),
                client_secret: String::new(),
                redirect_port: 8976,
                scopes: vec!["offline_access".into()],
                cloud_id: None,
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_oauth_rejects_zero_port_and_empty_scopes() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::OAuth {
                client_id: "c".into(),
                client_secret: "s".into(),
                redirect_port: 0,
                scopes: vec!["offline_access".into()],
                cloud_id: None,
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = Config {
            domain: None,
            auth: Some(AuthConfig::OAuth {
                client_id: "c".into(),
                client_secret: "s".into(),
                redirect_port: 8976,
                scopes: vec![],
                cloud_id: None,
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_oauth_accepts_well_formed() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::OAuth {
                client_id: "c".into(),
                client_secret: "s".into(),
                redirect_port: 8976,
                scopes: vec!["read:jira-work".into(), "offline_access".into()],
                cloud_id: Some("cloud-1".into()),
            }),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn blank_auth_method_env_falls_back_to_file() {
        // Regression guard for the consistency gap: every other blank env
        // already falls back to the file value; the method selector must too.
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        unsafe { std::env::set_var(ENV_AUTH_METHOD, "") };
        let file = AuthConfig::Basic {
            email: "file@x.com".into(),
            token: "file-tk".into(),
        };
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        };
        match r.resolve().unwrap().unwrap() {
            AuthConfig::Basic { email, .. } => assert_eq!(email, "file@x.com"),
            _ => panic!("expected Basic from file when env is blank"),
        }
        clear_auth_env();
    }

    #[test]
    fn blank_env_does_not_override_file_value() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        // Common foot-gun: shell rc has `export ATLASSIAN_EMAIL=""` (CI also
        // injects blanks). The file value must survive.
        unsafe { std::env::set_var(ENV_EMAIL, "") };
        let file = AuthConfig::Basic {
            email: "file@x.com".into(),
            token: "file-tk".into(),
        };
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        };
        let resolved = r.resolve().unwrap().unwrap();
        match resolved {
            AuthConfig::Basic { email, .. } => {
                assert_eq!(email, "file@x.com", "blank env must not override file");
            }
            _ => panic!("expected Basic"),
        }
        clear_auth_env();
    }

    #[test]
    fn whitespace_only_env_does_not_override_file_value() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        unsafe { std::env::set_var(ENV_API_TOKEN, "   \t  ") };
        let file = AuthConfig::Basic {
            email: "u@x.com".into(),
            token: "file-tk".into(),
        };
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        };
        match r.resolve().unwrap().unwrap() {
            AuthConfig::Basic { token, .. } => assert_eq!(token, "file-tk"),
            _ => panic!("expected Basic"),
        }
        clear_auth_env();
    }

    #[test]
    fn blank_cli_does_not_override_file_value() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        let file = AuthConfig::Basic {
            email: "file@x.com".into(),
            token: "file-tk".into(),
        };
        let overrides = CliOverrides {
            email: Some("".into()), // `--email ""` from a CI variable
            ..Default::default()
        };
        let r = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        };
        match r.resolve().unwrap().unwrap() {
            AuthConfig::Basic { email, .. } => assert_eq!(email, "file@x.com"),
            _ => panic!("expected Basic"),
        }
    }

    #[test]
    fn non_blank_env_still_overrides() {
        // Regression guard: legitimate non-blank env vars must still win.
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        unsafe { std::env::set_var(ENV_EMAIL, "env@x.com") };
        let file = AuthConfig::Basic {
            email: "file@x.com".into(),
            token: "tk".into(),
        };
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        };
        match r.resolve().unwrap().unwrap() {
            AuthConfig::Basic { email, .. } => assert_eq!(email, "env@x.com"),
            _ => panic!("expected Basic"),
        }
        clear_auth_env();
    }

    #[test]
    fn cli_overrides_debug_redacts_secrets() {
        let o = CliOverrides {
            token: Some("BASIC-LEAK".into()),
            client_secret: Some("OAUTH-LEAK".into()),
            ..Default::default()
        };
        let rendered = format!("{:?}", o);
        assert!(!rendered.contains("BASIC-LEAK"), "leaked: {rendered}");
        assert!(!rendered.contains("OAUTH-LEAK"), "leaked: {rendered}");
        assert!(
            rendered.contains("<redacted>"),
            "expected redaction marker: {rendered}"
        );
    }

    #[test]
    fn test_invalid_method_error_lists_all_three() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        unsafe { std::env::set_var(ENV_AUTH_METHOD, "saml") };
        let overrides = CliOverrides::default();
        let r = AuthResolver {
            file_auth: None,
            cli: &overrides,
        };
        // Use the chained representation so we see both the resolver's outer
        // context and AuthMethod::parse's inner suggestion list.
        let chain = format!("{:#}", r.resolve().unwrap_err());
        assert!(chain.contains("basic"), "{}", chain);
        assert!(chain.contains("service_account"), "{}", chain);
        assert!(chain.contains("oauth"), "{}", chain);
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
    fn validate_atlassian_domain_rejects_path_prefixed_spoof() {
        // The critical case: a value carrying a path whose tail looks like a
        // valid host. Suffix-only matching would accept this and then send
        // Basic credentials to evil.com.
        assert!(validate_atlassian_domain("https://evil.com/foo.atlassian.net").is_err());
        assert!(validate_atlassian_domain("evil.com/foo.atlassian.net").is_err());
        assert!(validate_atlassian_domain("foo.atlassian.net@evil.com").is_err());
        assert!(validate_atlassian_domain("foo.atlassian.net:8080").is_err());
        assert!(validate_atlassian_domain(".atlassian.net").is_err());
        assert!(validate_atlassian_domain("").is_err());
    }

    #[test]
    fn validate_atlassian_domain_accepts_clean_host() {
        assert_eq!(
            validate_atlassian_domain("https://my-site.atlassian.net/").unwrap(),
            "my-site.atlassian.net"
        );
        assert_eq!(
            validate_atlassian_domain("my-site.atlassian.net").unwrap(),
            "my-site.atlassian.net"
        );
    }

    #[test]
    fn validate_atlassian_domain_is_case_insensitive() {
        // Schemes and hostnames are case-insensitive; the validator must
        // accept mixed case and normalize the returned host to lowercase.
        assert_eq!(
            validate_atlassian_domain("HTTPS://Foo.ATLASSIAN.NET/").unwrap(),
            "foo.atlassian.net"
        );
        assert_eq!(
            validate_atlassian_domain("Foo.Atlassian.Net").unwrap(),
            "foo.atlassian.net"
        );
        assert_eq!(
            validate_atlassian_domain("HTTP://bar.atlassian.net").unwrap(),
            "bar.atlassian.net"
        );
    }

    #[test]
    fn validate_cloud_id_rejects_path_injection() {
        assert!(validate_cloud_id("abc/../../evil").is_err());
        assert!(validate_cloud_id("abc?x=1").is_err());
        assert!(validate_cloud_id("abc#frag").is_err());
        assert!(validate_cloud_id("").is_err());
        assert!(validate_cloud_id("has space").is_err());
    }

    #[test]
    fn validate_cloud_id_accepts_uuid_like() {
        assert!(validate_cloud_id("11111111-2222-3333-4444-555555555555").is_ok());
        assert!(validate_cloud_id("abc123DEF").is_ok());
    }

    #[test]
    fn validate_rejects_spoofed_cloud_id_for_service_account() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::ServiceAccount {
                client_id: "id".to_string(),
                client_secret: "secret".to_string(),
                cloud_id: Some("abc/evil?x=1".to_string()),
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_spoofed_cloud_id_for_scoped_token() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::ScopedToken {
                email: "u@x.com".to_string(),
                token: "tk".to_string(),
                cloud_id: Some("abc/evil?x=1".to_string()),
            }),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    /// Without a cloud_id there is nothing to build a gateway path from, so
    /// the domain it would be resolved from has to be present and real.
    #[test]
    fn validate_scoped_token_needs_a_cloud_id_or_a_domain() {
        let bare = Config {
            domain: None,
            auth: Some(AuthConfig::ScopedToken {
                email: "u@x.com".to_string(),
                token: "tk".to_string(),
                cloud_id: None,
            }),
            ..Default::default()
        };
        let err = bare.validate().unwrap_err().to_string();
        assert!(err.contains("cloud_id"), "{err}");

        let spoofed = Config {
            domain: Some("https://evil.com/foo.atlassian.net".to_string()),
            ..bare.clone()
        };
        assert!(spoofed.validate().is_err());

        let resolvable = Config {
            domain: Some("test.atlassian.net".to_string()),
            ..bare
        };
        assert!(resolvable.validate().is_ok());
    }

    /// A pinned cloud_id is self-sufficient: the gateway host is a constant,
    /// so no site domain is involved in any request.
    #[test]
    fn validate_scoped_token_with_a_pinned_cloud_id_needs_no_domain() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::ScopedToken {
                email: "u@x.com".to_string(),
                token: "tk".to_string(),
                cloud_id: Some("11111111-2222-3333-4444-555555555555".to_string()),
            }),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_resolver_scoped_token_method_switch_drops_file_fields() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_auth_env();
        unsafe { std::env::set_var(ENV_AUTH_METHOD, "scoped_token") };
        unsafe { std::env::set_var(ENV_CLOUD_ID, "env-cloud") };
        let file = AuthConfig::Basic {
            email: "file@x.com".into(),
            token: "file-tk".into(),
        };
        let overrides = CliOverrides {
            email: Some("new@user.com".into()),
            token: Some("new-tk".into()),
            ..Default::default()
        };
        let result = AuthResolver {
            file_auth: Some(&file),
            cli: &overrides,
        }
        .resolve()
        .unwrap()
        .unwrap();
        clear_auth_env();
        match result {
            AuthConfig::ScopedToken {
                email,
                token,
                cloud_id,
            } => {
                assert_eq!(email, "new@user.com");
                assert_eq!(token, "new-tk");
                assert_eq!(cloud_id.as_deref(), Some("env-cloud"));
            }
            _ => panic!("method switch should yield ScopedToken"),
        }
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

        config.performance.request_timeout_ms = 600_000;
        assert!(config.validate().is_ok());

        config.performance.request_timeout_ms = 600_001;
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
    /// `.atlassian/config.toml` under a regular file named `.atlassian`
    /// answers `NotADirectory`, and the walk passes through every ancestor up
    /// to `/` — so reading that as a failure lets any such file, anyone's, stop
    /// every command.
    #[test]
    fn a_config_dir_that_is_a_plain_file_holds_no_config() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join(".atlassian");
        std::fs::write(&blocker, "not a directory").unwrap();

        assert!(!crate::path_present(&blocker.join("config.toml")).unwrap());
    }

    /// A config file is where the secrets are, so a parse failure must name
    /// where it is and not what is there — `toml`'s own rendering quotes the
    /// offending line, which would put a malformed secret into the error object
    /// this CLI prints on stderr.
    #[test]
    fn a_parse_failure_names_the_place_and_not_the_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[default.auth]\nclient_secret = \"supersecret\" trailing\n",
        )
        .unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();

        let err = format!("{:#}", Config::profile_names(&path).unwrap_err());
        assert!(
            !err.contains("supersecret"),
            "the secret reached the error: {err}"
        );
        assert!(
            err.contains("line 2"),
            "the failure lost its position: {err}"
        );
    }

    /// The walk continues upward past a candidate it does not find, so reading
    /// one it merely could not stat as absent runs the command against a parent
    /// directory's site instead of the one the user put in this one.
    #[cfg(unix)]
    #[test]
    fn a_dangling_project_config_link_is_something_at_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join(".atlassian.toml");
        std::fs::write(&candidate, "").unwrap();
        assert!(crate::path_present(&candidate).unwrap());

        let missing = dir.path().join("nothing.toml");
        assert!(!crate::path_present(&missing).unwrap());

        // A dangling link is something at the path, not nothing.
        let link = dir.path().join("dangling.toml");
        std::os::unix::fs::symlink(dir.path().join("gone"), &link).unwrap();
        assert!(crate::path_present(&link).unwrap());
    }
}
