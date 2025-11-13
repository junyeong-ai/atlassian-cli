//! Shared test utilities for all modules
//!
//! This module provides common test helpers to avoid code duplication
//! across test modules.

#[cfg(test)]
use crate::config::{Config, PerformanceConfig};

/// Creates a test configuration with sensible defaults
#[cfg(test)]
pub fn create_test_config() -> Config {
    Config {
        domain: Some("test.atlassian.net".to_string()),
        email: Some("test@example.com".to_string()),
        token: Some("token123".to_string()),
        base_url: "https://test.atlassian.net".to_string(),
        performance: PerformanceConfig {
            request_timeout_ms: 30000,
        },
        ..Default::default()
    }
}

/// Creates a test configuration with custom field filtering
///
/// # Arguments
/// * `default_fields` - Optional default fields override
/// * `custom_fields` - Additional custom fields
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
///
/// # Arguments
/// * `projects` - Project keys to filter
/// * `spaces` - Space keys to filter
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
        assert_eq!(config.email, Some("test@example.com".to_string()));
        assert_eq!(config.token, Some("token123".to_string()));
        assert_eq!(config.base_url, "https://test.atlassian.net");
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
