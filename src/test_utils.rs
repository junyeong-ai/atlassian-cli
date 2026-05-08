//! Shared test utilities for all modules

#[cfg(test)]
use crate::auth::AuthConfig;
#[cfg(test)]
use crate::config::{Config, PerformanceConfig};

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
