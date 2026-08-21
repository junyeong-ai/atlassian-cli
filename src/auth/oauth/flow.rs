//! OAuth 2.0 Authorization Code + PKCE flow against Atlassian.
//!
//! Atlassian-specific bits:
//! - `audience=api.atlassian.com` + `prompt=consent` extra authorize params
//! - `offline_access` scope REQUIRED for refresh tokens
//! - Cloud ID discovered post-exchange via accessible-resources
//! - Refresh tokens **rotate** — caller must persist whichever value comes back
//!
//! Token-endpoint traffic runs on the binary's own `reqwest`/rustls stack,
//! bridged into the `oauth2` crate's generic `AsyncHttpClient` interface by
//! [`perform_oauth_request`] (the crate's bundled `reqwest` feature stays
//! disabled so the dependency tree carries a single reqwest).

use super::callback;
use super::store::TokenSet;
use crate::auth::DEFAULT_TOKEN_LIFETIME_SECS;
use anyhow::{Context, Result, bail};
use oauth2::basic::{BasicClient, BasicTokenResponse};
use oauth2::http;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) const AUTHORIZE_URL: &str = "https://auth.atlassian.com/authorize";
pub(super) const TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
pub(super) const ACCESSIBLE_RESOURCES_URL: &str =
    "https://api.atlassian.com/oauth/token/accessible-resources";

/// What `auth login` reports back to the caller.
pub struct LoginOutcome {
    pub tokens: TokenSet,
    pub authorized_sites: Vec<SiteInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SiteInfo {
    pub id: String,
    pub url: String,
    pub name: Option<String>,
}

/// Inputs to `login` / `refresh`. Borrowed for the duration of the call so we
/// do not duplicate `AuthConfig::OAuth`'s fields into a parallel struct.
pub struct FlowInputs<'a> {
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub redirect_port: u16,
    pub scopes: &'a [String],
    pub cloud_id_pin: Option<&'a str>,
}

type ConfiguredClient = BasicClient<
    EndpointSet,    // auth_uri set
    EndpointNotSet, // device_authorization_url not set
    EndpointNotSet, // introspection_url not set
    EndpointNotSet, // revocation_url not set
    EndpointSet,    // token_uri set
>;

pub(super) fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{}/callback", port)
}

fn build_client(inputs: &FlowInputs<'_>) -> Result<ConfiguredClient> {
    Ok(
        BasicClient::new(ClientId::new(inputs.client_id.to_string()))
            .set_client_secret(ClientSecret::new(inputs.client_secret.to_string()))
            .set_auth_uri(AuthUrl::new(AUTHORIZE_URL.to_string())?)
            .set_token_uri(TokenUrl::new(TOKEN_URL.to_string())?)
            .set_redirect_uri(RedirectUrl::new(redirect_uri(inputs.redirect_port))?),
    )
}

fn oauth_http_client() -> Result<reqwest::Client> {
    // Atlassian's auth.atlassian.com handles redirects internally; disabling
    // here matches the oauth2 crate's recommendation and prevents leaking
    // credentials to a redirected host.
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("Failed to build OAuth HTTP client")
}

/// Execute one `oauth2`-crate request and reshape the reply into the crate's
/// generic `HttpResponse`. Both sides speak the same `http` 1.x types, so the
/// bridge is a direct move of method, URI, headers, and body bytes; the
/// closure form `|req| perform_oauth_request(client.clone(), req)` satisfies
/// the crate's `AsyncHttpClient` blanket impl.
async fn perform_oauth_request(
    client: reqwest::Client,
    request: http::Request<Vec<u8>>,
) -> Result<http::Response<Vec<u8>>, reqwest::Error> {
    let (parts, body) = request.into_parts();
    let response = client
        .request(parts.method, parts.uri.to_string())
        .headers(parts.headers)
        .body(body)
        .send()
        .await?;

    let status = response.status();
    let headers = response.headers().clone();
    let body = response.bytes().await?.to_vec();

    let mut converted = http::Response::new(body);
    *converted.status_mut() = status;
    *converted.headers_mut() = headers;
    Ok(converted)
}

/// Run the full interactive login: authorize → redirect → code → token → site discovery.
pub async fn login(
    inputs: &FlowInputs<'_>,
    api_http: &reqwest::Client,
    open_browser: bool,
) -> Result<LoginOutcome> {
    let client = build_client(inputs)?;
    let oa_http = oauth_http_client()?;
    let listener = callback::bind(inputs.redirect_port).await?;

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let mut authorize = client
        .authorize_url(CsrfToken::new_random)
        .add_extra_param("audience", "api.atlassian.com")
        .add_extra_param("prompt", "consent")
        .set_pkce_challenge(pkce_challenge);
    for scope in inputs.scopes {
        authorize = authorize.add_scope(Scope::new(scope.clone()));
    }
    let (auth_url, csrf) = authorize.url();

    let redirect = redirect_uri(inputs.redirect_port);
    if open_browser && webbrowser::open(auth_url.as_str()).is_ok() {
        eprintln!("Opened browser for sign-in. Waiting on {} …", redirect);
    } else {
        eprintln!(
            "Open this URL in your browser to sign in:\n  {}\n\nWaiting on {} …",
            auth_url, redirect
        );
    }

    let result = callback::receive(listener, csrf.secret()).await?;

    let send = |req| perform_oauth_request(oa_http.clone(), req);
    let token_response: BasicTokenResponse = client
        .exchange_code(AuthorizationCode::new(result.code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(&send)
        .await
        .context("Failed to exchange authorization code for tokens")?;

    let sites =
        fetch_accessible_resources(api_http, token_response.access_token().secret()).await?;

    let cloud_id = resolve_cloud_id(inputs.cloud_id_pin, &sites)?;
    let tokens = to_token_set(&token_response, inputs.scopes, cloud_id);
    Ok(LoginOutcome {
        tokens,
        authorized_sites: sites,
    })
}

/// Exchange the stored refresh_token for a fresh access_token (and possibly a
/// rotated refresh_token). Atlassian rotates refresh tokens — callers MUST
/// overwrite stored state with the returned set.
pub async fn refresh(
    inputs: &FlowInputs<'_>,
    refresh_token: &SecretString,
    existing_cloud_id: String,
) -> Result<TokenSet> {
    let client = build_client(inputs)?;
    let oa_http = oauth_http_client()?;
    let send = |req| perform_oauth_request(oa_http.clone(), req);
    let token_response: BasicTokenResponse = client
        .exchange_refresh_token(&RefreshToken::new(
            refresh_token.expose_secret().to_string(),
        ))
        .request_async(&send)
        .await
        .context(
            "Failed to refresh OAuth token (refresh_token may be expired — try `auth login`)",
        )?;
    Ok(to_token_set(
        &token_response,
        inputs.scopes,
        existing_cloud_id,
    ))
}

async fn fetch_accessible_resources(
    http: &reqwest::Client,
    access_token: &str,
) -> Result<Vec<SiteInfo>> {
    let response = http
        .get(ACCESSIBLE_RESOURCES_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .context("Failed to call accessible-resources")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("accessible-resources failed ({}): {}", status, body);
    }
    response
        .json::<Vec<SiteInfo>>()
        .await
        .context("Failed to parse accessible-resources response")
}

fn resolve_cloud_id(pin: Option<&str>, sites: &[SiteInfo]) -> Result<String> {
    // Validated here, where both a pinned and a discovered id pass through:
    // `OAuthStrategy::resume` checks it too, but that is the next command. A
    // login that stored an id the proxy path will reject reports success and
    // leaves a session nothing can use — and an id from `accessible-resources`
    // is no more trusted for having come from an API than a pinned one is.
    let checked = |id: String| crate::config::validate_cloud_id(&id).map(|()| id);
    match (pin, sites) {
        (Some(p), _) => checked(p.to_string()),
        (None, []) => bail!("Login succeeded but no Atlassian sites are accessible to this user"),
        (None, [only]) => checked(only.id.clone()),
        (None, many) => {
            let list = many
                .iter()
                .map(|s| format!("  - {} ({})", s.url, s.id))
                .collect::<Vec<_>>()
                .join("\n");
            bail!(
                "Logged in, but multiple Atlassian sites are accessible. Pin one by setting cloud_id in [<profile>.auth]:\n{}",
                list
            )
        }
    }
}

fn to_token_set(
    token: &BasicTokenResponse,
    requested_scopes: &[String],
    cloud_id: String,
) -> TokenSet {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let lifetime = token
        .expires_in()
        .unwrap_or(Duration::from_secs(DEFAULT_TOKEN_LIFETIME_SECS));
    TokenSet {
        access_token: SecretString::new(token.access_token().secret().clone().into()),
        refresh_token: token
            .refresh_token()
            .map(|r| SecretString::new(r.secret().clone().into())),
        expires_at_unix: now_unix + lifetime.as_secs() as i64,
        scopes: token
            .scopes()
            .map(|s| s.iter().map(|s| s.to_string()).collect())
            .unwrap_or_else(|| requested_scopes.to_vec()),
        cloud_id: Some(cloud_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uri_uses_loopback_ip() {
        assert_eq!(redirect_uri(8976), "http://127.0.0.1:8976/callback");
    }

    #[tokio::test]
    async fn perform_oauth_request_round_trips_method_headers_and_body() {
        use wiremock::matchers::{body_string, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(header("Content-Type", "application/x-www-form-urlencoded"))
            .and(body_string("grant_type=authorization_code&code=abc"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_raw(r#"{"error":"invalid_grant"}"#, "application/json"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let request = http::Request::builder()
            .method("POST")
            .uri(format!("{}/oauth/token", server.uri()))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(b"grant_type=authorization_code&code=abc".to_vec())
            .unwrap();

        let response = perform_oauth_request(oauth_http_client().unwrap(), request)
            .await
            .unwrap();

        // Non-2xx must pass through untouched — the oauth2 crate parses the
        // RFC 6749 error body itself.
        assert_eq!(response.status(), 400);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(response.body(), br#"{"error":"invalid_grant"}"#);
    }

    #[tokio::test]
    async fn perform_oauth_request_does_not_follow_redirects() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("Location", "https://attacker.example/"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let request = http::Request::builder()
            .method("POST")
            .uri(format!("{}/oauth/token", server.uri()))
            .body(Vec::new())
            .unwrap();

        let response = perform_oauth_request(oauth_http_client().unwrap(), request)
            .await
            .unwrap();

        // Following the redirect would replay the client credentials to the
        // Location target; the 302 must surface as-is instead.
        assert_eq!(response.status(), 302);
    }

    /// A login that stored an id the proxy path rejects would report success
    /// and leave a session the next command cannot use. Neither source is
    /// trusted for where it came from.
    #[test]
    fn a_cloud_id_that_cannot_reach_the_proxy_fails_the_login() {
        assert!(resolve_cloud_id(Some("../evil"), &[]).is_err());
        let sites = vec![SiteInfo {
            id: "bad/id".into(),
            url: "https://x.atlassian.net".into(),
            name: None,
        }];
        assert!(resolve_cloud_id(None, &sites).is_err());
    }

    #[test]
    fn resolve_cloud_id_prefers_pin() {
        let sites = vec![SiteInfo {
            id: "auto".into(),
            url: "https://auto.atlassian.net".into(),
            name: None,
        }];
        assert_eq!(resolve_cloud_id(Some("pinned"), &sites).unwrap(), "pinned");
    }

    #[test]
    fn resolve_cloud_id_single_site() {
        let sites = vec![SiteInfo {
            id: "abc".into(),
            url: "https://x.atlassian.net".into(),
            name: None,
        }];
        assert_eq!(resolve_cloud_id(None, &sites).unwrap(), "abc");
    }

    #[test]
    fn resolve_cloud_id_zero_sites_errors() {
        assert!(resolve_cloud_id(None, &[]).is_err());
    }

    #[test]
    fn resolve_cloud_id_multi_sites_errors_with_list() {
        let sites = vec![
            SiteInfo {
                id: "a".into(),
                url: "https://a.atlassian.net".into(),
                name: None,
            },
            SiteInfo {
                id: "b".into(),
                url: "https://b.atlassian.net".into(),
                name: None,
            },
        ];
        let err = resolve_cloud_id(None, &sites).unwrap_err().to_string();
        assert!(err.contains("a.atlassian.net"));
        assert!(err.contains("b.atlassian.net"));
        assert!(err.contains("cloud_id"));
    }
}
