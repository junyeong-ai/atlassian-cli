//! Which release is the latest one, and fetching what it published.

use std::time::Duration;

use semver::Version;
use serde::Deserialize;

use super::DistError;

pub const REPO: &str = "junyeong-ai/atlassian-cli";

const API_BASE: &str = "https://api.github.com";
const WEB_BASE: &str = "https://github.com";

/// Which source answered when asked what the latest release is.
///
/// Carried rather than discarded because the two do not always agree, and the
/// disagreement runs one way: the web view trails the API by minutes after a
/// release is published, which is exactly when someone runs an update. Read in
/// that window it names the release before, and an update built on it calls the
/// running binary current — a wrong answer delivered with the confidence of a
/// right one. So the API settles it, the web view answers only where the API
/// could not, and an answer from the trailing source says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    Api,
    WebRedirect,
}

impl Provenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Provenance::Api => "api",
            Provenance::WebRedirect => "web-redirect",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Latest {
    pub version: Version,
    pub provenance: Provenance,
}

/// The version a release tag names.
pub fn parse_tag(tag: &str) -> Result<Version, DistError> {
    let version = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(version)
        .map_err(|e| DistError::Release(format!("`{tag}` does not name a version: {e}")))
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
}

pub struct ReleaseClient {
    http: reqwest::Client,
    api_base: String,
    web_base: String,
}

impl ReleaseClient {
    /// The client that talks to the real GitHub.
    pub fn github() -> Result<Self, DistError> {
        Self::build(API_BASE, WEB_BASE)
    }

    /// A client pointed at one origin, for driving the same code against a test
    /// server. The two bases differ only in host on GitHub and the paths below
    /// them do not collide, so a single origin serves both roles.
    #[cfg(test)]
    pub fn at(base: &str) -> Result<Self, DistError> {
        Self::build(base, base)
    }

    fn build(api_base: &str, web_base: &str) -> Result<Self, DistError> {
        let http = reqwest::Client::builder()
            // GitHub rejects an API request without one, and the version
            // identifies which build asked when a rate limit has to be traced.
            .user_agent(concat!("atlassian-cli/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(300))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| DistError::Network(format!("could not build an HTTP client: {e}")))?;
        Ok(Self {
            http,
            api_base: api_base.trim_end_matches('/').to_string(),
            web_base: web_base.trim_end_matches('/').to_string(),
        })
    }

    /// The download URL of one asset of one release.
    pub fn asset_url(&self, version: &Version, asset: &str) -> String {
        format!(
            "{}/{REPO}/releases/download/v{version}/{asset}",
            self.web_base
        )
    }

    /// The latest published release, and which source said so.
    pub async fn resolve_latest(&self) -> Result<Latest, DistError> {
        match self.latest_from_api().await {
            Ok(version) => Ok(Latest {
                version,
                provenance: Provenance::Api,
            }),
            // The API's failure is the one worth reporting: the web view is the
            // fallback, so its failure explains only that the fallback also did
            // not work.
            Err(from_api) => match self.latest_from_web().await {
                Ok(version) => Ok(Latest {
                    version,
                    provenance: Provenance::WebRedirect,
                }),
                Err(_) => Err(from_api),
            },
        }
    }

    async fn latest_from_api(&self) -> Result<Version, DistError> {
        let url = format!("{}/repos/{REPO}/releases/latest", self.api_base);
        let release: ApiRelease = self
            .http
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| DistError::Network(format!("asking GitHub for the latest release: {e}")))?
            .error_for_status()
            .map_err(|e| DistError::Release(format!("GitHub did not answer with a release: {e}")))?
            .json()
            .await
            .map_err(|e| {
                DistError::Release(format!("GitHub's answer did not name a release tag: {e}"))
            })?;
        parse_tag(&release.tag_name)
    }

    /// Read the version off wherever `/releases/latest` lands.
    async fn latest_from_web(&self) -> Result<Version, DistError> {
        let url = format!("{}/{REPO}/releases/latest", self.web_base);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| DistError::Network(format!("resolving the latest release page: {e}")))?
            .error_for_status()
            .map_err(|e| {
                DistError::Release(format!("the latest release page did not answer: {e}"))
            })?;
        let landed = response.url().as_str().to_string();
        let tag = landed
            .split_once("/releases/tag/")
            .and_then(|(_, rest)| rest.split(['/', '?', '#']).next())
            .filter(|tag| !tag.is_empty())
            .ok_or_else(|| DistError::Release(format!("`{landed}` does not name a release tag")))?;
        parse_tag(tag)
    }

    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>, DistError> {
        Ok(self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| DistError::Network(format!("downloading {url}: {e}")))?
            .error_for_status()
            .map_err(|e| DistError::Release(format!("{url} could not be downloaded: {e}")))?
            .bytes()
            .await
            .map_err(|e| DistError::Network(format!("reading {url}: {e}")))?
            .to_vec())
    }

    pub async fn fetch_text(&self, url: &str) -> Result<String, DistError> {
        String::from_utf8(self.fetch(url).await?)
            .map_err(|e| DistError::Release(format!("{url} is not text: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn a_tag_names_a_version_with_or_without_its_v() {
        assert_eq!(parse_tag("v0.10.0").unwrap(), Version::new(0, 10, 0));
        assert_eq!(parse_tag("0.10.0").unwrap(), Version::new(0, 10, 0));
        assert!(parse_tag("v").is_err());
        assert!(parse_tag("latest").is_err());
    }

    #[tokio::test]
    async fn the_api_settles_which_release_is_latest() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/repos/{REPO}/releases/latest")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "html_url": format!("https://github.com/{REPO}/releases/tag/v0.9.0"),
                "tag_name": "v0.10.1"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let latest = ReleaseClient::at(&server.uri())
            .unwrap()
            .resolve_latest()
            .await
            .unwrap();
        // Read from the release object's own tag, not from a URL elsewhere in
        // the body that carries a tag of its own.
        assert_eq!(latest.version, Version::new(0, 10, 1));
        assert_eq!(latest.provenance, Provenance::Api);
    }

    #[tokio::test]
    async fn the_web_view_answers_only_where_the_api_could_not() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/repos/{REPO}/releases/latest")))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{REPO}/releases/latest")))
            .respond_with(ResponseTemplate::new(302).insert_header(
                "location",
                format!("{}/{REPO}/releases/tag/v0.10.0", server.uri()).as_str(),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/{REPO}/releases/tag/v0.10.0")))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let latest = ReleaseClient::at(&server.uri())
            .unwrap()
            .resolve_latest()
            .await
            .unwrap();
        assert_eq!(latest.version, Version::new(0, 10, 0));
        // The caller has to be able to say the answer came from the source that
        // trails, because that is the one that can be wrong.
        assert_eq!(latest.provenance, Provenance::WebRedirect);
    }

    #[tokio::test]
    async fn a_release_channel_that_answers_nothing_reports_the_api_failure() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let err = ReleaseClient::at(&server.uri())
            .unwrap()
            .resolve_latest()
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("did not answer with a release"), "got: {err}");
    }

    #[tokio::test]
    async fn an_asset_is_fetched_from_the_release_it_belongs_to() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/{REPO}/releases/download/v0.10.0/atlassian-cli-v0.10.0-x.tar.gz"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"archive".to_vec()))
            .expect(1)
            .mount(&server)
            .await;

        let client = ReleaseClient::at(&server.uri()).unwrap();
        let url = client.asset_url(&Version::new(0, 10, 0), "atlassian-cli-v0.10.0-x.tar.gz");
        assert_eq!(client.fetch(&url).await.unwrap(), b"archive");
    }
}
