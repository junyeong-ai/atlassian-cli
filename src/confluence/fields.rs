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

impl Default for FieldConfiguration {
    fn default() -> Self {
        Self {
            body_format: Some("storage".to_string()),
            include_version: true,
            include_labels: false,
            include_properties: false,
            include_operations: false,
            custom_includes: vec![],
            include_all: false,
        }
    }
}

impl FieldConfiguration {
    pub fn from_env() -> Self {
        let custom_includes = std::env::var("CONFLUENCE_CUSTOM_INCLUDES")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.split(',')
                    .filter(|p| !p.is_empty())
                    .map(|p| p.trim().to_string())
                    .collect()
            })
            .unwrap_or_default();

        Self {
            custom_includes,
            ..Default::default()
        }
    }

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

    pub fn with_additional_includes(mut self, additional: Vec<String>) -> Self {
        for param in additional {
            if !self.custom_includes.contains(&param) {
                self.custom_includes.push(param);
            }
        }
        self
    }

    pub fn to_query_params(&self) -> Vec<(String, String)> {
        let mut params = Vec::new();

        if let Some(ref format) = self.body_format {
            params.push(("body-format".to_string(), format.clone()));
        }

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

        for param in &self.custom_includes {
            params.push((format!("include-{}", param), "true".to_string()));
        }

        params
    }
}

pub fn apply_v2_filtering(
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
) -> Vec<(String, String)> {
    if include_all_fields.unwrap_or(false) {
        return FieldConfiguration::all_fields().to_query_params();
    }

    let mut config = FieldConfiguration::from_env();

    if let Some(additional) = additional_includes {
        config = config.with_additional_includes(additional);
    }

    config.to_query_params()
}

pub fn build_search_expand(
    include_all_fields: Option<bool>,
    additional_expand: Option<Vec<String>>,
) -> String {
    let base_params = if include_all_fields.unwrap_or(false) {
        vec![
            "content.body.storage",
            "content.version",
            "content.space",
            "content.history",
            "content.metadata",
        ]
    } else {
        vec!["content.body.storage", "content.version"]
    };

    let mut expand: Vec<String> = base_params.iter().map(|s| s.to_string()).collect();

    if let Some(additional) = additional_expand {
        for param in additional {
            let prefixed = if param.starts_with("content.") {
                param
            } else {
                format!("content.{}", param)
            };
            if !expand.contains(&prefixed) {
                expand.push(prefixed);
            }
        }
    }

    expand.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_params() {
        let config = FieldConfiguration::default();
        assert_eq!(config.body_format, Some("storage".to_string()));
        assert!(config.include_version);
        assert!(!config.include_labels);
        assert!(config.custom_includes.is_empty());
    }

    #[test]
    fn test_all_fields() {
        let config = FieldConfiguration::all_fields();
        assert!(config.include_version);
        assert!(config.include_labels);
        assert!(config.include_properties);
        assert!(config.include_operations);
        assert!(config.include_all);
    }

    #[test]
    fn test_query_params_default() {
        let config = FieldConfiguration::default();
        let params = config.to_query_params();

        assert_eq!(params.len(), 2);
        assert!(params.contains(&("body-format".to_string(), "storage".to_string())));
        assert!(params.contains(&("include-version".to_string(), "true".to_string())));
    }

    #[test]
    fn test_query_params_all_fields() {
        let config = FieldConfiguration::all_fields();
        let params = config.to_query_params();

        assert_eq!(params.len(), 5);
        assert!(params.contains(&("include-labels".to_string(), "true".to_string())));
        assert!(params.contains(&("include-properties".to_string(), "true".to_string())));
        assert!(params.contains(&("include-operations".to_string(), "true".to_string())));
    }

    #[test]
    fn test_with_additional_includes() {
        let config = FieldConfiguration::default()
            .with_additional_includes(vec!["ancestors".to_string(), "children".to_string()]);

        assert_eq!(config.custom_includes.len(), 2);
        assert!(config.custom_includes.contains(&"ancestors".to_string()));
    }

    #[test]
    fn test_custom_includes_query_params() {
        let mut config = FieldConfiguration::default();
        config.custom_includes = vec!["ancestors".to_string(), "history".to_string()];
        let params = config.to_query_params();

        assert_eq!(params.len(), 4);
        assert!(params.contains(&("include-ancestors".to_string(), "true".to_string())));
        assert!(params.contains(&("include-history".to_string(), "true".to_string())));
    }

    #[test]
    fn test_search_expand_default() {
        let expand = build_search_expand(None, None);
        assert_eq!(expand, "content.body.storage,content.version");
    }

    #[test]
    fn test_search_expand_all_fields() {
        let expand = build_search_expand(Some(true), None);
        assert!(expand.contains("content.body.storage"));
        assert!(expand.contains("content.version"));
        assert!(expand.contains("content.space"));
        assert!(expand.contains("content.history"));
        assert!(expand.contains("content.metadata"));
    }

    #[test]
    fn test_search_expand_with_additional() {
        let additional = vec!["ancestors".to_string(), "children".to_string()];
        let expand = build_search_expand(None, Some(additional));
        assert!(expand.contains("content.ancestors"));
        assert!(expand.contains("content.children"));
    }

    #[test]
    fn test_search_expand_already_prefixed() {
        let additional = vec!["content.space".to_string()];
        let expand = build_search_expand(None, Some(additional));
        assert!(expand.contains("content.space"));
        assert_eq!(expand.matches("content.space").count(), 1);
    }
}
