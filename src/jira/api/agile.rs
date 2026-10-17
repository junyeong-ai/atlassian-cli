use super::{AGILE_PAGE, list_envelope, paginate};
use crate::client::{ApiClient, Service};
use crate::http_utils::encode_path_segment;
use anyhow::Result;
use serde_json::{Value, json};

/// Maximum issues per POST for Atlassian's Agile bulk endpoints
/// (`sprint/{id}/issue`, `backlog/issue`, `epic/{key}/issue`,
/// `epic/none/issue`). Hard-coded by Atlassian — exceeding it returns 400
/// and the whole batch is rejected. Do not raise without confirming the
/// upstream contract has changed.
const AGILE_BULK_LIMIT: usize = 50;

/// A project's boards, as the API sent them.
///
/// Unfiltered for the same reason as `fetch_links`: `resolve_board_id` picks a
/// board by its `id`, and a caller's `response_exclude_fields` is a setting
/// about display.
async fn fetch_boards(project: &str, client: &ApiClient) -> Result<Vec<Value>> {
    paginate(
        "/rest/agile/1.0/board",
        &[("projectKeyOrId", project.to_string())],
        "get boards",
        AGILE_PAGE,
        client,
    )
    .await
}

pub async fn get_boards(project: &str, client: &ApiClient) -> Result<Value> {
    Ok(list_envelope(fetch_boards(project, client).await?, client))
}

pub async fn resolve_board_id(project: &str, client: &ApiClient) -> Result<u64> {
    let items = fetch_boards(project, client).await?;

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
    let items = paginate(
        &path,
        &[("state", state.to_string())],
        "get sprints",
        AGILE_PAGE,
        client,
    )
    .await?;
    Ok(list_envelope(items, client))
}

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

        let request = client
            .post(Service::Jira, path)
            .await?
            .header("Content-Type", "application/json")
            .json(&body);
        client.execute(operation, request).await.map_err(|err| {
            if processed == 0 {
                err
            } else {
                err.context(format!(
                    "Failed to {operation} after {processed}/{total} issues already processed"
                ))
            }
        })?;

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
