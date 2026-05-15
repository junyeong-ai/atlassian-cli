use crate::auth::AuthStrategy;
use crate::config::Config;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;

pub(crate) const ATLASSIAN_PROXY_BASE: &str = "https://api.atlassian.com";

/// Extract path + query + fragment from an absolute URL by skipping `scheme://host`.
/// Returns `None` if there is no scheme separator or nothing after the host.
/// Safe against false matches in query strings — only examines the host boundary.
pub(crate) fn extract_path_and_query(url: &str) -> Option<&str> {
    let after_scheme = url.find("://").map(|i| &url[i + 3..])?;
    let boundary = after_scheme.find(['/', '?', '#'])?;
    Some(&after_scheme[boundary..])
}

/// Build a request URL through the Atlassian proxy host. Shared by every
/// auth method that routes through `api.atlassian.com/ex/...` (service_account,
/// oauth) — the format is dictated by Atlassian, so the same builder serves
/// every variant.
pub(crate) fn proxy_url(service: Service, cloud_id: &str, path: &str) -> String {
    format!(
        "{}/ex/{}/{}{}",
        ATLASSIAN_PROXY_BASE,
        service.path_segment(),
        cloud_id,
        path
    )
}

/// Rewrite an externally-supplied absolute URL through the Atlassian proxy.
/// Preserves the path+query+fragment of the original; returns the input
/// unchanged when it has no path-like suffix.
pub(crate) fn rewrite_via_proxy(service: Service, cloud_id: &str, external_url: &str) -> String {
    match extract_path_and_query(external_url) {
        Some(suffix) => proxy_url(service, cloud_id, suffix),
        None => external_url.to_string(),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Service {
    Jira,
    Confluence,
}

impl Service {
    pub(crate) fn path_segment(self) -> &'static str {
        match self {
            Service::Jira => "jira",
            Service::Confluence => "confluence",
        }
    }
}

pub struct ApiClient {
    http: reqwest::Client,
    strategy: Arc<dyn AuthStrategy>,
    config: Config,
}

impl ApiClient {
    /// Build a client from a validated `Config`.
    /// May make outbound calls (initial token, cloud_id discovery, token-store reads)
    /// — fails fast on bad credentials.
    pub async fn new(config: Config) -> Result<Self> {
        let auth_config = config
            .auth
            .clone()
            .context("Authentication not configured")?;

        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.performance.request_timeout_ms))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        let strategy: Arc<dyn AuthStrategy> = auth_config
            .into_strategy(config.domain.as_deref(), &config.profile, &http)
            .await?
            .into();

        Ok(Self {
            http,
            strategy,
            config,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Access the underlying strategy for diagnostics / introspection.
    pub fn strategy(&self) -> &dyn AuthStrategy {
        self.strategy.as_ref()
    }

    pub fn cloud_id(&self) -> Option<&str> {
        self.strategy.cloud_id()
    }

    pub async fn get(&self, service: Service, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.strategy.build_url(service, path);
        let header = self.strategy.authorization(&self.http).await?;
        Ok(self.http.get(&url).header("Authorization", header))
    }

    pub async fn post(&self, service: Service, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.strategy.build_url(service, path);
        let header = self.strategy.authorization(&self.http).await?;
        Ok(self.http.post(&url).header("Authorization", header))
    }

    pub async fn put(&self, service: Service, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.strategy.build_url(service, path);
        let header = self.strategy.authorization(&self.http).await?;
        Ok(self.http.put(&url).header("Authorization", header))
    }

    /// GET for an already-absolute URL (e.g. Confluence pagination `next`).
    pub async fn get_absolute(&self, url: &str) -> Result<reqwest::RequestBuilder> {
        let header = self.strategy.authorization(&self.http).await?;
        Ok(self.http.get(url).header("Authorization", header))
    }

    /// Rewrite an external absolute URL through the strategy
    /// (e.g. service_account swaps the host to the Atlassian proxy).
    pub fn rewrite_url(&self, service: Service, external_url: &str) -> String {
        self.strategy.rewrite_url(service, external_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthConfig;

    #[test]
    fn test_extract_path_and_query() {
        assert_eq!(
            extract_path_and_query("https://example.com/a/b?x=1"),
            Some("/a/b?x=1")
        );
        assert_eq!(extract_path_and_query("http://host/path"), Some("/path"));
        assert_eq!(extract_path_and_query("https://only-host.com"), None);
        assert_eq!(extract_path_and_query("not-a-url"), None);
        assert_eq!(
            extract_path_and_query("https://host.com?query=foo"),
            Some("?query=foo")
        );
        assert_eq!(
            extract_path_and_query("https://host.com#section"),
            Some("#section")
        );
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
    async fn test_new_missing_auth_fails() {
        let config = Config {
            domain: Some("test.atlassian.net".to_string()),
            auth: None,
            ..Default::default()
        };
        let result = ApiClient::new(config).await;
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("not configured"));
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
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("ATLASSIAN_DOMAIN")
        );
    }
}
