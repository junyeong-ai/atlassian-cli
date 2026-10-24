use super::{fetch_all_v2_results, v2_list_envelope};
use crate::client::{ApiClient, Service};
use crate::http_utils::{content_type_for_filename, encode_path_segment};
use crate::response::require_field;
use anyhow::Result;
use serde_json::{Value, json};

/// List the labels on a page (v2).
pub async fn get_labels(page_id: &str, client: &ApiClient) -> Result<Value> {
    let path = format!("/wiki/api/v2/pages/{}/labels", encode_path_segment(page_id));
    let items = fetch_all_v2_results(client, "get labels", &path, &[]).await?;
    Ok(v2_list_envelope(items, client))
}

/// Add a label to a page. v2 exposes no label-write endpoint, so this uses the
/// stable v1 content-label API. v1 POST adds without clearing existing labels,
/// so repeated calls are safe for agent retries. Side-effect only → `{}`.
pub async fn add_label(page_id: &str, label: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/wiki/rest/api/content/{}/label",
        encode_path_segment(page_id)
    );

    let request_body = json!([{ "prefix": "global", "name": label }]);

    let request = client
        .post(Service::Confluence, &url)
        .await?
        .header("Content-Type", "application/json")
        .json(&request_body);
    client.execute_idempotent("add label", request).await?;

    Ok(json!({}))
}

/// Remove a label from a page via the v1 content-label API. The label name is
/// the specificity guard (a targeted sub-resource removal), so no `--yes`. The
/// name rides in a query param via reqwest's builder, never the path.
pub async fn remove_label(page_id: &str, label: &str, client: &ApiClient) -> Result<Value> {
    let url = format!(
        "/wiki/rest/api/content/{}/label",
        encode_path_segment(page_id)
    );

    let request = client
        .delete(Service::Confluence, &url)
        .await?
        .query(&[("name", label)]);
    client.execute("remove label", request).await?;

    Ok(json!({}))
}

/// List all content properties on a page (v2). Properties are arbitrary JSON
/// key/value metadata attached to a page — a clean place to store structured,
/// machine-read state alongside the human-authored body.
pub async fn get_properties(page_id: &str, client: &ApiClient) -> Result<Value> {
    let path = format!(
        "/wiki/api/v2/pages/{}/properties",
        encode_path_segment(page_id)
    );
    let items = fetch_all_v2_results(client, "get properties", &path, &[]).await?;
    Ok(v2_list_envelope(items, client))
}

/// Create or update a content property on a page (upsert keyed by `key`). When
/// a property with `key` already exists it is updated with the required version
/// bump read from the same lookup; otherwise a new one is created. `value` is
/// arbitrary JSON. Returns `{"id": ...}`.
pub async fn set_property(
    page_id: &str,
    key: &str,
    value: Value,
    client: &ApiClient,
) -> Result<Value> {
    let encoded_page = encode_path_segment(page_id);
    let collection_url = format!("/wiki/api/v2/pages/{}/properties", encoded_page);

    let existing = fetch_property_by_key(client, &collection_url, key).await?;

    let request = if let Some(prop) = existing {
        let prop_id = prop["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Property lookup returned no id"))?;
        let next_version = prop["version"]["number"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Failed to read current property version"))?
            + 1;
        let url = format!(
            "/wiki/api/v2/pages/{}/properties/{}",
            encoded_page,
            encode_path_segment(prop_id)
        );
        let request_body = json!({
            "key": key,
            "value": value,
            "version": { "number": next_version },
        });
        client
            .put(Service::Confluence, &url)
            .await?
            .header("Content-Type", "application/json")
            .json(&request_body)
    } else {
        let request_body = json!({ "key": key, "value": value });
        client
            .post(Service::Confluence, &collection_url)
            .await?
            .header("Content-Type", "application/json")
            .json(&request_body)
    };

    let response = client.execute("set property", request).await?;

    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/id", "set property")? }))
}

/// Delete a content property from a page by key. The key is the specificity
/// guard → no `--yes`. A missing key is reported as an error rather than a
/// silent success.
pub async fn delete_property(page_id: &str, key: &str, client: &ApiClient) -> Result<Value> {
    let encoded_page = encode_path_segment(page_id);
    let collection_url = format!("/wiki/api/v2/pages/{}/properties", encoded_page);

    let prop = fetch_property_by_key(client, &collection_url, key)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Property '{}' not found", key))?;
    let prop_id = prop["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Property lookup returned no id"))?;

    let url = format!(
        "/wiki/api/v2/pages/{}/properties/{}",
        encoded_page,
        encode_path_segment(prop_id)
    );

    let request = client.delete(Service::Confluence, &url).await?;
    client.execute("delete property", request).await?;

    Ok(json!({}))
}

/// Look up a single content property by key on a page's property collection.
/// Returns the property object, or `None` when no property with that key
/// exists. Shared by `set_property` (create-vs-update decision) and
/// `delete_property` (id resolution); mirrors `fetch_space_by_key`.
async fn fetch_property_by_key(
    client: &ApiClient,
    collection_url: &str,
    key: &str,
) -> Result<Option<Value>> {
    let request = client
        .get(Service::Confluence, collection_url)
        .await?
        .header("Accept", "application/json")
        .query(&[("key", key)]);
    let response = client.execute("look up property", request).await?;

    let data: Value = response.json().await?;
    // No `results` array on a 2xx is drift, not an answer: reading it as "not
    // there" makes a lookup report an absence it never established.
    let Some(results) = data["results"].as_array() else {
        anyhow::bail!("lookup succeeded but its response had no 'results' array: {data}");
    };
    // The filter is the server's, and the id that comes back is what
    // `set_property` overwrites and `delete_property` deletes. Both are
    // irreversible for whatever the id actually names, so this refuses anything
    // it cannot confirm — a key that contradicts, and equally one that is
    // absent, since the v2 schema does not require it and an unconfirmed match
    // is what would take data nobody named.
    match results.first() {
        None => Ok(None),
        Some(found) if found["key"].as_str() == Some(key) => Ok(Some(found.clone())),
        Some(found) => anyhow::bail!(
            "looked up property '{key}' and the result {}, so it is not established as the one \
             asked for: {found}",
            match found["key"].as_str() {
                Some(other) => format!("names '{other}' instead"),
                None => "names no key".to_string(),
            }
        ),
    }
}

/// List the attachments on a page (v2).
pub async fn get_attachments(page_id: &str, client: &ApiClient) -> Result<Value> {
    let path = format!(
        "/wiki/api/v2/pages/{}/attachments",
        encode_path_segment(page_id)
    );
    let items = fetch_all_v2_results(client, "get attachments", &path, &[]).await?;
    Ok(v2_list_envelope(items, client))
}

/// Upload a local file as an attachment on a page. v2 exposes no
/// attachment-create endpoint, so this uses the stable v1 multipart API (the
/// same v1 exception as label writes). `PUT` upserts by filename — a new file
/// is created, an existing one gets a new version — so repeated calls are safe
/// for agent retries. The `X-Atlassian-Token: nocheck` header is required by
/// Confluence to bypass its XSRF check on multipart uploads.
///
/// Under OAuth this needs the `write:attachment:confluence` scope (basic-auth
/// tokens carry the user's own permissions and are unaffected).
pub async fn upload_attachment(
    page_id: &str,
    file_path: &str,
    comment: Option<&str>,
    minor_edit: bool,
    content_type: Option<&str>,
    client: &ApiClient,
) -> Result<Value> {
    let bytes = std::fs::read(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", file_path, e))?;
    // The displayed attachment name is the path's final component; this is
    // deterministic path parsing, not content sniffing.
    let file_name = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Could not derive a file name from '{}'", file_path))?
        .to_string();

    // An explicit `--content-type` wins; otherwise the type is mapped from the
    // extension so images/PDFs render inline instead of becoming opaque
    // `application/octet-stream` downloads.
    let mime = content_type.unwrap_or_else(|| content_type_for_filename(&file_name));

    // `minorEdit` is always sent (the v1 endpoint expects it); `true` suppresses
    // the watcher notification that a re-upload would otherwise fire.
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str(mime)
        .map_err(|e| anyhow::anyhow!("Invalid content type '{}': {}", mime, e))?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("minorEdit", minor_edit.to_string());
    if let Some(c) = comment {
        form = form.text("comment", c.to_string());
    }

    let url = format!(
        "/wiki/rest/api/content/{}/child/attachment",
        encode_path_segment(page_id)
    );

    let request = client
        .put(Service::Confluence, &url)
        .await?
        .header("X-Atlassian-Token", "nocheck")
        .multipart(form);
    let response = client.execute("upload attachment", request).await?;

    // v1 wraps the created/updated attachment in a `results` array.
    let data: Value = response.json().await?;
    Ok(json!({ "id": require_field(&data, "/results/0/id", "upload attachment")? }))
}
