use super::{fetch_all_v2_results, fetch_version_number, resolve_space_id, v2_list_envelope};
use crate::client::{ApiClient, Service};
use crate::confluence::fields::apply_v2_filtering;
use crate::filter;
use crate::http_utils::encode_path_segment;
use crate::markdown::confluence_to_markdown;
use crate::response::{require_field, require_u64};
use anyhow::Result;
use serde_json::{Value, json};

pub async fn get_page(
    page_id: &str,
    include_all_fields: Option<bool>,
    additional_includes: Option<Vec<String>>,
    as_markdown: bool,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!("/wiki/api/v2/pages/{}", encode_path_segment(page_id));

    let query_params = apply_v2_filtering(include_all_fields, additional_includes);

    let request = client
        .get(Service::Confluence, &url)
        .await?
        .header("Accept", "application/json")
        .query(&query_params);
    let response = client.execute("get page", request).await?;

    let mut data: Value = response.json().await?;
    filter::apply(&mut data, client.config());

    if as_markdown {
        convert_page_to_markdown(&mut data);
    }

    Ok(data)
}

pub async fn get_page_children(page_id: &str, client: &ApiClient) -> Result<Value> {
    let path = format!(
        "/wiki/api/v2/pages/{}/children",
        encode_path_segment(page_id)
    );
    let items = fetch_all_v2_results(client, "get child pages", &path, &[]).await?;
    Ok(v2_list_envelope(items, client))
}

pub async fn create_page(
    space_key: &str,
    title: &str,
    content: &str,
    parent_id: Option<&str>,
    client: &ApiClient,
) -> Result<Value> {
    // Resolve the space key to its numeric id via the shared helper (also used
    // by the `space` discovery commands).
    let space_id = resolve_space_id(space_key, client).await?;

    let url = "/wiki/api/v2/pages";

    let mut body = json!({
        "spaceId": space_id,
        "title": title,
        "body": {
            "representation": "storage",
            "value": content
        }
    });
    // `parentId` nests the new page under an existing page; omitting it creates
    // the page at the space root.
    if let Some(parent) = parent_id {
        body["parentId"] = json!(parent);
    }

    let request = client
        .post(Service::Confluence, url)
        .await?
        .header("Content-Type", "application/json")
        .json(&body);
    let response = client.execute("create page", request).await?;

    let data: Value = response.json().await?;
    // `id` is the part the contract promises and the only one a follow-up
    // command needs, so it is the only one required. `title` rides along
    // because a create is where a caller learns what the page was actually
    // named — and only where the response named it: reporting `null` would
    // say the page has no title, which a body that carried no title never
    // said.
    let mut created = json!({ "id": require_field(&data, "/id", "create page")? });
    if let Some(title) = data.get("title").filter(|t| !t.is_null()) {
        created["title"] = title.clone();
    }
    Ok(created)
}

/// Replace a page's title and body.
///
/// `base_version` is the version the new content was derived from. Given, the
/// update claims the version after it, and Confluence refuses the write with a
/// 409 when someone else saved in between; omitted, the update overwrites
/// whatever version is current.
pub async fn update_page(
    page_id: &str,
    title: &str,
    content: &str,
    parent_id: Option<&str>,
    base_version: Option<u64>,
    client: &ApiClient,
) -> Result<Value> {
    let url = format!("/wiki/api/v2/pages/{}", encode_path_segment(page_id));
    let current = match base_version {
        Some(version) => version,
        None => fetch_version_number(client, &url).await?,
    };
    let next_version = current
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("page version {current} has no successor"))?;

    // `status: "current"` is part of the v2 update contract and keeps the page
    // published; this CLI only edits live pages, so it is always "current".
    let mut body = json!({
        "id": page_id,
        "status": "current",
        "title": title,
        "body": {
            "representation": "storage",
            "value": content
        },
        "version": {
            "number": next_version
        }
    });
    // `parentId` re-parents the page when supplied. The v2 PUT preserves the
    // page's existing parent when the field is omitted, so a plain update never
    // detaches a child page.
    if let Some(parent) = parent_id {
        body["parentId"] = json!(parent);
    }

    let request = client
        .put(Service::Confluence, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&body);
    let response = client.execute("update page", request).await?;

    let data: Value = response.json().await?;
    Ok(json!({
        "id": require_field(&data, "/id", "update page")?,
        "version": require_u64(&data, "/version/number", "update page")?,
    }))
}

/// Move a page to the Confluence trash (v2 `DELETE` is recoverable, unlike
/// Jira issue deletion). Still a whole-resource destruction, so the CLI layer
/// requires an explicit `--yes`.
pub async fn delete_page(page_id: &str, client: &ApiClient) -> Result<Value> {
    let url = format!("/wiki/api/v2/pages/{}", encode_path_segment(page_id));

    let request = client.delete(Service::Confluence, &url).await?;
    client.execute("delete page", request).await?;

    Ok(json!({}))
}

fn convert_page_to_markdown(data: &mut Value) {
    let Some(body) = data
        .get_mut("body")
        .and_then(|b| b.get_mut("storage"))
        .and_then(|s| s.get_mut("value"))
    else {
        return;
    };
    if let Some(html) = body.as_str().map(|s| s.to_string()) {
        *body = Value::String(confluence_to_markdown(&html));
    }
}
