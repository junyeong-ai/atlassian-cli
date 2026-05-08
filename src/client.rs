use crate::auth::AuthConfig;
use crate::config::Config;
use crate::token::ServiceAccountTokenManager;
use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::Arc;
use std::time::Duration;

const ATLASSIAN_PROXY_BASE: &str = "https://api.atlassian.com";

/// Extract path + query + fragment from an absolute URL by skipping scheme://host.
/// Returns None if URL has no scheme or nothing after the host.
///
/// Handles URLs with any of: path (/), query (?), or fragment (#) as first component.
/// Safe against false matches in query strings — only examines the host boundary.
fn extract_path_and_query(url: &str) -> Option<&str> {
    // Find "://" to skip scheme
    let after_scheme = url.find("://").map(|i| &url[i + 3..])?;
    // Path/query/fragment separator after the host
    let boundary = after_scheme.find(['/', '?', '#'])?;
    Some(&after_scheme[boundary..])
}

#[derive(Debug, Clone, Copy)]
pub enum Service {
    Jira,
    Confluence,
}

impl Service {
    fn path_segment(self) -> &'static str {
        match self {
            Service::Jira => "jira",
            Service::Confluence => "confluence",
        }
    }
}

/// Runtime authentication state resolved from AuthConfig.
/// All fields are non-optional — validated during ApiClient::new().
enum AuthState {
    Basic {
        domain: String,
        encoded: String, // pre-computed base64(email:token)
    },
    ServiceAccount {
        cloud_id: String,
        token_manager: Arc<ServiceAccountTokenManager>,
    },
}

pub struct ApiClient {
    http: reqwest::Client,
    auth: AuthState,
    config: Config,
}

impl ApiClient {
    /// Creates an ApiClient from a validated Config.
    /// For service accounts, this fetches an initial token and discovers cloud_id if not provided.
    pub async fn new(config: Config) -> Result<Self> {
        let auth_config = config
            .auth
            .as_ref()
            .context("Authentication not configured")?;

        // Separate connect_timeout from overall timeout: a broken DNS / unreachable
        // host should fail fast instead of holding for the full request budget,
        // while legitimately slow responses still get the configured window.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.performance.request_timeout_ms))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        let auth = match auth_config {
            AuthConfig::Basic { email, token } => {
                let domain = config
                    .domain
                    .as_ref()
                    .context("ATLASSIAN_DOMAIN required for basic auth")?;

                let clean_domain = domain
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_end_matches('/');

                let encoded = STANDARD.encode(format!("{}:{}", email, token));

                AuthState::Basic {
                    domain: clean_domain.to_string(),
                    encoded,
                }
            }
            AuthConfig::ServiceAccount {
                client_id,
                client_secret,
                cloud_id,
            } => {
                let token_manager = Arc::new(ServiceAccountTokenManager::new(
                    client_id.clone(),
                    client_secret.clone(),
                ));

                // Always fetch a token on construction to verify credentials.
                // The token is cached in the manager for subsequent API calls,
                // so this doesn't add cost to the critical path — just fails fast
                // on invalid credentials instead of deferring the error to the
                // first API call (which would be ambiguous: network? scope? creds?).
                let access_token = token_manager.access_token(&http).await?;

                let resolved_cloud_id = match cloud_id {
                    Some(cid) => cid.clone(),
                    None => {
                        ServiceAccountTokenManager::discover_cloud_id(&http, &access_token).await?
                    }
                };

                AuthState::ServiceAccount {
                    cloud_id: resolved_cloud_id,
                    token_manager,
                }
            }
        };

        Ok(Self { http, auth, config })
    }

    /// Access non-auth config (filters, performance, optimization).
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Returns `true` if this client uses OAuth 2.0 service account authentication.
    pub fn is_service_account(&self) -> bool {
        matches!(self.auth, AuthState::ServiceAccount { .. })
    }

    /// Returns the resolved cloud_id for Service account clients, `None` for Basic auth.
    pub fn cloud_id(&self) -> Option<&str> {
        match &self.auth {
            AuthState::ServiceAccount { cloud_id, .. } => Some(cloud_id.as_str()),
            AuthState::Basic { .. } => None,
        }
    }

    /// Build a GET request with auth for a service-relative path.
    /// Path examples: "/rest/api/3/issue/KEY-123", "/wiki/api/v2/pages/123"
    pub async fn get(&self, service: Service, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.build_url(service, path);
        let header = self.auth_header_value().await?;
        Ok(self.http.get(&url).header("Authorization", header))
    }

    /// Build a POST request with auth for a service-relative path.
    pub async fn post(&self, service: Service, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.build_url(service, path);
        let header = self.auth_header_value().await?;
        Ok(self.http.post(&url).header("Authorization", header))
    }

    /// Build a PUT request with auth for a service-relative path.
    pub async fn put(&self, service: Service, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.build_url(service, path);
        let header = self.auth_header_value().await?;
        Ok(self.http.put(&url).header("Authorization", header))
    }

    /// Build a GET request for an already-complete absolute URL.
    /// Used for Confluence cursor-based pagination where the next URL
    /// comes from the API response (after rewrite_url).
    pub async fn get_absolute(&self, url: &str) -> Result<reqwest::RequestBuilder> {
        let header = self.auth_header_value().await?;
        Ok(self.http.get(url).header("Authorization", header))
    }

    /// Rewrite an external absolute URL to route through the correct auth path.
    ///
    /// For Basic auth: returns the URL unchanged (already correct form).
    /// For Service account: replaces scheme+host with Atlassian proxy, preserving path+query.
    ///   e.g. "https://domain.atlassian.net/wiki/rest/api/search?cursor=abc" →
    ///        "https://api.atlassian.com/ex/confluence/{cloud_id}/wiki/rest/api/search?cursor=abc"
    ///
    /// Uses proper URL parsing (not substring matching) to avoid false positives
    /// when path-like strings appear in query parameters.
    pub fn rewrite_url(&self, service: Service, external_url: &str) -> String {
        match &self.auth {
            AuthState::Basic { .. } => external_url.to_string(),
            AuthState::ServiceAccount { cloud_id, .. } => {
                let Some(path_with_query) = extract_path_and_query(external_url) else {
                    return external_url.to_string();
                };
                format!(
                    "{}/ex/{}/{}{}",
                    ATLASSIAN_PROXY_BASE,
                    service.path_segment(),
                    cloud_id,
                    path_with_query
                )
            }
        }
    }

    fn build_url(&self, service: Service, path: &str) -> String {
        match &self.auth {
            AuthState::Basic { domain, .. } => {
                format!("https://{}{}", domain, path)
            }
            AuthState::ServiceAccount { cloud_id, .. } => {
                format!(
                    "{}/ex/{}/{}{}",
                    ATLASSIAN_PROXY_BASE,
                    service.path_segment(),
                    cloud_id,
                    path
                )
            }
        }
    }

    async fn auth_header_value(&self) -> Result<String> {
        match &self.auth {
            AuthState::Basic { encoded, .. } => Ok(format!("Basic {}", encoded)),
            AuthState::ServiceAccount { token_manager, .. } => {
                let token = token_manager.access_token(&self.http).await?;
                Ok(format!("Bearer {}", token))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthConfig;
    use crate::config::{Config, PerformanceConfig};

    fn basic_config() -> Config {
        Config {
            domain: Some("test.atlassian.net".to_string()),
            auth: Some(AuthConfig::Basic {
                email: "test@example.com".to_string(),
                token: "token123".to_string(),
            }),
            performance: PerformanceConfig {
                request_timeout_ms: 30000,
                rate_limit_delay_ms: 200,
            },
            ..Default::default()
        }
    }

    fn service_account_config() -> Config {
        Config {
            domain: None,
            auth: Some(AuthConfig::ServiceAccount {
                client_id: "test-cid".to_string(),
                client_secret: "test-secret".to_string(),
                cloud_id: Some("cloud-abc-123".to_string()),
            }),
            performance: PerformanceConfig {
                request_timeout_ms: 30000,
                rate_limit_delay_ms: 200,
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_build_url_basic_jira() {
        let auth = AuthState::Basic {
            domain: "test.atlassian.net".to_string(),
            encoded: "dGVzdA==".to_string(),
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: basic_config(),
        };
        assert_eq!(
            client.build_url(Service::Jira, "/rest/api/3/issue/KEY-1"),
            "https://test.atlassian.net/rest/api/3/issue/KEY-1"
        );
    }

    #[test]
    fn test_build_url_basic_confluence() {
        let auth = AuthState::Basic {
            domain: "test.atlassian.net".to_string(),
            encoded: "dGVzdA==".to_string(),
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: basic_config(),
        };
        assert_eq!(
            client.build_url(Service::Confluence, "/wiki/api/v2/pages/123"),
            "https://test.atlassian.net/wiki/api/v2/pages/123"
        );
    }

    #[test]
    fn test_build_url_service_account_jira() {
        let token_manager = Arc::new(ServiceAccountTokenManager::new(
            "cid".to_string(),
            "secret".to_string(),
        ));
        let auth = AuthState::ServiceAccount {
            cloud_id: "cloud-abc-123".to_string(),
            token_manager,
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: service_account_config(),
        };
        assert_eq!(
            client.build_url(Service::Jira, "/rest/api/3/issue/KEY-1"),
            "https://api.atlassian.com/ex/jira/cloud-abc-123/rest/api/3/issue/KEY-1"
        );
    }

    #[test]
    fn test_build_url_service_account_confluence() {
        let token_manager = Arc::new(ServiceAccountTokenManager::new(
            "cid".to_string(),
            "secret".to_string(),
        ));
        let auth = AuthState::ServiceAccount {
            cloud_id: "cloud-abc-123".to_string(),
            token_manager,
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: service_account_config(),
        };
        assert_eq!(
            client.build_url(Service::Confluence, "/wiki/rest/api/search"),
            "https://api.atlassian.com/ex/confluence/cloud-abc-123/wiki/rest/api/search"
        );
    }

    #[test]
    fn test_rewrite_url_basic_passthrough() {
        let auth = AuthState::Basic {
            domain: "test.atlassian.net".to_string(),
            encoded: "dGVzdA==".to_string(),
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: basic_config(),
        };
        let url = "https://test.atlassian.net/wiki/rest/api/search?cursor=abc";
        assert_eq!(client.rewrite_url(Service::Confluence, url), url);
    }

    #[test]
    fn test_rewrite_url_service_account_confluence() {
        let token_manager = Arc::new(ServiceAccountTokenManager::new(
            "cid".to_string(),
            "secret".to_string(),
        ));
        let auth = AuthState::ServiceAccount {
            cloud_id: "cloud-abc-123".to_string(),
            token_manager,
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: service_account_config(),
        };
        let url = "https://oyitsm.atlassian.net/wiki/rest/api/search?cursor=abc";
        assert_eq!(
            client.rewrite_url(Service::Confluence, url),
            "https://api.atlassian.com/ex/confluence/cloud-abc-123/wiki/rest/api/search?cursor=abc"
        );
    }

    #[test]
    fn test_rewrite_url_service_account_rest_path() {
        let token_manager = Arc::new(ServiceAccountTokenManager::new(
            "cid".to_string(),
            "secret".to_string(),
        ));
        let auth = AuthState::ServiceAccount {
            cloud_id: "cloud-abc-123".to_string(),
            token_manager,
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: service_account_config(),
        };
        let url = "https://oyitsm.atlassian.net/rest/api/3/search?next=xyz";
        assert_eq!(
            client.rewrite_url(Service::Jira, url),
            "https://api.atlassian.com/ex/jira/cloud-abc-123/rest/api/3/search?next=xyz"
        );
    }

    #[test]
    fn test_rewrite_url_any_path_rewritten() {
        // rewrite_url now preserves any path after the host
        let token_manager = Arc::new(ServiceAccountTokenManager::new(
            "cid".to_string(),
            "secret".to_string(),
        ));
        let auth = AuthState::ServiceAccount {
            cloud_id: "cloud-abc-123".to_string(),
            token_manager,
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: service_account_config(),
        };
        let url = "https://example.com/unknown/path";
        assert_eq!(
            client.rewrite_url(Service::Jira, url),
            "https://api.atlassian.com/ex/jira/cloud-abc-123/unknown/path"
        );
    }

    #[test]
    fn test_rewrite_url_query_with_path_like_text_safe() {
        // Query string contains /wiki/ but it's in the query, not path.
        // Path extraction must use the host boundary.
        let token_manager = Arc::new(ServiceAccountTokenManager::new(
            "cid".to_string(),
            "secret".to_string(),
        ));
        let auth = AuthState::ServiceAccount {
            cloud_id: "cloud-abc-123".to_string(),
            token_manager,
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: service_account_config(),
        };
        let url = "https://oyitsm.atlassian.net/rest/api/3/issue/KEY-1?redirect=/wiki/foo";
        assert_eq!(
            client.rewrite_url(Service::Jira, url),
            "https://api.atlassian.com/ex/jira/cloud-abc-123/rest/api/3/issue/KEY-1?redirect=/wiki/foo"
        );
    }

    #[test]
    fn test_extract_path_and_query() {
        // Standard cases
        assert_eq!(
            extract_path_and_query("https://example.com/a/b?x=1"),
            Some("/a/b?x=1")
        );
        assert_eq!(extract_path_and_query("http://host/path"), Some("/path"));
        // Edge: no path/query/fragment
        assert_eq!(extract_path_and_query("https://only-host.com"), None);
        assert_eq!(extract_path_and_query("not-a-url"), None);
        // Edge: query-only URL (RFC-valid, defensive handling)
        assert_eq!(
            extract_path_and_query("https://host.com?query=foo"),
            Some("?query=foo")
        );
        // Edge: fragment without path
        assert_eq!(
            extract_path_and_query("https://host.com#section"),
            Some("#section")
        );
        // Edge: userinfo in URL — host boundary still found correctly
        assert_eq!(
            extract_path_and_query("https://user@host.com/path"),
            Some("/path")
        );
    }

    #[test]
    fn test_service_path_segment() {
        assert_eq!(Service::Jira.path_segment(), "jira");
        assert_eq!(Service::Confluence.path_segment(), "confluence");
    }

    #[tokio::test]
    async fn test_auth_header_basic() {
        let auth = AuthState::Basic {
            domain: "test.atlassian.net".to_string(),
            encoded: "dGVzdDp0b2tlbjEyMw==".to_string(),
        };
        let client = ApiClient {
            http: reqwest::Client::new(),
            auth,
            config: basic_config(),
        };
        let header = client.auth_header_value().await.unwrap();
        assert_eq!(header, "Basic dGVzdDp0b2tlbjEyMw==");
    }

    #[tokio::test]
    async fn test_new_missing_auth_fails() {
        let config = Config {
            domain: Some("test.atlassian.net".to_string()),
            auth: None,
            ..Default::default()
        };
        let result = ApiClient::new(config).await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn test_new_basic_missing_domain_fails() {
        let config = Config {
            domain: None,
            auth: Some(AuthConfig::Basic {
                email: "test@example.com".to_string(),
                token: "token".to_string(),
            }),
            ..Default::default()
        };
        let result = ApiClient::new(config).await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("ATLASSIAN_DOMAIN"));
    }
}
