use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const OAUTH_TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const ACCESSIBLE_RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";
/// Refresh token 5 minutes before expiry to handle network latency
/// and prevent token expiration during long pagination sequences.
const TOKEN_REFRESH_BUFFER_SECS: u64 = 300;

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct CloudResource {
    id: String,
    url: String,
}

struct TokenState {
    access_token: String,
    expires_at: Instant,
}

/// Thread-safe OAuth 2.0 token manager with transparent refresh.
///
/// # Concurrency
/// Uses `tokio::sync::Mutex` for the cached token state. The lock is held
/// during the refresh HTTP call — this intentionally serializes concurrent
/// refresh attempts so only one network request is made when the cache is
/// stale. In CLI (sequential) usage this is optimal; in highly concurrent
/// library usage, at most one refresh happens per expiry interval while
/// other callers wait and then see the fresh cached token.
pub struct TokenManager {
    client_id: String,
    client_secret: String,
    state: Mutex<Option<TokenState>>,
}

impl TokenManager {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
            state: Mutex::new(None),
        }
    }

    /// Returns a valid access token, refreshing if expired or near expiry.
    pub async fn access_token(&self, http: &reqwest::Client) -> Result<String> {
        let mut state = self.state.lock().await;

        if let Some(ref cached) = *state
            && cached.expires_at > Instant::now()
        {
            return Ok(cached.access_token.clone());
        }

        let new_state = self.fetch_token(http).await?;
        let token = new_state.access_token.clone();
        *state = Some(new_state);
        Ok(token)
    }

    async fn fetch_token(&self, http: &reqwest::Client) -> Result<TokenState> {
        let response = http
            .post(OAUTH_TOKEN_URL)
            .json(&serde_json::json!({
                "grant_type": "client_credentials",
                "client_id": self.client_id,
                "client_secret": self.client_secret,
            }))
            .send()
            .await
            .context("Failed to connect to OAuth token endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("OAuth token request failed ({}): {}", status, body);
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .context("Failed to parse OAuth token response")?;

        let expires_at = Instant::now()
            + Duration::from_secs(
                token_response
                    .expires_in
                    .saturating_sub(TOKEN_REFRESH_BUFFER_SECS),
            );

        Ok(TokenState {
            access_token: token_response.access_token,
            expires_at,
        })
    }

    /// Discovers cloud_id via the accessible-resources API.
    /// Returns the single cloud_id if exactly one site is found.
    /// Errors if zero or multiple sites are found.
    pub async fn discover_cloud_id(http: &reqwest::Client, access_token: &str) -> Result<String> {
        let response = http
            .get(ACCESSIBLE_RESOURCES_URL)
            .header("Authorization", format!("Bearer {}", access_token))
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
            0 => bail!("No accessible Atlassian sites found for this OAuth credential"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_manager_creation() {
        let manager = TokenManager::new("client-id".to_string(), "client-secret".to_string());
        assert_eq!(manager.client_id, "client-id");
        assert_eq!(manager.client_secret, "client-secret");
    }

    #[test]
    fn test_token_state_expiry() {
        let state = TokenState {
            access_token: "test-token".to_string(),
            expires_at: Instant::now() + Duration::from_secs(3600),
        };
        assert!(state.expires_at > Instant::now());
    }

    #[test]
    fn test_token_state_expired() {
        let state = TokenState {
            access_token: "test-token".to_string(),
            expires_at: Instant::now() - Duration::from_secs(1),
        };
        assert!(state.expires_at < Instant::now());
    }

    #[test]
    fn test_token_response_deserialization() {
        let json = r#"{"access_token": "abc123", "expires_in": 3600, "token_type": "Bearer"}"#;
        let response: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.access_token, "abc123");
        assert_eq!(response.expires_in, 3600);
    }

    #[test]
    fn test_cloud_resource_deserialization() {
        let json = r#"{"id": "cloud-123", "url": "https://test.atlassian.net", "name": "test"}"#;
        let resource: CloudResource = serde_json::from_str(json).unwrap();
        assert_eq!(resource.id, "cloud-123");
        assert_eq!(resource.url, "https://test.atlassian.net");
    }

    #[test]
    fn test_token_refresh_buffer_constant() {
        // 5 min buffer prevents token expiry during long pagination sequences
        assert_eq!(TOKEN_REFRESH_BUFFER_SECS, 300);
    }

    #[test]
    fn test_expires_at_applies_buffer() {
        // Simulate a token that expires in 3600s — effective expiry should
        // be now + (3600 - 300) = now + 3300s
        let expires_in: u64 = 3600;
        let effective = expires_in.saturating_sub(TOKEN_REFRESH_BUFFER_SECS);
        assert_eq!(effective, 3300);
    }

    #[test]
    fn test_expires_at_saturates_for_short_tokens() {
        // Token with < 5min lifetime: saturating_sub prevents underflow
        let expires_in: u64 = 100;
        let effective = expires_in.saturating_sub(TOKEN_REFRESH_BUFFER_SECS);
        assert_eq!(effective, 0);
    }
}
