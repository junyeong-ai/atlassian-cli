//! `AuthStrategy` impl for OAuth 3LO (user-delegated) auth.

use super::flow::{self, FlowInputs};
use super::store::{TokenSet, TokenStore};
use crate::auth::strategy::{AuthStrategy, Identity, probe_myself};
use crate::auth::{AuthMethod, TOKEN_REFRESH_BUFFER_SECS};
use crate::client::{Service, proxy_url, rewrite_via_proxy};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use secrecy::ExposeSecret;
use tokio::sync::Mutex;

/// Configuration for the OAuth flow. Mirrors the static fields of
/// `AuthConfig::OAuth` — pure config data, no runtime context. The profile
/// name (used as the token-storage key) is a separate runtime argument
/// passed to `login`/`resume` so the same `OAuthParams` instance can be
/// reused across profiles without confusion.
#[derive(Clone)]
pub struct OAuthParams {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_port: u16,
    pub scopes: Vec<String>,
    pub cloud_id: Option<String>,
}

impl std::fmt::Debug for OAuthParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthParams")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("redirect_port", &self.redirect_port)
            .field("scopes", &self.scopes)
            .field("cloud_id", &self.cloud_id)
            .finish()
    }
}

impl OAuthParams {
    fn flow_inputs(&self) -> FlowInputs<'_> {
        FlowInputs {
            client_id: &self.client_id,
            client_secret: &self.client_secret,
            redirect_port: self.redirect_port,
            scopes: &self.scopes,
            cloud_id_pin: self.cloud_id.as_deref(),
        }
    }
}

#[derive(Debug)]
pub struct OAuthStrategy {
    params: OAuthParams,
    profile: String,
    /// Cached at construction. Atlassian does not rotate cloud_id; even on
    /// refresh we keep the original site so `build_url` (sync method) can
    /// read it without locking the token mutex.
    cloud_id: String,
    store: TokenStore,
    /// Serializes refresh attempts: the first stale caller refreshes; others
    /// wake to the fresh token without issuing duplicate HTTP requests.
    tokens: Mutex<TokenSet>,
}

impl OAuthStrategy {
    /// Run the interactive flow and persist the resulting tokens.
    ///
    /// Returns the user-facing report. The caller typically exits the process
    /// after printing it; if a long-lived strategy is needed, call `resume`.
    pub async fn login(
        params: OAuthParams,
        profile: &str,
        api_http: &reqwest::Client,
        open_browser: bool,
    ) -> Result<flow::LoginOutcome> {
        let outcome = flow::login(&params.flow_inputs(), api_http, open_browser).await?;
        let store = TokenStore::new(profile)?;
        let backend = store.save(&outcome.tokens).await?;
        tracing::debug!(
            "Saved OAuth tokens for profile '{}' via {}",
            profile,
            backend
        );
        Ok(outcome)
    }

    /// Load persisted tokens for `profile`. Errors with a clear `auth login`
    /// hint when no tokens are stored.
    pub async fn resume(params: OAuthParams, profile: &str) -> Result<Self> {
        let store = TokenStore::new(profile)?;
        let tokens = store
            .load()
            .await?
            .with_context(|| {
                format!(
                    "No OAuth tokens stored for profile '{profile}'. Run `atlassian-cli auth login` first."
                )
            })?
            .tokens;
        let cloud_id = params
            .cloud_id
            .clone()
            .or_else(|| tokens.cloud_id.clone())
            .context(
                "OAuth tokens are missing cloud_id. Run `atlassian-cli auth login` to re-discover.",
            )?;
        // Defense in depth: the cloud_id is interpolated into the proxy path.
        // Validate here so neither a pinned config value (reached via
        // `load_without_validation`) nor a tampered `credentials.json` can
        // smuggle URL structure into the request path.
        crate::config::validate_cloud_id(&cloud_id)?;
        if tokens.refresh_token.is_none() && tokens.is_expired_with_buffer(0) {
            bail!(
                "Stored OAuth access_token has expired and no refresh_token is available. Run `atlassian-cli auth login`."
            );
        }
        Ok(Self {
            params,
            profile: profile.to_string(),
            cloud_id,
            store,
            tokens: Mutex::new(tokens),
        })
    }

    /// Force a refresh regardless of expiry. Used by `auth refresh`.
    pub async fn force_refresh(&self) -> Result<TokenSet> {
        let mut guard = self.tokens.lock().await;
        self.refresh_locked(&mut guard).await?;
        Ok(guard.clone())
    }

    async fn refresh_locked(&self, current: &mut TokenSet) -> Result<()> {
        let refresh_token = current
            .refresh_token
            .as_ref()
            .context("Cannot refresh: no refresh_token stored (run `auth login`)")?
            .clone();
        let new_tokens = flow::refresh(
            &self.params.flow_inputs(),
            &refresh_token,
            self.cloud_id.clone(),
        )
        .await?;
        // Atlassian rotates refresh tokens; if the response omits one we carry
        // the previous through to keep the session alive.
        let merged = TokenSet {
            access_token: new_tokens.access_token,
            refresh_token: new_tokens.refresh_token.or(Some(refresh_token)),
            expires_at_unix: new_tokens.expires_at_unix,
            scopes: if new_tokens.scopes.is_empty() {
                current.scopes.clone()
            } else {
                new_tokens.scopes
            },
            cloud_id: Some(self.cloud_id.clone()),
        };
        self.store.save(&merged).await?;
        *current = merged;
        Ok(())
    }
}

#[async_trait]
impl AuthStrategy for OAuthStrategy {
    fn method(&self) -> AuthMethod {
        AuthMethod::OAuth
    }

    async fn authorization(&self, _http: &reqwest::Client) -> Result<String> {
        let mut guard = self.tokens.lock().await;
        if guard.is_expired_with_buffer(TOKEN_REFRESH_BUFFER_SECS as i64) {
            self.refresh_locked(&mut guard).await?;
        }
        Ok(format!("Bearer {}", guard.access_token.expose_secret()))
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

    async fn probe_identity(&self, client: &crate::ApiClient) -> Result<Option<Identity>> {
        Ok(Some(probe_myself(client).await?))
    }

    fn identity_label(&self) -> String {
        format!(
            "OAuth user (profile: {}, cloud: {})",
            self.profile, self.cloud_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_params_debug_redacts_client_secret() {
        let p = OAuthParams {
            client_id: "cid".into(),
            client_secret: "OAUTH-PARAMS-LEAK".into(),
            redirect_port: 8976,
            scopes: vec![],
            cloud_id: None,
        };
        let rendered = format!("{:?}", p);
        assert!(
            !rendered.contains("OAUTH-PARAMS-LEAK"),
            "leaked: {rendered}"
        );
        assert!(rendered.contains("<redacted>"));
    }
}
