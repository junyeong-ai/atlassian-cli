use serde_json::Value;

pub const DEFAULT_EXCLUDE_FIELDS: &[&str] = &[
    "avatarUrls",
    "iconUrl",
    "profilePicture",
    "icon",
    "self",
    "expand",
    "avatarId",
    "accountType",
    "projectTypeKey",
    "simplified",
    "_expandable",
    "_links",
    "childTypes",
    "macroRenderedOutput",
    "restrictions",
    "breadcrumbs",
    "entityType",
    "iconCssClass",
    "colorName",
    "hasScreen",
    "isAvailable",
    "isConditional",
    "isGlobal",
    "isInitial",
    "isLooped",
    "friendlyLastModified",
    "editui",
    "edituiv2",
    "ari",
    "base64EncodedAri",
    "confRev",
    "syncRev",
    "syncRevSource",
    "ncsStepVersion",
    "ncsStepVersionSource",
    "embeddedContent",
    "representation",
    "extensions",
];

pub fn apply(value: &mut Value, config: &crate::config::Config) {
    let exclude_fields = config
        .optimization
        .response_exclude_fields
        .as_ref()
        .map(|v| v.iter().map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_else(|| DEFAULT_EXCLUDE_FIELDS.to_vec());

    apply_recursive(value, &exclude_fields);
}

fn apply_recursive(value: &mut Value, exclude_fields: &[&str]) {
    match value {
        Value::Object(map) => {
            for field in exclude_fields {
                map.remove(*field);
            }
            map.retain(|_, v| !matches!(v, Value::String(s) if s.is_empty()));
            for nested in map.values_mut() {
                apply_recursive(nested, exclude_fields);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                apply_recursive(item, exclude_fields);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_config;
    use serde_json::json;

    #[test]
    fn test_default_fields_count() {
        assert_eq!(DEFAULT_EXCLUDE_FIELDS.len(), 38);
    }

    #[test]
    fn test_remove_excluded_fields() {
        let config = create_test_config();
        let mut data = json!({
            "name": "John",
            "avatarUrls": {"16x16": "url"},
            "self": "https://api"
        });

        apply(&mut data, &config);
        let obj = data.as_object().unwrap();
        assert!(obj.contains_key("name"));
        assert!(!obj.contains_key("avatarUrls"));
        assert!(!obj.contains_key("self"));
    }

    #[test]
    fn test_remove_empty_strings() {
        let config = create_test_config();
        let mut data = json!({
            "name": "",
            "status": null,
            "valid": "data"
        });

        apply(&mut data, &config);
        let obj = data.as_object().unwrap();
        assert!(!obj.contains_key("name"));
        assert!(obj.contains_key("status"));
        assert!(obj.contains_key("valid"));
    }

    #[test]
    fn test_recursive() {
        let config = create_test_config();
        let mut data = json!({
            "issues": [
                {"key": "P-1", "self": "url1"},
                {"key": "P-2", "self": "url2"}
            ]
        });

        apply(&mut data, &config);
        let issues = data["issues"].as_array().unwrap();
        assert!(!issues[0].as_object().unwrap().contains_key("self"));
        assert!(!issues[1].as_object().unwrap().contains_key("self"));
    }
}
