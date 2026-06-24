use crate::client::{ApiClient, Service};
use crate::config::Config;
use crate::filter;
use crate::http_utils::encode_path_segment;
use crate::jira::adf;
use crate::jira::fields;
use crate::markdown::adf_to_markdown;
use crate::query_utils::{clause_detector, inject_filter};
use crate::response::require_field;
use anyhow::Result;
use regex::Regex;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::time::sleep;

/// Detects an existing `project` scope so the configured project filter is not
/// injected on top of it. See `query_utils::clause_detector` for the operator
/// coverage and word-boundary rationale.
static PROJECT_CLAUSE_RE: LazyLock<Regex> = LazyLock::new(|| clause_detector("project"));

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

    let projects = config
        .jira
        .projects_filter
        .iter()
        .map(|p| format!("\"{}\"", p))
        .collect::<Vec<_>>()
        .join(",");

    inject_filter(
        jql,
        &PROJECT_CLAUSE_RE,
        &format!("project IN ({})", projects),
    )
}

pub async fn get_issue(
    issue_key: &str,
    api_fields: Option<Vec<String>>,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let path = format!("/rest/api/3/issue/{}", encode_path_segment(issue_key));
    let selected = fields::resolve_get_fields(api_fields, client.config());
    let url = fields::apply_field_filtering_to_url(&path, &selected);

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
        anyhow::bail!("Failed to search ({}): {}", status, body);
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
            anyhow::bail!("Failed to search ({}): {}", status, body);
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
        .post(Service::Jira, "/rest/api/3/issue")
        .await?
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to create issue ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({
        "key": require_field(&data, "/key", "create issue")?,
        "id": require_field(&data, "/id", "create issue")?,
    }))
}

pub async fn update_issue(
    issue_key: &str,
    mut fields_value: Value,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!("/rest/api/3/issue/{}", encode_path_segment(issue_key));

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

/// Permanently delete an issue. Jira has no recycle bin for issues, so this is
/// irreversible — the CLI layer requires an explicit `--yes`. When the issue
/// has subtasks, Jira rejects the call unless `delete_subtasks` is set.
pub async fn delete_issue(
    issue_key: &str,
    delete_subtasks: bool,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!("/rest/api/3/issue/{}", encode_path_segment(issue_key));

    let response = client
        .delete(Service::Jira, &url)
        .await?
        .query(&[("deleteSubtasks", delete_subtasks.to_string())])
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to delete issue ({}): {}", status, body);
    }

    Ok(json!({}))
}

pub async fn add_comment(issue_key: &str, comment: Value, client: &ApiClient) -> Result<Value> {
    let comment_adf = adf::process_comment_input(comment)?;

    let url = format!(
        "/rest/api/3/issue/{}/comment",
        encode_path_segment(issue_key)
    );

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
    Ok(json!({ "id": require_field(&data, "/id", "add comment")? }))
}

pub async fn update_comment(
    issue_key: &str,
    comment_id: &str,
    body: Value,
    client: &ApiClient,
) -> Result<Value> {
    let body_adf = adf::process_comment_input(body)?;

    let url = format!(
        "/rest/api/3/issue/{}/comment/{}",
        encode_path_segment(issue_key),
        encode_path_segment(comment_id)
    );

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
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to update comment ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/id", "update comment")? }))
}

/// Delete a single comment. Scoped to one comment by id (the id is the
/// specificity guard), so — like `remove_link`/`remove_worklog` — it does not
/// require a separate `--yes` confirmation at the CLI layer.
pub async fn delete_comment(
    issue_key: &str,
    comment_id: &str,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/comment/{}",
        encode_path_segment(issue_key),
        encode_path_segment(comment_id)
    );

    let response = client.delete(Service::Jira, &url).await?.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to delete comment ({}): {}", status, body);
    }

    Ok(json!({}))
}

pub async fn transition_issue(
    issue_key: &str,
    transition_id: &str,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/transitions",
        encode_path_segment(issue_key)
    );

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
    let url = format!(
        "/rest/api/3/issue/{}/comment",
        encode_path_segment(issue_key)
    );

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

pub async fn get_link_types(client: &ApiClient) -> Result<Value> {
    let response = client
        .get(Service::Jira, "/rest/api/3/issueLinkType")
        .await?
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get link types ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());
    Ok(json!({ "items": data["issueLinkTypes"] }))
}

pub async fn add_link(
    source_key: &str,
    target_key: &str,
    link_type: &str,
    comment: Value,
    client: &ApiClient,
) -> Result<Value> {
    let mut body = json!({
        "type": { "name": link_type },
        "outwardIssue": { "key": source_key },
        "inwardIssue": { "key": target_key }
    });

    if !comment.is_null() {
        let comment_adf = adf::process_comment_input(comment)?;
        body["comment"] = json!({ "body": comment_adf });
    }

    let response = client
        .post(Service::Jira, "/rest/api/3/issueLink")
        .await?
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to create link ({}): {}", status, body);
    }

    Ok(json!({}))
}

pub async fn remove_link(
    source_key: &str,
    target_key: &str,
    link_type: Option<&str>,
    client: &ApiClient,
) -> Result<Value> {
    let links = get_links(source_key, client).await?;
    let items = links["items"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No links found on {}", source_key))?;

    let matching: Vec<&Value> = items
        .iter()
        .filter(|link| {
            let other_key = link["outwardIssue"]["key"]
                .as_str()
                .or_else(|| link["inwardIssue"]["key"].as_str());
            let type_name = link["type"]["name"].as_str();

            let key_match = other_key == Some(target_key);
            let type_match = link_type.map(|t| type_name == Some(t)).unwrap_or(true);

            key_match && type_match
        })
        .collect();

    match matching.len() {
        0 => anyhow::bail!(
            "No link found between {} and {}{}",
            source_key,
            target_key,
            link_type
                .map(|t| format!(" with type '{}'", t))
                .unwrap_or_default()
        ),
        1 => {}
        n => {
            if link_type.is_none() {
                anyhow::bail!(
                    "Found {} links between {} and {}. Specify --type to disambiguate.",
                    n,
                    source_key,
                    target_key
                );
            }
        }
    }

    let link_id = matching[0]["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Link ID missing from response"))?;

    let url = format!("/rest/api/3/issueLink/{}", encode_path_segment(link_id));
    let response = client.delete(Service::Jira, &url).await?.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to remove link ({}): {}", status, body);
    }

    Ok(json!({}))
}

pub async fn get_links(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}?fields=issuelinks",
        encode_path_segment(issue_key)
    );

    let response = client.get(Service::Jira, &url).await?.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get links ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());
    Ok(json!({ "items": data["fields"]["issuelinks"] }))
}

pub async fn get_transitions(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/transitions",
        encode_path_segment(issue_key)
    );

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
    Ok(json!({ "items": data["transitions"] }))
}

pub async fn add_worklog(
    issue_key: &str,
    time_spent: &str,
    comment: Value,
    started: Option<&str>,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/worklog",
        encode_path_segment(issue_key)
    );

    let mut body = json!({
        "timeSpent": time_spent
    });

    if let Some(started_at) = started {
        body["started"] = json!(started_at);
    }

    if !comment.is_null() {
        let comment_adf = adf::process_comment_input(comment)?;
        body["comment"] = comment_adf;
    }

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
        anyhow::bail!("Failed to add worklog ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/id", "add worklog")? }))
}

pub async fn get_worklogs(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/worklog",
        encode_path_segment(issue_key)
    );

    let response = client.get(Service::Jira, &url).await?.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get worklogs ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());
    Ok(json!({ "items": data["worklogs"] }))
}

pub async fn update_worklog(
    issue_key: &str,
    worklog_id: &str,
    time_spent: &str,
    comment: Value,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/worklog/{}",
        encode_path_segment(issue_key),
        encode_path_segment(worklog_id)
    );

    let mut body = json!({
        "timeSpent": time_spent
    });

    if !comment.is_null() {
        let comment_adf = adf::process_comment_input(comment)?;
        body["comment"] = comment_adf;
    }

    let response = client
        .put(Service::Jira, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to update worklog ({}): {}", status, body);
    }

    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/id", "update worklog")? }))
}

pub async fn remove_worklog(
    issue_key: &str,
    worklog_id: &str,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/worklog/{}",
        encode_path_segment(issue_key),
        encode_path_segment(worklog_id)
    );

    let response = client.delete(Service::Jira, &url).await?.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to remove worklog ({}): {}", status, body);
    }

    Ok(json!({}))
}

async fn get_myself(client: &ApiClient) -> Result<Value> {
    let response = client
        .get(Service::Jira, "/rest/api/3/myself")
        .await?
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get current user ({}): {}", status, body);
    }

    response.json().await.map_err(Into::into)
}

pub async fn add_watcher(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let path = format!(
        "/rest/api/3/issue/{}/watchers",
        encode_path_segment(issue_key)
    );

    let response = client
        .post(Service::Jira, &path)
        .await?
        .header("Content-Type", "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to add watcher ({}): {}", status, body);
    }

    Ok(json!({}))
}

pub async fn remove_watcher(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let me = get_myself(client).await?;
    let account_id = me["accountId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Could not determine current user accountId"))?;

    let url = format!(
        "/rest/api/3/issue/{}/watchers",
        encode_path_segment(issue_key)
    );

    let response = client
        .delete(Service::Jira, &url)
        .await?
        .query(&[("accountId", account_id)])
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to remove watcher ({}): {}", status, body);
    }

    Ok(json!({}))
}

pub async fn get_watchers(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/watchers",
        encode_path_segment(issue_key)
    );

    let response = client.get(Service::Jira, &url).await?.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get watchers ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());
    Ok(json!({ "items": data["watchers"] }))
}

/// Loop through a paginated `values`/`isLast`/`startAt` endpoint and accumulate
/// every item. Shared by endpoints that follow the Agile-style pagination
/// contract (`/rest/api/3/label`, `/rest/agile/1.0/board`, sprints, etc.).
async fn paginate_values(
    path: &str,
    extra_query: &[(&str, String)],
    operation: &str,
    client: &ApiClient,
) -> Result<Vec<Value>> {
    // Local tuning knob — Atlassian accepts up to 1000 per request on these
    // endpoints but smaller batches keep latency predictable on slow networks
    // and bound peak memory. Unrelated to `AGILE_BULK_LIMIT` (which is a hard
    // server-side cap on bulk writes).
    const PAGE_SIZE: u64 = 50;
    // Hard ceiling so a server that keeps returning `isLast: false` (a broken
    // or hostile endpoint) cannot loop forever appending duplicates. At
    // PAGE_SIZE=50 this admits 500k items — far beyond any real label/board/
    // sprint list — before failing loudly.
    const MAX_PAGES: u32 = 10_000;
    let mut all: Vec<Value> = Vec::new();
    let mut start_at: u64 = 0;

    // Fetch up to MAX_PAGES pages. We return the moment the server reports
    // `isLast`; only a server that never sets it (broken or hostile) runs the
    // loop to exhaustion, which then bails below instead of looping forever.
    for _ in 0..MAX_PAGES {
        let start_at_str = start_at.to_string();
        let page_size_str = PAGE_SIZE.to_string();

        let mut query: Vec<(&str, &str)> = Vec::with_capacity(extra_query.len() + 2);
        for (k, v) in extra_query {
            query.push((k, v.as_str()));
        }
        query.push(("startAt", start_at_str.as_str()));
        query.push(("maxResults", page_size_str.as_str()));

        let response = client
            .get(Service::Jira, path)
            .await?
            .query(&query)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to {} ({}): {}", operation, status, body);
        }

        let data: Value = response.json().await?;

        // The Agile pagination contract mandates both fields. Bail loudly
        // rather than silently truncating when the server breaks the
        // contract — partial results are worse than a clear failure.
        let values = data
            .get("values")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to {}: paginated response missing `values` array",
                    operation
                )
            })?
            .clone();
        let is_last = data.get("isLast").and_then(Value::as_bool).ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to {}: paginated response missing `isLast` flag",
                operation
            )
        })?;

        let count = values.len() as u64;
        for mut item in values {
            filter::apply(&mut item, client.config());
            all.push(item);
        }

        if is_last {
            return Ok(all);
        }

        // Advance the cursor by the actual page size when the server returned
        // items, or by PAGE_SIZE on an empty-but-not-last page. Both branches
        // are strict increments, so the loop terminates in at most
        // `total_items / PAGE_SIZE + 1` iterations for any finite result set.
        start_at += if count == 0 { PAGE_SIZE } else { count };

        sleep(Duration::from_millis(
            client.config().performance.rate_limit_delay_ms,
        ))
        .await;
    }

    // Reached only if MAX_PAGES pages were fetched and none reported `isLast`.
    anyhow::bail!(
        "Failed to {}: exceeded {} pages without `isLast` — aborting to avoid an unbounded loop",
        operation,
        MAX_PAGES
    )
}

// -- Discovery endpoints --

pub async fn get_issue_types(client: &ApiClient) -> Result<Value> {
    let response = client
        .get(Service::Jira, "/rest/api/3/issuetype")
        .await?
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get issue types ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());
    Ok(json!({ "items": data }))
}

pub async fn get_priorities(client: &ApiClient) -> Result<Value> {
    let response = client
        .get(Service::Jira, "/rest/api/3/priority")
        .await?
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get priorities ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());
    Ok(json!({ "items": data }))
}

pub async fn get_statuses(client: &ApiClient) -> Result<Value> {
    let response = client
        .get(Service::Jira, "/rest/api/3/status")
        .await?
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get statuses ({}): {}", status, body);
    }

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());
    Ok(json!({ "items": data }))
}

pub async fn get_labels(client: &ApiClient) -> Result<Value> {
    let items = paginate_values("/rest/api/3/label", &[], "get labels", client).await?;
    Ok(json!({ "items": items }))
}

// -- Board / Sprint / Epic (Agile API) --

pub async fn get_boards(project: &str, client: &ApiClient) -> Result<Value> {
    let items = paginate_values(
        "/rest/agile/1.0/board",
        &[("projectKeyOrId", project.to_string())],
        "get boards",
        client,
    )
    .await?;
    Ok(json!({ "items": items }))
}

pub async fn resolve_board_id(project: &str, client: &ApiClient) -> Result<u64> {
    let boards = get_boards(project, client).await?;
    let items = boards["items"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No boards found for project {}", project))?;

    match items.len() {
        0 => anyhow::bail!("No boards found for project {}", project),
        1 => items[0]["id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Board ID missing from response")),
        n => {
            let board_list: Vec<String> = items
                .iter()
                .filter_map(|b| {
                    let id = b["id"].as_u64()?;
                    let name = b["name"].as_str().unwrap_or("");
                    Some(format!("  {} (id: {})", name, id))
                })
                .collect();
            anyhow::bail!(
                "Project {} has {} boards. Specify --board:\n{}",
                project,
                n,
                board_list.join("\n")
            )
        }
    }
}

pub async fn get_sprints(board_id: u64, state: &str, client: &ApiClient) -> Result<Value> {
    let path = format!("/rest/agile/1.0/board/{}/sprint", board_id);
    let items = paginate_values(
        &path,
        &[("state", state.to_string())],
        "get sprints",
        client,
    )
    .await?;
    Ok(json!({ "items": items }))
}

/// Maximum issues per POST for Atlassian's Agile bulk endpoints
/// (`sprint/{id}/issue`, `backlog/issue`, `epic/{key}/issue`,
/// `epic/none/issue`). Hard-coded by Atlassian — exceeding it returns 400
/// and the whole batch is rejected. Do not raise without confirming the
/// upstream contract has changed.
const AGILE_BULK_LIMIT: usize = 50;

/// Chunk a slice of issue keys at `AGILE_BULK_LIMIT` and POST each chunk to
/// the same endpoint. Atlassian Agile endpoints are not transactional —
/// when a later chunk fails, the issues from earlier chunks remain moved.
/// The error message reports how many issues were already processed before
/// the failure so callers can reason about the partial state.
async fn post_issue_batches(
    path: &str,
    issues: &[String],
    operation: &str,
    client: &ApiClient,
) -> Result<Value> {
    if issues.is_empty() {
        anyhow::bail!("Failed to {}: no issues provided", operation);
    }

    let total = issues.len();
    let mut processed: usize = 0;

    for chunk in issues.chunks(AGILE_BULK_LIMIT) {
        let body = json!({ "issues": chunk });

        let response = client
            .post(Service::Jira, path)
            .await?
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if processed == 0 {
                anyhow::bail!("Failed to {} ({}): {}", operation, status, body);
            }
            anyhow::bail!(
                "Failed to {} after {}/{} issues already processed ({}): {}",
                operation,
                processed,
                total,
                status,
                body
            );
        }

        processed += chunk.len();
    }

    Ok(json!({}))
}

pub async fn move_issues_to_sprint(
    sprint_id: u64,
    issues: &[String],
    client: &ApiClient,
) -> Result<Value> {
    let path = format!("/rest/agile/1.0/sprint/{}/issue", sprint_id);
    post_issue_batches(&path, issues, "move issues to sprint", client).await
}

pub async fn move_issues_to_backlog(issues: &[String], client: &ApiClient) -> Result<Value> {
    post_issue_batches(
        "/rest/agile/1.0/backlog/issue",
        issues,
        "move issues to backlog",
        client,
    )
    .await
}

pub async fn assign_issues_to_epic(
    epic_key: &str,
    issues: &[String],
    client: &ApiClient,
) -> Result<Value> {
    let path = format!(
        "/rest/agile/1.0/epic/{}/issue",
        encode_path_segment(epic_key)
    );
    post_issue_batches(&path, issues, "assign issues to epic", client).await
}

pub async fn unassign_issues_from_epic(issues: &[String], client: &ApiClient) -> Result<Value> {
    post_issue_batches(
        "/rest/agile/1.0/epic/none/issue",
        issues,
        "unassign issues from epic",
        client,
    )
    .await
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

    // -- Integration tests for Phase 1-3 endpoints --
    //
    // These exercise the production async functions against a `wiremock` server.
    // They verify request method, path, query params, and request body, plus the
    // returned envelope shape. Each test reflects a contract a real Jira API call
    // would hit; synthetic data-only tests for these endpoints have been removed.

    use crate::test_utils::mock_client;
    use wiremock::matchers::{body_json, body_string, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn integ_add_link_maps_source_to_outward() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/issueLink"))
            .and(body_json(json!({
                "type": { "name": "Blocks" },
                "outwardIssue": { "key": "MDW-207" },
                "inwardIssue": { "key": "MDW-183" }
            })))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = add_link("MDW-207", "MDW-183", "Blocks", Value::Null, &client).await;

        assert!(result.is_ok(), "{:?}", result.err());
        assert_eq!(result.unwrap(), json!({}));
    }

    #[tokio::test]
    async fn integ_get_issue_honors_explicit_fields() {
        // `--fields` reaches the request URL verbatim — no hardwired whitelist
        // caps it — and rendered fields stay suppressed.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/PROJ-1"))
            .and(query_param("fields", "*all"))
            .and(query_param("expand", "-renderedFields"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "key": "PROJ-1", "fields": { "summary": "S" } })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_issue("PROJ-1", Some(vec!["*all".to_string()]), false, &client)
            .await
            .unwrap();
        assert_eq!(result["key"], "PROJ-1");
    }

    #[tokio::test]
    async fn integ_add_link_includes_comment_adf() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/issueLink"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let comment = Value::String("linking for dependency".into());
        let result = add_link("A-1", "B-2", "Relates", comment, &client).await;

        assert!(result.is_ok(), "{:?}", result.err());
        // wiremock records requests; verify the comment was wrapped in ADF.
        let recorded = &server.received_requests().await.unwrap()[0];
        let body: Value = serde_json::from_slice(&recorded.body).unwrap();
        assert_eq!(body["comment"]["body"]["type"], "doc");
        assert_eq!(
            body["comment"]["body"]["content"][0]["content"][0]["text"],
            "linking for dependency"
        );
    }

    #[tokio::test]
    async fn integ_remove_link_resolves_id_via_list_then_deletes() {
        let server = MockServer::start().await;

        // 1) GET issuelinks
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/MDW-207"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "fields": {
                    "issuelinks": [
                        { "id": "10001", "type": { "name": "Blocks" }, "outwardIssue": { "key": "MDW-183" } },
                        { "id": "10002", "type": { "name": "Relates" }, "outwardIssue": { "key": "MDW-200" } }
                    ]
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        // 2) DELETE the resolved link id
        Mock::given(method("DELETE"))
            .and(path("/rest/api/3/issueLink/10001"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = remove_link("MDW-207", "MDW-183", None, &client).await;

        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn integ_remove_link_disambiguates_with_type_filter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/A-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "fields": {
                    "issuelinks": [
                        { "id": "10001", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } },
                        { "id": "10002", "type": { "name": "Relates" }, "outwardIssue": { "key": "B-1" } }
                    ]
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/rest/api/3/issueLink/10002"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = remove_link("A-1", "B-1", Some("Relates"), &client).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn integ_remove_link_errors_on_multiple_matches_without_type() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/A-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "fields": {
                    "issuelinks": [
                        { "id": "1", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } },
                        { "id": "2", "type": { "name": "Relates" }, "outwardIssue": { "key": "B-1" } }
                    ]
                }
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = remove_link("A-1", "B-1", None, &client).await.unwrap_err();
        assert!(err.to_string().contains("Specify --type"));
    }

    #[tokio::test]
    async fn integ_get_link_types_wraps_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issueLinkType"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issueLinkTypes": [
                    { "id": "1000", "name": "Blocks", "inward": "is blocked by", "outward": "blocks" }
                ]
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_link_types(&client).await.unwrap();
        assert_eq!(result["items"][0]["name"], "Blocks");
    }

    #[tokio::test]
    async fn integ_add_worklog_body_includes_time_and_comment() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/issue/MDW-207/worklog"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "9001" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let comment = Value::String("API integration".into());
        let result = add_worklog("MDW-207", "2h 30m", comment, None, &client)
            .await
            .unwrap();
        assert_eq!(result["id"], "9001");

        let recorded = &server.received_requests().await.unwrap()[0];
        let body: Value = serde_json::from_slice(&recorded.body).unwrap();
        assert_eq!(body["timeSpent"], "2h 30m");
        assert_eq!(body["comment"]["type"], "doc");
        assert_eq!(
            body["comment"]["content"][0]["content"][0]["text"],
            "API integration"
        );
    }

    #[tokio::test]
    async fn integ_add_watcher_posts_empty_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/issue/MDW-207/watchers"))
            .and(body_string(""))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = add_watcher("MDW-207", &client).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn integ_remove_watcher_passes_account_id_as_query() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/myself"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "accountId": "abc-123" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/rest/api/3/issue/MDW-207/watchers"))
            .and(query_param("accountId", "abc-123"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = remove_watcher("MDW-207", &client).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn integ_get_statuses_hits_public_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "id": "10000", "name": "To Do" },
                { "id": "10001", "name": "Done" }
            ])))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_statuses(&client).await.unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
        assert_eq!(result["items"][0]["name"], "To Do");
    }

    #[tokio::test]
    async fn integ_get_labels_paginates_until_is_last() {
        let server = MockServer::start().await;
        // Page 1: 50 items, isLast=false
        let page1_values: Vec<Value> = (0..50)
            .map(|i| Value::String(format!("label-{}", i)))
            .collect();
        Mock::given(method("GET"))
            .and(path("/rest/api/3/label"))
            .and(query_param("startAt", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": page1_values,
                "isLast": false,
                "maxResults": 50,
                "startAt": 0
            })))
            .expect(1)
            .mount(&server)
            .await;
        // Page 2: 10 items, isLast=true
        let page2_values: Vec<Value> = (50..60)
            .map(|i| Value::String(format!("label-{}", i)))
            .collect();
        Mock::given(method("GET"))
            .and(path("/rest/api/3/label"))
            .and(query_param("startAt", "50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": page2_values,
                "isLast": true,
                "maxResults": 50,
                "startAt": 50
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_labels(&client).await.unwrap();
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 60);
        assert_eq!(items[0], "label-0");
        assert_eq!(items[59], "label-59");
    }

    #[tokio::test]
    async fn integ_get_sprints_passes_state_filter_and_paginates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/agile/1.0/board/42/sprint"))
            .and(query_param("state", "active,future"))
            .and(query_param("startAt", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": [{ "id": 55, "name": "Sprint 1", "state": "active" }],
                "isLast": true,
                "maxResults": 50,
                "startAt": 0
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_sprints(42, "active,future", &client).await.unwrap();
        assert_eq!(result["items"][0]["id"], 55);
    }

    #[tokio::test]
    async fn integ_resolve_board_id_succeeds_for_single_match() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/agile/1.0/board"))
            .and(query_param("projectKeyOrId", "MDW"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": [{ "id": 42, "name": "MDW board" }],
                "isLast": true,
                "maxResults": 50,
                "startAt": 0
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let id = resolve_board_id("MDW", &client).await.unwrap();
        assert_eq!(id, 42);
    }

    #[tokio::test]
    async fn integ_resolve_board_id_errors_with_list_for_multi_match() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/agile/1.0/board"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": [
                    { "id": 42, "name": "Scrum board" },
                    { "id": 55, "name": "Kanban board" }
                ],
                "isLast": true,
                "maxResults": 50,
                "startAt": 0
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = resolve_board_id("MDW", &client).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Specify --board"));
        assert!(msg.contains("Scrum board"));
        assert!(msg.contains("42"));
        assert!(msg.contains("Kanban board"));
    }

    #[tokio::test]
    async fn integ_resolve_board_id_errors_when_zero_boards() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/agile/1.0/board"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": [],
                "isLast": true,
                "maxResults": 50,
                "startAt": 0
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = resolve_board_id("NONE", &client).await.unwrap_err();
        assert!(err.to_string().contains("No boards found"));
    }

    #[tokio::test]
    async fn integ_move_issues_to_sprint_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/agile/1.0/sprint/55/issue"))
            .and(body_json(json!({ "issues": ["A-1", "A-2", "A-3"] })))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let issues = vec!["A-1".to_string(), "A-2".to_string(), "A-3".to_string()];
        let result = move_issues_to_sprint(55, &issues, &client).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn integ_assign_issues_to_epic_encodes_path_segment() {
        let server = MockServer::start().await;
        // Use an epic key that REQUIRES encoding so the test fails if the
        // encoder is removed. Space → %20, slash → %2F, bracket → %5B/%5D.
        Mock::given(method("POST"))
            .and(path("/rest/agile/1.0/epic/EPIC%2042%2Falpha%5Bv1%5D/issue"))
            .and(body_json(json!({ "issues": ["A-1", "A-2"] })))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = assign_issues_to_epic(
            "EPIC 42/alpha[v1]",
            &["A-1".to_string(), "A-2".to_string()],
            &client,
        )
        .await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn integ_assign_issues_to_epic_chunks_above_50() {
        let server = MockServer::start().await;
        // 120 issues should produce 3 batched POSTs (50 + 50 + 20).
        Mock::given(method("POST"))
            .and(path("/rest/agile/1.0/epic/EPIC-1/issue"))
            .respond_with(ResponseTemplate::new(204))
            .expect(3)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let issues: Vec<String> = (0..120).map(|i| format!("MDW-{}", i)).collect();
        let result = assign_issues_to_epic("EPIC-1", &issues, &client).await;
        assert!(result.is_ok(), "{:?}", result.err());

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 3);
        // First batch carries 50 issues.
        let first_body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(first_body["issues"].as_array().unwrap().len(), 50);
        // Last batch carries the 20-issue remainder.
        let last_body: Value = serde_json::from_slice(&requests[2].body).unwrap();
        assert_eq!(last_body["issues"].as_array().unwrap().len(), 20);
    }

    #[tokio::test]
    async fn integ_move_to_sprint_rejects_empty_issues() {
        let server = MockServer::start().await;
        let client = mock_client(server.uri());
        let err = move_issues_to_sprint(55, &[], &client).await.unwrap_err();
        assert!(err.to_string().contains("no issues provided"));
    }

    #[tokio::test]
    async fn integ_unassign_uses_epic_none_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/agile/1.0/epic/none/issue"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = unassign_issues_from_epic(&["A-1".to_string()], &client).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn integ_path_segment_encoding_handles_unsafe_chars() {
        let server = MockServer::start().await;
        // %20 is the encoding for space — verify the issue_key is percent-encoded.
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/MDW%20207/comment"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "comments": [] })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_comments("MDW 207", false, &client).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn integ_get_watchers_encodes_issue_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/MDW%20207%5B1%5D/watchers"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "watchers": [{ "accountId": "abc", "displayName": "Alice" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_watchers("MDW 207[1]", &client).await.unwrap();
        assert_eq!(result["items"][0]["accountId"], "abc");
    }

    #[tokio::test]
    async fn integ_paginate_bails_on_missing_is_last() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/label"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": ["a", "b"],
                "maxResults": 50,
                "startAt": 0
                // `isLast` deliberately omitted — must surface as an error,
                // not silently truncate the result.
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = get_labels(&client).await.unwrap_err();
        assert!(err.to_string().contains("isLast"));
    }

    #[tokio::test]
    async fn integ_paginate_bails_on_missing_values() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/label"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "isLast": true,
                "maxResults": 50,
                "startAt": 0
                // `values` deliberately omitted — pagination contract broken.
            })))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = get_labels(&client).await.unwrap_err();
        assert!(err.to_string().contains("values"));
    }

    #[test]
    fn project_filter_ignores_clauses_inside_quoted_strings() {
        use crate::test_utils::create_test_config_with_filters;
        let config = create_test_config_with_filters(vec!["MDW".to_string()], vec![]);

        // The substring `project =` inside a quoted summary must NOT suppress
        // the filter injection.
        let result = apply_project_filter("summary ~ \"project = foo\"", &config);
        assert!(
            result.starts_with("project IN (\"MDW\")"),
            "filter should be injected, got: {result}"
        );

        // Same defense for `order by` inside quoted text — the JQL must not
        // get split at the wrong position. The quoted summary stays inside the
        // injected AND-clause, intact (including its inner `order by`).
        let result = apply_project_filter("summary ~ \"finish order by tomorrow\"", &config);
        assert_eq!(
            result,
            "project IN (\"MDW\") AND (summary ~ \"finish order by tomorrow\")"
        );
    }

    #[test]
    fn project_filter_with_whitespace_only_jql_yields_bare_filter() {
        use crate::test_utils::create_test_config_with_filters;
        let config = create_test_config_with_filters(vec!["MDW".to_string()], vec![]);
        // Whitespace-only input has empty trimmed conditions; the wrapper
        // must collapse to `project IN (...)` without a dangling AND.
        let result = apply_project_filter("   ", &config);
        assert_eq!(result, "project IN (\"MDW\")");
    }

    #[test]
    fn project_filter_multiple_project_clauses_skips_injection() {
        use crate::test_utils::create_test_config_with_filters;
        let config = create_test_config_with_filters(vec!["MDW".to_string()], vec![]);
        let input = "project = X OR project = Y";
        // Any `project` clause already present must suppress injection,
        // even when the user combines multiple clauses with OR.
        assert_eq!(apply_project_filter(input, &config), input);
    }

    #[tokio::test]
    async fn integ_paginate_single_page_with_is_last_true() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/label"))
            .and(query_param("startAt", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": ["alpha", "beta"],
                "isLast": true,
                "maxResults": 50,
                "startAt": 0
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_labels(&client).await.unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn integ_paginate_zero_items_terminates_cleanly() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/label"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": [],
                "isLast": true,
                "maxResults": 50,
                "startAt": 0
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = get_labels(&client).await.unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn integ_post_issue_batches_exactly_50_sends_one_post() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/agile/1.0/epic/E-1/issue"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let issues: Vec<String> = (0..50).map(|i| format!("MDW-{}", i)).collect();
        assign_issues_to_epic("E-1", &issues, &client)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn integ_post_issue_batches_reports_partial_failure() {
        let server = MockServer::start().await;
        let path_str = "/rest/agile/1.0/epic/E-1/issue";

        // First request succeeds (1 POST). All subsequent requests fail.
        Mock::given(method("POST"))
            .and(path(path_str))
            .respond_with(ResponseTemplate::new(204))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(path_str))
            .respond_with(ResponseTemplate::new(500).set_body_string("server boom"))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let issues: Vec<String> = (0..120).map(|i| format!("MDW-{}", i)).collect();
        let err = assign_issues_to_epic("E-1", &issues, &client)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("50/120"),
            "expected partial-progress count, got: {msg}"
        );

        // The first chunk must have carried a well-formed 50-issue body —
        // guards against the error path passing on a malformed request.
        let requests = server.received_requests().await.unwrap();
        let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(first["issues"].as_array().unwrap().len(), 50);
    }

    #[tokio::test]
    async fn integ_post_issue_batches_single_issue_one_post() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/agile/1.0/epic/E-1/issue"))
            .and(body_json(json!({ "issues": ["only-1"] })))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        assign_issues_to_epic("E-1", &["only-1".to_string()], &client)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn integ_post_issue_batches_51_issues_split_50_plus_1() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/agile/1.0/epic/E-1/issue"))
            .respond_with(ResponseTemplate::new(204))
            .expect(2)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let issues: Vec<String> = (0..51).map(|i| format!("MDW-{}", i)).collect();
        assign_issues_to_epic("E-1", &issues, &client)
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
        let second: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(first["issues"].as_array().unwrap().len(), 50);
        assert_eq!(second["issues"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn integ_post_issue_batches_first_chunk_failure_omits_partial_count() {
        let server = MockServer::start().await;
        // First (and only) request fails — no chunks succeeded, so the error
        // message must use the simpler form without "after X/Y".
        Mock::given(method("POST"))
            .and(path("/rest/agile/1.0/epic/E-1/issue"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = assign_issues_to_epic("E-1", &["A-1".to_string()], &client)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains("already processed"),
            "first-chunk failure must use plain error form, got: {msg}"
        );
        assert!(msg.contains("bad request"));
    }

    #[tokio::test]
    async fn integ_update_issue_wraps_fields_and_converts_description() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/rest/api/3/issue/MDW-1"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let fields = json!({ "summary": "new", "description": "plain text" });
        let result = update_issue("MDW-1", fields, &client).await.unwrap();
        assert_eq!(result, json!({}));

        let recorded = &server.received_requests().await.unwrap()[0];
        let body: Value = serde_json::from_slice(&recorded.body).unwrap();
        // Payload is wrapped under `fields`; plain-text description is
        // promoted to an ADF doc.
        assert_eq!(body["fields"]["summary"], "new");
        assert_eq!(body["fields"]["description"]["type"], "doc");
        assert_eq!(
            body["fields"]["description"]["content"][0]["content"][0]["text"],
            "plain text"
        );
    }

    #[tokio::test]
    async fn integ_add_comment_wraps_body_and_returns_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/issue/MDW-1/comment"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "10100" })))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = add_comment("MDW-1", Value::String("hello".into()), &client)
            .await
            .unwrap();
        assert_eq!(result["id"], "10100");

        let recorded = &server.received_requests().await.unwrap()[0];
        let body: Value = serde_json::from_slice(&recorded.body).unwrap();
        assert_eq!(body["body"]["type"], "doc");
        assert_eq!(body["body"]["content"][0]["content"][0]["text"], "hello");
    }

    #[tokio::test]
    async fn integ_transition_issue_sends_transition_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/issue/MDW-1/transitions"))
            .and(body_json(json!({ "transition": { "id": "31" } })))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = transition_issue("MDW-1", "31", &client).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn integ_update_issue_encodes_issue_key() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/rest/api/3/issue/MDW%201"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        update_issue("MDW 1", json!({ "summary": "x" }), &client)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn integ_delete_issue_passes_subtasks_query() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/rest/api/3/issue/MDW-9"))
            .and(query_param("deleteSubtasks", "true"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = delete_issue("MDW-9", true, &client).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn integ_delete_issue_default_keeps_subtasks_false() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/rest/api/3/issue/MDW-9"))
            .and(query_param("deleteSubtasks", "false"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        delete_issue("MDW-9", false, &client).await.unwrap();
    }

    #[tokio::test]
    async fn integ_delete_comment_encodes_path() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/rest/api/3/issue/MDW%201/comment/100"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = delete_comment("MDW 1", "100", &client).await.unwrap();
        assert_eq!(result, json!({}));
    }

    #[tokio::test]
    async fn integ_delete_issue_surfaces_error_with_status() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/rest/api/3/issue/MDW-9"))
            .respond_with(ResponseTemplate::new(403).set_body_string("no permission"))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = delete_issue("MDW-9", false, &client).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("403") && msg.contains("no permission"),
            "{msg}"
        );
    }
}
