//! `AuthStrategy` — the runtime auth contract.
//!
//! `ApiClient` holds `Arc<dyn AuthStrategy>`. Each variant of `AuthConfig`
//! resolves into one impl. Adding a method means writing one trait impl;
//! `ApiClient` and the CLI handlers never branch on the variant.

use crate::auth::AuthMethod;
use crate::client::Service;
use anyhow::{Context, Result};
use async_trait::async_trait;

/// Identity discovered by probing `/rest/api/3/myself`. Surfaced by
/// `auth status` / `config validate`. `None` means "this method cannot
/// safely probe" (e.g. service accounts lack `read:jira-user`); the caller
/// should treat that as "credentials are valid but no human identity".
#[derive(Debug, Clone)]
pub struct Identity {
    pub display_name: String,
    pub email: Option<String>,
}

/// Runtime contract for an authentication method.
///
/// All three obligations live here:
/// 1. Produce the `Authorization` header (refreshing tokens transparently).
/// 2. Build request URLs (direct domain vs. `api.atlassian.com/ex/...` proxy).
///    `build_url` is the only place a request host is decided, so a host can
///    never arrive from a server response.
/// 3. Probe and label the principal (for diagnostics / `auth status`).
#[async_trait]
pub trait AuthStrategy: Send + Sync + std::fmt::Debug {
    /// The method tag for this strategy.
    fn method(&self) -> AuthMethod;

    /// Authorization header value. May refresh tokens; uses the provided client.
    async fn authorization(&self, http: &reqwest::Client) -> Result<String>;

    /// Build the request URL for a service-relative path.
    fn build_url(&self, service: Service, path: &str) -> String;

    /// Resolved `cloud_id` for proxy-based methods. `None` for direct-domain auth.
    fn cloud_id(&self) -> Option<&str> {
        None
    }

    /// Probe `/rest/api/3/myself` to recover the human identity behind this
    /// strategy. Returns `Ok(None)` when the principal is non-human or the
    /// strategy is not entitled to call the endpoint — callers must NOT
    /// interpret that as a credential failure.
    ///
    /// Default impl returns `Ok(None)` (no probe). Override for user-delegated
    /// methods (basic, oauth) to fetch the identity.
    async fn probe_identity(&self, _client: &crate::ApiClient) -> Result<Option<Identity>> {
        Ok(None)
    }

    /// Human-readable identity label for diagnostics that don't perform a probe.
    fn identity_label(&self) -> String;
}

/// Shared `/rest/api/3/myself` probe used by every user-delegated strategy
/// (basic, oauth). Lives here — at the trait level — so neither strategy
/// reaches into the other's module to share this helper.
pub(crate) async fn probe_myself(client: &crate::ApiClient) -> Result<Identity> {
    let request = client
        .get(Service::Jira, "/rest/api/3/myself")
        .await?
        .header("Accept", "application/json");
    let response = client.execute("verify credentials", request).await?;
    let data: serde_json::Value = response.json().await.context("Failed to parse /myself")?;
    Ok(Identity {
        display_name: data["displayName"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string(),
        email: data["emailAddress"].as_str().map(|s| s.to_string()),
    })
}
