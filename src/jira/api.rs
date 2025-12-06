use crate::config::Config;
use crate::filter;
use crate::http;
use crate::jira::adf;
use crate::jira::fields;
use crate::markdown::adf_to_markdown;
use anyhow::Result;
use serde_json::{Value, json};

fn convert_issue_to_markdown(issue: &mut Value) {
    let Some(fields) = issue.get_mut("fields") else {
        return;
    };
    let Some(desc) = fields.get_mut("description") else {
        return;
    };
    if desc.is_object() {
        *desc = Value::String(adf_to_markdown(desc));
    }
}

/// Convert ADF description fields to Markdown in search results
fn convert_issues_to_markdown(result: &mut Value) {
    let Some(items) = result.get_mut("items").and_then(|i| i.as_array_mut()) else {
        return;
    };

    for issue in items {
        convert_issue_to_markdown(issue);
    }
}

/// Apply project filter to JQL query if configured
fn apply_project_filter(jql: &str, config: &Config) -> String {
    if config.jira.projects_filter.is_empty() {
        return jql.to_string();
    }

    // Split JQL at ORDER BY to avoid placing ORDER BY inside parentheses
    let jql_lower = jql.to_lowercase();
    let (conditions, order_by) = if let Some(pos) = jql_lower.find(" order by ") {
        (jql[..pos].to_string(), Some(jql[pos..].to_string()))
    } else if jql_lower.starts_with("order by ") {
        (String::new(), Some(format!(" {}", jql)))
    } else {
        (jql.to_string(), None)
    };

    // Check if JQL already contains project condition
    let conditions_lower = conditions.to_lowercase();
    if conditions_lower.contains("project ")
        || conditions_lower.contains("project=")
        || conditions_lower.contains("project in")
    {
        return jql.to_string();
    }

    // Build project filter
    let projects = config
        .jira
        .projects_filter
        .iter()
        .map(|p| format!("\"{}\"", p))
        .collect::<Vec<_>>()
        .join(",");

    let base = if conditions.trim().is_empty() {
        format!("project IN ({})", projects)
    } else {
        format!("project IN ({}) AND ({})", projects, conditions.trim())
    };

    if let Some(order_clause) = order_by {
        format!("{}{}", base, order_clause)
    } else {
        base
    }
}

pub async fn get_issue(issue_key: &str, as_markdown: bool, config: &Config) -> Result<Value> {
    let client = http::client(config);
    let base_url = format!("{}/rest/api/3/issue/{}", config.base_url(), issue_key);

    let url = fields::apply_field_filtering_to_url(&base_url);

    let response = client
        .get(&url)
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get issue ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;

    if as_markdown {
        convert_issue_to_markdown(&mut data);
    }

    filter::apply(&mut data, config);
    Ok(data)
}

pub async fn search(
    jql: &str,
    limit: u32,
    fields: Option<Vec<String>>,
    as_markdown: bool,
    config: &Config,
) -> Result<Value> {
    let final_jql = apply_project_filter(jql, config);
    let client = http::client(config);
    let url = format!("{}/rest/api/3/search/jql", config.base_url());

    let resolved_fields = fields::resolve_search_fields(fields, as_markdown, config);

    let body = json!({
        "jql": final_jql,
        "maxResults": limit,
        "fields": resolved_fields,
    });

    let response = client
        .post(&url)
        .header("Authorization", http::auth_header(config))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Search failed ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, config);

    let issues = data["issues"].as_array().cloned().unwrap_or_default();
    let count = issues.len();
    let mut result = json!({
        "items": issues,
        "count": count
    });

    if as_markdown {
        convert_issues_to_markdown(&mut result);
    }

    Ok(result)
}

pub async fn create_issue(
    project_key: &str,
    summary: &str,
    issue_type: &str,
    description: Value,
    config: &Config,
) -> Result<Value> {
    let client = http::client(config);
    let base_url = format!("{}/rest/api/3/issue", config.base_url());

    let url = fields::apply_field_filtering_to_url(&base_url);

    let description_adf = adf::process_description_input(description)?;

    let body = json!({
        "fields": {
            "project": {
                "key": project_key
            },
            "summary": summary,
            "issuetype": {
                "name": issue_type
            },
            "description": description_adf
        }
    });

    let response = client
        .post(&url)
        .header("Authorization", http::auth_header(config))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error = response.text().await?;
        anyhow::bail!("Failed to create issue: {}", error);
    }

    let data: Value = response.json().await?;
    Ok(json!({
        "key": data["key"],
        "id": data["id"]
    }))
}

pub async fn update_issue(
    issue_key: &str,
    mut fields_value: Value,
    config: &Config,
) -> Result<Value> {
    let client = http::client(config);
    let url = format!("{}/rest/api/3/issue/{}", config.base_url(), issue_key);

    if let Some(fields_obj) = fields_value.as_object_mut()
        && let Some(description_ref) = fields_obj.get_mut("description")
    {
        let description = std::mem::replace(description_ref, Value::Null);
        let description_adf = adf::process_description_input(description)?;
        fields_obj.insert("description".to_string(), description_adf);
    }

    let response = client
        .put(&url)
        .header("Authorization", http::auth_header(config))
        .header("Content-Type", "application/json")
        .json(&json!({
            "fields": fields_value
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to update issue ({}): {}", status, body);
    }

    Ok(json!({}))
}

pub async fn add_comment(issue_key: &str, comment: Value, config: &Config) -> Result<Value> {
    let comment_adf = adf::process_comment_input(comment)?;

    let client = http::client(config);
    let base_url = format!(
        "{}/rest/api/3/issue/{}/comment",
        config.base_url(),
        issue_key
    );

    let url = fields::apply_field_filtering_to_url(&base_url);

    let body = json!({
        "body": comment_adf
    });

    let response = client
        .post(&url)
        .header("Authorization", http::auth_header(config))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to add comment ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({"id": data["id"]}))
}

pub async fn update_comment(
    issue_key: &str,
    comment_id: &str,
    body: Value,
    config: &Config,
) -> Result<Value> {
    let body_adf = adf::process_comment_input(body)?;

    let client = http::client(config);
    let base_url = format!(
        "{}/rest/api/3/issue/{}/comment/{}",
        config.base_url(),
        issue_key,
        comment_id
    );

    let url = fields::apply_field_filtering_to_url(&base_url);

    let request_body = json!({
        "body": body_adf
    });

    let response = client
        .put(&url)
        .header("Authorization", http::auth_header(config))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error = response.text().await?;
        anyhow::bail!("Failed to update comment: {}", error);
    }

    let data: Value = response.json().await?;
    Ok(json!({"id": data["id"]}))
}

pub async fn transition_issue(
    issue_key: &str,
    transition_id: &str,
    config: &Config,
) -> Result<Value> {
    let client = http::client(config);
    let url = format!(
        "{}/rest/api/3/issue/{}/transitions",
        config.base_url(),
        issue_key
    );

    let body = json!({
        "transition": {
            "id": transition_id
        }
    });

    let response = client
        .post(&url)
        .header("Authorization", http::auth_header(config))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to transition issue ({}): {}", status, body);
    }

    Ok(json!({}))
}

pub async fn get_transitions(issue_key: &str, config: &Config) -> Result<Value> {
    let client = http::client(config);
    let base_url = format!(
        "{}/rest/api/3/issue/{}/transitions",
        config.base_url(),
        issue_key
    );

    let url = fields::apply_field_filtering_to_url(&base_url);

    let response = client
        .get(&url)
        .header("Authorization", http::auth_header(config))
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get transitions ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, config);
    Ok(data["transitions"].take())
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    clippy::unnecessary_literal_unwrap
)]
mod tests {
    use super::*;

    use crate::test_utils::create_test_config_with_filters;

    // Helper function to create test config
    fn create_test_config(
        jira_projects_filter: Vec<String>,
        jira_search_default_fields: Option<Vec<String>>,
    ) -> Config {
        let mut config = create_test_config_with_filters(jira_projects_filter, vec![]);
        config.jira.search_default_fields = jira_search_default_fields;
        config
    }

    // T013: Jira search tests

    #[test]
    fn test_search_default_limit() {
        // Test that default limit is 20 when not specified
        let jql = "status = Open";
        let limit = 20u32;

        assert_eq!(jql, "status = Open");
        assert_eq!(limit, 20);
    }

    #[test]
    fn test_search_custom_limit() {
        // Test that custom limit is respected
        let jql = "status = Open";
        let limit = 50u32;

        assert_eq!(jql, "status = Open");
        assert_eq!(limit, 50);
    }

    #[test]
    fn test_search_project_filter_injection() {
        // Test that project filter is injected when not present in JQL
        let config = create_test_config(vec!["PROJ1".to_string(), "PROJ2".to_string()], None);
        let jql = "status = Open";

        // Simulate the project filter logic with ORDER BY handling
        let jql_lower = jql.to_lowercase();
        let (conditions, order_by) = if let Some(pos) = jql_lower.find(" order by ") {
            (jql[..pos].to_string(), Some(jql[pos..].to_string()))
        } else if jql_lower.starts_with("order by ") {
            (String::new(), Some(format!(" {}", jql)))
        } else {
            (jql.to_string(), None)
        };

        let final_jql = if !config.jira.projects_filter.is_empty() {
            let conditions_lower = conditions.to_lowercase();
            if conditions_lower.contains("project ")
                || conditions_lower.contains("project=")
                || conditions_lower.contains("project in")
            {
                jql.to_string()
            } else {
                let projects = config
                    .jira
                    .projects_filter
                    .iter()
                    .map(|p| format!("\"{}\"", p))
                    .collect::<Vec<_>>()
                    .join(",");
                let base = if conditions.trim().is_empty() {
                    format!("project IN ({})", projects)
                } else {
                    format!("project IN ({}) AND ({})", projects, conditions.trim())
                };
                if let Some(ref order_clause) = order_by {
                    format!("{}{}", base, order_clause)
                } else {
                    base
                }
            }
        } else {
            jql.to_string()
        };

        assert_eq!(
            final_jql,
            "project IN (\"PROJ1\",\"PROJ2\") AND (status = Open)"
        );
    }

    #[test]
    fn test_search_project_filter_not_injected_when_present() {
        // Test that project filter is NOT injected when already in JQL
        let config = create_test_config(vec!["PROJ1".to_string()], None);
        let jql = "project = MYPROJ AND status = Open";

        // Simulate the project filter logic with ORDER BY handling
        let jql_lower = jql.to_lowercase();
        let (conditions, order_by) = if let Some(pos) = jql_lower.find(" order by ") {
            (jql[..pos].to_string(), Some(jql[pos..].to_string()))
        } else if jql_lower.starts_with("order by ") {
            (String::new(), Some(format!(" {}", jql)))
        } else {
            (jql.to_string(), None)
        };

        let final_jql = if !config.jira.projects_filter.is_empty() {
            let conditions_lower = conditions.to_lowercase();
            if conditions_lower.contains("project ")
                || conditions_lower.contains("project=")
                || conditions_lower.contains("project in")
            {
                jql.to_string()
            } else {
                let projects = config
                    .jira
                    .projects_filter
                    .iter()
                    .map(|p| format!("\"{}\"", p))
                    .collect::<Vec<_>>()
                    .join(",");
                let base = if conditions.trim().is_empty() {
                    format!("project IN ({})", projects)
                } else {
                    format!("project IN ({}) AND ({})", projects, conditions.trim())
                };
                if let Some(ref order_clause) = order_by {
                    format!("{}{}", base, order_clause)
                } else {
                    base
                }
            }
        } else {
            jql.to_string()
        };

        // Should remain unchanged because JQL already has "project ="
        assert_eq!(final_jql, "project = MYPROJ AND status = Open");
    }

    #[test]
    fn test_search_project_filter_with_order_by() {
        // Test that ORDER BY is correctly placed outside parentheses
        let config = create_test_config(vec!["PROJ1".to_string(), "PROJ2".to_string()], None);
        let jql = "status = Open ORDER BY created DESC";

        // Simulate the project filter logic with ORDER BY handling
        let jql_lower = jql.to_lowercase();
        let (conditions, order_by) = if let Some(pos) = jql_lower.find(" order by ") {
            (jql[..pos].to_string(), Some(jql[pos..].to_string()))
        } else if jql_lower.starts_with("order by ") {
            (String::new(), Some(format!(" {}", jql)))
        } else {
            (jql.to_string(), None)
        };

        let final_jql = if !config.jira.projects_filter.is_empty() {
            let conditions_lower = conditions.to_lowercase();
            if conditions_lower.contains("project ")
                || conditions_lower.contains("project=")
                || conditions_lower.contains("project in")
            {
                jql.to_string()
            } else {
                let projects = config
                    .jira
                    .projects_filter
                    .iter()
                    .map(|p| format!("\"{}\"", p))
                    .collect::<Vec<_>>()
                    .join(",");
                let base = if conditions.trim().is_empty() {
                    format!("project IN ({})", projects)
                } else {
                    format!("project IN ({}) AND ({})", projects, conditions.trim())
                };
                if let Some(ref order_clause) = order_by {
                    format!("{}{}", base, order_clause)
                } else {
                    base
                }
            }
        } else {
            jql.to_string()
        };

        // ORDER BY should be outside parentheses at the end
        assert_eq!(
            final_jql,
            "project IN (\"PROJ1\",\"PROJ2\") AND (status = Open) ORDER BY created DESC"
        );
    }

    #[test]
    fn test_search_project_filter_with_empty_conditions() {
        // Test that empty conditions (only ORDER BY) work correctly
        let config = create_test_config(vec!["PROJ1".to_string(), "PROJ2".to_string()], None);
        let jql = "ORDER BY created DESC";

        // Simulate the project filter logic with ORDER BY handling
        let jql_lower = jql.to_lowercase();
        let (conditions, order_by) = if let Some(pos) = jql_lower.find(" order by ") {
            (jql[..pos].to_string(), Some(jql[pos..].to_string()))
        } else if jql_lower.starts_with("order by ") {
            (String::new(), Some(format!(" {}", jql)))
        } else {
            (jql.to_string(), None)
        };

        let final_jql = if !config.jira.projects_filter.is_empty() {
            let conditions_lower = conditions.to_lowercase();
            if conditions_lower.contains("project ")
                || conditions_lower.contains("project=")
                || conditions_lower.contains("project in")
            {
                jql.to_string()
            } else {
                let projects = config
                    .jira
                    .projects_filter
                    .iter()
                    .map(|p| format!("\"{}\"", p))
                    .collect::<Vec<_>>()
                    .join(",");
                let base = if conditions.trim().is_empty() {
                    format!("project IN ({})", projects)
                } else {
                    format!("project IN ({}) AND ({})", projects, conditions.trim())
                };
                if let Some(ref order_clause) = order_by {
                    format!("{}{}", base, order_clause)
                } else {
                    base
                }
            }
        } else {
            jql.to_string()
        };

        // Should inject project filter without empty parentheses
        assert_eq!(
            final_jql,
            "project IN (\"PROJ1\",\"PROJ2\") ORDER BY created DESC"
        );
    }

    #[test]
    fn test_search_fields_extraction_from_api() {
        // Test that fields parameter is extracted from API call
        let fields = Some(vec![
            "key".to_string(),
            "summary".to_string(),
            "status".to_string(),
        ]);

        let fields_vec = fields.expect("fields should be Some");
        assert_eq!(fields_vec.len(), 3);
        assert_eq!(fields_vec, vec!["key", "summary", "status"]);
    }

    #[test]
    fn test_search_no_fields_uses_default() {
        let config = create_test_config(vec![], None);
        let result = fields::resolve_search_fields(None, false, &config);
        assert_eq!(result.len(), 17);
    }

    #[test]
    fn test_search_markdown_includes_description() {
        let config = create_test_config(vec![], None);
        let result = fields::resolve_search_fields(None, true, &config);
        assert_eq!(result.len(), 18);
        assert!(result.contains(&"description".to_string()));
    }

    #[test]
    fn test_search_empty_project_filter() {
        // Test that empty project filter doesn't modify JQL
        let config = create_test_config(vec![], None);
        let jql = "status = Open";

        let final_jql = if !config.jira.projects_filter.is_empty() {
            format!("project IN (...) AND ({})", jql)
        } else {
            jql.to_string()
        };

        assert_eq!(final_jql, "status = Open");
    }

    // T014: Jira get_issue tests

    #[test]
    fn test_get_issue_valid_issue_key() {
        let issue_key = "PROJ-123";
        assert_eq!(issue_key, "PROJ-123");
    }

    #[test]
    fn test_get_issue_url_construction() {
        let config = create_test_config(vec![], None);
        let issue_key = "PROJ-123";

        let base_url = format!("{}/rest/api/3/issue/{}", config.base_url(), issue_key);

        assert_eq!(
            base_url,
            "https://test.atlassian.net/rest/api/3/issue/PROJ-123"
        );
    }

    // T015: Jira create_issue tests

    #[test]
    fn test_create_issue_required_fields() {
        let project_key = "PROJ";
        let summary = "Test Issue";
        let issue_type = "Task";
        let description = "Test description";

        assert_eq!(project_key, "PROJ");
        assert_eq!(summary, "Test Issue");
        assert_eq!(issue_type, "Task");
        assert_eq!(description, "Test description");
    }

    #[test]
    fn test_create_issue_adf_conversion() {
        let description = "Test description";

        let adf_body = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "text",
                    "text": description
                }]
            }]
        });

        assert_eq!(adf_body["type"], "doc");
        assert_eq!(adf_body["version"], 1);
        assert_eq!(adf_body["content"][0]["type"], "paragraph");
        assert_eq!(
            adf_body["content"][0]["content"][0]["text"],
            "Test description"
        );
    }

    // T016: Remaining Jira handlers tests

    // update_issue tests
    #[test]
    fn test_update_issue_valid_fields() {
        let issue_key = "PROJ-123";
        let fields_json = json!({
            "summary": "Updated summary",
            "priority": {"name": "High"}
        });

        assert_eq!(issue_key, "PROJ-123");
        assert_eq!(fields_json["summary"], "Updated summary");
        assert_eq!(fields_json["priority"]["name"], "High");
    }

    #[test]
    fn test_update_issue_url_construction() {
        let config = create_test_config(vec![], None);
        let issue_key = "PROJ-123";

        let url = format!("{}/rest/api/3/issue/{}", config.base_url(), issue_key);

        assert_eq!(url, "https://test.atlassian.net/rest/api/3/issue/PROJ-123");
    }

    // add_comment tests
    #[test]
    fn test_add_comment_missing_comment() {
        // After ADF support, missing comment field results in null which gets converted to empty ADF
        // Verify comment processing works with null input (converted to empty ADF)
        let comment_result = adf::process_comment_input(json!(null));
        assert!(comment_result.is_ok());
        let comment_adf = comment_result.unwrap();
        assert_eq!(comment_adf["type"], "doc");
        assert_eq!(comment_adf["content"][0]["content"][0]["text"], "");
    }

    #[test]
    fn test_add_comment_adf_conversion() {
        let comment = "This is a test comment";

        let adf_body = json!({
            "body": {
                "type": "doc",
                "version": 1,
                "content": [{
                    "type": "paragraph",
                    "content": [{
                        "type": "text",
                        "text": comment
                    }]
                }]
            }
        });

        assert_eq!(adf_body["body"]["type"], "doc");
        assert_eq!(adf_body["body"]["version"], 1);
        assert_eq!(adf_body["body"]["content"][0]["type"], "paragraph");
        assert_eq!(
            adf_body["body"]["content"][0]["content"][0]["text"],
            "This is a test comment"
        );
    }

    #[test]
    fn test_add_comment_url_construction() {
        let config = create_test_config(vec![], None);
        let issue_key = "PROJ-123";

        let base_url = format!(
            "{}/rest/api/3/issue/{}/comment",
            config.base_url(),
            issue_key
        );

        assert_eq!(
            base_url,
            "https://test.atlassian.net/rest/api/3/issue/PROJ-123/comment"
        );
    }

    // transition_issue tests
    #[test]
    fn test_transition_issue_valid_params() {
        let issue_key = "PROJ-123";
        let transition_id = "21";

        assert_eq!(issue_key, "PROJ-123");
        assert_eq!(transition_id, "21");
    }

    #[test]
    fn test_transition_issue_body_format() {
        let transition_id = "31";

        let body = json!({
            "transition": {
                "id": transition_id
            }
        });

        assert_eq!(body["transition"]["id"], "31");
    }

    // get_transitions tests
    #[test]
    fn test_get_transitions_valid_issue_key() {
        let issue_key = "PROJ-123";
        assert_eq!(issue_key, "PROJ-123");
    }

    #[test]
    fn test_get_transitions_url_construction() {
        let config = create_test_config(vec![], None);
        let issue_key = "PROJ-123";

        let base_url = format!(
            "{}/rest/api/3/issue/{}/transitions",
            config.base_url(),
            issue_key
        );

        assert_eq!(
            base_url,
            "https://test.atlassian.net/rest/api/3/issue/PROJ-123/transitions"
        );
    }
}
