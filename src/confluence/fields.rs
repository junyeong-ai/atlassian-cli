/// Configuration for Confluence API v2 parameters
#[derive(Debug, Clone)]
pub struct FieldConfiguration {
    pub body_format: Option<String>,
    pub include_version: bool,
    pub include_labels: bool,
    pub include_properties: bool,
    pub include_operations: bool,
    pub custom_includes: Vec<String>,
    pub include_all: bool,
}

impl FieldConfiguration {
    /// Create a new field configuration with essential parameters for v2 API
    pub fn from_env() -> Self {
        let custom_includes_env = std::env::var("CONFLUENCE_CUSTOM_INCLUDES").unwrap_or_default();
        let custom_includes: Vec<String> = if custom_includes_env.is_empty() {
            vec![]
        } else {
            custom_includes_env
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string())
                .collect()
        };

        if !custom_includes.is_empty() {
            tracing::info!(
                "Loaded {} custom include parameters: {:?}",
                custom_includes.len(),
                custom_includes
            );
        }

        Self {
            body_format: Some("storage".to_string()),
            include_version: true,
            include_labels: false,
            include_properties: false,
            include_operations: false,
            custom_includes,
            include_all: false,
        }
    }

    /// Create a configuration that includes all fields
    pub fn all_fields() -> Self {
        Self {
            body_format: Some("storage".to_string()),
            include_version: true,
            include_labels: true,
            include_properties: true,
            include_operations: true,
            custom_includes: vec![],
            include_all: true,
        }
    }

    /// Override to include additional parameters for a specific request.
    /// Consumes self and returns a modified configuration (builder pattern).
    pub fn with_additional_includes(mut self, additional: Vec<String>) -> Self {
        for param in additional {
            if !self.custom_includes.contains(&param) {
                self.custom_includes.push(param);
            }
        }
        self
    }

    /// Get query parameters as a vector of tuples for v2 API
    pub fn to_query_params(&self) -> Vec<(String, String)> {
        let mut params = Vec::new();

        // Add body-format parameter
        if let Some(ref format) = self.body_format {
            params.push(("body-format".to_string(), format.clone()));
        }

        // Add include-* boolean parameters
        if self.include_version {
            params.push(("include-version".to_string(), "true".to_string()));
        }
        if self.include_labels || self.include_all {
            params.push(("include-labels".to_string(), "true".to_string()));
        }
        if self.include_properties || self.include_all {
            params.push(("include-properties".to_string(), "true".to_string()));
        }
        if self.include_operations || self.include_all {
            params.push(("include-operations".to_string(), "true".to_string()));
        }

        // Add custom include parameters
        for param in &self.custom_includes {
            params.push((format!("include-{}", param), "true".to_string()));
        }

        params
    }
}

/// Helper function to apply field filtering for v2 API
pub fn apply_v2_filtering(
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
) -> Vec<(String, String)> {
    if include_all_fields.unwrap_or(false) {
        tracing::debug!("Field filtering disabled: include_all_fields=true");
        return FieldConfiguration::all_fields().to_query_params();
    }

    let mut config = FieldConfiguration::from_env();
    tracing::debug!(
        "Loaded {} custom include parameters from environment",
        config.custom_includes.len()
    );

    if let Some(additional) = additional_includes {
        tracing::debug!("Adding {} additional include parameters", additional.len());
        config = config.with_additional_includes(additional);
    }

    let params = config.to_query_params();

    tracing::debug!("Applying v2 filtering with {} parameters", params.len());

    params
}

/// Legacy helper for v1 search endpoint (still uses expand parameter)
pub fn apply_expand_filtering(
    url: &str,
    include_all_fields: Option<bool>,
    additional_expand: Option<Vec<String>>,
) -> (String, Option<String>) {
    // For v1 search API, we still use expand parameter
    let expand_params = if include_all_fields.unwrap_or(false) {
        vec!["body.storage", "version", "space", "history", "metadata"]
    } else {
        vec!["body.storage", "version"]
    };

    let mut expand = expand_params
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    if let Some(additional) = additional_expand {
        for param in additional {
            if !expand.contains(&param) {
                expand.push(param);
            }
        }
    }

    (url.to_string(), Some(expand.join(",")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // T012: Confluence field filtering tests

    #[test]
    fn test_field_configuration_default_params() {
        // Test default v2 API parameters
        unsafe {
            std::env::remove_var("CONFLUENCE_CUSTOM_INCLUDES");
        }
        let config = FieldConfiguration::from_env();

        assert_eq!(config.body_format, Some("storage".to_string()));
        assert!(config.include_version);
        assert!(!config.include_labels);
        assert!(!config.include_properties);
        assert!(!config.include_operations);
        assert!(config.custom_includes.is_empty());
        assert!(!config.include_all);
    }

    #[test]
    fn test_field_configuration_all_fields() {
        let config = FieldConfiguration::all_fields();

        assert_eq!(config.body_format, Some("storage".to_string()));
        assert!(config.include_version);
        assert!(config.include_labels);
        assert!(config.include_properties);
        assert!(config.include_operations);
        assert!(config.include_all);
    }

    #[test]
    fn test_to_query_params_default() {
        unsafe {
            std::env::remove_var("CONFLUENCE_CUSTOM_INCLUDES");
        }
        let config = FieldConfiguration::from_env();
        let params = config.to_query_params();

        assert_eq!(params.len(), 2);
        assert!(params.contains(&("body-format".to_string(), "storage".to_string())));
        assert!(params.contains(&("include-version".to_string(), "true".to_string())));
    }

    #[test]
    fn test_to_query_params_all_fields() {
        let config = FieldConfiguration::all_fields();
        let params = config.to_query_params();

        // body-format, include-version, include-labels, include-properties, include-operations
        assert_eq!(params.len(), 5);
        assert!(params.contains(&("body-format".to_string(), "storage".to_string())));
        assert!(params.contains(&("include-version".to_string(), "true".to_string())));
        assert!(params.contains(&("include-labels".to_string(), "true".to_string())));
        assert!(params.contains(&("include-properties".to_string(), "true".to_string())));
        assert!(params.contains(&("include-operations".to_string(), "true".to_string())));
    }

    #[test]
    fn test_with_additional_includes() {
        unsafe {
            std::env::remove_var("CONFLUENCE_CUSTOM_INCLUDES");
        }
        let config = FieldConfiguration::from_env();
        let updated =
            config.with_additional_includes(vec!["ancestors".to_string(), "children".to_string()]);

        assert_eq!(updated.custom_includes.len(), 2);
        assert!(updated.custom_includes.contains(&"ancestors".to_string()));
        assert!(updated.custom_includes.contains(&"children".to_string()));
    }

    #[test]
    fn test_custom_includes_as_query_params() {
        let mut config = FieldConfiguration::from_env();
        config.custom_includes = vec!["ancestors".to_string(), "history".to_string()];
        let params = config.to_query_params();

        // body-format, include-version, include-ancestors, include-history
        assert_eq!(params.len(), 4);
        assert!(params.contains(&("include-ancestors".to_string(), "true".to_string())));
        assert!(params.contains(&("include-history".to_string(), "true".to_string())));
    }

    #[test]
    fn test_apply_v2_filtering_default() {
        unsafe {
            std::env::remove_var("CONFLUENCE_CUSTOM_INCLUDES");
        }
        let params = apply_v2_filtering(None, None);

        assert_eq!(params.len(), 2);
        assert!(params.contains(&("body-format".to_string(), "storage".to_string())));
        assert!(params.contains(&("include-version".to_string(), "true".to_string())));
    }

    #[test]
    fn test_apply_v2_filtering_all_fields() {
        let params = apply_v2_filtering(Some(true), None);

        // Should use all_fields configuration
        assert_eq!(params.len(), 5);
        assert!(params.contains(&("include-labels".to_string(), "true".to_string())));
        assert!(params.contains(&("include-properties".to_string(), "true".to_string())));
    }

    #[test]
    fn test_apply_v2_filtering_with_additional() {
        unsafe {
            std::env::remove_var("CONFLUENCE_CUSTOM_INCLUDES");
        }
        let additional = vec!["ancestors".to_string(), "history".to_string()];
        let params = apply_v2_filtering(None, Some(additional));

        // body-format, include-version, include-ancestors, include-history
        assert_eq!(params.len(), 4);
        assert!(params.contains(&("include-ancestors".to_string(), "true".to_string())));
        assert!(params.contains(&("include-history".to_string(), "true".to_string())));
    }

    #[test]
    fn test_apply_expand_filtering_default() {
        let (url, expand) = apply_expand_filtering(
            "https://test.atlassian.net/wiki/rest/api/search",
            None,
            None,
        );

        assert_eq!(url, "https://test.atlassian.net/wiki/rest/api/search");
        assert_eq!(expand, Some("body.storage,version".to_string()));
    }

    #[test]
    fn test_apply_expand_filtering_all_fields() {
        let (url, expand) = apply_expand_filtering(
            "https://test.atlassian.net/wiki/rest/api/search",
            Some(true),
            None,
        );

        assert_eq!(url, "https://test.atlassian.net/wiki/rest/api/search");
        assert_eq!(
            expand,
            Some("body.storage,version,space,history,metadata".to_string())
        );
    }

    #[test]
    fn test_apply_expand_filtering_with_additional() {
        let additional = vec!["ancestors".to_string(), "children".to_string()];
        let (url, expand) = apply_expand_filtering(
            "https://test.atlassian.net/wiki/rest/api/search",
            None,
            Some(additional),
        );

        assert_eq!(url, "https://test.atlassian.net/wiki/rest/api/search");
        let expand_str = expand.unwrap();
        assert!(expand_str.contains("body.storage"));
        assert!(expand_str.contains("version"));
        assert!(expand_str.contains("ancestors"));
        assert!(expand_str.contains("children"));
    }

    #[test]
    fn test_field_configuration_to_query_params() {
        let config = FieldConfiguration::from_env();
        let params = config.to_query_params();

        assert_eq!(params.len(), 2);
        assert!(params.contains(&("body-format".to_string(), "storage".to_string())));
    }

    #[test]
    fn test_field_configuration_all_fields_to_query_params() {
        let config = FieldConfiguration::all_fields();
        let params = config.to_query_params();

        assert_eq!(params.len(), 5);
        assert!(params.contains(&("include-labels".to_string(), "true".to_string())));
    }
}
