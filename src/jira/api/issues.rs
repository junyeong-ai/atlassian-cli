use super::{MAX_RESULTS_PER_PAGE, apply_project_filter, list_envelope};
use crate::client::{ApiClient, Service};
use crate::filter;
use crate::http_utils::encode_path_segment;
use crate::jira::adf;
use crate::jira::fields;
use crate::markdown::adf_to_markdown;
use crate::response::{require_array, require_field};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{self, Write};
use std::time::Duration;
use tokio::time::sleep;

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

pub async fn get_issue(
    issue_key: &str,
    api_fields: Option<Vec<String>>,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let path = format!("/rest/api/3/issue/{}", encode_path_segment(issue_key));
    let selected = fields::resolve_get_fields(api_fields, client.config()).join(",");

    let request = client
        .get(Service::Jira, &path)
        .await?
        // Through the query builder, not into the URL string: the selector is
        // the caller's `--fields`, and a value carrying `&` or `#` written into
        // the URL would start a parameter of its own or a fragment, changing
        // the request the server answers.
        .query(&[("fields", selected.as_str())])
        .header("Accept", "application/json");
    let response = client.execute("get issue", request).await?;

    let mut data: Value = response.json().await?;

    // Filtered before converted, as the comment and search reads are: the ADF
    // description becomes a string, and a key the caller excluded inside it
    // would no longer be there to exclude.
    filter::apply(&mut data, client.config());
    if as_markdown {
        convert_issue_to_markdown(&mut data);
    }

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

    let request = client
        .post(Service::Jira, url)
        .await?
        .header("Content-Type", "application/json")
        .json(&body);
    let response = client.execute_idempotent("search", request).await?;

    let data: Value = response.json().await?;

    // A 2xx whose body has no `issues` array is schema drift, and reporting it
    // as no matches is the silent truncation `paginate` exists to prevent. Read
    // before filtering, as every other list does, so the error can only mean
    // the response lacked the array rather than that a caller excluded it.
    let Some(issues) = data["issues"].as_array().cloned() else {
        anyhow::bail!("search succeeded but its response had no 'issues' array: {data}");
    };
    let count = issues.len();
    let mut result = list_envelope(issues, client);
    result["count"] = json!(count);

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

    // The same bound `paginate` carries, for the same reason: the walk ends
    // when the server stops handing out a token, and only a server that never
    // does runs this to exhaustion — which then bails below rather than
    // looping forever. A token it has already used is drift, the token-shaped
    // counterpart of `CursorTrail`'s repeated path.
    const MAX_PAGES: u32 = 10_000;
    let mut all_issues: Vec<Value> = Vec::new();
    let mut total = 0usize;
    let mut next_page_token: Option<String> = None;
    let mut seen_tokens: std::collections::HashSet<String> = std::collections::HashSet::new();

    for page_num in 1..=MAX_PAGES {
        let mut body = json!({
            "jql": final_jql,
            "maxResults": MAX_RESULTS_PER_PAGE,
            "fields": resolved_fields,
        });

        if let Some(ref token) = next_page_token {
            body["nextPageToken"] = json!(token);
        }

        let request = client
            .post(Service::Jira, url)
            .await?
            .header("Content-Type", "application/json")
            .json(&body);
        let response = client.execute_idempotent("search", request).await?;

        let data: Value = response.json().await?;

        // The walk takes its items and its end signal off the body before
        // anything filters it, as `paginate` and both Confluence walks do:
        // `response_exclude_fields` is the caller's list, and a name in it that
        // matches a control field would delete the answer this loop reads.
        // Filtering one issue at a time reaches exactly what it is for.
        let Some(issues) = data["issues"].as_array().cloned() else {
            anyhow::bail!("search succeeded but its response had no 'issues' array: {data}");
        };
        let next_signal = data["nextPageToken"].clone();
        let count = issues.len();

        let processed_issues: Vec<Value> = issues
            .into_iter()
            .map(|mut issue| {
                filter::apply(&mut issue, client.config());
                if as_markdown {
                    convert_issue_to_markdown(&mut issue);
                }
                issue
            })
            .collect();

        if stream {
            for issue in &processed_issues {
                println!("{}", serde_json::to_string(issue)?);
            }
            io::stdout().flush()?;
        }

        total += count;
        if !stream {
            all_issues.extend(processed_issues);
        }

        // Jira's /search/jql endpoint does not return a total count — only
        // nextPageToken/isLast. Show cumulative fetched count instead.
        eprintln!(
            "  Page {}: {} issues (cumulative: {})",
            page_num, count, total
        );

        // Absent or null is the endpoint's own end-of-results signal; a token
        // that is there and is not a string is drift, the same distinction the
        // Confluence walk draws on `_links.next`. An empty page is neither —
        // with a token still live, breaking on one drops every page after it.
        if next_signal.is_null() {
            // Cleared rather than merely broken out of: the check after the
            // loop reads this to tell "the endpoint said stop" from "the page
            // bound ran out".
            next_page_token = None;
            break;
        }
        let Some(token) = next_signal.as_str() else {
            anyhow::bail!(
                "search succeeded but its 'nextPageToken' was not a string: {next_signal}"
            );
        };
        // Absent or null is this endpoint's end signal and nothing else is. An
        // empty string names no page, and sent back it would either repeat one
        // or fail a request later under a token that reads like our own bug.
        if token.is_empty() {
            anyhow::bail!(
                "search returned an empty 'nextPageToken', which is neither a page to fetch nor \
                 the absent field that ends the walk"
            );
        }
        if !seen_tokens.insert(token.to_string()) {
            anyhow::bail!(
                "search returned a page token it had already used ({token}), so the walk would \
                 not advance"
            );
        }
        next_page_token = Some(token.to_string());

        sleep(Duration::from_millis(
            client.config().performance.rate_limit_delay_ms,
        ))
        .await;
    }

    if next_page_token.is_some() {
        anyhow::bail!("search did not finish within {MAX_PAGES} pages");
    }

    eprintln!("\nTotal: {} issues fetched", total);

    // Stream mode already wrote each item to stdout above. Returning Null signals
    // the caller to skip stdout output — any further JSON would corrupt the JSONL
    // stream a consumer is likely piping into `jq`/`xargs`/etc.
    if stream {
        Ok(Value::Null)
    } else {
        Ok(json!({
            "items": all_issues,
            "total": total
        }))
    }
}

/// Fold `--fields` into what the arguments already said.
///
/// A field named in both places is refused rather than merged: which of the
/// two was meant is not something this code can tell, and taking either
/// silently drops what the caller wrote in the other. Description values are
/// normalized to ADF before insertion.
fn merge_extra_fields(fields: &mut Value, extra: Value) -> Result<()> {
    let Value::Object(extra) = extra else {
        anyhow::bail!(
            "--fields takes a JSON object of Jira field names, e.g. \
             '{{\"labels\":[\"api\"]}}' — got {extra}"
        );
    };
    let target = fields
        .as_object_mut()
        .expect("the fields under construction are an object");
    for (name, value) in extra {
        if target.contains_key(&name) {
            anyhow::bail!(
                "--fields sets `{name}`, which this command already took from its own \
                 argument — give it in one place or the other"
            );
        }
        let value = if name == "description" {
            adf::process_description_input(value)?
        } else {
            value
        };
        target.insert(name, value);
    }
    Ok(())
}

pub async fn create_issue(
    project_key: &str,
    summary: &str,
    issue_type: &str,
    description: Value,
    parent_key: Option<&str>,
    extra_fields: Option<Value>,
    client: &ApiClient,
) -> Result<Value> {
    // Jira takes only the fields on this type's own create screen, so one
    // nobody asked for is a create refused rather than a field ignored. Each
    // optional field is written only where the caller gave it — an omitted
    // `--description` is not an empty description, it is no description.
    let mut fields = json!({
        "project": {
            "key": project_key
        },
        "summary": summary,
        "issuetype": {
            "name": issue_type
        }
    });
    if !description.is_null() {
        fields["description"] = adf::process_description_input(description)?;
    }
    // Every sub-task type requires it, and a team-managed project models an
    // epic's children the same way, so the field is what makes either of those
    // creatable at all.
    //
    // Which types require it is not decided here. The create screen's own
    // metadata is the answer, and guessing from the type's name would be wrong
    // in every language and every renamed scheme — an omission comes back from
    // Jira naming the field.
    if let Some(parent) = parent_key {
        fields["parent"] = json!({ "key": parent });
    }
    if let Some(extra) = extra_fields {
        merge_extra_fields(&mut fields, extra)?;
    }
    let body = json!({ "fields": fields });

    let request = client
        .post(Service::Jira, "/rest/api/3/issue")
        .await?
        .header("Content-Type", "application/json")
        .json(&body);
    let response = client.execute("create issue", request).await?;

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
        && !description_ref.is_null()
    {
        let description = std::mem::replace(description_ref, Value::Null);
        let description_adf = adf::process_description_input(description)?;
        fields_obj.insert("description".to_string(), description_adf);
    }

    let request = client
        .put(Service::Jira, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&json!({
            "fields": fields_value
        }));
    client.execute("update issue", request).await?;

    Ok(json!({}))
}

/// Put each issue under `epic_key` by setting its `parent`, the field Jira
/// uses for the epic in company-managed and team-managed projects alike.
pub async fn assign_issues_to_epic(
    epic_key: &str,
    issues: &[String],
    client: &ApiClient,
) -> Result<Value> {
    edit_parents(issues, Some(epic_key), "assign issues to epic", client).await
}

/// Take each issue out of its epic by clearing its `parent`. A sub-task
/// cannot lose its parent, and Jira refuses the edit.
pub async fn unassign_issues_from_epic(issues: &[String], client: &ApiClient) -> Result<Value> {
    edit_parents(issues, None, "unassign issues from epic", client).await
}

/// Set or clear the parent of each issue in turn. Jira has no transactional
/// bulk edit, so a failure names the issue it stopped at and how many were
/// already edited.
async fn edit_parents(
    issues: &[String],
    parent: Option<&str>,
    operation: &str,
    client: &ApiClient,
) -> Result<Value> {
    if issues.is_empty() {
        anyhow::bail!("Failed to {operation}: no issues provided");
    }
    let body = json!({ "fields": { "parent": parent.map(|key| json!({ "key": key })) } });
    for (edited, issue) in issues.iter().enumerate() {
        edit_parent(issue, &body, parent.is_none(), operation, client)
            .await
            .with_context(|| {
                format!(
                    "Failed to {operation} at {issue} after {edited}/{} issues were edited",
                    issues.len()
                )
            })?;
    }
    Ok(json!({}))
}

/// Edit one issue's parent. A cleared parent is read back, because Jira
/// answers 204 to the documented `update.parent.set.none` while leaving the
/// parent in place; clearing is confirmed rather than assumed. The read-back
/// fails under its own operation, so a caller can tell an edit Jira refused
/// from one it accepted but could not show.
async fn edit_parent(
    issue: &str,
    body: &Value,
    confirm_cleared: bool,
    operation: &str,
    client: &ApiClient,
) -> Result<()> {
    let path = format!("/rest/api/3/issue/{}", encode_path_segment(issue));
    let request = client
        .put(Service::Jira, &path)
        .await?
        .header("Content-Type", "application/json")
        .json(body);
    client.execute(operation, request).await?;
    if !confirm_cleared {
        return Ok(());
    }
    let request = client
        .get(Service::Jira, &path)
        .await?
        .query(&[("fields", "parent")]);
    let read_back = "read back the issue parent";
    let unconfirmed =
        || format!("Jira accepted clearing the parent of {issue}, but reading it back failed");
    let data: Value = client
        .execute(read_back, request)
        .await
        .with_context(unconfirmed)?
        .json()
        .await
        .with_context(unconfirmed)?;
    // Jira omits `fields` when every requested field is empty, so the issue's
    // own key is what shows this is an issue rather than a drifted body.
    require_field(&data, "/key", read_back).with_context(unconfirmed)?;
    if let Some(parent) = data.pointer("/fields/parent/key").and_then(Value::as_str) {
        anyhow::bail!(
            "Jira accepted clearing the parent of {issue}, but it is still under {parent}"
        );
    }
    Ok(())
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

    let request = client
        .delete(Service::Jira, &url)
        .await?
        .query(&[("deleteSubtasks", delete_subtasks.to_string())]);
    client.execute("delete issue", request).await?;

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

    let request = client
        .post(Service::Jira, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&body);
    client.execute("transition issue", request).await?;

    Ok(json!({}))
}

pub async fn get_transitions(issue_key: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/rest/api/3/issue/{}/transitions",
        encode_path_segment(issue_key)
    );

    let request = client
        .get(Service::Jira, &url)
        .await?
        .header("Accept", "application/json");
    let response = client.execute("get transitions", request).await?;

    let data: Value = response.json().await?;
    Ok(list_envelope(
        require_array(&data, "/transitions", "get transitions")?,
        client,
    ))
}
