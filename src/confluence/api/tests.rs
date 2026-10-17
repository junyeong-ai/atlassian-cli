use super::*;
use crate::test_utils::{create_test_config_with_filters, mock_client};
use wiremock::matchers::{
    body_json, body_string_contains, header, method, path, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_test_config(confluence_spaces_filter: Vec<String>) -> Config {
    create_test_config_with_filters(vec![], confluence_spaces_filter)
}

#[test]
fn effective_search_limit_clamps_to_cap() {
    assert_eq!(effective_search_limit(10), 10);
    assert_eq!(effective_search_limit(1000), MAX_SEARCH_LIMIT);
    assert_eq!(effective_search_limit(MAX_SEARCH_LIMIT), MAX_SEARCH_LIMIT);
    assert_eq!(effective_search_limit(0), 1);
}

#[tokio::test]
async fn integ_search_all_honors_limit_on_first_page() {
    let server = MockServer::start().await;
    // The `--all` first page must carry the user's clamped limit, not the
    // hardcoded body cap. limit=10 → query param "10".
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .and(query_param("limit", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [],
            "totalSize": 0
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = search_all("type = page", 10, None, None, false, false, &client)
        .await
        .unwrap();
    assert_eq!(result["total"], 0);
}

#[tokio::test]
async fn integ_search_returns_envelope_with_server_total() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "content": { "id": "1", "title": "P" } }],
            "totalSize": 7
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = search("type = page", 10, None, None, false, &client)
        .await
        .unwrap();
    assert_eq!(result["total"], 7);
    assert_eq!(result["count"], 1);
    assert_eq!(result["items"][0]["id"], "1");
}

#[tokio::test]
async fn integ_search_bails_on_missing_results() {
    // A 2xx that omits `results` is schema drift, not a zero-match search.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "totalSize": 0 })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = search("type = page", 10, None, None, false, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("no 'results' array"), "got: {err}");
}

/// A lookup's 2xx without a `results` array is drift; reading it as "not
/// found" reports an absence nothing established.
#[tokio::test]
async fn integ_space_lookup_bails_on_missing_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "_links": {} })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = resolve_space_id("ENG", &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("no 'results' array"), "got: {err}");
}

/// A space that came back without an `id` is not a space that is not there.
#[tokio::test]
async fn integ_a_space_without_an_id_is_not_a_missing_space() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "results": [{ "key": "ENG" }] })),
        )
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = resolve_space_id("ENG", &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("no string 'id'"), "got: {err}");
    assert!(!err.contains("not found"), "got: {err}");
}

/// v1 search answers space and user entities without a `content` object.
/// Dropping them reports no matches for a query that had them.
#[tokio::test]
async fn integ_search_keeps_a_result_that_carries_no_content() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "space": { "key": "ENG" }, "title": "Engineering" }],
            "totalSize": 1
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = search("type = space", 10, None, None, false, &client)
        .await
        .unwrap();

    assert_eq!(result["count"], 1, "{result}");
    assert_eq!(result["items"][0]["title"], "Engineering", "{result}");
}

#[tokio::test]
async fn integ_search_requires_numeric_total_size() {
    // Present `results` but absent `totalSize` must fail, not fabricate one.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = search("type = page", 10, None, None, false, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("totalSize"), "got: {err}");
}

#[tokio::test]
async fn integ_get_page_encodes_page_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/12%20345"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "12 345" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_page("12 345", None, None, false, &client)
        .await
        .unwrap();
    assert_eq!(result["id"], "12 345");
}

#[tokio::test]
async fn integ_delete_page_encodes_id() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/wiki/api/v2/pages/9%2F9"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = delete_page("9/9", &client).await.unwrap();
    assert_eq!(result, json!({}));
}

#[test]
fn test_rate_limit_delay_default() {
    let config = create_test_config(vec![]);
    assert_eq!(config.performance.rate_limit_delay_ms, 200);
}

#[test]
fn test_apply_space_filter_injection() {
    let config = create_test_config(vec!["SPACE1".to_string(), "SPACE2".to_string()]);
    let result = apply_space_filter("type = page", &config);
    assert_eq!(result, "space IN (\"SPACE1\",\"SPACE2\") AND (type = page)");
}

#[test]
fn test_apply_space_filter_not_injected_when_present() {
    let config = create_test_config(vec!["SPACE1".to_string()]);
    let result = apply_space_filter("space = MYSPACE AND type = page", &config);
    assert_eq!(result, "space = MYSPACE AND type = page");
}

#[test]
fn test_apply_space_filter_empty_filter() {
    let config = create_test_config(vec![]);
    let result = apply_space_filter("type = page", &config);
    assert_eq!(result, "type = page");
}

#[test]
fn test_apply_space_filter_ignores_quoted_keyword() {
    let config = create_test_config(vec!["SPACE1".to_string()]);
    let result = apply_space_filter("title ~ \"space = anywhere\"", &config);
    assert!(
        result.starts_with("space IN (\"SPACE1\")"),
        "filter should be injected, got: {result}"
    );
}

#[test]
fn test_apply_space_filter_skips_word_boundary_non_match() {
    let config = create_test_config(vec!["SPACE1".to_string()]);
    let result = apply_space_filter("mySpace = X", &config);
    assert_eq!(result, "space IN (\"SPACE1\") AND (mySpace = X)");
}

#[test]
fn test_apply_space_filter_whitespace_only_cql_collapses_to_bare_filter() {
    let config = create_test_config(vec!["SPACE1".to_string()]);
    // Whitespace-only CQL collapses to a bare filter — no dangling
    // `AND (   )`. Matches the Jira-side behavior in apply_project_filter.
    let result = apply_space_filter("   ", &config);
    assert_eq!(result, "space IN (\"SPACE1\")");
}

/// v1 pagination resolved end to end: join `_links`, then reduce.
fn v1_resolved(base: Value, next: &str) -> Option<String> {
    link_path(&v1_next_link(&base, next)).map(str::to_owned)
}

#[test]
fn v1_joins_a_rooted_next_onto_the_base_path() {
    // `_links.base` carries the `/wiki` prefix that `next` omits.
    assert_eq!(
        v1_resolved(
            json!({ "base": "https://test.atlassian.net/wiki" }),
            "/rest/api/search?cql=type%3Dpage&cursor=abc123"
        )
        .unwrap(),
        "/wiki/rest/api/search?cql=type%3Dpage&cursor=abc123"
    );
}

#[test]
fn v1_leaves_an_absolute_next_unjoined() {
    // An absolute `next` already carries `/wiki`; joining would double it.
    assert_eq!(
        v1_resolved(
            json!({ "base": "https://test.atlassian.net/wiki" }),
            "https://test.atlassian.net/wiki/rest/api/search?cursor=xyz"
        )
        .unwrap(),
        "/wiki/rest/api/search?cursor=xyz"
    );
}

#[test]
fn v1_survives_a_missing_base() {
    assert_eq!(
        v1_resolved(json!({}), "/rest/api/search?cursor=z").unwrap(),
        "/rest/api/search?cursor=z"
    );
}

#[test]
fn link_path_discards_the_host_a_response_names() {
    // The decisive property: a pagination link pointing at a foreign host
    // contributes its path only. The host always comes from `build_url`,
    // so credentials cannot be steered elsewhere by a response body.
    assert_eq!(
        link_path("https://evil.example/wiki/api/v2/pages?cursor=x"),
        Some("/wiki/api/v2/pages?cursor=x")
    );
    assert_eq!(
        v1_resolved(
            json!({ "base": "https://test.atlassian.net/wiki" }),
            "https://evil.example/wiki/rest/api/search?cursor=x"
        )
        .unwrap(),
        "/wiki/rest/api/search?cursor=x"
    );
    // A `base` naming a foreign host is reduced the same way.
    assert_eq!(
        v1_resolved(
            json!({ "base": "https://evil.example/wiki" }),
            "/rest/api/search"
        )
        .unwrap(),
        "/wiki/rest/api/search"
    );
}

#[test]
fn link_path_passes_rooted_links_through_and_rejects_hostless_absolutes() {
    assert_eq!(
        link_path("/wiki/api/v2/pages?cursor=x"),
        Some("/wiki/api/v2/pages?cursor=x")
    );
    assert_eq!(link_path("https://test.atlassian.net"), None);
}

#[test]
fn link_path_rejects_a_link_that_would_extend_the_authority() {
    // No scheme and no leading `/`. Treating it as a path would build
    // `https://site@evil.example/…`, which RFC 3986 resolves with
    // `evil.example` as the host and `site` as userinfo — the request's
    // credentials would go to a host named by the response.
    for link in [
        "@evil.example/wiki/api/v2/pages?cursor=x",
        "evil.example/wiki/api/v2/pages",
        ":@evil.example/x",
    ] {
        assert_eq!(link_path(link), None, "must reject {link}");
        assert_eq!(
            v1_resolved(json!({ "base": "https://s.atlassian.net/wiki" }), link),
            None,
            "must reject {link}"
        );
    }
}

#[test]
fn link_path_keeps_a_rooted_link_whose_query_contains_a_scheme() {
    // Confluence echoes `cql` into `next` without percent-encoding it, so
    // searching for a URL produces a genuinely rooted link containing
    // `://`. Reading that as "absolute" would drop the path and cursor.
    let n = "/wiki/api/v2/pages?cursor=abc&cql=text~http://x.example/p";
    assert_eq!(link_path(n), Some(n));

    let v1 = "/rest/api/search?cql=text~%22http://x.example%22&cursor=abc";
    assert_eq!(
        v1_resolved(json!({ "base": "https://s.atlassian.net/wiki" }), v1).unwrap(),
        format!("/wiki{v1}")
    );
}

#[test]
fn cursor_trail_rejects_a_revisited_path() {
    let mut trail = CursorTrail::default();
    assert_eq!(trail.step("search", "/a?cursor=1").unwrap(), "/a?cursor=1");
    assert_eq!(trail.step("search", "/a?cursor=2").unwrap(), "/a?cursor=2");
    // Same resolved path behind a different host: still not advancing.
    let err = trail
        .step("search", "https://elsewhere.invalid/a?cursor=1")
        .unwrap_err()
        .to_string();
    assert!(err.contains("did not advance"), "{err}");
}

#[test]
fn cursor_trail_rejects_a_link_with_no_path() {
    let err = CursorTrail::default()
        .step("search", "https://elsewhere.invalid")
        .unwrap_err()
        .to_string();
    assert!(err.contains("no path"), "{err}");
}

#[test]
fn every_built_url_keeps_the_configured_host_as_the_authority() {
    // Parse rather than string-compare: the defect this guards against is
    // precisely one that string inspection reads as harmless.
    use crate::auth::AuthStrategy;
    let basic =
        crate::auth::BasicStrategy::new(Some("test.atlassian.net"), "u@x".into(), "tk".into())
            .unwrap();
    for path in [
        "/wiki/api/v2/pages",
        "@evil.example/wiki",
        "//evil.example/wiki",
        "wiki/api/v2/pages",
    ] {
        let url = reqwest::Url::parse(&basic.build_url(Service::Confluence, path)).unwrap();
        assert_eq!(url.host_str(), Some("test.atlassian.net"), "path {path}");
        assert_eq!(url.username(), "", "path {path} leaked userinfo");
    }
}

#[tokio::test]
async fn integ_create_page_resolves_space_then_posts_storage_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .and(query_param("keys", "ENG"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "sid", "key": "ENG" }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/wiki/api/v2/pages"))
        .and(body_json(json!({
            "spaceId": "sid",
            "title": "Spec",
            "body": { "representation": "storage", "value": "<p>x</p>" }
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "pid", "title": "Spec"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = create_page("ENG", "Spec", "<p>x</p>", None, None, None, &client)
        .await
        .unwrap();
    assert_eq!(result, json!({ "id": "pid", "title": "Spec" }));
}

/// A `"title": null` beside the id would report a page named nothing. The
/// create landed and the id is there, so the write is not a failure — the
/// title is simply not among what came back, and is not reported.
#[tokio::test]
async fn integ_create_page_reports_no_title_where_the_response_carried_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "sid", "key": "ENG" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/wiki/api/v2/pages"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "pid" })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = create_page("ENG", "Spec", "<p>x</p>", None, None, None, &client)
        .await
        .unwrap();
    assert_eq!(result, json!({ "id": "pid" }));
}

#[tokio::test]
async fn integ_create_page_nests_under_parent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .and(query_param("keys", "ENG"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "results": [{ "id": "sid", "key": "ENG" }] })),
        )
        .mount(&server)
        .await;
    // `--parent` rides in the body as `parentId`; body_json is exact, so its
    // presence (and absence in the root-level test above) is asserted.
    Mock::given(method("POST"))
        .and(path("/wiki/api/v2/pages"))
        .and(body_json(json!({
            "spaceId": "sid",
            "title": "Child",
            "body": { "representation": "storage", "value": "<p>x</p>" },
            "parentId": "999"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "pid", "title": "Child"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = create_page("ENG", "Child", "<p>x</p>", Some("999"), None, None, &client)
        .await
        .unwrap();
    assert_eq!(result["id"], "pid");
}

#[tokio::test]
async fn integ_update_page_reads_version_then_puts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/12345"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "version": { "number": 5 } })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/wiki/api/v2/pages/12345"))
        .and(body_json(json!({
            "id": "12345",
            "status": "current",
            "title": "Updated",
            "body": { "representation": "storage", "value": "<p>y</p>" },
            "version": { "number": 6 }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "12345", "version": { "number": 6 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = update_page("12345", "Updated", "<p>y</p>", None, None, None, &client)
        .await
        .unwrap();
    assert_eq!(result, json!({ "id": "12345", "version": 6 }));
}

#[tokio::test]
async fn integ_update_page_reparents_when_parent_given() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/12345"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "version": { "number": 5 } })),
        )
        .mount(&server)
        .await;
    // `--parent` adds `parentId` to the PUT body; body_json is exact, so the
    // sibling test above (no parentId) and this one together pin both paths.
    Mock::given(method("PUT"))
        .and(path("/wiki/api/v2/pages/12345"))
        .and(body_json(json!({
            "id": "12345",
            "status": "current",
            "title": "Updated",
            "body": { "representation": "storage", "value": "<p>y</p>" },
            "version": { "number": 6 },
            "parentId": "777"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "12345", "version": { "number": 6 }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = update_page(
        "12345",
        "Updated",
        "<p>y</p>",
        Some("777"),
        None,
        None,
        &client,
    )
    .await
    .unwrap();
    assert_eq!(result["version"], 6);
}

// --- Footer comment write -------------------------------------------

#[tokio::test]
async fn integ_add_comment_posts_storage_body_and_returns_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/wiki/api/v2/footer-comments"))
        .and(body_json(json!({
            "pageId": "123",
            "body": { "representation": "storage", "value": "<p>hi</p>" }
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "555" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = add_comment("123", "<p>hi</p>", None, &client)
        .await
        .unwrap();
    assert_eq!(result, json!({ "id": "555" }));
}

#[tokio::test]
async fn integ_add_comment_reply_includes_parent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/wiki/api/v2/footer-comments"))
        // A reply carries ONLY parentCommentId — sending pageId too makes
        // the real v2 API 400 ("one and only one of … pageId … or
        // parentCommentId"). body_json is an exact match, so the absence of
        // pageId here is asserted.
        .and(body_json(json!({
            "body": { "representation": "storage", "value": "ok" },
            "parentCommentId": "999"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "556" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = add_comment("123", "ok", Some("999"), &client)
        .await
        .unwrap();
    assert_eq!(result["id"], "556");
}

#[tokio::test]
async fn integ_update_comment_reads_version_then_puts() {
    let server = MockServer::start().await;
    // First the current version is read, then the PUT bumps it to +1.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/footer-comments/77"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "id": "77", "version": { "number": 3 } })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/wiki/api/v2/footer-comments/77"))
        .and(body_json(json!({
            "version": { "number": 4 },
            "body": { "representation": "storage", "value": "edited" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "77" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = update_comment("77", "edited", &client).await.unwrap();
    assert_eq!(result["id"], "77");
}

#[tokio::test]
async fn integ_delete_comment_encodes_id() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/wiki/api/v2/footer-comments/7%2F7"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = delete_comment("7/7", &client).await.unwrap();
    assert_eq!(result, json!({}));
}

// --- Labels ----------------------------------------------------------

#[tokio::test]
async fn integ_add_label_posts_v1_global_prefix() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/wiki/rest/api/content/123/label"))
        .and(body_json(json!([{ "prefix": "global", "name": "urgent" }])))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = add_label("123", "urgent", &client).await.unwrap();
    assert_eq!(result, json!({}));
}

#[tokio::test]
async fn integ_remove_label_passes_name_query() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/wiki/rest/api/content/123/label"))
        .and(query_param("name", "urgent"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = remove_label("123", "urgent", &client).await.unwrap();
    assert_eq!(result, json!({}));
}

#[tokio::test]
async fn integ_get_labels_returns_items_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/123/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "1", "name": "urgent" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_labels("123", &client).await.unwrap();
    assert_eq!(result["items"][0]["name"], "urgent");
}

// --- Content properties ---------------------------------------------

#[tokio::test]
async fn integ_get_properties_returns_items_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/123/properties"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "p1", "key": "state", "value": { "phase": 1 } }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_properties("123", &client).await.unwrap();
    assert_eq!(result["items"][0]["key"], "state");
}

#[tokio::test]
async fn integ_set_property_creates_when_absent() {
    let server = MockServer::start().await;
    // Lookup by key returns no match → POST a new property.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/123/properties"))
        .and(query_param("key", "state"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/wiki/api/v2/pages/123/properties"))
        .and(body_json(
            json!({ "key": "state", "value": { "phase": 2 } }),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "p9" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = set_property("123", "state", json!({ "phase": 2 }), &client)
        .await
        .unwrap();
    assert_eq!(result, json!({ "id": "p9" }));
}

#[tokio::test]
async fn integ_set_property_updates_when_present() {
    let server = MockServer::start().await;
    // Lookup returns an existing property with version 4 → PUT with 5.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/123/properties"))
        .and(query_param("key", "state"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "p1", "key": "state", "version": { "number": 4 } }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/wiki/api/v2/pages/123/properties/p1"))
        .and(body_json(json!({
            "key": "state",
            "value": { "phase": 3 },
            "version": { "number": 5 }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "p1" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = set_property("123", "state", json!({ "phase": 3 }), &client)
        .await
        .unwrap();
    assert_eq!(result, json!({ "id": "p1" }));
}

#[tokio::test]
async fn integ_delete_property_resolves_key_then_deletes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/123/properties"))
        .and(query_param("key", "state"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "p1", "key": "state" }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/wiki/api/v2/pages/123/properties/p1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = delete_property("123", "state", &client).await.unwrap();
    assert_eq!(result, json!({}));
}

#[tokio::test]
async fn integ_delete_property_missing_key_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/123/properties"))
        .and(query_param("key", "ghost"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = delete_property("123", "ghost", &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("not found"), "got: {err}");
}

// --- Spaces ----------------------------------------------------------

#[tokio::test]
async fn integ_get_spaces_returns_items_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "1", "key": "ENG" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_spaces(&client).await.unwrap();
    assert_eq!(result["items"][0]["key"], "ENG");
}

#[tokio::test]
async fn integ_get_space_filters_by_key_and_returns_single() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .and(query_param("keys", "ENG"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "1", "key": "ENG" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_space("ENG", &client).await.unwrap();
    assert_eq!(result["id"], "1");
    assert_eq!(result["key"], "ENG");
}

#[tokio::test]
async fn integ_resolve_space_id_extracts_numeric_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .and(query_param("keys", "ENG"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "42", "key": "ENG" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let id = resolve_space_id("ENG", &client).await.unwrap();
    assert_eq!(id, "42");
}

// --- Attachments -----------------------------------------------------

#[tokio::test]
async fn integ_get_attachments_returns_items_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/123/attachments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "att1", "title": "spec.pdf" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_attachments("123", &client).await.unwrap();
    assert_eq!(result["items"][0]["title"], "spec.pdf");
}

#[tokio::test]
async fn integ_upload_attachment_puts_multipart_with_token_header() {
    let server = MockServer::start().await;
    // The v1 upload is a multipart PUT guarded by the XSRF-bypass header. The
    // body itself must carry the file bytes, the comment, and minorEdit —
    // assert on the multipart payload so a dropped part fails the test.
    Mock::given(method("PUT"))
        .and(path("/wiki/rest/api/content/123/child/attachment"))
        .and(header("X-Atlassian-Token", "nocheck"))
        .and(body_string_contains("name=\"file\""))
        .and(body_string_contains("hello"))
        .and(body_string_contains("name=\"comment\""))
        .and(body_string_contains("v2"))
        .and(body_string_contains("name=\"minorEdit\""))
        .and(body_string_contains("true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "att9" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), b"hello").unwrap();

    let client = mock_client(server.uri());
    let result = upload_attachment(
        "123",
        tmp.path().to_str().unwrap(),
        Some("v2"),
        true,
        None,
        &client,
    )
    .await
    .unwrap();
    assert_eq!(result, json!({ "id": "att9" }));
}

#[tokio::test]
async fn integ_upload_attachment_reports_missing_file() {
    let server = MockServer::start().await;
    let client = mock_client(server.uri());
    let err = upload_attachment("123", "/no/such/file.bin", None, false, None, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Failed to read"), "got: {err}");
}

// --- Comment threads --------------------------------------------------

/// Mount one comment collection. Callers mount an empty collection for
/// every leaf too — a thread walk still has to ask, and a missing mock
/// would look like a network failure rather than an unasked question.
async fn mount_comments(server: &MockServer, at: &str, results: Value) {
    Mock::given(method("GET"))
        .and(path(at.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": results })))
        .mount(server)
        .await;
}

/// Where one comment sits in a listing: its id, depth, the comment it
/// answers, its family, and the page it belongs to.
type Placement<'a> = (&'a str, u64, Option<&'a str>, &'a str, Option<&'a str>);

fn thread_shape(listing: &Value) -> Vec<Placement<'_>> {
    listing["items"]
        .as_array()
        .expect("listing has an items array")
        .iter()
        .map(|comment| {
            (
                comment["id"].as_str().expect("id"),
                comment["depth"].as_u64().expect("depth"),
                comment["parentCommentId"].as_str(),
                comment["location"].as_str().expect("location"),
                comment["pageId"].as_str(),
            )
        })
        .collect()
}

#[tokio::test]
async fn integ_get_comments_walks_every_reply_depth_first() {
    let server = MockServer::start().await;
    // r1 ─┬ a1 ─ g1        A page endpoint answers with roots only, so
    //     └ a2             everything below the first row is reachable
    // r2                   solely through the `children` collections.
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/footer-comments",
        json!([{ "id": "r1", "pageId": "5" }, { "id": "r2", "pageId": "5" }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/r1/children",
        json!([{ "id": "a1" }, { "id": "a2" }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/a1/children",
        json!([{ "id": "g1" }]),
    )
    .await;
    for leaf in ["r2", "a2", "g1"] {
        mount_comments(
            &server,
            &format!("/wiki/api/v2/footer-comments/{leaf}/children"),
            json!([]),
        )
        .await;
    }

    let client = mock_client(server.uri());
    let result = get_comments("5", &[CommentFamily::Footer], true, false, &client)
        .await
        .unwrap();

    assert_eq!(
        thread_shape(&result),
        vec![
            ("r1", 0, None, "footer", Some("5")),
            ("a1", 1, Some("r1"), "footer", Some("5")),
            ("g1", 2, Some("a1"), "footer", Some("5")),
            ("a2", 1, Some("r1"), "footer", Some("5")),
            ("r2", 0, None, "footer", Some("5")),
        ]
    );
    // A root answers nothing, and that is stated rather than left out: an
    // absent key and a null one would read the same to a consumer building
    // the tree back up.
    assert!(
        result["items"][0]
            .get("parentCommentId")
            .is_some_and(Value::is_null)
    );
}

#[tokio::test]
async fn integ_get_comments_follows_the_cursor_inside_a_reply_collection() {
    let server = MockServer::start().await;
    // A thread longer than one page is exactly where a single GET would
    // silently drop replies.
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/footer-comments",
        json!([{ "id": "r1" }]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/footer-comments/r1/children"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "a1" }],
            "_links": { "next": "/wiki/api/v2/footer-comments/r1/children?cursor=P2" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/footer-comments/r1/children"))
        .and(query_param("cursor", "P2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "results": [{ "id": "a2" }] })),
        )
        .expect(1)
        .mount(&server)
        .await;
    for leaf in ["a1", "a2"] {
        mount_comments(
            &server,
            &format!("/wiki/api/v2/footer-comments/{leaf}/children"),
            json!([]),
        )
        .await;
    }

    let client = mock_client(server.uri());
    let result = get_comments("5", &[CommentFamily::Footer], true, false, &client)
        .await
        .unwrap();
    assert_eq!(
        thread_shape(&result),
        vec![
            ("r1", 0, None, "footer", Some("5")),
            ("a1", 1, Some("r1"), "footer", Some("5")),
            ("a2", 1, Some("r1"), "footer", Some("5")),
        ]
    );
}

#[tokio::test]
async fn integ_roots_only_never_asks_for_replies() {
    let server = MockServer::start().await;
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/footer-comments",
        json!([{ "id": "r1" }]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/footer-comments/r1/children"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(0)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_comments("5", &[CommentFamily::Footer], false, false, &client)
        .await
        .unwrap();
    assert_eq!(
        thread_shape(&result),
        vec![("r1", 0, None, "footer", Some("5"))]
    );
}

#[tokio::test]
async fn integ_a_family_that_was_not_asked_for_is_never_requested() {
    let server = MockServer::start().await;
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/footer-comments",
        json!([{ "id": "f1" }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/f1/children",
        json!([]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/5/inline-comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(0)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    get_comments("5", &[CommentFamily::Footer], true, false, &client)
        .await
        .unwrap();
}

#[tokio::test]
async fn integ_both_families_are_listed_and_tagged() {
    let server = MockServer::start().await;
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/footer-comments",
        json!([{ "id": "f1" }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/f1/children",
        json!([]),
    )
    .await;
    // The inline family is a whole second set of comments on the same page;
    // listing only the footer one reports a page as quieter than it is.
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/inline-comments",
        json!([{
            "id": "i1",
            "resolutionStatus": "open",
            "properties": {
                "inlineMarkerRef": "marker-1",
                "inlineOriginalSelection": "the highlighted words"
            }
        }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/inline-comments/i1/children",
        json!([{ "id": "i2" }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/inline-comments/i2/children",
        json!([]),
    )
    .await;

    let client = mock_client(server.uri());
    let result = get_comments("5", &CommentFamily::ALL, true, false, &client)
        .await
        .unwrap();

    assert_eq!(
        thread_shape(&result),
        vec![
            ("f1", 0, None, "footer", Some("5")),
            ("i1", 0, None, "inline", Some("5")),
            ("i2", 1, Some("i1"), "inline", Some("5")),
        ]
    );
    // What makes an inline comment findable in the page is its anchor, so
    // the response filter must not be treating it as noise.
    assert_eq!(result["items"][1]["resolutionStatus"], json!("open"));
    assert_eq!(
        result["items"][1]["properties"]["inlineOriginalSelection"],
        json!("the highlighted words")
    );
}

#[tokio::test]
async fn integ_a_reply_id_from_the_server_cannot_steer_the_request_path() {
    let server = MockServer::start().await;
    // A page id is user input, but a comment id reaches the path from a
    // RESPONSE — and the walk asks that path for the comment's children.
    // Left raw, the `..` segments below resolve away and the next request
    // lands on `/wiki/pages/999/children`, a path the walk never chose.
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/footer-comments",
        json!([{ "id": "../../../pages/999" }]),
    )
    .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    get_comments("5", &[CommentFamily::Footer], true, false, &client)
        .await
        .unwrap();

    let requested: Vec<String> = server
        .received_requests()
        .await
        .expect("the mock server recorded its requests")
        .iter()
        .map(|request| request.url.path().to_string())
        .collect();
    assert_eq!(
        requested,
        vec![
            "/wiki/api/v2/pages/5/footer-comments".to_string(),
            "/wiki/api/v2/footer-comments/..%2F..%2F..%2Fpages%2F999/children".to_string(),
        ]
    );
}

#[tokio::test]
async fn integ_thread_walk_bails_before_an_endless_supply_of_new_replies() {
    let server = MockServer::start().await;
    // `seen` catches a repeat; nothing catches a server that answers every
    // level with an id it has not used before.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/5/footer-comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "root" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(EverFreshReply::default())
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let roots = fetch_all_v2_results(
        &client,
        "get comments",
        &CommentFamily::Footer.page_path("5"),
        &comment_query(),
    )
    .await
    .unwrap();
    let mut walk = ThreadWalk::new(&client, CommentFamily::Footer, Some("5"), true);
    walk.limit = 3;
    let err = walk
        .collect("get comments", roots, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("unbounded walk"), "got: {err}");
}

#[tokio::test]
async fn integ_thread_walk_counts_the_replies_it_is_holding_not_only_the_ones_it_took() {
    let server = MockServer::start().await;
    // One level of a wide reply set already outgrows the comments admitted
    // so far, which is why the ceiling counts the queue too.
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/footer-comments",
        json!([{ "id": "root" }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/root/children",
        json!(
            (0..10)
                .map(|n| json!({ "id": format!("r{n}") }))
                .collect::<Vec<_>>()
        ),
    )
    .await;

    let client = mock_client(server.uri());
    let roots = fetch_all_v2_results(
        &client,
        "get comments",
        &CommentFamily::Footer.page_path("5"),
        &comment_query(),
    )
    .await
    .unwrap();
    let mut walk = ThreadWalk::new(&client, CommentFamily::Footer, Some("5"), true);
    walk.limit = 5;
    let err = walk
        .collect("get comments", roots, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("unbounded walk"), "got: {err}");
}

/// One reply per request, under an id never used before — the shape `seen`
/// cannot terminate on.
#[derive(Default)]
struct EverFreshReply(std::sync::atomic::AtomicUsize);

impl wiremock::Respond for EverFreshReply {
    fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
        let n = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ResponseTemplate::new(200)
            .set_body_json(json!({ "results": [{ "id": format!("fresh-{n}") }] }))
    }
}

#[tokio::test]
async fn integ_thread_walk_bails_when_a_comment_answers_itself() {
    let server = MockServer::start().await;
    // A tree cannot reach a comment twice, so this is drift — and a walk
    // that followed it would loop instead of returning a short list.
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/footer-comments",
        json!([{ "id": "r1" }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/r1/children",
        json!([{ "id": "r1" }]),
    )
    .await;

    let client = mock_client(server.uri());
    let err = get_comments("5", &[CommentFamily::Footer], true, false, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("reachable from itself"), "unexpected: {err}");
}

#[tokio::test]
async fn integ_get_comments_converts_storage_at_every_depth() {
    let server = MockServer::start().await;
    // `as_markdown=true` converts a comment's storage HTML in place —
    // replies included, or a thread reads half markdown and half HTML.
    mount_comments(
        &server,
        "/wiki/api/v2/pages/5/footer-comments",
        json!([{
            "id": "c1",
            "body": { "storage": { "value": "<p>hello <strong>world</strong></p>" } }
        }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/c1/children",
        json!([{
            "id": "c2",
            "body": { "storage": { "value": "<p>a <em>reply</em></p>" } }
        }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/c2/children",
        json!([]),
    )
    .await;

    let client = mock_client(server.uri());
    let result = get_comments("5", &[CommentFamily::Footer], true, true, &client)
        .await
        .unwrap();
    for (index, word) in [(0, "world"), (1, "reply")] {
        let body = result["items"][index]["body"]["storage"]["value"]
            .as_str()
            .unwrap();
        assert!(!body.contains("<p>"), "expected HTML stripped, got: {body}");
        assert!(body.contains(word), "expected text preserved, got: {body}");
    }
}

#[tokio::test]
async fn integ_get_comment_reads_only_the_named_family() {
    let server = MockServer::start().await;
    // The family is part of the address. Falling back to the other one on a
    // 404 would make "wrong family", "deleted" and "not visible to you" the
    // same answer.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/inline-comments/77"))
        .and(query_param("body-format", "storage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "77",
            "body": { "storage": { "value": "<p>anchored</p>" } }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/footer-comments/77"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "77" })))
        .expect(0)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_comment("77", CommentFamily::Inline, false, &client)
        .await
        .unwrap();
    assert_eq!(result["id"], json!("77"));
    assert_eq!(result["location"], json!("inline"));
}

#[tokio::test]
async fn integ_get_comment_encodes_the_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/footer-comments/7%2F7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "7/7" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_comment("7/7", CommentFamily::Footer, false, &client)
        .await
        .unwrap();
    assert_eq!(result["id"], json!("7/7"));
}

#[tokio::test]
async fn integ_replies_are_rooted_under_the_comment_that_was_asked_for() {
    let server = MockServer::start().await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/r1/children",
        json!([{ "id": "a1" }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/a1/children",
        json!([{ "id": "g1" }]),
    )
    .await;
    mount_comments(
        &server,
        "/wiki/api/v2/footer-comments/g1/children",
        json!([]),
    )
    .await;

    let client = mock_client(server.uri());
    let result = get_comment_replies("r1", CommentFamily::Footer, false, &client)
        .await
        .unwrap();
    // The origin is not a member of its own reply listing, and no page id
    // is claimed — a walk that began at a comment was never told one.
    assert_eq!(
        thread_shape(&result),
        vec![
            ("a1", 0, Some("r1"), "footer", None),
            ("g1", 1, Some("a1"), "footer", None),
        ]
    );
}

// --- v2 cursor pagination --------------------------------------------

#[tokio::test]
async fn integ_list_follows_relative_next_cursor() {
    let server = MockServer::start().await;
    // Page 1 (no cursor) hands back a relative `_links.next`; the helper must
    // follow it and accumulate, not stop at the first page.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "1", "key": "A" }],
            "_links": { "next": "/wiki/api/v2/spaces?cursor=P2" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .and(query_param("cursor", "P2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "2", "key": "B" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_spaces(&client).await.unwrap();
    let keys: Vec<&str> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec!["A", "B"]);
}

#[tokio::test]
async fn integ_list_follows_absolute_next_cursor_on_the_configured_host() {
    let server = MockServer::start().await;
    // An absolute `_links.next` contributes its path only. Here it names a
    // host the client was never configured with: pagination must continue
    // against the mock server, so the second page is served and the
    // credential is never offered to the host the response named.
    let next = "https://elsewhere.invalid/wiki/api/v2/pages/9/labels?cursor=P2".to_string();
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/9/labels"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "name": "a" }],
            "_links": { "next": next }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/9/labels"))
        .and(query_param("cursor", "P2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "name": "b" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_labels("9", &client).await.unwrap();
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["items"][1]["name"], "b");
}

/// The walk that actually happens: more than one page, ending when the
/// response stops naming a next one. Its Jira and v2 counterparts each
/// have this test; without it a `--all` that fetched everything and then
/// threw it away would look green.
#[tokio::test]
async fn integ_v1_search_all_returns_every_page_it_fetched() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "content": { "id": "1", "title": "a" } }],
            "totalSize": 2,
            "_links": {
                "base": format!("{}/wiki", server.uri()),
                "next": "/rest/api/search?cursor=P2"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .and(query_param("cursor", "P2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "content": { "id": "2", "title": "b" } }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = search_all("type = page", 50, None, None, false, false, &client)
        .await
        .unwrap();
    let ids: Vec<&str> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["1", "2"], "{result}");
    assert_eq!(result["total"], 2, "{result}");
}

/// A page can come back empty while `next` is still live — v1 trims
/// results the caller may not read after paging. Stopping on the count
/// drops every page behind it.
#[tokio::test]
async fn integ_v1_search_all_walks_past_a_page_that_matched_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [],
            "totalSize": 1,
            "_links": {
                "base": format!("{}/wiki", server.uri()),
                "next": "/rest/api/search?cursor=P2"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .and(query_param("cursor", "P2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "content": { "id": "7", "title": "found" } }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = search_all("type = page", 50, None, None, false, false, &client)
        .await
        .unwrap();
    assert_eq!(result["items"][0]["id"], "7", "{result}");
    assert_eq!(result["total"], 1, "{result}");
}

/// Absent or null ends the walk; present-and-not-a-string is neither an
/// end signal nor a link, so the pages fetched so far are not the whole
/// result set.
#[tokio::test]
async fn integ_v1_search_all_bails_on_a_next_that_is_not_a_string() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "content": { "id": "1", "title": "a" } }],
            "totalSize": 9,
            "_links": {
                "base": format!("{}/wiki", server.uri()),
                "next": { "href": "/rest/api/search?cursor=P2" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = search_all("type = page", 50, None, None, false, false, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("was not a string"), "unexpected error: {err}");
}

/// `location` is what makes an id this module hands out a complete address,
/// and `depth`/`parentCommentId` are how a caller rebuilds the thread. They
/// are the walk's answers, not the API's, so the response filter must not
/// be able to take them — as it cannot from `comment get`.
#[tokio::test]
async fn integ_the_walk_stamps_survive_the_response_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/9/footer-comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "r1", "title": "root", "avatarUrls": { "16x16": "u" } }]
        })))
        .mount(&server)
        .await;

    let mut config = crate::test_utils::create_test_config();
    config.optimization.response_exclude_fields = Some(vec![
        "location".to_string(),
        "depth".to_string(),
        "parentCommentId".to_string(),
        "pageId".to_string(),
        "avatarUrls".to_string(),
    ]);
    let client = crate::test_utils::mock_client_with_config(server.uri(), config);

    let result = get_comments("9", &[CommentFamily::Footer], false, false, &client)
        .await
        .unwrap();
    let item = &result["items"][0];
    assert_eq!(item["location"], "footer", "{result}");
    assert_eq!(item["depth"], 0, "{result}");
    assert_eq!(item["parentCommentId"], Value::Null, "{result}");
    assert_eq!(item["pageId"], "9", "{result}");
    // What the filter is actually for still goes.
    assert!(item.get("avatarUrls").is_none(), "{result}");
}

/// The `key` filter is the server's, and what comes back is what a delete
/// removes by id. A result naming a different key is not the property that
/// was asked for, and deleting it would take data nobody named.
#[tokio::test]
async fn integ_a_property_lookup_refuses_a_result_naming_another_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/9/properties"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "p-owner", "key": "owner" }]
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = delete_property("9", "state", &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("names 'owner'"), "{err}");
}

/// `keys=` resolves space aliases, and a lookup by a historical one returns
/// a space naming neither the requested key nor the currently active alias.
/// Any equality check here would refuse it, so this pins that there is
/// none.
#[tokio::test]
async fn integ_a_space_lookup_uses_a_result_reached_through_an_alias() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .and(query_param("keys", "OLDKEY"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "sid", "key": "NEWKEY", "currentActiveAlias": "TEAM" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/wiki/api/v2/pages"))
        .and(body_string_contains("\"spaceId\":\"sid\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": "p1" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    create_page("OLDKEY", "T", "<p>x</p>", None, None, None, &client)
        .await
        .expect("a historical alias names neither field and still resolves");
}

/// A property id is overwritten by `set_property` and removed by
/// `delete_property`, so an unconfirmed match is refused — a key that
/// contradicts and equally one that is absent.
#[tokio::test]
async fn integ_a_property_lookup_refuses_a_result_that_names_no_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/9/properties"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "results": [{ "id": "p1" }] })),
        )
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = delete_property("9", "state", &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("names no key"), "{err}");
}

/// v2's end-of-list signal is an absent or null `next` and nothing else.
/// An empty string is not it, and treating one as the end would report the
/// pages fetched so far as the whole collection — which is what the v1 and
/// Jira walks already refuse.
#[tokio::test]
async fn integ_list_refuses_an_empty_next_instead_of_calling_it_the_end() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/9/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "name": "a" }],
            "_links": { "next": "" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_labels("9", &client).await.unwrap_err().to_string();
    assert!(err.contains("no path"), "unexpected error: {err}");
}

/// The v2 half of the same distinction.
#[tokio::test]
async fn integ_list_bails_on_a_next_that_is_not_a_string() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/9/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "name": "a" }],
            "_links": { "next": ["/wiki/api/v2/pages/9/labels?cursor=P2"] }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_labels("9", &client).await.unwrap_err().to_string();
    assert!(err.contains("was not a string"), "unexpected error: {err}");
}

#[tokio::test]
async fn integ_v1_search_all_bails_when_the_cursor_stops_advancing() {
    let server = MockServer::start().await;
    // v1 walks the same cursor forever unless the trail stops it.
    Mock::given(method("GET"))
        .and(path("/wiki/rest/api/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "content": { "id": "1", "title": "a" } }],
            "totalSize": 99,
            "_links": {
                "base": format!("{}/wiki", server.uri()),
                "next": "/rest/api/search?cursor=stuck"
            }
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = search_all("type = page", 50, None, None, false, false, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("did not advance"), "unexpected error: {err}");
}

#[tokio::test]
async fn integ_list_bails_when_only_the_discarded_prefix_advances() {
    let server = MockServer::start().await;
    // Same resolved path every time, dressed in a different host. The
    // request never advances, so the guard must catch it — which it only
    // can because it keys on the path rather than on the raw link.
    let mut hosts = ["https://a.invalid", "https://b.invalid"].iter().cycle();
    for _ in 0..2 {
        let next = format!(
            "{}/wiki/api/v2/pages/9/labels?cursor=P2",
            hosts.next().unwrap()
        );
        Mock::given(method("GET"))
            .and(path("/wiki/api/v2/pages/9/labels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{ "name": "a" }],
                "_links": { "next": next }
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
    }

    let client = mock_client(server.uri());
    let err = get_labels("9", &client).await.unwrap_err().to_string();
    assert!(err.contains("did not advance"), "unexpected error: {err}");
}

#[tokio::test]
async fn integ_list_bails_on_a_pathless_next_instead_of_truncating() {
    let server = MockServer::start().await;
    // A `next` that carries no path is schema drift. Returning the first
    // page as if it were the whole collection is the failure this helper
    // exists to prevent, so it must surface as an error.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/9/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "name": "a" }],
            "_links": { "next": "https://elsewhere.invalid" }
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_labels("9", &client).await.unwrap_err().to_string();
    assert!(err.contains("no path"), "unexpected error: {err}");
}

#[tokio::test]
async fn integ_get_comments_sends_body_format_then_accumulates() {
    let server = MockServer::start().await;
    // The `body-format` and `limit` queries ride only on the first request;
    // the cursor link carries its own params on the follow-up.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/5/footer-comments"))
        .and(query_param("body-format", "storage"))
        .and(query_param("limit", COMMENT_PAGE_SIZE))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "c1" }],
            "_links": { "next": "/wiki/api/v2/pages/5/footer-comments?cursor=P2" }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/5/footer-comments"))
        .and(query_param("cursor", "P2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "c2" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_comments("5", &[CommentFamily::Footer], false, false, &client)
        .await
        .unwrap();
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn integ_pagination_bails_on_non_advancing_cursor() {
    let server = MockServer::start().await;
    // A cursor that points back at itself must terminate with an error
    // rather than looping forever.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "1" }],
            "_links": { "next": "/wiki/api/v2/spaces?cursor=LOOP" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/spaces"))
        .and(query_param("cursor", "LOOP"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{ "id": "2" }],
            "_links": { "next": "/wiki/api/v2/spaces?cursor=LOOP" }
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_spaces(&client).await.unwrap_err().to_string();
    assert!(err.contains("did not advance"), "got: {err}");
}

#[tokio::test]
async fn integ_pagination_bails_on_missing_results() {
    let server = MockServer::start().await;
    // A 2xx page without a `results` array is anomalous — surface it loudly
    // instead of silently returning an empty list.
    Mock::given(method("GET"))
        .and(path("/wiki/api/v2/pages/7/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_labels("7", &client).await.unwrap_err().to_string();
    assert!(err.contains("no 'results' array"), "got: {err}");
}

#[tokio::test]
async fn search_preserves_explicit_space_and_quoted_sorting_on_the_wire() {
    use crate::test_utils::mock_client_with_config;
    let server = MockServer::start().await;
    let client = mock_client_with_config(server.uri(), create_test_config(vec!["DEFAULT".into()]));
    for (input, expected) in [
        ("\"space\" = OTHER", "\"space\" = OTHER"),
        (
            "space.title ~ \"Engineering\"",
            "space.title ~ \"Engineering\"",
        ),
        (
            "type = page ORDER BY\"lastmodified\"",
            "space IN (\"DEFAULT\") AND (type = page) ORDER BY\"lastmodified\"",
        ),
    ] {
        Mock::given(method("GET"))
            .and(path("/wiki/rest/api/search"))
            .and(query_param("cql", expected))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"results":[{"content":{"id":"42"}}], "totalSize":1})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let result = search(input, 1, None, None, false, &client).await.unwrap();
        assert_eq!(result["items"][0]["id"], "42");
    }
}
