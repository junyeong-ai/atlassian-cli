use super::{WORKLOG_PAGE, list_envelope, paginate};
use crate::client::{ApiClient, Service};
use crate::http_utils::encode_path_segment;
use crate::jira::adf;
use crate::response::require_field;
use anyhow::Result;
use serde_json::{Value, json};

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

    // Passed through as written. Jira takes this field in one shape —
    // milliseconds and an offset with no colon — and narrower than the ISO
    // 8601 a caller may reasonably offer, so a `Z` or a `+09:00` comes back
    // refused. Reshaping it here would mean parsing dates to guess what was
    // meant, and the value is the caller's to state.
    if let Some(started_at) = started {
        body["started"] = json!(started_at);
    }

    if !comment.is_null() {
        let comment_adf = adf::process_comment_input(comment)?;
        body["comment"] = comment_adf;
    }

    let request = client
        .post(Service::Jira, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&body);
    let response = client.execute("add worklog", request).await?;

    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/id", "add worklog")? }))
}

pub async fn get_worklogs(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/worklog",
        encode_path_segment(issue_key)
    );

    let items = paginate(&url, &[], "get worklogs", WORKLOG_PAGE, client).await?;
    Ok(list_envelope(items, client))
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

    let request = client
        .put(Service::Jira, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&body);
    let response = client.execute("update worklog", request).await?;

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

    let request = client.delete(Service::Jira, &url).await?;
    client.execute("remove worklog", request).await?;

    Ok(json!({}))
}
