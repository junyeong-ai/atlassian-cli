use anyhow::Result;
use serde_json::Value;
use std::sync::OnceLock;

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
];

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub fields_removed: usize,
    pub empty_strings_removed: usize,
}

pub struct Filter {
    exclude_fields: Vec<String>,
}

impl Filter {
    pub fn new(config: &crate::config::Config) -> Self {
        let exclude_fields = config
            .optimization
            .response_exclude_fields
            .clone()
            .unwrap_or_else(|| {
                DEFAULT_EXCLUDE_FIELDS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });

        Self { exclude_fields }
    }

    #[cfg(not(test))]
    pub fn apply(&self, value: &mut Value) -> Result<()> {
        self.apply_recursive(value);
        Ok(())
    }

    #[cfg(test)]
    pub fn apply(&self, value: &mut Value) -> Result<Stats> {
        let mut stats = Stats::default();
        self.apply_recursive(value, &mut stats);
        Ok(stats)
    }

    #[cfg(not(test))]
    fn apply_recursive(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for field in &self.exclude_fields {
                    map.remove(field);
                }
                map.retain(|_, v| !matches!(v, Value::String(s) if s.is_empty()));
                for nested in map.values_mut() {
                    self.apply_recursive(nested);
                }
            }
            Value::Array(arr) => {
                for item in arr.iter_mut() {
                    self.apply_recursive(item);
                }
            }
            _ => {}
        }
    }

    #[cfg(test)]
    fn apply_recursive(&self, value: &mut Value, stats: &mut Stats) {
        match value {
            Value::Object(map) => {
                for field in &self.exclude_fields {
                    if map.remove(field).is_some() {
                        stats.fields_removed += 1;
                    }
                }
                map.retain(|_, v| {
                    if let Value::String(s) = v
                        && s.is_empty()
                    {
                        stats.empty_strings_removed += 1;
                        return false;
                    }
                    true
                });
                for nested in map.values_mut() {
                    self.apply_recursive(nested, stats);
                }
            }
            Value::Array(arr) => {
                for item in arr.iter_mut() {
                    self.apply_recursive(item, stats);
                }
            }
            _ => {}
        }
    }
}

static FILTER: OnceLock<Filter> = OnceLock::new();

pub fn apply(value: &mut Value, config: &crate::config::Config) -> Result<()> {
    let filter = FILTER.get_or_init(|| Filter::new(config));
    filter.apply(value)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_config;
    use serde_json::json;

    #[test]
    fn test_default_fields_count() {
        assert_eq!(DEFAULT_EXCLUDE_FIELDS.len(), 27);
    }

    #[test]
    fn test_remove_excluded_fields() {
        let config = create_test_config();
        let filter = Filter::new(&config);
        let mut data = json!({
            "name": "John",
            "avatarUrls": {"16x16": "url"},
            "self": "https://api"
        });

        let stats = filter.apply(&mut data).unwrap();
        assert_eq!(stats.fields_removed, 2);
        assert!(data.as_object().unwrap().contains_key("name"));
    }

    #[test]
    fn test_remove_empty_strings() {
        let config = create_test_config();
        let filter = Filter::new(&config);
        let mut data = json!({
            "name": "",
            "status": null,
            "valid": "data"
        });

        let stats = filter.apply(&mut data).unwrap();
        assert_eq!(stats.empty_strings_removed, 1);
        assert!(!data.as_object().unwrap().contains_key("name"));
        assert!(data.as_object().unwrap().contains_key("status"));
    }

    #[test]
    fn test_recursive() {
        let config = create_test_config();
        let filter = Filter::new(&config);
        let mut data = json!({
            "issues": [
                {"key": "P-1", "self": "url1"},
                {"key": "P-2", "self": "url2"}
            ]
        });

        let stats = filter.apply(&mut data).unwrap();
        assert_eq!(stats.fields_removed, 2);
    }
}
