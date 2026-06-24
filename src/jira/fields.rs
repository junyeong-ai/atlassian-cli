pub const DEFAULT_SEARCH_FIELDS: &[&str] = &[
    "key",
    "summary",
    "status",
    "priority",
    "issuetype",
    "assignee",
    "reporter",
    "creator",
    "created",
    "updated",
    "duedate",
    "resolutiondate",
    "project",
    "labels",
    "components",
    "parent",
    "subtasks",
];

pub fn resolve_search_fields(
    api_fields: Option<Vec<String>>,
    include_description: bool,
    config: &crate::config::Config,
) -> Vec<String> {
    if let Some(fields) = api_fields
        && !fields.is_empty()
    {
        return fields;
    }

    if let Some(ref env_defaults) = config.jira.search_default_fields {
        let mut fields = env_defaults.clone();
        if include_description && !fields.iter().any(|f| f == "description") {
            fields.push("description".to_string());
        }
        return fields;
    }

    let custom_count = config.jira.search_custom_fields.len();
    let extra = if include_description { 1 } else { 0 };
    let mut fields = Vec::with_capacity(DEFAULT_SEARCH_FIELDS.len() + custom_count + extra);

    fields.extend(DEFAULT_SEARCH_FIELDS.iter().map(|s| s.to_string()));

    if include_description {
        fields.push("description".to_string());
    }

    if custom_count > 0 {
        fields.extend_from_slice(&config.jira.search_custom_fields);
    }

    fields
}

pub const ESSENTIAL_FIELDS: &[&str] = &[
    "key",
    "summary",
    "description",
    "issuetype",
    "status",
    "priority",
    "assignee",
    "reporter",
    "created",
    "updated",
    "project",
];

/// Resolve which fields a single-issue `get` should request. An explicit
/// `--fields` list (including `*all` for the full issue) wins; otherwise the
/// essential set is returned, extended with any configured custom fields so a
/// single-issue fetch is at least as detailed as a search.
pub fn resolve_get_fields(
    api_fields: Option<Vec<String>>,
    config: &crate::config::Config,
) -> Vec<String> {
    if let Some(fields) = api_fields
        && !fields.is_empty()
    {
        return fields;
    }

    let mut fields: Vec<String> = ESSENTIAL_FIELDS.iter().map(|s| s.to_string()).collect();
    fields.extend_from_slice(&config.jira.search_custom_fields);
    fields
}

/// Append a `fields=` selector and suppress the heavy rendered-fields expansion
/// on an issue URL. The selector is the caller's resolved field list (joined),
/// so a single hardwired whitelist no longer caps what `get` can return.
pub fn apply_field_filtering_to_url(base_url: &str, fields: &[String]) -> String {
    let fields = fields.join(",");

    let url_with_fields = if base_url.contains('?') {
        format!("{}&fields={}", base_url, fields)
    } else {
        format!("{}?fields={}", base_url, fields)
    };

    // Exclude heavy rendered fields
    format!("{}&expand=-renderedFields", url_with_fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_config_with_fields;

    #[test]
    fn test_default_search_fields_count() {
        assert_eq!(DEFAULT_SEARCH_FIELDS.len(), 17);
    }

    #[test]
    fn test_resolve_api_fields_override() {
        let config = create_test_config_with_fields(Some(vec!["key".to_string()]), vec![]);
        let api_fields = Some(vec!["key".to_string(), "summary".to_string()]);
        let result = resolve_search_fields(api_fields, false, &config);
        assert_eq!(result, vec!["key", "summary"]);
    }

    #[test]
    fn test_resolve_env_override() {
        let config = create_test_config_with_fields(
            Some(vec!["key".to_string(), "summary".to_string()]),
            vec![],
        );
        let result = resolve_search_fields(None, false, &config);
        assert_eq!(result, vec!["key", "summary"]);
    }

    #[test]
    fn test_resolve_env_with_description() {
        let config = create_test_config_with_fields(
            Some(vec!["key".to_string(), "summary".to_string()]),
            vec![],
        );
        let result = resolve_search_fields(None, true, &config);
        assert_eq!(result, vec!["key", "summary", "description"]);
    }

    #[test]
    fn test_resolve_defaults_with_custom() {
        let config = create_test_config_with_fields(None, vec!["customfield_10015".to_string()]);
        let result = resolve_search_fields(None, false, &config);
        assert_eq!(result.len(), 18);
        assert!(result.contains(&"customfield_10015".to_string()));
    }

    #[test]
    fn test_resolve_defaults_with_description() {
        let config = create_test_config_with_fields(None, vec![]);
        let result = resolve_search_fields(None, true, &config);
        assert_eq!(result.len(), 18);
        assert!(result.contains(&"description".to_string()));
    }

    #[test]
    fn test_resolve_defaults_only() {
        let config = create_test_config_with_fields(None, vec![]);
        let result = resolve_search_fields(None, false, &config);
        assert_eq!(result.len(), 17);
        assert!(!result.contains(&"description".to_string()));
    }

    #[test]
    fn test_resolve_empty_api_fields_fallback() {
        let config = create_test_config_with_fields(Some(vec!["key".to_string()]), vec![]);
        let result = resolve_search_fields(Some(vec![]), false, &config);
        assert_eq!(result, vec!["key"]);
    }

    #[test]
    fn test_essential_fields() {
        assert_eq!(ESSENTIAL_FIELDS.len(), 11);
        assert!(ESSENTIAL_FIELDS.contains(&"description"));
        assert!(ESSENTIAL_FIELDS.contains(&"key"));
    }

    fn essential() -> Vec<String> {
        ESSENTIAL_FIELDS.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_apply_field_filtering_to_url() {
        let base_url = "https://test.atlassian.net/rest/api/3/issue/TEST-123";
        let result = apply_field_filtering_to_url(base_url, &essential());
        assert!(result.contains("?fields="));
        assert!(result.contains("&expand=-renderedFields"));
        assert!(result.contains("key,summary,description"));
    }

    #[test]
    fn test_apply_field_filtering_with_existing_query() {
        let base_url = "https://test.atlassian.net/rest/api/3/issue/TEST-123?foo=bar";
        let result = apply_field_filtering_to_url(base_url, &essential());
        assert!(result.contains("&fields="));
        assert!(result.contains("foo=bar"));
    }

    #[test]
    fn test_apply_field_filtering_honors_explicit_selection() {
        // An explicit selection (e.g. `*all`) overrides the essential whitelist.
        let result = apply_field_filtering_to_url("/rest/api/3/issue/T-1", &["*all".to_string()]);
        assert!(result.contains("fields=*all"));
    }

    #[test]
    fn test_resolve_get_fields_explicit_wins() {
        let config = create_test_config_with_fields(None, vec!["customfield_1".to_string()]);
        let result = resolve_get_fields(Some(vec!["*all".to_string()]), &config);
        assert_eq!(result, vec!["*all"]);
    }

    #[test]
    fn test_resolve_get_fields_defaults_include_custom() {
        let config = create_test_config_with_fields(None, vec!["customfield_1".to_string()]);
        let result = resolve_get_fields(None, &config);
        assert_eq!(result.len(), ESSENTIAL_FIELDS.len() + 1);
        assert!(result.contains(&"customfield_1".to_string()));
        assert!(result.contains(&"description".to_string()));
    }
}
