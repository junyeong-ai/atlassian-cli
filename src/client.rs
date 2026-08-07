use crate::auth::{AuthMethod, AuthStrategy};
use crate::config::Config;
use anyhow::{Context, Result};
use reqwest::StatusCode;
use std::sync::Arc;
use std::time::Duration;

pub(crate) const ATLASSIAN_PROXY_BASE: &str = "https://api.atlassian.com";

/// Ceiling on 429 retries per request. Atlassian's point-based rate limits
/// refill continuously, so a few short waits clear transient exhaustion;
/// anything that survives this many retries is a budget problem the caller
/// must see.
const MAX_RATE_LIMIT_RETRIES: u32 = 3;

/// Upper bound on any single retry wait. A server-provided `Retry-After`
/// beyond this indicates an exhausted budget, not a transient spike — the
/// capped wait gives one last attempt instead of stalling a pipeline for
/// minutes.
const RATE_LIMIT_DELAY_CAP: Duration = Duration::from_secs(60);

/// Base delay for the exponential backoff used when a 429 carries no
/// parseable `Retry-After` (doubles per attempt: 500ms, 1s, 2s).
const RATE_LIMIT_BACKOFF_BASE: Duration = Duration::from_millis(500);

/// A non-2xx response from the Atlassian API.
///
/// Every API call surfaces failures through this type (via
/// [`ApiClient::execute`]) so the CLI layer can map the status to an exit
/// code and a structured error object. `Display` renders the canonical
/// `Failed to {operation} ({status}): {body}` message.
#[derive(Debug)]
pub struct ApiError {
    pub operation: String,
    pub status: StatusCode,
    pub body: String,
    /// Actionable remediation for failures whose cause is invisible in the
    /// server response (e.g. a scoped token used with basic auth). Surfaced
    /// as a discrete field in the CLI's structured error output.
    pub hint: Option<&'static str>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Failed to {} ({}): {}",
            self.operation, self.status, self.body
        )
    }
}

impl std::error::Error for ApiError {}

/// Delay before a 429 retry: the server's `Retry-After` (delta-seconds form)
/// when present and parseable, else exponential backoff. Capped either way —
/// see [`RATE_LIMIT_DELAY_CAP`].
fn rate_limit_delay(headers: &reqwest::header::HeaderMap, attempt: u32) -> Duration {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| RATE_LIMIT_BACKOFF_BASE * 2u32.saturating_pow(attempt))
        .min(RATE_LIMIT_DELAY_CAP)
}

/// Borrow everything after the authority of an absolute URL — path, query and
/// fragment. `None` when the input carries no scheme or stops at the authority.
///
/// This is RFC 3986's own rule rather than an approximation of it: a scheme
/// cannot contain `:` or `/`, so the first `://` is the scheme separator, and
/// the authority runs to the first `/`, `?` or `#` that follows. A test pins
/// that agreement against the `url` crate.
///
/// The result borrows from the input so a pagination cursor survives byte for
/// byte. Round-tripping through a URL type instead would rewrite it — `url`
/// removes dot segments, turning `/a/./b` into `/a/b` — and a cursor is opaque
/// server state that must come back exactly as it was handed out.
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
    // Separator written here for the same reason as `BasicStrategy::build_url`:
    // `path` may only land in the path, never reach the authority.
    format!(
        "{}/ex/{}/{}/{}",
        ATLASSIAN_PROXY_BASE,
        service.path_segment(),
        cloud_id,
        path.strip_prefix('/').unwrap_or(path)
    )
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

    #[cfg(test)]
    pub(crate) fn new_with_strategy(strategy: Arc<dyn AuthStrategy>, config: Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.performance.request_timeout_ms))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("test http client builds");
        Self {
            http,
            strategy,
            config,
        }
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

    pub async fn delete(&self, service: Service, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.strategy.build_url(service, path);
        let header = self.strategy.authorization(&self.http).await?;
        Ok(self.http.delete(&url).header("Authorization", header))
    }

    /// Send a prepared request, retrying rate-limited attempts and converting
    /// any non-2xx response into a typed [`ApiError`] named after `operation`.
    ///
    /// Only 429 is retried: it is the one status that guarantees the server
    /// did not process the request, so a retry is safe even for
    /// non-idempotent writes (retrying 5xx could duplicate a write that the
    /// server committed before failing). The wait honors `Retry-After` and
    /// falls back to exponential backoff; because the wait can cross a
    /// token's refresh threshold, each retry re-derives the `Authorization`
    /// header instead of replaying the one built before the wait. Requests
    /// with a streaming body (multipart uploads) cannot be cloned and are
    /// sent exactly once.
    pub async fn execute(
        &self,
        operation: &str,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let mut request = request.build()?;
        let mut attempt: u32 = 0;
        loop {
            let retry = request.try_clone();
            let response = self.http.execute(request).await?;
            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }
            if status == StatusCode::TOO_MANY_REQUESTS
                && attempt < MAX_RATE_LIMIT_RETRIES
                && let Some(mut next) = retry
            {
                let delay = rate_limit_delay(response.headers(), attempt);
                tracing::warn!(
                    operation,
                    attempt = attempt + 1,
                    delay_ms = delay.as_millis() as u64,
                    "rate limited (429), retrying"
                );
                tokio::time::sleep(delay).await;
                let authorization = self.strategy.authorization(&self.http).await?;
                next.headers_mut().insert(
                    reqwest::header::AUTHORIZATION,
                    authorization
                        .parse()
                        .context("Authorization header is not a valid header value")?,
                );
                request = next;
                attempt += 1;
                continue;
            }
            let body = response.text().await.unwrap_or_default();
            return Err(ApiError {
                operation: operation.to_string(),
                status,
                body,
                hint: self.error_hint(status),
            }
            .into());
        }
    }

    /// Remediation hint for failures whose cause the server response does not
    /// explain. Currently: a 401 under basic auth — commonly a wrong or
    /// expired token, but also what a scoped API token produces when used
    /// against the site URL (scoped tokens only work through the
    /// `api.atlassian.com` gateway), which nothing in the server response
    /// reveals.
    fn error_hint(&self, status: StatusCode) -> Option<&'static str> {
        (status == StatusCode::UNAUTHORIZED && self.strategy.method() == AuthMethod::Basic)
            .then_some(
                "the token was rejected — verify it is valid and not expired, and note that \
                 basic auth needs a classic (unscoped) API token: scoped tokens only work \
                 through the api.atlassian.com gateway (oauth or service_account method)",
            )
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
        // Path-like text inside the query must not shift the host boundary.
        assert_eq!(
            extract_path_and_query("https://host.com/rest/api/3/issue/K-1?redirect=/wiki/foo"),
            Some("/rest/api/3/issue/K-1?redirect=/wiki/foo")
        );
    }

    #[test]
    fn extract_path_and_query_agrees_with_a_reference_url_parser() {
        // Differential check against `url`: the authority boundary must land
        // where a standards implementation puts it. Cursors are compared as
        // written, since `url` normalizes dot segments and this must not.
        for url in [
            "https://site.atlassian.net/wiki/api/v2/spaces?limit=2&cursor=eyJpZCI6MjJ9",
            "https://site.atlassian.net/rest/api/search?cursor=_t_WyJc%3D_h_W10%3D&cql=type=page",
            "https://site.atlassian.net/wiki/rest/api/search?cql=text%20~%20%22a//b%22",
            "https://user@site.atlassian.net/wiki/api/v2/pages?cursor=a%2fb",
            "https://site.atlassian.net:8443/wiki/api/v2/pages?cursor=x",
            "https://site.atlassian.net/a?b=c#frag",
        ] {
            let parsed = reqwest::Url::parse(url).expect("valid url");
            let mut reference = parsed.path().to_string();
            if let Some(q) = parsed.query() {
                reference.push('?');
                reference.push_str(q);
            }
            if let Some(f) = parsed.fragment() {
                reference.push('#');
                reference.push_str(f);
            }
            assert_eq!(
                extract_path_and_query(url),
                Some(reference.as_str()),
                "{url}"
            );
        }
    }

    #[test]
    fn test_service_path_segment() {
        assert_eq!(Service::Jira.path_segment(), "jira");
        assert_eq!(Service::Confluence.path_segment(), "confluence");
    }

    #[test]
    fn rate_limit_delay_honors_retry_after_seconds() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "2".parse().unwrap());
        assert_eq!(rate_limit_delay(&headers, 0), Duration::from_secs(2));
    }

    #[test]
    fn rate_limit_delay_caps_excessive_retry_after() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "3600".parse().unwrap());
        assert_eq!(rate_limit_delay(&headers, 0), RATE_LIMIT_DELAY_CAP);
    }

    #[test]
    fn rate_limit_delay_backs_off_exponentially_without_header() {
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(rate_limit_delay(&headers, 0), RATE_LIMIT_BACKOFF_BASE);
        assert_eq!(rate_limit_delay(&headers, 1), RATE_LIMIT_BACKOFF_BASE * 2);
        assert_eq!(rate_limit_delay(&headers, 2), RATE_LIMIT_BACKOFF_BASE * 4);
    }

    #[test]
    fn rate_limit_delay_falls_back_on_http_date_form() {
        // The HTTP-date form of Retry-After is valid per RFC 9110 but rare
        // from Atlassian; it must select the backoff path, not panic.
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            "Fri, 11 Jul 2026 08:00:00 GMT".parse().unwrap(),
        );
        assert_eq!(rate_limit_delay(&headers, 0), RATE_LIMIT_BACKOFF_BASE);
    }

    #[tokio::test]
    async fn execute_retries_429_then_succeeds() {
        use crate::test_utils::mock_client;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let request = client.get(Service::Jira, "/probe").await.unwrap();
        let response = client.execute("probe", request).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn execute_surfaces_429_after_retries_exhausted() {
        use crate::test_utils::mock_client;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "0")
                    .set_body_string("budget exhausted"),
            )
            .expect(1 + MAX_RATE_LIMIT_RETRIES as u64)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let request = client.get(Service::Jira, "/probe").await.unwrap();
        let err = client.execute("probe", request).await.unwrap_err();
        let api_err = err.downcast_ref::<ApiError>().expect("typed ApiError");
        assert_eq!(api_err.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(api_err.operation, "probe");
        assert!(err.to_string().contains("Failed to probe (429"));
        assert!(err.to_string().contains("budget exhausted"));
    }

    #[tokio::test]
    async fn execute_rederives_authorization_on_retry() {
        use crate::auth::AuthMethod;
        use std::sync::atomic::{AtomicU32, Ordering};
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        /// Returns `Bearer token-{n}` with a fresh `n` per call, so the test
        /// can assert that a retry carries a newly derived header rather than
        /// a replay of the pre-wait one.
        #[derive(Debug)]
        struct CountingAuth {
            base_url: String,
            calls: AtomicU32,
        }

        #[async_trait::async_trait]
        impl AuthStrategy for CountingAuth {
            fn method(&self) -> AuthMethod {
                AuthMethod::Basic
            }

            async fn authorization(&self, _http: &reqwest::Client) -> Result<String> {
                let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(format!("Bearer token-{n}"))
            }

            fn build_url(&self, _service: Service, path: &str) -> String {
                format!("{}{}", self.base_url, path)
            }

            fn identity_label(&self) -> String {
                "counting".to_string()
            }
        }

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .and(header("Authorization", "Bearer token-1"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .and(header("Authorization", "Bearer token-2"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let strategy = Arc::new(CountingAuth {
            base_url: server.uri(),
            calls: AtomicU32::new(0),
        });
        let client =
            ApiClient::new_with_strategy(strategy, crate::test_utils::create_test_config());
        let request = client.get(Service::Jira, "/probe").await.unwrap();
        let response = client.execute("probe", request).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn execute_does_not_retry_server_errors() {
        use crate::test_utils::mock_client;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/probe"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let request = client.post(Service::Jira, "/probe").await.unwrap();
        let err = client.execute("probe", request).await.unwrap_err();
        let api_err = err.downcast_ref::<ApiError>().expect("typed ApiError");
        assert_eq!(api_err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(api_err.hint.is_none());
    }

    #[tokio::test]
    async fn execute_hints_on_401_under_basic_auth() {
        use crate::test_utils::mock_client;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let request = client.get(Service::Jira, "/probe").await.unwrap();
        let err = client.execute("probe", request).await.unwrap_err();
        let api_err = err.downcast_ref::<ApiError>().expect("typed ApiError");
        assert_eq!(api_err.status, StatusCode::UNAUTHORIZED);
        let hint = api_err.hint.expect("401 under basic auth carries a hint");
        assert!(hint.contains("classic"));
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
