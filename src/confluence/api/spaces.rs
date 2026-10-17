use super::{fetch_all_v2_results, v2_list_envelope};
use crate::client::{ApiClient, Service};
use crate::filter;
use anyhow::Result;
use serde_json::Value;

/// Fetch a single space object by key via the v2 spaces endpoint (`?keys=`),
/// or `None` when no space matches. Returns the raw API object **without**
/// `filter::apply` so each caller decides: `resolve_space_id` reads the
/// unfiltered `id`; `get_space` applies the field filter before handing the
/// object to the user.
async fn fetch_space_by_key(space_key: &str, client: &ApiClient) -> Result<Option<Value>> {
    let request = client
        .get(Service::Confluence, "/wiki/api/v2/spaces")
        .await?
        .header("Accept", "application/json")
        .query(&[("keys", space_key)]);
    let response = client
        .execute(&format!("get space '{space_key}'"), request)
        .await?;

    let data: Value = response.json().await?;
    // No `results` array on a 2xx is drift, not an answer: reading it as "not
    // there" makes a lookup report an absence it never established.
    let Some(results) = data["results"].as_array() else {
        anyhow::bail!("lookup succeeded but its response had no 'results' array: {data}");
    };
    // Deliberately not compared against `space_key`, and not against
    // `currentActiveAlias` either. `keys=` accepts a space alias, and aliases
    // exist so an old URL keeps working — so a lookup by a *historical* alias
    // resolves correctly while the result names neither the requested string in
    // `key` nor in `currentActiveAlias`, which the schema defines as the
    // *currently active* one. Any equality check here refuses that lookup.
    // What this id decides — which space a page is created in — is undone by
    // deleting the page, unlike the property lookup below, whose id is what an
    // overwrite and a delete land on.
    Ok(results.first().cloned())
}

/// Resolve a Confluence space key to its numeric space id. The single
/// space-key→id lookup, shared by `create_page` and the `space` commands.
pub async fn resolve_space_id(space_key: &str, client: &ApiClient) -> Result<String> {
    // Two facts, kept apart: no space by that key, and a space whose `id` the
    // response did not carry. Collapsing them reports an absence never
    // established — the property lookup below already keeps them separate.
    let Some(space) = fetch_space_by_key(space_key, client).await? else {
        anyhow::bail!("Space '{space_key}' not found");
    };
    space["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Space '{space_key}' has no string 'id': {space}"))
}

/// List spaces visible to the caller (v2), following pagination to completion.
pub async fn get_spaces(client: &ApiClient) -> Result<Value> {
    let items = fetch_all_v2_results(client, "list spaces", "/wiki/api/v2/spaces", &[]).await?;
    Ok(v2_list_envelope(items, client))
}

/// Fetch a single space by key (v2). Returns the space object after filtering.
pub async fn get_space(space_key: &str, client: &ApiClient) -> Result<Value> {
    let mut space = fetch_space_by_key(space_key, client)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Space '{}' not found", space_key))?;
    filter::apply(&mut space, client.config());
    Ok(space)
}
