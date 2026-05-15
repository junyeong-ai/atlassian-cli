use super::strategy::{AuthStrategy, Identity, probe_myself};
use crate::auth::AuthMethod;
use crate::client::Service;
use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use secrecy::{ExposeSecret, SecretString};

/// Basic HTTP auth with `email:api_token`. Routes directly to the user's
/// `{domain}.atlassian.net` host; the principal is the token owner.
pub struct BasicStrategy {
    domain: String,
    email: String,
    encoded: SecretString,
}

impl std::fmt::Debug for BasicStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BasicStrategy")
            .field("domain", &self.domain)
            .field("email", &self.email)
            .field("encoded", &"<redacted>")
            .finish()
    }
}

impl BasicStrategy {
    pub fn new(domain: Option<&str>, email: String, token: String) -> Result<Self> {
        let domain = domain.context("ATLASSIAN_DOMAIN required for basic auth")?;
        let clean = domain
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        let encoded = SecretString::new(STANDARD.encode(format!("{}:{}", email, token)).into());
        Ok(Self {
            domain: clean,
            email,
            encoded,
        })
    }
}

#[async_trait]
impl AuthStrategy for BasicStrategy {
    fn method(&self) -> AuthMethod {
        AuthMethod::Basic
    }

    async fn authorization(&self, _http: &reqwest::Client) -> Result<String> {
        Ok(format!("Basic {}", self.encoded.expose_secret()))
    }

    fn build_url(&self, _service: Service, path: &str) -> String {
        format!("https://{}{}", self.domain, path)
    }

    async fn probe_identity(&self, client: &crate::ApiClient) -> Result<Option<Identity>> {
        Ok(Some(probe_myself(client).await?))
    }

    fn identity_label(&self) -> String {
        format!("Basic auth ({})", self.email)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_strips_scheme_and_trailing_slash() {
        let s = BasicStrategy::new(
            Some("https://test.atlassian.net/"),
            "u@x".into(),
            "tk".into(),
        )
        .unwrap();
        assert_eq!(s.domain, "test.atlassian.net");
    }

    #[test]
    fn new_rejects_missing_domain() {
        assert!(BasicStrategy::new(None, "u@x".into(), "tk".into()).is_err());
    }

    #[tokio::test]
    async fn auth_header_is_base64_basic() {
        let s = BasicStrategy::new(Some("test.atlassian.net"), "test".into(), "token123".into())
            .unwrap();
        let http = reqwest::Client::new();
        assert_eq!(
            s.authorization(&http).await.unwrap(),
            "Basic dGVzdDp0b2tlbjEyMw=="
        );
    }

    #[test]
    fn build_url_uses_direct_domain() {
        let s = BasicStrategy::new(Some("test.atlassian.net"), "u@x".into(), "tk".into()).unwrap();
        assert_eq!(
            s.build_url(Service::Jira, "/rest/api/3/issue/X-1"),
            "https://test.atlassian.net/rest/api/3/issue/X-1"
        );
    }

    #[test]
    fn rewrite_url_passthrough() {
        let s = BasicStrategy::new(Some("test.atlassian.net"), "u@x".into(), "tk".into()).unwrap();
        let url = "https://test.atlassian.net/wiki/rest/api/search?cursor=abc";
        assert_eq!(s.rewrite_url(Service::Confluence, url), url);
    }

    #[test]
    fn debug_redacts_encoded_credentials() {
        let s = BasicStrategy::new(Some("test.atlassian.net"), "u@x".into(), "secret-tk".into())
            .unwrap();
        let d = format!("{:?}", s);
        assert!(!d.contains("secret-tk"));
        assert!(d.contains("<redacted>"));
    }

    #[test]
    fn method_returns_basic() {
        let s = BasicStrategy::new(Some("test.atlassian.net"), "u@x".into(), "tk".into()).unwrap();
        assert_eq!(s.method(), AuthMethod::Basic);
    }
}
