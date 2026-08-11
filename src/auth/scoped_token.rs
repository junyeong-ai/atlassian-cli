use super::strategy::{AuthStrategy, Identity, encode_basic_credential, probe_myself};
use crate::auth::AuthMethod;
use crate::client::{Service, proxy_url};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

/// Unauthenticated endpoint that publishes a site's cloud id. It is the first
/// method in Atlassian's own "find your cloud ID" guide, and the only one that
/// needs nothing but the site host — `accessible-resources` is reachable with
/// an OAuth bearer token, which this method by definition does not have.
const TENANT_INFO_PATH: &str = "/_edge/tenant_info";

#[derive(Deserialize)]
struct TenantInfo {
    #[serde(rename = "cloudId")]
    cloud_id: String,
}

/// Basic HTTP auth with `email:api_token`, sent through the
/// `api.atlassian.com/ex/{service}/{cloud_id}` gateway. The principal is the
/// token owner.
///
/// The gateway is the only host that honours an API token carrying scopes;
/// at the site host such a token is not merely rejected but ignored, which
/// surfaces as an anonymous 401 rather than anything naming the real cause.
/// The converse also holds — a classic (unscoped) token is rejected here —
/// so the two token shapes are distinct methods rather than one method with
/// a routing switch.
pub struct ScopedTokenStrategy {
    cloud_id: String,
    email: String,
    encoded: SecretString,
}

impl std::fmt::Debug for ScopedTokenStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedTokenStrategy")
            .field("cloud_id", &self.cloud_id)
            .field("email", &self.email)
            .field("encoded", &"<redacted>")
            .finish()
    }
}

impl ScopedTokenStrategy {
    pub async fn connect(
        domain: Option<&str>,
        email: String,
        token: String,
        cloud_id: Option<String>,
        http: &reqwest::Client,
    ) -> Result<Self> {
        let cloud_id = match cloud_id {
            Some(pinned) => pinned,
            None => Self::discover_cloud_id(domain, http).await?,
        };
        // Defense in depth: the cloud_id is interpolated into the proxy path,
        // so a strategy must never hold an unvalidated one even when reached
        // without going through `Config::validate` first.
        crate::config::validate_cloud_id(&cloud_id)?;
        Ok(Self {
            cloud_id,
            encoded: encode_basic_credential(&email, &token),
            email,
        })
    }

    /// Derive the site origin the cloud id is published at. Kept apart from
    /// the round trip below so the origin has exactly one source: a host that
    /// has passed `validate_atlassian_domain`.
    async fn discover_cloud_id(domain: Option<&str>, http: &reqwest::Client) -> Result<String> {
        let domain = domain.context(
            "scoped_token auth needs a cloud_id, or a domain to resolve one from. \
             Set --cloud-id / ATLASSIAN_CLOUD_ID, or --domain / ATLASSIAN_DOMAIN.",
        )?;
        let host = crate::config::validate_atlassian_domain(domain)?;
        Self::fetch_cloud_id(&format!("https://{host}"), http).await
    }

    /// The `tenant_info` round trip. Deliberately unauthenticated: the
    /// credential belongs to the gateway, and this origin is exactly where an
    /// API token with scopes is silently ignored.
    ///
    /// Redirects are left on the shared client's default policy, unlike the
    /// OAuth token exchange which hard-disables them. The reason they are
    /// dangerous there — a redirect hands the target a credential — does not
    /// apply to a request that carries none, and the only value read back is
    /// charset-checked by `validate_cloud_id` before it reaches a URL.
    async fn fetch_cloud_id(origin: &str, http: &reqwest::Client) -> Result<String> {
        let url = format!("{origin}{TENANT_INFO_PATH}");

        let response = http
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("Failed to reach {url} to resolve cloud_id"))?;

        if !response.status().is_success() {
            let status = response.status();
            bail!(
                "Failed to resolve cloud_id from {} ({}). \
                 Resolution needs the site host to answer from this network; when it does not, \
                 pin the value with cloud_id in [auth] or ATLASSIAN_CLOUD_ID and the site host \
                 stops being involved at all.",
                url,
                status
            );
        }

        let info: TenantInfo = response
            .json()
            .await
            .with_context(|| format!("Failed to parse the cloud_id response from {url}"))?;
        Ok(info.cloud_id)
    }
}

#[async_trait]
impl AuthStrategy for ScopedTokenStrategy {
    fn method(&self) -> AuthMethod {
        AuthMethod::ScopedToken
    }

    async fn authorization(&self, _http: &reqwest::Client) -> Result<String> {
        Ok(format!("Basic {}", self.encoded.expose_secret()))
    }

    fn build_url(&self, service: Service, path: &str) -> String {
        proxy_url(service, &self.cloud_id, path)
    }

    fn cloud_id(&self) -> Option<&str> {
        Some(&self.cloud_id)
    }

    /// The principal is a person, so `/myself` answers — provided the token
    /// carries a Jira read scope. A token scoped to Confluence alone fails the
    /// probe, and that failure is reported rather than swallowed: the gateway
    /// answers insufficient scope with the same 401 it uses for a bad
    /// credential, so the two are indistinguishable here.
    async fn probe_identity(&self, client: &crate::ApiClient) -> Result<Option<Identity>> {
        Ok(Some(probe_myself(client).await?))
    }

    fn identity_label(&self) -> String {
        format!(
            "Scoped API token ({}, cloud: {})",
            self.email, self.cloud_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fixture() -> ScopedTokenStrategy {
        ScopedTokenStrategy {
            cloud_id: "cloud-abc-123".into(),
            email: "u@x.com".into(),
            encoded: encode_basic_credential("u@x.com", "scoped-token"),
        }
    }

    #[test]
    fn build_url_jira_uses_proxy() {
        assert_eq!(
            fixture().build_url(Service::Jira, "/rest/api/3/issue/K-1"),
            "https://api.atlassian.com/ex/jira/cloud-abc-123/rest/api/3/issue/K-1"
        );
    }

    #[test]
    fn build_url_confluence_uses_proxy() {
        assert_eq!(
            fixture().build_url(Service::Confluence, "/wiki/api/v2/pages"),
            "https://api.atlassian.com/ex/confluence/cloud-abc-123/wiki/api/v2/pages"
        );
    }

    /// The separator belongs to the origin, so a path that opens with `@`
    /// must extend the path and never reach the authority.
    #[test]
    fn build_url_keeps_path_out_of_the_authority() {
        assert_eq!(
            fixture().build_url(Service::Jira, "@evil.com/x"),
            "https://api.atlassian.com/ex/jira/cloud-abc-123/@evil.com/x"
        );
    }

    #[tokio::test]
    async fn authorization_is_the_basic_credential() {
        let expected = encode_basic_credential("u@x.com", "scoped-token");
        let header = fixture()
            .authorization(&reqwest::Client::new())
            .await
            .unwrap();
        assert_eq!(header, format!("Basic {}", expected.expose_secret()));
    }

    #[test]
    fn method_and_cloud_id_are_exposed() {
        let s = fixture();
        assert_eq!(s.method(), AuthMethod::ScopedToken);
        assert_eq!(s.cloud_id(), Some("cloud-abc-123"));
    }

    #[test]
    fn debug_does_not_leak_the_credential() {
        let d = format!("{:?}", fixture());
        assert!(!d.contains("scoped-token"));
        assert!(d.contains("<redacted>"));
    }

    #[tokio::test]
    async fn connect_rejects_a_cloud_id_carrying_url_structure() {
        let err = ScopedTokenStrategy::connect(
            None,
            "u@x.com".into(),
            "t".into(),
            Some("abc/../evil?x=1".into()),
            &reqwest::Client::new(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("Invalid cloud_id"), "{err}");
    }

    #[tokio::test]
    async fn discovery_without_a_domain_names_both_ways_out() {
        let err = ScopedTokenStrategy::connect(
            None,
            "u@x.com".into(),
            "t".into(),
            None,
            &reqwest::Client::new(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("cloud_id"), "{err}");
        assert!(err.contains("domain"), "{err}");
    }

    #[tokio::test]
    async fn discovery_rejects_a_spoofed_domain_before_any_request() {
        let err = ScopedTokenStrategy::connect(
            Some("https://evil.com/foo.atlassian.net"),
            "u@x.com".into(),
            "t".into(),
            None,
            &reqwest::Client::new(),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("Invalid Atlassian domain"), "{err}");
        // Failing at the validator rather than at the socket is the point: if
        // the check were dropped this would become a live-network test that
        // passes or flakes on someone else's DNS instead of failing here.
        assert!(!err.contains("Failed to reach"), "{err}");
    }

    /// A pinned cloud_id makes the site host irrelevant: no domain is needed
    /// and no request is issued. The absence of a mock server here is the
    /// assertion — any HTTP attempt would have nowhere to go.
    #[tokio::test]
    async fn a_pinned_cloud_id_short_circuits_discovery() {
        let s = ScopedTokenStrategy::connect(
            None,
            "u@x.com".into(),
            "t".into(),
            Some("cloud-abc-123".into()),
            &reqwest::Client::new(),
        )
        .await
        .unwrap();
        assert_eq!(s.cloud_id(), Some("cloud-abc-123"));
    }

    /// The load-bearing property of the lookup: it reaches the site origin
    /// carrying no credential at all.
    #[tokio::test]
    async fn discovery_reads_the_cloud_id_and_sends_no_credential() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(TENANT_INFO_PATH))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"cloudId": "cloud-9"})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let id = ScopedTokenStrategy::fetch_cloud_id(&server.uri(), &reqwest::Client::new())
            .await
            .unwrap();
        assert_eq!(id, "cloud-9");

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0].headers.get("authorization").is_none(),
            "the site origin must never receive the credential"
        );
    }

    #[tokio::test]
    async fn discovery_failure_points_at_pinning_the_value() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(TENANT_INFO_PATH))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let err = ScopedTokenStrategy::fetch_cloud_id(&server.uri(), &reqwest::Client::new())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("403"), "{err}");
        assert!(err.contains("cloud_id"), "{err}");
    }

    /// A cloud id is server-supplied, so the shared `validate_cloud_id` that
    /// `connect` applies to both branches is what stops a hostile one.
    #[tokio::test]
    async fn a_cloud_id_from_the_response_is_still_subject_to_validation() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(TENANT_INFO_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"cloudId": "abc/../../evil"})),
            )
            .mount(&server)
            .await;

        let raw = ScopedTokenStrategy::fetch_cloud_id(&server.uri(), &reqwest::Client::new())
            .await
            .unwrap();
        assert_eq!(raw, "abc/../../evil");
        assert!(crate::config::validate_cloud_id(&raw).is_err());
    }

    #[tokio::test]
    async fn discovery_reports_a_malformed_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(TENANT_INFO_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let err = ScopedTokenStrategy::fetch_cloud_id(&server.uri(), &reqwest::Client::new())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("cloud_id"), "{err}");
    }
}
