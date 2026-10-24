use crate::client::{ApiClient, Service};
use crate::config::Config;
use crate::filter;
use crate::http_utils::encode_path_segment;
use crate::query_utils::inject_filter;
use crate::response::{WHOLE_BODY, require_array};
use anyhow::Result;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

mod issues;
pub use issues::{
    assign_issues_to_epic, create_issue, delete_issue, get_issue, get_transitions, search,
    search_all, transition_issue, unassign_issues_from_epic, update_issue,
};
mod comments;
pub use comments::{add_comment, delete_comment, get_comments, update_comment};
mod links;
pub use links::{add_link, get_link_types, get_links, remove_link, remove_link_by_id};
mod worklogs;
pub use worklogs::{add_worklog, get_worklogs, remove_worklog, update_worklog};
mod agile;
pub use agile::{
    get_boards, get_sprints, move_issues_to_backlog, move_issues_to_sprint, resolve_board_id,
};

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

    inject_filter(jql, "project", &format!("project IN ({})", projects))
}

async fn get_myself(client: &ApiClient) -> Result<Value> {
    let request = client.get(Service::Jira, "/rest/api/3/myself").await?;
    let response = client.execute("get current user", request).await?;

    response.json().await.map_err(Into::into)
}

pub async fn add_watcher(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let path = format!(
        "/rest/api/3/issue/{}/watchers",
        encode_path_segment(issue_key)
    );

    let request = client
        .post(Service::Jira, &path)
        .await?
        .header("Content-Type", "application/json");
    client.execute_idempotent("add watcher", request).await?;

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

    let request = client
        .delete(Service::Jira, &url)
        .await?
        .query(&[("accountId", account_id)]);
    client.execute("remove watcher", request).await?;

    Ok(json!({}))
}

pub async fn get_watchers(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/watchers",
        encode_path_segment(issue_key)
    );

    let request = client.get(Service::Jira, &url).await?;
    let response = client.execute("get watchers", request).await?;

    let data: Value = response.json().await?;
    Ok(list_envelope(
        require_array(&data, "/watchers", "get watchers")?,
        client,
    ))
}

/// The `{"items": [...]}` envelope, with the response filter applied to the
/// items and never to the envelope.
///
/// The wrapper is this CLI's contract rather than the API's answer, so
/// `response_exclude_fields` has no business editing it: naming `items` would
/// otherwise return `{}` with exit 0 where a list was promised.
fn list_envelope(mut items: Vec<Value>, client: &ApiClient) -> Value {
    for item in &mut items {
        filter::apply(item, client.config());
    }
    json!({ "items": items })
}

/// How a Jira collection reports that a page is the last one.
///
/// The two generations answer the same question in different terms and cannot
/// be read interchangeably: an Agile collection states it outright, a classic
/// one states only how many items exist. Holding the difference here keeps both
/// on one loop instead of a second paginator that drifts from this one.
#[derive(Clone, Copy)]
enum PageEnd {
    /// An `isLast` boolean.
    IsLast,
    /// A `total` count the accumulated length is measured against.
    Total,
}

/// The shape of one paginated collection: where its items sit, and how it says
/// it is finished.
#[derive(Clone, Copy)]
struct PageContract {
    items: &'static str,
    end: PageEnd,
}

/// `/rest/api/3/label`, `/rest/agile/1.0/board`, sprints — everything on the
/// Agile-style contract.
const AGILE_PAGE: PageContract = PageContract {
    items: "values",
    end: PageEnd::IsLast,
};

/// `/rest/api/3/issue/{key}/comment`.
const COMMENT_PAGE: PageContract = PageContract {
    items: "comments",
    end: PageEnd::Total,
};

/// `/rest/api/3/issue/{key}/worklog`.
const WORKLOG_PAGE: PageContract = PageContract {
    items: "worklogs",
    end: PageEnd::Total,
};

/// Loop through a paginated `startAt`/`maxResults` endpoint and accumulate every
/// item, following `contract` to know where the items are and when to stop.
///
/// Returns the items as the API sent them. Filtering belongs to whoever is
/// about to print them — `resolve_board_id` reads an `id` off this, and a
/// caller's `response_exclude_fields` naming a field an internal decision turns
/// on would otherwise decide something rather than trim a display.
async fn paginate(
    path: &str,
    extra_query: &[(&str, String)],
    operation: &str,
    contract: PageContract,
    client: &ApiClient,
) -> Result<Vec<Value>> {
    // Local tuning knob — Atlassian accepts up to 1000 per request on these
    // endpoints but smaller batches keep latency predictable on slow networks
    // and bound peak memory. Unrelated to `AGILE_BULK_LIMIT` (which is a hard
    // server-side cap on bulk writes).
    const PAGE_SIZE: u64 = 50;
    // Hard ceiling so a server that never reports the end (a broken or hostile
    // endpoint) cannot loop forever appending duplicates. At PAGE_SIZE=50 this
    // admits 500k items — far beyond any real collection — before failing
    // loudly.
    const MAX_PAGES: u32 = 10_000;
    let mut all: Vec<Value> = Vec::new();
    let mut start_at: u64 = 0;

    // Fetch up to MAX_PAGES pages. We return the moment the server reports the
    // end; only a server that never does runs the loop to exhaustion, which
    // then bails below instead of looping forever.
    for _ in 0..MAX_PAGES {
        let start_at_str = start_at.to_string();
        let page_size_str = PAGE_SIZE.to_string();

        let mut query: Vec<(&str, &str)> = Vec::with_capacity(extra_query.len() + 2);
        for (k, v) in extra_query {
            query.push((k, v.as_str()));
        }
        query.push(("startAt", start_at_str.as_str()));
        query.push(("maxResults", page_size_str.as_str()));

        let request = client.get(Service::Jira, path).await?.query(&query);
        let response = client.execute(operation, request).await?;

        let data: Value = response.json().await?;

        // A page that carries neither its items nor the field that ends the
        // collection has broken the contract. Bail loudly rather than silently
        // truncating — partial results are worse than a clear failure.
        let page = data
            .get(contract.items)
            .and_then(Value::as_array)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to {}: paginated response missing `{}` array",
                    operation,
                    contract.items
                )
            })?
            .clone();
        let end = match contract.end {
            PageEnd::IsLast => data.get("isLast").and_then(Value::as_bool).ok_or_else(|| {
                anyhow::anyhow!(
                    "Failed to {}: paginated response missing `isLast` flag",
                    operation
                )
            })?,
            PageEnd::Total => {
                let total = data.get("total").and_then(Value::as_u64).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Failed to {}: paginated response missing `total` count",
                        operation
                    )
                })?;
                // The count is the whole contract here, so a page that adds
                // nothing while items remain is the server declining to hand
                // them over — not the end of the collection.
                if page.is_empty() && (all.len() as u64) < total {
                    anyhow::bail!(
                        "Failed to {}: server reported {} items but stopped returning them at {}",
                        operation,
                        total,
                        all.len()
                    );
                }
                all.len() as u64 + page.len() as u64 >= total
            }
        };

        let count = page.len() as u64;
        all.extend(page);

        if end {
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

    // Reached only if MAX_PAGES pages were fetched and none reported the end.
    anyhow::bail!(
        "Failed to {}: exceeded {} pages without reaching the end of the collection — aborting to avoid an unbounded loop",
        operation,
        MAX_PAGES
    )
}

// -- Discovery endpoints --

pub async fn get_issue_types(client: &ApiClient) -> Result<Value> {
    let request = client.get(Service::Jira, "/rest/api/3/issuetype").await?;
    let response = client.execute("get issue types", request).await?;

    let data: Value = response.json().await?;
    Ok(list_envelope(
        require_array(&data, WHOLE_BODY, "get issue types")?,
        client,
    ))
}

/// `/priority` is deprecated in favour of the paginated `/priority/search`, but
/// that endpoint requires the admin scope `manage:jira-configuration` under
/// OAuth where this one takes `read:jira-work`, so moving would cost OAuth users
/// the command.
pub async fn get_priorities(client: &ApiClient) -> Result<Value> {
    let request = client.get(Service::Jira, "/rest/api/3/priority").await?;
    let response = client.execute("get priorities", request).await?;

    let data: Value = response.json().await?;
    Ok(list_envelope(
        require_array(&data, WHOLE_BODY, "get priorities")?,
        client,
    ))
}

pub async fn get_statuses(client: &ApiClient) -> Result<Value> {
    let request = client.get(Service::Jira, "/rest/api/3/status").await?;
    let response = client.execute("get statuses", request).await?;

    let data: Value = response.json().await?;
    Ok(list_envelope(
        require_array(&data, WHOLE_BODY, "get statuses")?,
        client,
    ))
}

pub async fn get_labels(client: &ApiClient) -> Result<Value> {
    let items = paginate("/rest/api/3/label", &[], "get labels", AGILE_PAGE, client).await?;
    Ok(list_envelope(items, client))
}

/// Users whose display name or email matches `query`. Their `accountId` is
/// what assignee fields, JQL and CQL take; it identifies the same person in
/// Jira and Confluence.
pub async fn search_users(query: &str, limit: u32, client: &ApiClient) -> Result<Value> {
    let max_results = limit.to_string();
    let request = client
        .get(Service::Jira, "/rest/api/3/user/search")
        .await?
        .query(&[("query", query), ("maxResults", max_results.as_str())]);
    let response = client.execute("search users", request).await?;

    let data: Value = response.json().await?;
    Ok(list_envelope(
        require_array(&data, WHOLE_BODY, "search users")?,
        client,
    ))
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    clippy::unnecessary_literal_unwrap
)]
#[path = "api/tests.rs"]
mod tests;
