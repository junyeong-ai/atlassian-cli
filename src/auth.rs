//! Authentication: serde-friendly `AuthConfig` enum + runtime `AuthStrategy` trait.
//!
//! Two complementary layers:
//!
//! - [`AuthConfig`] mirrors the on-disk TOML. Four variants: [`Basic`](AuthConfig::Basic)
//!   (classic API token, sent to the site host), [`ScopedToken`](AuthConfig::ScopedToken)
//!   (API token with scopes, sent through the gateway),
//!   [`ServiceAccount`](AuthConfig::ServiceAccount) (OAuth 2.0 client_credentials,
//!   non-human principal), and [`OAuth`](AuthConfig::OAuth) (3LO Authorization
//!   Code + PKCE, user-delegated).
//! - [`AuthStrategy`] is the runtime contract — each variant resolves to a
//!   `Box<dyn AuthStrategy>` that produces auth headers, builds URLs, and
//!   probes identity. `ApiClient` holds an `Arc<dyn AuthStrategy>` and never
//!   matches on the variant.
//!
//! Adding a method is one module + one enum variant — no `ApiClient` or
//! call-site changes.

use anyhow::Result;
use serde::{Deserialize, Serialize};

mod basic;
pub mod oauth;
mod scoped_token;
mod service_account;
mod strategy;

pub use basic::BasicStrategy;
pub use oauth::{
    KeyringEnumeration, LoadedTokens, LoginOutcome, OAuthParams, OAuthStrategy, SiteInfo,
    StoredProfiles, TokenStorageBackend, TokenStore, credentials_file, stored_profiles,
};
pub use scoped_token::ScopedTokenStrategy;
pub use service_account::ServiceAccountStrategy;
pub use strategy::{AuthStrategy, Identity};

// ---------------------------------------------------------------------------
// Shared constants — single source of truth for OAuth defaults and the
// token-lifecycle parameters that govern both service_account and 3LO flows.
// ---------------------------------------------------------------------------

/// Default loopback port for the OAuth redirect URI.
/// Must match what is registered at developer.atlassian.com.
pub const DEFAULT_OAUTH_REDIRECT_PORT: u16 = 8976;

/// Default OAuth scope set. Covers read+write on Jira; Confluence scopes are
/// excluded because they require a separate OAuth app entitlement at
/// developer.atlassian.com. Add them in your config if your app has them.
/// `offline_access` is REQUIRED for refresh tokens — keep it.
pub const DEFAULT_OAUTH_SCOPES: &[&str] = &[
    "read:jira-user",
    "read:jira-work",
    "write:jira-work",
    "offline_access",
];

pub fn default_oauth_scopes() -> Vec<String> {
    DEFAULT_OAUTH_SCOPES
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

fn default_redirect_port() -> u16 {
    DEFAULT_OAUTH_REDIRECT_PORT
}

/// Refresh access tokens this many seconds before their official expiry.
/// Absorbs network latency and protects long pagination runs. Shared by
/// every OAuth-style strategy (service_account, oauth).
pub(crate) const TOKEN_REFRESH_BUFFER_SECS: u64 = 300;

/// Fallback access-token lifetime when the token endpoint omits `expires_in`.
/// Atlassian always populates it, but the OAuth 2.0 spec marks it optional —
/// defensively use a conservative 1 hour.
pub(crate) const DEFAULT_TOKEN_LIFETIME_SECS: u64 = 3600;

// ---------------------------------------------------------------------------
// AuthMethod — type-safe method tag.
// ---------------------------------------------------------------------------

/// Stable identifier for each auth method. Used by `AuthStrategy::method()`,
/// `AuthResolver`, and CLI output. The lowercase string form (`as_str`) is
/// what appears in TOML `method = "..."` and the `ATLASSIAN_AUTH_METHOD` env.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthMethod {
    Basic,
    ScopedToken,
    ServiceAccount,
    OAuth,
}

impl AuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthMethod::Basic => "basic",
            AuthMethod::ScopedToken => "scoped_token",
            AuthMethod::ServiceAccount => "service_account",
            AuthMethod::OAuth => "oauth",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "basic" => Ok(AuthMethod::Basic),
            "scoped_token" => Ok(AuthMethod::ScopedToken),
            "service_account" => Ok(AuthMethod::ServiceAccount),
            "oauth" => Ok(AuthMethod::OAuth),
            other => anyhow::bail!(
                "Unknown auth method '{}'. Use 'basic', 'scoped_token', 'service_account', or 'oauth'",
                other
            ),
        }
    }
}

impl std::fmt::Display for AuthMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// AuthConfig — serde-tagged enum mirroring TOML.
// ---------------------------------------------------------------------------

/// Authentication configuration.
///
/// Selected explicitly via the `method` field (no heuristic auto-detection).
///
/// TOML examples:
/// ```toml
/// [default.auth]
/// method = "basic"
/// email = "user@example.com"
/// token = "classic-api-token"
///
/// [default.auth]
/// method = "scoped_token"
/// email = "user@example.com"
/// token = "api-token-with-scopes"
/// # cloud_id resolves from the site domain when omitted
///
/// [default.auth]
/// method = "service_account"
/// client_id = "your-client-id"
/// client_secret = "your-secret"
///
/// [default.auth]
/// method = "oauth"
/// client_id = "your-oauth-app-client-id"
/// client_secret = "your-secret"
/// # redirect_port and scopes default to sensible values; override only if needed
/// ```
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "lowercase", deny_unknown_fields)]
pub enum AuthConfig {
    Basic {
        #[serde(default)]
        email: String,
        #[serde(default, skip_serializing)]
        token: String,
    },
    /// API token carrying scopes. Same credential shape as `Basic`, but the
    /// gateway is the only host that honours it, so it needs a `cloud_id`.
    #[serde(rename = "scoped_token")]
    ScopedToken {
        #[serde(default)]
        email: String,
        #[serde(default, skip_serializing)]
        token: String,
        /// Resolved from the site domain when None.
        cloud_id: Option<String>,
    },
    #[serde(rename = "service_account")]
    ServiceAccount {
        #[serde(default)]
        client_id: String,
        #[serde(default, skip_serializing)]
        client_secret: String,
        /// Auto-discovered via accessible-resources API when None.
        cloud_id: Option<String>,
    },
    /// OAuth 2.0 (3LO) — user-delegated access via Authorization Code + PKCE.
    /// Tokens are stored persistently (keyring + 0600 file fallback) and
    /// refreshed transparently. The user authenticates once via
    /// `atlassian-cli auth login`.
    OAuth {
        #[serde(default)]
        client_id: String,
        #[serde(default, skip_serializing)]
        client_secret: String,
        /// Loopback redirect port. MUST match the URI registered in the
        /// Atlassian developer console.
        #[serde(default = "default_redirect_port")]
        redirect_port: u16,
        /// Requested OAuth scopes. `offline_access` is required for refresh.
        #[serde(default = "default_oauth_scopes")]
        scopes: Vec<String>,
        /// Pin to one Atlassian site when the user has access to many.
        cloud_id: Option<String>,
    },
}

/// Sentinel rendered in `Debug` output in place of secret fields. Centralised
/// so log scrapers / tests can match on a stable token.
const REDACTED: &str = "<redacted>";

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthConfig::Basic { email, .. } => f
                .debug_struct("Basic")
                .field("email", email)
                .field("token", &REDACTED)
                .finish(),
            AuthConfig::ScopedToken {
                email, cloud_id, ..
            } => f
                .debug_struct("ScopedToken")
                .field("email", email)
                .field("token", &REDACTED)
                .field("cloud_id", cloud_id)
                .finish(),
            AuthConfig::ServiceAccount {
                client_id,
                cloud_id,
                ..
            } => f
                .debug_struct("ServiceAccount")
                .field("client_id", client_id)
                .field("client_secret", &REDACTED)
                .field("cloud_id", cloud_id)
                .finish(),
            AuthConfig::OAuth {
                client_id,
                redirect_port,
                scopes,
                cloud_id,
                ..
            } => f
                .debug_struct("OAuth")
                .field("client_id", client_id)
                .field("client_secret", &REDACTED)
                .field("redirect_port", redirect_port)
                .field("scopes", scopes)
                .field("cloud_id", cloud_id)
                .finish(),
        }
    }
}

impl AuthConfig {
    /// The method tag for this configuration.
    pub fn method(&self) -> AuthMethod {
        match self {
            AuthConfig::Basic { .. } => AuthMethod::Basic,
            AuthConfig::ScopedToken { .. } => AuthMethod::ScopedToken,
            AuthConfig::ServiceAccount { .. } => AuthMethod::ServiceAccount,
            AuthConfig::OAuth { .. } => AuthMethod::OAuth,
        }
    }

    /// Extract the OAuth parameters from this config. Errors with an actionable
    /// message if the variant is not `OAuth` or required fields are empty.
    /// Used by every `auth` subcommand so error wording stays uniform.
    pub fn oauth_params(&self, profile: &str) -> Result<OAuthParams> {
        match self {
            AuthConfig::OAuth {
                client_id,
                client_secret,
                redirect_port,
                scopes,
                cloud_id,
            } => {
                if client_id.is_empty() {
                    anyhow::bail!(
                        "OAuth client_id is required (set in [{profile}.auth] or via ATLASSIAN_CLIENT_ID)"
                    );
                }
                if client_secret.is_empty() {
                    anyhow::bail!(
                        "OAuth client_secret is required (set in [{profile}.auth] or via ATLASSIAN_CLIENT_SECRET)"
                    );
                }
                Ok(OAuthParams {
                    client_id: client_id.clone(),
                    client_secret: client_secret.clone(),
                    redirect_port: *redirect_port,
                    scopes: scopes.clone(),
                    cloud_id: cloud_id.clone(),
                })
            }
            other => anyhow::bail!(
                "Profile '{profile}' has auth method '{}', not 'oauth'. The `auth` subcommand is only for OAuth profiles.",
                other.method()
            ),
        }
    }

    /// Resolve into a runtime strategy. May fetch tokens / discover cloud_id
    /// (service_account) or load persisted tokens (oauth).
    ///
    /// `profile` keys per-profile OAuth token storage.
    pub async fn into_strategy(
        self,
        domain: Option<&str>,
        profile: &str,
        http: &reqwest::Client,
    ) -> Result<Box<dyn AuthStrategy>> {
        match self {
            AuthConfig::Basic { email, token } => {
                Ok(Box::new(BasicStrategy::new(domain, email, token)?))
            }
            AuthConfig::ScopedToken {
                email,
                token,
                cloud_id,
            } => Ok(Box::new(
                ScopedTokenStrategy::connect(domain, email, token, cloud_id, http).await?,
            )),
            AuthConfig::ServiceAccount {
                client_id,
                client_secret,
                cloud_id,
            } => Ok(Box::new(
                ServiceAccountStrategy::connect(client_id, client_secret, cloud_id, http).await?,
            )),
            AuthConfig::OAuth { .. } => {
                let params = self.oauth_params(profile)?;
                Ok(Box::new(OAuthStrategy::resume(params, profile).await?))
            }
        }
    }

    /// Render this config as TOML-flavored lines for `config show` output.
    /// Secrets are masked. Each variant owns its own formatting so adding a
    /// new variant does not require touching `main.rs`.
    pub fn display_lines(&self) -> Vec<String> {
        let mut out = vec![format!("method = \"{}\"", self.method())];
        match self {
            AuthConfig::Basic { email, token } => {
                out.push(format!("email = {:?}", email));
                push_secret(&mut out, "token", token, "ATLASSIAN_API_TOKEN");
            }
            AuthConfig::ScopedToken {
                email,
                token,
                cloud_id,
            } => {
                out.push(format!("email = {:?}", email));
                push_secret(&mut out, "token", token, "ATLASSIAN_API_TOKEN");
                match cloud_id {
                    Some(cid) => out.push(format!("cloud_id = {:?}", cid)),
                    None => out.push("# cloud_id = (will be resolved from the site domain)".into()),
                }
            }
            AuthConfig::ServiceAccount {
                client_id,
                client_secret,
                cloud_id,
            } => {
                out.push(format!("client_id = {:?}", client_id));
                push_secret(
                    &mut out,
                    "client_secret",
                    client_secret,
                    "ATLASSIAN_CLIENT_SECRET",
                );
                match cloud_id {
                    Some(cid) => out.push(format!("cloud_id = {:?}", cid)),
                    None => out.push("# cloud_id = (will be auto-discovered)".into()),
                }
            }
            AuthConfig::OAuth {
                client_id,
                client_secret,
                redirect_port,
                scopes,
                cloud_id,
            } => {
                out.push(format!("client_id = {:?}", client_id));
                push_secret(
                    &mut out,
                    "client_secret",
                    client_secret,
                    "ATLASSIAN_CLIENT_SECRET",
                );
                out.push(format!("redirect_port = {}", redirect_port));
                out.push(format!("scopes = {:?}", scopes));
                match cloud_id {
                    Some(cid) => out.push(format!("cloud_id = {:?}", cid)),
                    None => out.push("# cloud_id = (will be discovered at login)".into()),
                }
            }
        }
        out
    }
}

fn push_secret(out: &mut Vec<String>, key: &str, value: &str, env: &str) {
    if value.is_empty() {
        out.push(format!("# {key} = (not set — provide via {env})"));
    } else {
        out.push(format!("{key} = \"{}\"", mask_secret(value)));
    }
}

fn mask_secret(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < 4 {
        "***".into()
    } else {
        format!("{}***", chars[..4].iter().collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_method_round_trip() {
        for m in [
            AuthMethod::Basic,
            AuthMethod::ScopedToken,
            AuthMethod::ServiceAccount,
            AuthMethod::OAuth,
        ] {
            assert_eq!(AuthMethod::parse(m.as_str()).unwrap(), m);
            assert_eq!(format!("{}", m), m.as_str());
        }
    }

    #[test]
    fn auth_method_parse_trims_and_lowercases() {
        assert_eq!(
            AuthMethod::parse(" Service_Account ").unwrap(),
            AuthMethod::ServiceAccount
        );
    }

    #[test]
    fn auth_method_parse_rejects_unknown() {
        let err = AuthMethod::parse("saml").unwrap_err().to_string();
        assert!(err.contains("basic"));
        assert!(err.contains("scoped_token"));
        assert!(err.contains("service_account"));
        assert!(err.contains("oauth"));
    }

    #[test]
    fn auth_config_method_returns_correct_variant() {
        let b = AuthConfig::Basic {
            email: "a".into(),
            token: "b".into(),
        };
        let s = AuthConfig::ServiceAccount {
            client_id: "c".into(),
            client_secret: "s".into(),
            cloud_id: None,
        };
        let o = AuthConfig::OAuth {
            client_id: "c".into(),
            client_secret: "s".into(),
            redirect_port: 8976,
            scopes: vec![],
            cloud_id: None,
        };
        let t = AuthConfig::ScopedToken {
            email: "a".into(),
            token: "b".into(),
            cloud_id: None,
        };
        assert_eq!(b.method(), AuthMethod::Basic);
        assert_eq!(t.method(), AuthMethod::ScopedToken);
        assert_eq!(s.method(), AuthMethod::ServiceAccount);
        assert_eq!(o.method(), AuthMethod::OAuth);
    }

    #[test]
    fn scoped_token_deserialization() {
        let auth: AuthConfig = toml::from_str(
            r#"
            method = "scoped_token"
            email = "user@example.com"
            token = "scoped"
            cloud_id = "abc"
        "#,
        )
        .unwrap();
        let AuthConfig::ScopedToken {
            email,
            token,
            cloud_id,
        } = auth
        else {
            panic!("expected ScopedToken")
        };
        assert_eq!(email, "user@example.com");
        assert_eq!(token, "scoped");
        assert_eq!(cloud_id.as_deref(), Some("abc"));
    }

    /// `cloud_id` is what separates the two token methods; `deny_unknown_fields`
    /// must keep it from being set on the one that has no use for it.
    #[test]
    fn basic_rejects_a_cloud_id_field() {
        let r: Result<AuthConfig, _> = toml::from_str(
            r#"
            method = "basic"
            email = "user@example.com"
            token = "classic"
            cloud_id = "abc"
        "#,
        );
        assert!(r.is_err());
    }

    #[test]
    fn display_lines_scoped_token_masks_token_and_notes_resolution() {
        let c = AuthConfig::ScopedToken {
            email: "u@x.com".into(),
            token: "ATATT-very-long-token".into(),
            cloud_id: None,
        };
        let lines = c.display_lines();
        assert_eq!(lines[0], "method = \"scoped_token\"");
        assert!(lines.iter().any(|l| l.contains("ATAT***")));
        assert!(!lines.iter().any(|l| l.contains("very-long-token")));
        assert!(lines.iter().any(|l| l.contains("cloud_id")));
    }

    #[test]
    fn basic_auth_deserialization() {
        let auth: AuthConfig = toml::from_str(
            r#"
            method = "basic"
            email = "user@example.com"
            token = "my-token"
        "#,
        )
        .unwrap();
        let AuthConfig::Basic { email, token } = auth else {
            panic!("expected Basic")
        };
        assert_eq!(email, "user@example.com");
        assert_eq!(token, "my-token");
    }

    #[test]
    fn service_account_deserialization() {
        let auth: AuthConfig = toml::from_str(
            r#"
            method = "service_account"
            client_id = "cid"
            client_secret = "sec"
            cloud_id = "abc"
        "#,
        )
        .unwrap();
        let AuthConfig::ServiceAccount {
            client_id,
            client_secret,
            cloud_id,
        } = auth
        else {
            panic!("expected ServiceAccount")
        };
        assert_eq!(client_id, "cid");
        assert_eq!(client_secret, "sec");
        assert_eq!(cloud_id.as_deref(), Some("abc"));
    }

    #[test]
    fn oauth_full_deserialization() {
        let auth: AuthConfig = toml::from_str(
            r#"
            method = "oauth"
            client_id = "oauth-cid"
            client_secret = "oauth-secret"
            redirect_port = 9000
            scopes = ["read:jira-work", "offline_access"]
            cloud_id = "abc"
        "#,
        )
        .unwrap();
        let AuthConfig::OAuth {
            client_id,
            client_secret,
            redirect_port,
            scopes,
            cloud_id,
        } = auth
        else {
            panic!("expected OAuth")
        };
        assert_eq!(client_id, "oauth-cid");
        assert_eq!(client_secret, "oauth-secret");
        assert_eq!(redirect_port, 9000);
        assert_eq!(scopes, vec!["read:jira-work", "offline_access"]);
        assert_eq!(cloud_id.as_deref(), Some("abc"));
    }

    #[test]
    fn oauth_defaults_applied_on_minimal_toml() {
        let auth: AuthConfig = toml::from_str(r#"method = "oauth""#).unwrap();
        let AuthConfig::OAuth {
            redirect_port,
            scopes,
            ..
        } = auth
        else {
            panic!("expected OAuth")
        };
        assert_eq!(redirect_port, DEFAULT_OAUTH_REDIRECT_PORT);
        assert_eq!(scopes, default_oauth_scopes());
        assert!(scopes.contains(&"offline_access".to_string()));
    }

    #[test]
    fn serialization_skips_all_secrets() {
        let configs = [
            AuthConfig::Basic {
                email: "u".into(),
                token: "very-secret-token".into(),
            },
            AuthConfig::ScopedToken {
                email: "u".into(),
                token: "very-secret-scoped".into(),
                cloud_id: None,
            },
            AuthConfig::ServiceAccount {
                client_id: "c".into(),
                client_secret: "very-secret-sa".into(),
                cloud_id: None,
            },
            AuthConfig::OAuth {
                client_id: "c".into(),
                client_secret: "very-secret-oauth".into(),
                redirect_port: 8976,
                scopes: vec![],
                cloud_id: None,
            },
        ];
        for c in configs {
            let s = toml::to_string(&c).unwrap();
            assert!(!s.contains("very-secret"), "leak in: {}", s);
        }
    }

    #[test]
    fn invalid_method_in_toml_fails() {
        let r: Result<AuthConfig, _> = toml::from_str(r#"method = "unknown""#);
        assert!(r.is_err());
    }

    #[test]
    fn display_lines_basic_masks_token() {
        let c = AuthConfig::Basic {
            email: "u@x.com".into(),
            token: "ATATT-very-long-token".into(),
        };
        let lines = c.display_lines();
        assert_eq!(lines[0], "method = \"basic\"");
        assert!(lines.iter().any(|l| l.contains("u@x.com")));
        // Token preview shows first 4 chars + ***, never the full token.
        assert!(lines.iter().any(|l| l.contains("ATAT***")));
        assert!(!lines.iter().any(|l| l.contains("very-long-token")));
    }

    #[test]
    fn display_lines_oauth_includes_all_fields() {
        let c = AuthConfig::OAuth {
            client_id: "cid".into(),
            client_secret: "ATOA-secret".into(),
            redirect_port: 8976,
            scopes: vec!["offline_access".into()],
            cloud_id: Some("cloud-1".into()),
        };
        let lines = c.display_lines();
        assert_eq!(lines[0], "method = \"oauth\"");
        assert!(lines.iter().any(|l| l.contains("redirect_port = 8976")));
        assert!(lines.iter().any(|l| l.contains("offline_access")));
        assert!(lines.iter().any(|l| l.contains("cloud_id")));
        assert!(!lines.iter().any(|l| l.contains("ATOA-secret")));
    }

    #[test]
    fn mask_secret_short_uses_full_redaction() {
        assert_eq!(mask_secret("ab"), "***");
        assert_eq!(mask_secret("abcd"), "abcd***");
        assert_eq!(mask_secret("abcdef"), "abcd***");
    }

    /// Regression guard: every `AuthConfig` variant must redact its secret
    /// fields in `Debug` output, irrespective of how the secret is spelled.
    #[test]
    fn debug_never_leaks_secrets() {
        let cases = [
            AuthConfig::Basic {
                email: "u@x".into(),
                token: "MY-BASIC-SECRET".into(),
            },
            AuthConfig::ScopedToken {
                email: "u@x".into(),
                token: "MY-SCOPED-SECRET".into(),
                cloud_id: Some("cloud-1".into()),
            },
            AuthConfig::ServiceAccount {
                client_id: "cid".into(),
                client_secret: "MY-SA-SECRET".into(),
                cloud_id: None,
            },
            AuthConfig::OAuth {
                client_id: "cid".into(),
                client_secret: "MY-OAUTH-SECRET".into(),
                redirect_port: 8976,
                scopes: vec![],
                cloud_id: None,
            },
        ];
        for c in cases {
            let rendered = format!("{:?}", c);
            assert!(
                !rendered.contains("MY-"),
                "secret leaked in Debug: {rendered}"
            );
            assert!(
                rendered.contains("<redacted>"),
                "expected redaction marker in: {rendered}"
            );
        }
    }
}
