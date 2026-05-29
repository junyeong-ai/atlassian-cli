//! Shared test utilities for all modules

#[cfg(test)]
use crate::auth::{AuthConfig, AuthMethod, AuthStrategy};
#[cfg(test)]
use crate::client::{ApiClient, Service};
#[cfg(test)]
use crate::config::{Config, PerformanceConfig};
#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
use std::sync::Arc;

/// Creates a test configuration with basic auth defaults
#[cfg(test)]
pub fn create_test_config() -> Config {
    Config {
        domain: Some("test.atlassian.net".to_string()),
        auth: Some(AuthConfig::Basic {
            email: "test@example.com".to_string(),
            token: "token123".to_string(),
        }),
        performance: PerformanceConfig {
            request_timeout_ms: 30000,
            rate_limit_delay_ms: 200,
        },
        ..Default::default()
    }
}

/// Creates a test configuration with Service account defaults
#[cfg(test)]
pub fn create_test_service_account_config() -> Config {
    Config {
        domain: None,
        auth: Some(AuthConfig::ServiceAccount {
            client_id: "test-client-id".to_string(),
            client_secret: "test-secret".to_string(),
            cloud_id: Some("test-cloud-id".to_string()),
        }),
        performance: PerformanceConfig {
            request_timeout_ms: 30000,
            rate_limit_delay_ms: 200,
        },
        ..Default::default()
    }
}

/// Creates a test configuration with custom field filtering
#[cfg(test)]
pub fn create_test_config_with_fields(
    default_fields: Option<Vec<String>>,
    custom_fields: Vec<String>,
) -> Config {
    let mut config = create_test_config();
    config.jira.search_default_fields = default_fields;
    config.jira.search_custom_fields = custom_fields;
    config
}

/// Creates a test configuration with project and space filters
#[cfg(test)]
pub fn create_test_config_with_filters(projects: Vec<String>, spaces: Vec<String>) -> Config {
    let mut config = create_test_config();
    config.jira.projects_filter = projects;
    config.confluence.spaces_filter = spaces;
    config
}

/// Auth strategy used in tests — routes every request at a fixed base URL.
/// Pair with `wiremock::MockServer` to drive real HTTP traffic through the
/// production `ApiClient` and verify request shape end-to-end.
#[cfg(test)]
#[derive(Debug)]
pub struct MockAuthStrategy {
    base_url: String,
}

#[cfg(test)]
impl MockAuthStrategy {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl AuthStrategy for MockAuthStrategy {
    fn method(&self) -> AuthMethod {
        AuthMethod::Basic
    }

    async fn authorization(&self, _http: &reqwest::Client) -> Result<String> {
        Ok("Bearer test-token".to_string())
    }

    fn build_url(&self, _service: Service, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn identity_label(&self) -> String {
        "mock".to_string()
    }
}

/// Build an `ApiClient` that talks to the given mock-server base URL.
#[cfg(test)]
pub fn mock_client(base_url: impl Into<String>) -> ApiClient {
    let strategy: Arc<dyn AuthStrategy> = Arc::new(MockAuthStrategy::new(base_url));
    ApiClient::new_with_strategy(strategy, create_test_config())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_test_config() {
        let config = create_test_config();
        assert_eq!(config.domain, Some("test.atlassian.net".to_string()));
        match &config.auth {
            Some(AuthConfig::Basic { email, token }) => {
                assert_eq!(email, "test@example.com");
                assert_eq!(token, "token123");
            }
            _ => panic!("Expected Basic auth"),
        }
    }

    #[test]
    fn test_create_test_service_account_config() {
        let config = create_test_service_account_config();
        assert!(config.domain.is_none());
        match &config.auth {
            Some(AuthConfig::ServiceAccount {
                client_id,
                cloud_id,
                ..
            }) => {
                assert_eq!(client_id, "test-client-id");
                assert_eq!(cloud_id, &Some("test-cloud-id".to_string()));
            }
            _ => panic!("Expected Service account auth"),
        }
    }

    #[test]
    fn test_create_test_config_with_fields() {
        let config = create_test_config_with_fields(
            Some(vec!["key".to_string(), "summary".to_string()]),
            vec!["customfield_10015".to_string()],
        );
        assert_eq!(
            config.jira.search_default_fields,
            Some(vec!["key".to_string(), "summary".to_string()])
        );
        assert_eq!(
            config.jira.search_custom_fields,
            vec!["customfield_10015".to_string()]
        );
    }

    #[test]
    fn test_create_test_config_with_filters() {
        let config =
            create_test_config_with_filters(vec!["PROJ1".to_string()], vec!["SPACE1".to_string()]);
        assert_eq!(config.jira.projects_filter, vec!["PROJ1".to_string()]);
        assert_eq!(config.confluence.spaces_filter, vec!["SPACE1".to_string()]);
    }
}
