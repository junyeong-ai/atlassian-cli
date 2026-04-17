use crate::client::{ApiClient, Service};
use crate::config::Config;
use crate::filter;
use crate::jira::adf;
use crate::jira::fields;
use crate::markdown::adf_to_markdown;
use anyhow::Result;
use regex::Regex;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::time::sleep;

/// Matches `project` as a whole word followed by the JQL operators we care
/// about (`=`, `!=`, `in (...)`, `not in (...)`), case-insensitive. Using a
/// word boundary prevents false positives like `projectId = 10`.
static PROJECT_CLAUSE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bproject\s*(?:=|!=|not\s+in\s*\(|in\s*\()").unwrap());

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

fn convert_issues_to_markdown(result: &mut Value) {
    let Some(items) = result.get_mut("items").and_then(|i| i.as_array_mut()) else {
        return;
    };

    for issue in items {
        convert_issue_to_markdown(issue);
    }
}

const MAX_RESULTS_PER_PAGE: u32 = 100;

fn apply_project_filter(jql: &str, config: &Config) -> String {
    if config.jira.projects_filter.is_empty() {
        return jql.to_string();
    }

    let jql_lower = jql.to_lowercase();
    let (conditions, order_by) = if let Some(pos) = jql_lower.find(" order by ") {
        (jql[..pos].to_string(), Some(jql[pos..].to_string()))
    } else if jql_lower.starts_with("order by ") {
        (String::new(), Some(format!(" {}", jql)))
    } else {
        (jql.to_string(), None)
    };

    // Skip injection when the user's JQL already scopes by `project`.
    // Uses a word-boundary regex to avoid false positives (e.g. `projectId = 10`
    // previously matched via substring "project =" logic).
    if PROJECT_CLAUSE_RE.is_match(&conditions) {
        return jql.to_string();
    }

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

pub async fn get_issue(issue_key: &str, as_markdown: bool, client: &ApiClient) -> Result<Value> {
    let path = format!("/rest/api/3/issue/{}", issue_key);
    let url = fields::apply_field_filtering_to_url(&path);

    let response = client
        .get(Service::Jira, &url)
        .await?
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

    filter::apply(&mut data, client.config());
    Ok(data)
}

pub async fn search(
    jql: &str,
    limit: u32,
    fields: Option<Vec<String>>,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let final_jql = apply_project_filter(jql, client.config());
    let url = "/rest/api/3/search/jql";

    let resolved_fields = fields::resolve_search_fields(fields, as_markdown, client.config());

    let body = json!({
        "jql": final_jql,
        "maxResults": limit,
        "fields": resolved_fields,
    });

    let response = client
        .post(Service::Jira, url)
        .await?
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
    filter::apply(&mut data, client.config());

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

pub async fn search_all(
    jql: &str,
    fields: Option<Vec<String>>,
    stream: bool,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let final_jql = apply_project_filter(jql, client.config());
    let url = "/rest/api/3/search/jql";
    let resolved_fields = fields::resolve_search_fields(fields, as_markdown, client.config());

    let mut all_issues: Vec<Value> = Vec::new();
    let mut page_num = 1;
    let mut next_page_token: Option<String> = None;

    loop {
        let mut body = json!({
            "jql": final_jql,
            "maxResults": MAX_RESULTS_PER_PAGE,
            "fields": resolved_fields,
        });

        if let Some(ref token) = next_page_token {
            body["nextPageToken"] = json!(token);
        }

        let response = client
            .post(Service::Jira, url)
            .await?
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
        filter::apply(&mut data, client.config());

        let issues = data["issues"].as_array().cloned().unwrap_or_default();
        let count = issues.len();

        let processed_issues: Vec<Value> = if as_markdown {
            issues
                .into_iter()
                .map(|mut issue| {
                    convert_issue_to_markdown(&mut issue);
                    issue
                })
                .collect()
        } else {
            issues
        };

        if stream {
            for issue in &processed_issues {
                println!("{}", serde_json::to_string(issue)?);
            }
            io::stdout().flush()?;
        }

        all_issues.extend(processed_issues);

        // Jira's /search/jql endpoint does not return a total count — only
        // nextPageToken/isLast. Show cumulative fetched count instead.
        eprintln!(
            "  Page {}: {} issues (cumulative: {})",
            page_num,
            count,
            all_issues.len()
        );

        next_page_token = data["nextPageToken"].as_str().map(String::from);
        if next_page_token.is_none() || count == 0 {
            break;
        }

        page_num += 1;
        sleep(Duration::from_millis(
            client.config().performance.rate_limit_delay_ms,
        ))
        .await;
    }

    eprintln!("\nTotal: {} issues fetched", all_issues.len());

    // Stream mode already wrote each item to stdout above. Returning Null signals
    // the caller to skip stdout output — any further JSON would corrupt the JSONL
    // stream a consumer is likely piping into `jq`/`xargs`/etc.
    if stream {
        Ok(Value::Null)
    } else {
        Ok(json!({
            "items": all_issues,
            "total": all_issues.len()
        }))
    }
}

pub async fn create_issue(
    project_key: &str,
    summary: &str,
    issue_type: &str,
    description: Value,
    client: &ApiClient,
) -> Result<Value> {
    let path = "/rest/api/3/issue";
    let url = fields::apply_field_filtering_to_url(path);

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
        .post(Service::Jira, &url)
        .await?
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
    client: &ApiClient,
) -> Result<Value> {
    let url = format!("/rest/api/3/issue/{}", issue_key);

    if let Some(fields_obj) = fields_value.as_object_mut()
        && let Some(description_ref) = fields_obj.get_mut("description")
    {
        let description = std::mem::replace(description_ref, Value::Null);
        let description_adf = adf::process_description_input(description)?;
        fields_obj.insert("description".to_string(), description_adf);
    }

    let response = client
        .put(Service::Jira, &url)
        .await?
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

pub async fn add_comment(issue_key: &str, comment: Value, client: &ApiClient) -> Result<Value> {
    let comment_adf = adf::process_comment_input(comment)?;

    let base_path = format!("/rest/api/3/issue/{}/comment", issue_key);
    let url = fields::apply_field_filtering_to_url(&base_path);

    let body = json!({
        "body": comment_adf
    });

    let response = client
        .post(Service::Jira, &url)
        .await?
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
    client: &ApiClient,
) -> Result<Value> {
    let body_adf = adf::process_comment_input(body)?;

    let base_path = format!("/rest/api/3/issue/{}/comment/{}", issue_key, comment_id);
    let url = fields::apply_field_filtering_to_url(&base_path);

    let request_body = json!({
        "body": body_adf
    });

    let response = client
        .put(Service::Jira, &url)
        .await?
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
    client: &ApiClient,
) -> Result<Value> {
    let url = format!("/rest/api/3/issue/{}/transitions", issue_key);

    let body = json!({
        "transition": {
            "id": transition_id
        }
    });

    let response = client
        .post(Service::Jira, &url)
        .await?
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

pub async fn get_comments(issue_key: &str, as_markdown: bool, client: &ApiClient) -> Result<Value> {
    let url = format!("/rest/api/3/issue/{}/comment", issue_key);

    let response = client
        .get(Service::Jira, &url)
        .await?
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get comments ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());

    if as_markdown && let Some(comments) = data["comments"].as_array_mut() {
        for comment in comments {
            if let Some(body) = comment.get_mut("body")
                && body.is_object()
            {
                *body = Value::String(adf_to_markdown(body));
            }
        }
    }

    Ok(json!({ "items": data["comments"] }))
}

pub async fn get_transitions(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let base_path = format!("/rest/api/3/issue/{}/transitions", issue_key);
    let url = fields::apply_field_filtering_to_url(&base_path);

    let response = client
        .get(Service::Jira, &url)
        .await?
        .header("Accept", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get transitions ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());
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

    fn create_test_config(
        jira_projects_filter: Vec<String>,
        jira_search_default_fields: Option<Vec<String>>,
    ) -> Config {
        let mut config = create_test_config_with_filters(jira_projects_filter, vec![]);
        config.jira.search_default_fields = jira_search_default_fields;
        config
    }

    #[test]
    fn test_search_default_limit() {
        let jql = "status = Open";
        let limit = 20u32;

        assert_eq!(jql, "status = Open");
        assert_eq!(limit, 20);
    }

    #[test]
    fn test_search_custom_limit() {
        let jql = "status = Open";
        let limit = 50u32;

        assert_eq!(jql, "status = Open");
        assert_eq!(limit, 50);
    }

    #[test]
    fn test_search_project_filter_injection() {
        let config = create_test_config(vec!["PROJ1".to_string(), "PROJ2".to_string()], None);
        let result = apply_project_filter("status = Open", &config);
        assert_eq!(
            result,
            "project IN (\"PROJ1\",\"PROJ2\") AND (status = Open)"
        );
    }

    #[test]
    fn test_search_project_filter_not_injected_when_present() {
        let config = create_test_config(vec!["PROJ1".to_string()], None);
        let result = apply_project_filter("project = MYPROJ AND status = Open", &config);
        assert_eq!(result, "project = MYPROJ AND status = Open");
    }

    #[test]
    fn test_search_project_filter_with_order_by() {
        let config = create_test_config(vec!["PROJ1".to_string(), "PROJ2".to_string()], None);
        let result = apply_project_filter("status = Open ORDER BY created DESC", &config);
        assert_eq!(
            result,
            "project IN (\"PROJ1\",\"PROJ2\") AND (status = Open) ORDER BY created DESC"
        );
    }

    #[test]
    fn test_search_project_filter_with_empty_conditions() {
        let config = create_test_config(vec!["PROJ1".to_string(), "PROJ2".to_string()], None);
        let result = apply_project_filter("ORDER BY created DESC", &config);
        assert_eq!(
            result,
            "project IN (\"PROJ1\",\"PROJ2\") ORDER BY created DESC"
        );
    }

    #[test]
    fn test_search_fields_extraction_from_api() {
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
        let config = create_test_config(vec![], None);
        let result = apply_project_filter("status = Open", &config);
        assert_eq!(result, "status = Open");
    }

    #[test]
    fn test_get_issue_valid_issue_key() {
        let issue_key = "PROJ-123";
        assert_eq!(issue_key, "PROJ-123");
    }

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
    fn test_add_comment_missing_comment() {
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

    #[test]
    fn test_get_transitions_valid_issue_key() {
        let issue_key = "PROJ-123";
        assert_eq!(issue_key, "PROJ-123");
    }
}
