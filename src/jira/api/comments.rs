use super::{COMMENT_PAGE, list_envelope, paginate};
use crate::client::{ApiClient, Service};
use crate::http_utils::encode_path_segment;
use crate::jira::adf;
use crate::markdown::adf_to_markdown;
use crate::response::require_field;
use anyhow::Result;
use serde_json::{Value, json};

pub async fn add_comment(issue_key: &str, comment: Value, client: &ApiClient) -> Result<Value> {
    let comment_adf = adf::process_comment_input(comment)?;

    let url = format!(
        "/rest/api/3/issue/{}/comment",
        encode_path_segment(issue_key)
    );

    let body = json!({
        "body": comment_adf
    });

    let request = client
        .post(Service::Jira, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&body);
    let response = client.execute("add comment", request).await?;

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

    let request = client
        .put(Service::Jira, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&request_body);
    let response = client.execute("update comment", request).await?;

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

    let request = client.delete(Service::Jira, &url).await?;
    client.execute("delete comment", request).await?;

    Ok(json!({}))
}

pub async fn get_comments(issue_key: &str, as_markdown: bool, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/comment",
        encode_path_segment(issue_key)
    );
    let items = paginate(&url, &[], "get comments", COMMENT_PAGE, client).await?;

    // Filtered first, then converted: `adf_to_markdown` collapses the body
    // object into a string, so a key the caller excluded would no longer be
    // there to exclude. The Confluence side orders it the same way.
    let mut envelope = list_envelope(items, client);
    if as_markdown && let Some(comments) = envelope["items"].as_array_mut() {
        for comment in comments {
            if let Some(body) = comment.get_mut("body")
                && body.is_object()
            {
                *body = Value::String(adf_to_markdown(body));
            }
        }
    }

    Ok(envelope)
}
