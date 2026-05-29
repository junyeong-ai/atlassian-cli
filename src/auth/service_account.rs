use super::strategy::AuthStrategy;
use crate::auth::{AuthMethod, DEFAULT_TOKEN_LIFETIME_SECS, TOKEN_REFRESH_BUFFER_SECS};
use crate::client::{Service, proxy_url, rewrite_via_proxy};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const ACCESSIBLE_RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct CloudResource {
    id: String,
    url: String,
}

struct TokenState {
    access_token: SecretString,
    expires_at: Instant,
}

/// In-memory OAuth 2.0 client_credentials token manager with transparent refresh.
///
/// The mutex serializes refresh attempts so concurrent callers issue only one
/// HTTP request when the cache is stale.
pub(crate) struct ServiceAccountTokenManager {
    client_id: String,
    client_secret: SecretString,
    state: Mutex<Option<TokenState>>,
}

impl std::fmt::Debug for ServiceAccountTokenManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceAccountTokenManager")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl ServiceAccountTokenManager {
    pub(crate) fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret: SecretString::new(client_secret.into()),
            state: Mutex::new(None),
        }
    }

    pub(crate) async fn access_token(&self, http: &reqwest::Client) -> Result<String> {
        let mut state = self.state.lock().await;
        if let Some(ref cached) = *state
            && cached.expires_at > Instant::now()
        {
            return Ok(cached.access_token.expose_secret().to_string());
        }
        let new_state = self.fetch_token(http).await?;
        let token = new_state.access_token.expose_secret().to_string();
        *state = Some(new_state);
        Ok(token)
    }

    async fn fetch_token(&self, http: &reqwest::Client) -> Result<TokenState> {
        let response = http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.expose_secret()),
            ])
            .send()
            .await
            .context("Failed to connect to service account token endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!(
                "Service account token request failed ({}): {}",
                status,
                body
            );
        }

        let r: TokenResponse = response
            .json()
            .await
            .context("Failed to parse service account token response")?;

        let lifetime = r.expires_in.unwrap_or(DEFAULT_TOKEN_LIFETIME_SECS);
        let expires_at = Instant::now()
            + Duration::from_secs(lifetime.saturating_sub(TOKEN_REFRESH_BUFFER_SECS));
        Ok(TokenState {
            access_token: SecretString::new(r.access_token.into()),
            expires_at,
        })
    }

    /// Discover the single accessible cloud_id via the public API.
    /// Errors on zero or multiple matches (user must disambiguate via config).
    pub(crate) async fn discover_cloud_id(
        http: &reqwest::Client,
        access_token: &str,
    ) -> Result<String> {
        let response = http
            .get(ACCESSIBLE_RESOURCES_URL)
            .bearer_auth(access_token)
            .send()
            .await
            .context("Failed to connect to accessible-resources endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!(
                "Failed to fetch accessible resources ({}): {}",
                status,
                body
            );
        }

        let resources: Vec<CloudResource> = response
            .json()
            .await
            .context("Failed to parse accessible-resources response")?;

        match resources.len() {
            0 => bail!("No accessible Atlassian sites found for this service account credential"),
            1 => Ok(resources.into_iter().next().unwrap().id),
            n => {
                let sites: Vec<String> = resources
                    .iter()
                    .map(|r| format!("{} ({})", r.url, r.id))
                    .collect();
                bail!(
                    "Multiple Atlassian sites found ({}). Specify cloud_id in config:\n  {}",
                    n,
                    sites.join("\n  ")
                );
            }
        }
    }
}

/// Non-human OAuth 2.0 client_credentials principal.
#[derive(Debug)]
pub struct ServiceAccountStrategy {
    cloud_id: String,
    client_id: String,
    token_manager: ServiceAccountTokenManager,
}

impl ServiceAccountStrategy {
    pub async fn connect(
        client_id: String,
        client_secret: String,
        cloud_id: Option<String>,
        http: &reqwest::Client,
    ) -> Result<Self> {
        let token_manager = ServiceAccountTokenManager::new(client_id.clone(), client_secret);
        // Fail fast on bad credentials; the manager caches the token for reuse.
        let access_token = token_manager.access_token(http).await?;
        let resolved_cloud_id = match cloud_id {
            Some(c) => c,
            None => ServiceAccountTokenManager::discover_cloud_id(http, &access_token).await?,
        };
        // Defense in depth: the cloud_id is interpolated into the proxy path,
        // so a strategy must never hold an unvalidated one even when reached
        // without going through `Config::validate` (e.g. `auth` subcommands).
        crate::config::validate_cloud_id(&resolved_cloud_id)?;
        Ok(Self {
            cloud_id: resolved_cloud_id,
            client_id,
            token_manager,
        })
    }
}

#[async_trait]
impl AuthStrategy for ServiceAccountStrategy {
    fn method(&self) -> AuthMethod {
        AuthMethod::ServiceAccount
    }

    async fn authorization(&self, http: &reqwest::Client) -> Result<String> {
        let token = self.token_manager.access_token(http).await?;
        Ok(format!("Bearer {}", token))
    }

    fn build_url(&self, service: Service, path: &str) -> String {
        proxy_url(service, &self.cloud_id, path)
    }

    fn rewrite_url(&self, service: Service, external_url: &str) -> String {
        rewrite_via_proxy(service, &self.cloud_id, external_url)
    }

    fn cloud_id(&self) -> Option<&str> {
        Some(&self.cloud_id)
    }

    // probe_identity intentionally falls back to the default impl: service
    // accounts do not have a /myself identity and typically lack the
    // read:jira-user scope.

    fn identity_label(&self) -> String {
        let preview: String = self.client_id.chars().take(8).collect();
        format!(
            "Service Account (client_id: {}…, cloud: {})",
            preview, self.cloud_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ServiceAccountStrategy {
        ServiceAccountStrategy {
            cloud_id: "cloud-abc-123".into(),
            client_id: "cid".into(),
            token_manager: ServiceAccountTokenManager::new("cid".into(), "secret".into()),
        }
    }

    #[test]
    fn build_url_jira_uses_proxy() {
        let s = fixture();
        assert_eq!(
            s.build_url(Service::Jira, "/rest/api/3/issue/K-1"),
            "https://api.atlassian.com/ex/jira/cloud-abc-123/rest/api/3/issue/K-1"
        );
    }

    #[test]
    fn build_url_confluence_uses_proxy() {
        let s = fixture();
        assert_eq!(
            s.build_url(Service::Confluence, "/wiki/rest/api/search"),
            "https://api.atlassian.com/ex/confluence/cloud-abc-123/wiki/rest/api/search"
        );
    }

    #[test]
    fn rewrite_url_swaps_host() {
        let s = fixture();
        let url = "https://oyitsm.atlassian.net/wiki/rest/api/search?cursor=abc";
        assert_eq!(
            s.rewrite_url(Service::Confluence, url),
            "https://api.atlassian.com/ex/confluence/cloud-abc-123/wiki/rest/api/search?cursor=abc"
        );
    }

    #[test]
    fn rewrite_url_query_with_path_like_text_is_safe() {
        let s = fixture();
        let url = "https://oyitsm.atlassian.net/rest/api/3/issue/K-1?redirect=/wiki/foo";
        assert_eq!(
            s.rewrite_url(Service::Jira, url),
            "https://api.atlassian.com/ex/jira/cloud-abc-123/rest/api/3/issue/K-1?redirect=/wiki/foo"
        );
    }

    #[test]
    fn cloud_id_exposed() {
        assert_eq!(fixture().cloud_id(), Some("cloud-abc-123"));
    }

    #[test]
    fn method_returns_service_account() {
        assert_eq!(fixture().method(), AuthMethod::ServiceAccount);
    }

    #[test]
    fn token_response_deserializes_with_and_without_expires_in() {
        let with_expiry = r#"{"access_token":"abc","expires_in":3600,"token_type":"Bearer"}"#;
        let r: TokenResponse = serde_json::from_str(with_expiry).unwrap();
        assert_eq!(r.access_token, "abc");
        assert_eq!(r.expires_in, Some(3600));

        let no_expiry = r#"{"access_token":"abc","token_type":"Bearer"}"#;
        let r: TokenResponse = serde_json::from_str(no_expiry).unwrap();
        assert_eq!(r.expires_in, None);
    }

    #[test]
    fn buffer_saturates_for_short_tokens() {
        let expires_in: u64 = 100;
        assert_eq!(expires_in.saturating_sub(TOKEN_REFRESH_BUFFER_SECS), 0);
    }

    #[test]
    fn debug_does_not_leak_client_secret() {
        let tm = ServiceAccountTokenManager::new("cid".into(), "secret-value".into());
        let d = format!("{:?}", tm);
        assert!(!d.contains("secret-value"));
        assert!(d.contains("<redacted>"));
    }
}
