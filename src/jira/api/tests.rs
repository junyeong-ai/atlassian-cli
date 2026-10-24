use super::*;
use crate::jira::{adf, fields};

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
fn test_add_comment_missing_comment() {
    let comment_result = adf::process_comment_input(json!(null));
    assert!(comment_result.is_ok());
    let comment_adf = comment_result.unwrap();
    assert_eq!(comment_adf["type"], "doc");
    assert_eq!(
        comment_adf["content"],
        json!([{ "type": "paragraph", "content": [] }])
    );
}

// -- Integration tests for Phase 1-3 endpoints --
//
// These exercise the production async functions against a `wiremock` server.
// They verify request method, path, query params, and request body, plus the
// returned envelope shape. Each test reflects a contract a real Jira API call
// would hit; synthetic data-only tests for these endpoints have been removed.

use crate::test_utils::mock_client;
use wiremock::matchers::{
    body_json, body_string, body_string_contains, method, path, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A 2xx without an `issues` array is drift. Reading it as a page of no
/// matches is how a search reports an empty result set for a query that
/// had one.
#[tokio::test]
async fn integ_search_bails_when_the_body_lost_its_issues_array() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "total": 3 })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = search("project = X", 50, None, false, &client)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("no 'issues' array"), "{err}");
}

/// The envelope is documented as a list, and `require_field` would let any
/// non-null shape through into it — `{"items": {}}` reads as a list to
/// nothing and as "none" to `jq`.
#[tokio::test]
async fn integ_transitions_envelope_must_be_a_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1/transitions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "transitions": {} })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_transitions("PROJ-1", &client)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("no 'transitions' array"), "{err}");
}

/// The `{"items": [...]}` wrapper is this CLI's contract, not a field of the
/// response, so the caller's filter reaches the items and stops there — and
/// an internal decision reads what the API sent, not what a display setting
/// left of it.
#[tokio::test]
async fn integ_the_list_envelope_is_not_the_filter_s_to_edit() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/agile/1.0/board"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "isLast": true,
            "values": [{ "id": 7, "name": "Board", "avatarUrls": { "16x16": "u" } }]
        })))
        .mount(&server)
        .await;

    let mut config = crate::test_utils::create_test_config();
    // `id` is in the list on purpose: the resolver picks a board by it, so
    // this is what fails if `paginate` ever filters on the way out again.
    config.optimization.response_exclude_fields = Some(vec![
        "items".to_string(),
        "id".to_string(),
        "avatarUrls".to_string(),
    ]);
    let client = crate::test_utils::mock_client_with_config(server.uri(), config);

    let result = get_boards("PROJ", &client).await.unwrap();
    assert_eq!(
        result["items"].as_array().map(Vec::len),
        Some(1),
        "{result}"
    );
    assert!(result["items"][0].get("avatarUrls").is_none(), "{result}");
    // And the resolver decides from the API's answer, not the filtered view.
    assert_eq!(resolve_board_id("PROJ", &client).await.unwrap(), 7);
}

/// The command refuses two links it can read and cannot tell apart, so an
/// entry it cannot read is the same refusal: it may be that second link.
/// A match beside it is not evidence that it is not.
#[tokio::test]
async fn integ_remove_link_will_not_call_a_match_the_only_one_beside_an_unreadable_entry() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                // Names the issue asked about, but not the type asked for:
                // another `Blocks` to PROJ-2 is exactly what it may be.
                { "id": "unknown", "type": { "id": "10000" },
                  "outwardIssue": { "key": "PROJ-2" } },
                // Names no issue at all, so no key rules it out either.
                { "id": "nokey", "type": { "name": "Blocks" } },
                { "id": "known", "type": { "name": "Blocks" },
                  "outwardIssue": { "key": "PROJ-2" } }
            ]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("a second link from PROJ-1 to PROJ-2"), "{err}");
    // Two entries left it open and for different reasons; a fixed
    // parenthetical would describe one of them wrongly.
    assert!(err.contains("(names no type; names no issue)"), "{err}");

    // The same entry where nothing matched: absence is not claimed either.
    let err = remove_link("PROJ-1", "PROJ-9", Some("Blocks"), &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("whether one from PROJ-1 to PROJ-9 is among them"),
        "{err}"
    );
    assert!(err.contains("(names no issue)"), "{err}");
}

/// An issue's own links name one end. An entry naming both is the shape
/// the standalone link resource has, where the sides are absolute and not
/// relative to the issue the list came from — so which end is "the other"
/// is a guess, and here that guess deletes.
#[tokio::test]
async fn integ_remove_link_will_not_read_a_direction_off_an_entry_naming_both_ends() {
    // The two arrangements a reading could still make the link asked for:
    // the outward side is this issue's other end and it is the target, and
    // the sides taken absolutely as this issue and the target. Whichever
    // side is read first, the entry carries the pair.
    for entry in [
        json!({ "id": "77", "type": { "name": "Blocks" },
                "outwardIssue": { "key": "PROJ-1" },
                "inwardIssue": { "key": "PROJ-2" } }),
        json!({ "id": "88", "type": { "name": "Blocks" },
                "outwardIssue": { "key": "PROJ-2" },
                "inwardIssue": { "key": "PROJ-1" } }),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/PROJ-2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "fields": { "issuelinks": [entry.clone()] } })),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .expect(0)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = remove_link("PROJ-2", "PROJ-1", Some("Blocks"), &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("left the question open (carries both ends)"),
            "{entry}: {err}"
        );
    }
}

/// Every shape the classification can be handed, and the one it may act
/// on. Each side is absent, names the issue asked about, names the issue
/// asked from, names a third, is there without a readable key, or is not
/// an issue at all. Each is crossed with a type that is absent, matches,
/// or differs, and with `--type` given and omitted.
///
/// The removal is irreversible and these entries decide it, so what may be
/// deleted is stated once, over the whole space: the outward side names
/// the issue asked about, there is no inward side, and the type was either
/// not asked for or read and equal. Every other shape refuses.
#[tokio::test]
async fn integ_remove_link_deletes_only_the_shape_that_answers_the_request() {
    for (outward, inward, kind, requested, entry) in shape_space() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/PROJ-1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "fields": { "issuelinks": [entry.clone()] } })),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let removed = remove_link("PROJ-1", "PROJ-2", requested, &client)
            .await
            .is_ok();
        let answers = outward == "names the target"
            && inward == "absent"
            && (requested.is_none() || kind == "matches");

        assert_eq!(
            removed, answers,
            "outward {outward}, inward {inward}, type {kind}, --type {requested:?}: {entry}"
        );
    }
}

/// The same space beside a link that plainly answers the request. Removing
/// it is right exactly where the other entry could not be a second one:
/// every side it carries said which issue and none is the one asked about;
/// or its type is there and is not the one asked for; or it carries an
/// inward side and no outward one, which reads as running the other way
/// and so not removable from here whatever else it says.
///
/// Withholding anywhere else refuses a removal that no reading of the
/// unread fields forbids — the failure this pairing exists to catch, and
/// one the single-entry space above cannot see.
#[tokio::test]
async fn integ_remove_link_withholds_only_where_the_other_entry_could_be_a_second_one() {
    for (outward, inward, kind, requested, other) in shape_space() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/PROJ-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "fields": { "issuelinks": [other.clone(),
                    { "id": "MATCH", "type": { "name": "Blocks" },
                      "outwardIssue": { "key": "PROJ-2" } }] }
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/rest/api/3/issueLink/MATCH"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        // Without this the wrong link being deleted 404s and reads as a
        // refusal, which is what half these cells expect anyway.
        Mock::given(method("DELETE"))
            .and(path("/rest/api/3/issueLink/OTHER"))
            .respond_with(ResponseTemplate::new(204))
            .expect(0)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let removed = remove_link("PROJ-1", "PROJ-2", requested, &client)
            .await
            .is_ok();
        // Could this entry be a second link from the source to the
        // target of the type asked for, under any reading of what it left
        // unsaid? Where it could not, the match beside it is the only one
        // and must go.
        let ruled_out = if requested.is_some() && kind == "differs" {
            true
        } else if outward == "names another issue" {
            // Every reading of the entry puts that issue in it, and this
            // request names two others.
            true
        } else if outward == "not an issue at all" || inward == "not an issue at all" {
            // Nothing else about the entry reads, so nothing else about it
            // is ruled out either.
            false
        } else if outward != "absent" && inward != "absent" {
            // Both ends carried, so which side is this issue's other end
            // is not readable. Two readings could still make it the link
            // asked for: the outward side is the other end and it is the
            // target, or the sides are absolute and they are the source
            // and the target. The third — the inward side is the other
            // end — runs the other way and never is.
            let could_be = match outward {
                "names the target" | "no readable key" => true,
                "names the source" => inward == "names the target" || inward == "no readable key",
                _ => false,
            };
            !could_be
        } else if outward == "absent" && inward == "absent" {
            false
        } else if inward != "absent" {
            // Inward only: reads as running the other way.
            true
        } else {
            // Outward only: it is this issue's other end, and it said so.
            outward == "names the source"
        };

        assert_eq!(
            removed, ruled_out,
            "beside outward {outward}, inward {inward}, type {kind}, \
             --type {requested:?}: {other}"
        );
    }
}

/// What one part of an entry says, and the JSON that says it. The label is
/// what a failed assertion names, so a failure reads back as the shape it
/// was handed.
type Part = (&'static str, fn() -> Value);

/// One entry of the space: the outward side, the inward side, the type,
/// the `--type` the request carried, and the entry those three describe.
type Shape = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    Value,
);

/// Every entry the two checks above are run over, labelled by what each
/// part of it says.
fn shape_space() -> Vec<Shape> {
    const SIDES: [Part; 6] = [
        ("absent", || json!(null)),
        ("names the target", || json!({ "key": "PROJ-2" })),
        ("names the source", || json!({ "key": "PROJ-1" })),
        ("names another issue", || json!({ "key": "PROJ-9" })),
        ("no readable key", || json!({ "key": 42 })),
        ("not an issue at all", || json!("PROJ-2")),
    ];
    const TYPES: [Part; 3] = [
        ("absent", || json!(null)),
        ("matches", || json!({ "name": "Blocks" })),
        ("differs", || json!({ "name": "Relates" })),
    ];

    let mut space = Vec::new();
    for (outward, outward_side) in SIDES {
        for (inward, inward_side) in SIDES {
            for (kind, link_type) in TYPES {
                for requested in [None, Some("Blocks")] {
                    let mut entry = json!({ "id": "OTHER" });
                    for (field, value) in [
                        ("outwardIssue", outward_side()),
                        ("inwardIssue", inward_side()),
                        ("type", link_type()),
                    ] {
                        if !value.is_null() {
                            entry[field] = value;
                        }
                    }
                    space.push((outward, inward, kind, requested, entry));
                }
            }
        }
    }
    space
}

/// A side that is there and did not say which issue is still a side that
/// is there. Reading only the key takes it for a side that is absent, and
/// the entry then looks like the clean one-ended shape it is not.
#[tokio::test]
async fn integ_remove_link_reads_a_side_that_is_there_but_says_nothing() {
    for second_side in [json!({ "key": 42 }), json!({}), json!({ "key": null })] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/PROJ-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "fields": { "issuelinks": [
                    { "id": "7", "type": { "name": "Blocks" },
                      "outwardIssue": { "key": "PROJ-2" },
                      "inwardIssue": second_side }
                ]}
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .expect(0)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let err = remove_link("PROJ-1", "PROJ-2", None, &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("left the question open (carries both ends)"),
            "{second_side}: {err}"
        );
    }
}

/// The same on the other side: an outward side that says nothing cannot be
/// ruled out by the key it did not give, so it is not a definite absence
/// and it is not a link running the other way either.
#[tokio::test]
async fn integ_remove_link_does_not_rule_out_a_side_by_a_key_it_never_gave() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                { "id": "9", "type": { "name": "Blocks" },
                  "outwardIssue": { "key": 42 } }
            ]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = remove_link("PROJ-1", "PROJ-2", None, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("left the question open (names no issue)"),
        "{err}"
    );
}

/// Direction is an exclusion too. An inward entry is not removable from
/// this issue whatever its unread type turns out to be, so it is not the
/// second link that would make the removal a guess, and withholding on it
/// would refuse where every answer says proceed.
#[tokio::test]
async fn integ_remove_link_does_not_withhold_for_an_entry_direction_rules_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                { "id": "77", "type": { "name": "Blocks" },
                  "outwardIssue": { "key": "PROJ-2" } },
                // Runs the other way, and names no type — but the direction
                // it does name is enough to rule it out.
                { "id": "88", "type": { "id": "10003" },
                  "inwardIssue": { "key": "PROJ-2" } }
            ]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/rest/api/3/issueLink/77"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
        .await
        .expect("the link `add` created, beside one that runs the other way");
}

/// An entry an exclusion already ruled out is not a candidate, so it does
/// not withhold the removal — one link the caller cannot resolve must not
/// refuse every other link on the issue.
#[tokio::test]
async fn integ_remove_link_ignores_an_unreadable_entry_the_type_rules_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                // No issue named, but the type is there and is not the one
                // asked for — it cannot be a second `Blocks` to PROJ-2.
                { "id": "elsewhere", "type": { "name": "Relates" } },
                { "id": "known", "type": { "name": "Blocks" },
                  "outwardIssue": { "key": "PROJ-2" } }
            ]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/rest/api/3/issueLink/known"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
        .await
        .expect("an entry ruled out by type is not a candidate");
}

/// "A blocks B" and "B blocks A" are two links, both listed on A with the
/// same type and the same other issue. `add` takes the source as the
/// outward side, so `remove` takes that one — and the reverse is not a
/// candidate at all, filtered before anything counts matches.
#[tokio::test]
async fn integ_remove_link_takes_the_outward_link_and_not_the_reverse_one() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                { "id": "inward", "type": { "name": "Blocks" },
                  "inwardIssue": { "key": "PROJ-2" } },
                { "id": "outward", "type": { "name": "Blocks" },
                  "outwardIssue": { "key": "PROJ-2" } }
            ]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/rest/api/3/issueLink/outward"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
        .await
        .expect("the outward link is the one `add` would have made");
}

/// The pair and the type are everything this command takes, so two links
/// that agree on both and on direction leave it nothing to choose by.
/// Removing either would be a guess about which one the caller meant.
#[tokio::test]
async fn integ_remove_link_refuses_two_links_it_cannot_tell_apart() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                { "id": "a", "type": { "name": "Blocks" },
                  "outwardIssue": { "key": "PROJ-2" } },
                { "id": "b", "type": { "name": "Blocks" },
                  "outwardIssue": { "key": "PROJ-2" } }
            ]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot choose"), "{err}");
}

/// The schema does not require `type.name`, so an inward entry can name
/// its direction and not its type. Direction settles it either way — it is
/// not removable from this issue under any type — and the answer says so
/// without calling it a link of the type asked for.
#[tokio::test]
async fn integ_remove_link_does_not_claim_a_direction_for_a_type_it_never_read() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                { "id": "x", "type": { "id": "10000" },
                  "inwardIssue": { "key": "PROJ-2" } }
            ]}
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
        .await
        .unwrap_err()
        .to_string();
    // Its direction is named, because the entry named it. Its type is not,
    // because the entry did not.
    assert!(
        err.contains("at least one link between them runs the other way"),
        "{err}"
    );
    assert!(!err.contains("Blocks running"), "{err}");
}

/// A lone link running the other way is a different link. Removing it for a
/// request that named this issue as the source would take one nobody asked
/// about, so the answer names the issue it starts at instead.
#[tokio::test]
async fn integ_remove_link_leaves_a_lone_reverse_link_alone() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                { "id": "inward", "type": { "name": "Blocks" },
                  "inwardIssue": { "key": "PROJ-2" } }
            ]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/rest/api/3/issueLink/inward"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("at least one link between them runs the other way"),
        "{err}"
    );
    assert!(err.contains("run this on PROJ-2"), "{err}");
}

/// A reverse link the requested type rules out is not counted, so any
/// total the answer stated would be one the exclusion could falsify. The
/// answer says the same thing with that link present and absent.
#[tokio::test]
async fn integ_remove_link_says_the_same_whether_or_not_a_type_rules_a_reverse_link_out() {
    async fn answer(links: Value) -> String {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/PROJ-1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "fields": { "issuelinks": links } })),
            )
            .mount(&server)
            .await;
        let client = mock_client(server.uri());
        remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
            .await
            .unwrap_err()
            .to_string()
    }

    let blocks = json!({ "id": "b", "type": { "name": "Blocks" },
                         "inwardIssue": { "key": "PROJ-2" } });
    let relates = json!({ "id": "r", "type": { "name": "Relates" },
                          "inwardIssue": { "key": "PROJ-2" } });

    let alone = answer(json!([blocks.clone()])).await;
    // Two links between the issues run the other way and one was ruled
    // out, so any total here would be a number the exclusion falsifies.
    // Existence is what survives it.
    assert!(
        alone.contains("at least one link between them runs the other way"),
        "{alone}"
    );
    assert_eq!(alone, answer(json!([blocks, relates])).await);
}

/// An entry a present field already excludes is decided, not undecided: it
/// carries no issue key, but its type is not the one asked for, so it could
/// never have been the link — and "no such link" stays a provable answer.
#[tokio::test]
async fn integ_remove_link_still_answers_where_a_present_field_settles_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                { "id": "1", "type": { "name": "Relates" } },
                { "id": "2", "type": { "name": "Blocks" }, "outwardIssue": { "key": "PROJ-3" } }
            ]}
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("No link runs from"), "{err}");
}

/// Once the description is a string, an exclude naming a key inside it has
/// nothing left to match, so the filter has to run while the ADF is still
/// an object.
#[tokio::test]
async fn integ_an_issue_is_filtered_before_its_description_becomes_a_string() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "key": "ABC-1",
            "fields": {
                "description": {
                    "type": "doc",
                    "content": [{
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": "sensitive" }]
                    }]
                }
            }
        })))
        .mount(&server)
        .await;

    let mut config = crate::test_utils::create_test_config();
    config.optimization.response_exclude_fields = Some(vec!["text".to_string()]);
    let client = crate::test_utils::mock_client_with_config(server.uri(), config);

    let result = get_issue("ABC-1", None, true, &client).await.unwrap();
    let description = result["fields"]["description"].as_str().unwrap_or_default();
    assert!(
        !description.contains("sensitive"),
        "the excluded field reached the converted description: {result}"
    );
}

/// `adf_to_markdown` turns the body object into a string, and a key the
/// caller excluded is not there to exclude once it is one. Filtering comes
/// first for the same reason the single-issue read does it first.
#[tokio::test]
async fn integ_comments_are_filtered_before_the_body_becomes_a_string() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "startAt": 0,
            "total": 1,
            "comments": [{
                "id": "1",
                "body": {
                    "type": "doc",
                    "content": [{
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": "sensitive" }]
                    }]
                }
            }]
        })))
        .mount(&server)
        .await;

    // `text` lives inside the ADF body. Filtered first it is gone before
    // conversion; converted first it is already part of the string and no
    // exclude can reach it.
    let mut config = crate::test_utils::create_test_config();
    config.optimization.response_exclude_fields = Some(vec!["text".to_string()]);
    let client = crate::test_utils::mock_client_with_config(server.uri(), config);

    let result = get_comments("ABC-1", true, &client).await.unwrap();
    let body = result["items"][0]["body"].as_str().unwrap_or_default();
    assert!(
        !body.contains("sensitive"),
        "the excluded field reached the converted body: {result}"
    );
}

/// A plain-text description becomes ADF on the way out, and the envelope a
/// caller chains on is required rather than passed through — a 2xx that
/// lost `key` would otherwise hand back `null` and fail somewhere later.
#[tokio::test]
async fn integ_create_issue_converts_the_description_and_requires_its_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue"))
        .and(body_string_contains("\"type\":\"doc\""))
        // The text itself, not just the wrapper: an empty document would
        // satisfy a shape-only match while dropping what was written.
        .and(body_string_contains("\"text\":\"plain text\""))
        .and(body_string_contains("\"key\":\"PROJ\""))
        .and(body_string_contains("\"name\":\"Task\""))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(json!({ "id": "10001", "key": "PROJ-1", "self": "…" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let created = create_issue(
        "PROJ",
        "Summary",
        "Task",
        json!("plain text"),
        None,
        None,
        &client,
    )
    .await
    .unwrap();
    assert_eq!(created, json!({ "key": "PROJ-1", "id": "10001" }));
}

/// A sub-task's parent is what makes it a sub-task, and Jira takes it on
/// the create screen rather than afterwards — there is no issue to update
/// into place, because without the field the create is refused. The same
/// field puts an issue under an epic in a team-managed project.
#[tokio::test]
async fn integ_create_issue_sends_the_parent_it_was_given() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue"))
        .and(body_string_contains("\"parent\":{\"key\":\"PROJ-1\"}"))
        .and(body_string_contains("\"name\":\"Sub-task\""))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "id": "10002", "key": "PROJ-2" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let created = create_issue(
        "PROJ",
        "Summary",
        "Sub-task",
        json!("plain text"),
        Some("PROJ-1"),
        None,
        &client,
    )
    .await
    .unwrap();
    assert_eq!(created, json!({ "key": "PROJ-2", "id": "10002" }));
}

/// Without one the field is absent, not null: a type that takes no parent
/// has no field for it on its create screen, and Jira rejects a value for
/// a field that is not there.
#[tokio::test]
async fn integ_create_issue_omits_the_parent_field_when_it_has_none() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue"))
        .and(|request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).expect("json body");
            body["fields"].get("parent").is_none()
        })
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "id": "10003", "key": "PROJ-3" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    create_issue("PROJ", "Summary", "Task", json!("x"), None, None, &client)
        .await
        .expect("a create with no parent");
}

/// Jira takes only the fields on the type's create screen, so a field
/// nobody asked for is a create refused. An omitted `--description` is not
/// an empty description — the key is absent, as the parent's is.
#[tokio::test]
async fn integ_create_issue_omits_every_field_it_was_not_given() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue"))
        .and(|request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).expect("json body");
            let fields = &body["fields"];
            fields.get("description").is_none() && fields.get("parent").is_none()
        })
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "id": "10004", "key": "PROJ-4" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    create_issue("PROJ", "Summary", "Task", Value::Null, None, None, &client)
        .await
        .expect("a create with neither a description nor a parent");
}

/// A project may require a field this command has no argument for, and
/// then nothing at all can be created through it — the shape the parent
/// had. `--fields` carries those, and a `description` inside it becomes
/// ADF exactly as `update` makes it.
#[tokio::test]
async fn integ_create_issue_carries_fields_it_has_no_argument_for() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue"))
        .and(|request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).expect("json body");
            let fields = &body["fields"];
            fields["customfield_10010"] == json!("X")
                && fields["components"] == json!([{ "name": "api" }])
                && fields["description"]["type"] == json!("doc")
                && fields["description"]["content"][0]["content"][0]["text"]
                    == json!("written through --fields")
                && fields["summary"] == json!("Summary")
        })
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "id": "10005", "key": "PROJ-5" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    create_issue(
        "PROJ",
        "Summary",
        "Task",
        Value::Null,
        None,
        Some(json!({
            "customfield_10010": "X",
            "components": [{ "name": "api" }],
            "description": "written through --fields"
        })),
        &client,
    )
    .await
    .expect("a create carrying fields this command has no argument for");
}

/// Named in both places, neither is dropped in favour of the other: which
/// one was meant is not something this code can tell.
#[tokio::test]
async fn integ_create_issue_refuses_a_field_given_twice() {
    let server = MockServer::start().await;
    // A body a create would accept: without the refusal the request goes
    // out and reads as success, so `unwrap_err` fails on the removal
    // itself rather than on a response it could not parse.
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({ "id": "10006", "key": "PROJ-6" })),
        )
        .expect(0)
        .mount(&server)
        .await;
    let client = mock_client(server.uri());

    // Every field this command writes itself, so the refusal is the rule
    // and not a list of the keys someone thought of.
    for (extra, named) in [
        (json!({ "summary": "another" }), "summary"),
        (json!({ "description": "another" }), "description"),
        (json!({ "parent": { "key": "PROJ-9" } }), "parent"),
        (json!({ "project": { "key": "OTHER" } }), "project"),
        (json!({ "issuetype": { "name": "Bug" } }), "issuetype"),
    ] {
        let err = create_issue(
            "PROJ",
            "Summary",
            "Task",
            json!("a description"),
            Some("PROJ-1"),
            Some(extra.clone()),
            &client,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains(&format!("`{named}`")), "{extra}: {err}");
        assert!(err.contains("one place or the other"), "{extra}: {err}");
    }

    // And a value that is not an object at all names what it takes.
    let err = create_issue(
        "PROJ",
        "Summary",
        "Task",
        Value::Null,
        None,
        Some(json!(["labels"])),
        &client,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("JSON object of Jira field names"), "{err}");
}

#[tokio::test]
async fn integ_create_issue_bails_when_the_envelope_lost_its_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "id": "10001" })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = create_issue("PROJ", "S", "Task", json!("x"), None, None, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("no 'key'"), "{err}");
}

/// The happy path of the single-page read: what it asks for, and the
/// `{items, count}` envelope it promises.
#[tokio::test]
async fn integ_search_asks_for_a_page_and_answers_with_the_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .and(body_string_contains("\"maxResults\":25"))
        .and(body_string_contains("project = X"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "issues": [{ "key": "X-1" }, { "key": "X-2" }] })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = search("project = X", 25, None, false, &client)
        .await
        .unwrap();
    assert_eq!(result["count"], 2, "{result}");
    assert_eq!(result["items"][1]["key"], "X-2", "{result}");
}

/// `--fields` is the caller's, so it goes through the query builder. Written
/// into the URL, a value carrying `&` would start a parameter of its own and
/// change the request — here it must stay one encoded `fields` value.
#[tokio::test]
async fn integ_get_issue_sends_a_field_selector_the_caller_cannot_break_out_of() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .and(query_param("fields", "summary&expand=changelog"))
        .and(query_param_is_missing("expand"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "key": "PROJ-1" })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_issue(
        "PROJ-1",
        Some(vec!["summary&expand=changelog".to_string()]),
        false,
        &client,
    )
    .await
    .unwrap();
    assert_eq!(result["key"], "PROJ-1");
}

/// `response_exclude_fields` trims what a read prints. `remove_link` picks
/// its link by `type.name` and the other issue's `key`, so reading those
/// off a trimmed body would have the match fail and the command report no
/// such link on an issue that has one — an absence the setting invented.
#[tokio::test]
async fn integ_remove_link_matches_on_what_the_api_sent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [{
                "id": "77",
                "type": { "name": "Blocks" },
                "outwardIssue": { "key": "PROJ-2" }
            }]}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/rest/api/3/issueLink/77"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let mut config = crate::test_utils::create_test_config();
    config.optimization.response_exclude_fields = Some(vec!["type".to_string()]);
    let client = crate::test_utils::mock_client_with_config(server.uri(), config);

    remove_link("PROJ-1", "PROJ-2", Some("Blocks"), &client)
        .await
        .expect("the link the API reported was there");
}

/// `response_exclude_fields` is the caller's, so a name in it that matches
/// a control field must not reach the body this loop reads its own end
/// signal from — filtered first, an excluded `nextPageToken` says the
/// endpoint had finished and the walk returns page one as the whole result.
#[tokio::test]
async fn integ_search_all_does_not_read_its_end_signal_from_a_filtered_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .and(body_string_contains("nextPageToken"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "issues": [{ "key": "X-2" }] })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "issues": [{ "key": "X-1" }], "nextPageToken": "P2" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let mut config = crate::test_utils::create_test_config();
    config.optimization.response_exclude_fields =
        Some(vec!["nextPageToken".to_string(), "avatarUrls".to_string()]);
    let client = crate::test_utils::mock_client_with_config(server.uri(), config);

    let result = search_all("project = X", None, false, false, &client)
        .await
        .unwrap();

    assert_eq!(
        result["total"], 2,
        "the walk stopped at the filtered field: {result}"
    );
}

/// Absent ends the walk; present-and-not-a-string is neither an end signal
/// nor a token, so returning the pages fetched so far would report a
/// truncated search as the whole result set.
#[tokio::test]
async fn integ_search_all_bails_on_a_page_token_that_is_not_a_string() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "issues": [{ "key": "X-1" }], "nextPageToken": 42 })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = search_all("project = X", None, false, false, &client)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("'nextPageToken' was not a string"), "{err}");
}

/// An empty token is neither. Sent back it would fetch a page again or fail
/// a request under a token that reads like this tool's own bug, so it is
/// refused where it arrives — the posture the two Confluence walks take
/// toward an empty `_links.next`.
#[tokio::test]
async fn integ_search_all_bails_on_an_empty_page_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "issues": [{ "key": "X-1" }], "nextPageToken": "" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = search_all("project = X", None, false, false, &client)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("empty 'nextPageToken'"), "{err}");
}

/// The discovery endpoints answer with their list and no envelope around
/// it, so the body itself is what the `{"items": [...]}` contract names.
#[tokio::test]
async fn integ_discovery_envelope_must_be_a_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/priority"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "values": [{ "id": "1" }], "isLast": true })),
        )
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_priorities(&client).await.unwrap_err().to_string();

    assert!(err.contains("response was not an array"), "{err}");
}

/// Worklogs are a `total`-contract collection like comments. Reading one
/// page of a long log reports part of the time booked as all of it.
#[tokio::test]
async fn integ_get_worklogs_reads_past_the_first_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/worklog"))
        .and(query_param("startAt", "0"))
        .and(query_param("maxResults", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "startAt": 0,
            "total": 3,
            "worklogs": [{ "id": "1" }, { "id": "2" }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/worklog"))
        .and(query_param("startAt", "2"))
        .and(query_param("maxResults", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "startAt": 2,
            "total": 3,
            "worklogs": [{ "id": "3" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_worklogs("ABC-1", &client).await.unwrap();
    let ids: Vec<&str> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["1", "2", "3"]);
}

/// The set an issue's watchers arrive in is not paginated, but the envelope
/// is still a list — a 2xx that lost the array must not read as nobody.
#[tokio::test]
async fn integ_watchers_envelope_must_be_a_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/watchers"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "watchCount": 2 })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_watchers("ABC-1", &client)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("no 'watchers' array"), "{err}");
}

/// The walk that actually happens: more than one page, ending when the
/// endpoint stops handing out a token. Every other bound in this crate has
/// this test; without it, a `--all` that fetched everything and then threw
/// it away looked green.
#[tokio::test]
async fn integ_search_all_returns_every_page_it_fetched() {
    for stream in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/search/jql"))
            .and(body_string_contains("nextPageToken"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "issues": [{ "key": "X-2" }] })),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/search/jql"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "issues": [{ "key": "X-1" }], "nextPageToken": "P2" })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = mock_client(server.uri());
        let result = search_all("project = X", None, stream, false, &client)
            .await
            .unwrap();

        if stream {
            assert!(result.is_null());
            continue;
        }
        assert_eq!(result["total"], 2, "{result}");
        let keys: Vec<&str> = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, vec!["X-1", "X-2"], "{result}");
    }
}

/// An empty page with a live token is not the end of the results; a token
/// the walk has already used is drift.
#[tokio::test]
async fn integ_search_all_walks_past_an_empty_page_and_refuses_a_repeated_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .and(body_string_contains("nextPageToken"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "issues": [{ "key": "X-1" }], "nextPageToken": "P2" })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "issues": [], "nextPageToken": "P2" })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = search_all("project = X", None, false, false, &client)
        .await
        .unwrap_err()
        .to_string();

    // Reaching the second page at all proves the empty one did not end it.
    assert!(err.contains("already used"), "{err}");
}

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
    // caps it.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/PROJ-1"))
        .and(query_param("fields", "*all"))
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

/// The id form is the way out of a refusal the walk could not settle, so
/// it may not take that walk: an issue the links cannot be read off is
/// exactly where this is reached for.
#[tokio::test]
async fn integ_remove_link_by_id_deletes_without_reading_the_issue() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/rest/api/3/issueLink/10001"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let removed = remove_link_by_id("10001", &client).await;

    assert!(removed.is_ok(), "{:?}", removed.err());
    assert_eq!(removed.unwrap(), json!({}));
}

/// The id reaches a path segment, and one carrying a separator would
/// address a different endpoint entirely.
#[tokio::test]
async fn integ_remove_link_by_id_encodes_the_id_into_one_segment() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/rest/api/3/issueLink/..%2F..%2Fissue%2FA-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let removed = remove_link_by_id("../../issue/A-1", &client).await;

    assert!(removed.is_ok(), "{:?}", removed.err());
}

/// A refusal that cannot choose names what there is to choose between.
/// Both forms of it carry the list, because each writes its own message
/// and only the one that ran would say so.
#[tokio::test]
async fn integ_remove_link_names_the_ids_it_offers_a_choice_between() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/A-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": {
                "issuelinks": [
                    { "id": "10001", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } },
                    { "id": "10002", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } }
                ]
            }
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    for link_type in [None, Some("Blocks")] {
        let err = remove_link("A-1", "B-1", link_type, &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("--id"), "{err}");
        assert!(err.contains("10001, 10002"), "{err}");
    }
}

/// An entry that went unread is a link whose id this walk never saw, so
/// the ones it did see are not the whole of what to choose between —
/// naming them would offer a choice that leaves one out.
#[tokio::test]
async fn integ_remove_link_names_no_ids_where_an_entry_went_unread() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/A-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": {
                "issuelinks": [
                    { "id": "10001", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } },
                    { "id": "10002", "type": { "name": "Blocks" }, "outwardIssue": "not an issue" }
                ]
            }
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = remove_link("A-1", "B-1", None, &client)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("--id"), "{err}");
    assert!(err.contains("jira link list A-1"), "{err}");
    assert!(!err.contains("10001"), "{err}");
}

/// Two matches settle the refusal without the unread entry being read, but
/// they do not settle what there is to choose between: that entry may be a
/// third link from here to the same issue, and naming the two ids offers a
/// choice that leaves it out.
#[tokio::test]
async fn integ_remove_link_names_no_ids_where_an_entry_went_unread_beside_two_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/A-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": {
                "issuelinks": [
                    { "id": "1", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } },
                    { "id": "2", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } },
                    { "id": "3", "type": { "name": "Blocks" }, "outwardIssue": "not an issue" }
                ]
            }
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    for link_type in [None, Some("Blocks")] {
        let err = remove_link("A-1", "B-1", link_type, &client)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("at least 2 links"), "{err}");
        assert!(err.contains("jira link list A-1"), "{err}");
        assert!(!err.contains("(1, 2)"), "{err}");
    }
}

/// The same rule one step in: the choice is among the matches alone, and
/// one of them carrying no readable id leaves the list short by that one.
#[tokio::test]
async fn integ_remove_link_names_no_ids_where_a_match_carries_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/A-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": {
                "issuelinks": [
                    { "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } },
                    { "id": "10002", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } }
                ]
            }
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = remove_link("A-1", "B-1", None, &client)
        .await
        .unwrap_err()
        .to_string();

    assert!(err.contains("jira link list A-1"), "{err}");
    assert!(!err.contains("10002"), "{err}");
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

/// The advice is a promise: name a type and this command removes one. It
/// is checked by keeping it — every type in the response is tried, and the
/// advice is given exactly where one of them gets there.
///
/// Naming a type does not merely narrow the matches. It also takes out
/// every entry the walk could not read that carries a different name, and
/// leaves the ones carrying none or the same. Both directions are here.
#[tokio::test]
async fn integ_remove_link_offers_a_type_only_where_naming_one_removes_a_link() {
    let outward = json!({ "key": "B-1" });
    for links in [
        // Two of the same name: neither name leaves one.
        json!([
            { "id": "1", "type": { "name": "Blocks" }, "outwardIssue": outward },
            { "id": "2", "type": { "name": "Blocks" }, "outwardIssue": outward }
        ]),
        // Two names, each on two: neither leaves one either.
        json!([
            { "id": "1", "type": { "name": "Blocks" }, "outwardIssue": outward },
            { "id": "2", "type": { "name": "Blocks" }, "outwardIssue": outward },
            { "id": "3", "type": { "name": "Relates" }, "outwardIssue": outward },
            { "id": "4", "type": { "name": "Relates" }, "outwardIssue": outward }
        ]),
        // A match carrying no name: no `--type` selects it.
        json!([
            { "id": "1", "type": { "name": "Blocks" }, "outwardIssue": outward },
            { "id": "2", "type": { "id": "10000" }, "outwardIssue": outward }
        ]),
        // An unread entry carrying no name: no `--type` reads it either.
        json!([
            { "id": "1", "type": { "name": "Blocks" }, "outwardIssue": outward },
            { "id": "2", "type": { "name": "Relates" }, "outwardIssue": outward },
            { "id": "3", "type": { "id": "10000" } }
        ]),
        // Two names, and one leaves the other's match alone.
        json!([
            { "id": "1", "type": { "name": "Blocks" }, "outwardIssue": outward },
            { "id": "2", "type": { "name": "Relates" }, "outwardIssue": outward }
        ]),
        // An unread entry a name reaches past: `Relates` excludes it by
        // type and leaves the one match it does not.
        json!([
            { "id": "1", "type": { "name": "Blocks" }, "outwardIssue": outward },
            { "id": "2", "type": { "name": "Relates" }, "outwardIssue": outward },
            { "id": "3", "type": { "name": "Blocks" } }
        ]),
        // One unread entry under each name: whichever is named, the entry
        // carrying it stays unread beside the one match left, so neither
        // name gets there.
        json!([
            { "id": "1", "type": { "name": "Blocks" }, "outwardIssue": outward },
            { "id": "2", "type": { "name": "Relates" }, "outwardIssue": outward },
            { "id": "3", "type": { "name": "Blocks" } },
            { "id": "4", "type": { "name": "Relates" } }
        ]),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/issue/A-1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "fields": { "issuelinks": links.clone() } })),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let client = mock_client(server.uri());

        let advised = remove_link("A-1", "B-1", None, &client)
            .await
            .unwrap_err()
            .to_string()
            .contains("Specify --type");

        let mut removed_by = Vec::new();
        for name in ["Blocks", "Relates"] {
            if remove_link("A-1", "B-1", Some(name), &client).await.is_ok() {
                removed_by.push(name);
            }
        }

        assert_eq!(
            advised,
            !removed_by.is_empty(),
            "advice {advised} but {removed_by:?} removed one: {links}"
        );
    }
}

/// An entry the walk could not read leaves open whether there is a second
/// link — but not where two it could read already match. That question is
/// answered, and the refusal is the one that says so.
#[tokio::test]
async fn integ_remove_link_does_not_reopen_an_ambiguity_it_has_already_settled() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/A-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "fields": { "issuelinks": [
                { "id": "a", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } },
                { "id": "b", "type": { "name": "Blocks" }, "outwardIssue": { "key": "B-1" } },
                { "id": "x", "type": { "name": "Blocks" } }
            ]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = remove_link("A-1", "B-1", Some("Blocks"), &client)
        .await
        .unwrap_err()
        .to_string();
    // Two were read and one was not, so two is a floor and not a total.
    assert!(err.contains("Found at least 2 links"), "{err}");
    assert!(!err.contains("cannot be told"), "{err}");
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
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "accountId": "abc-123" })))
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
async fn integ_post_issue_batches_split_at_the_bulk_limit() {
    for (count, batches) in [(50, vec![50]), (51, vec![50, 1])] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/agile/1.0/sprint/55/issue"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let issues: Vec<String> = (0..count).map(|i| format!("MDW-{i}")).collect();
        move_issues_to_sprint(55, &issues, &mock_client(server.uri()))
            .await
            .unwrap();

        let sizes: Vec<usize> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                body["issues"].as_array().unwrap().len()
            })
            .collect();
        assert_eq!(sizes, batches, "{count} issues");
    }
}

/// The bulk endpoints are not transactional, so a failure after a batch
/// landed says how many issues already moved; a first-batch failure has
/// nothing to report beyond the error itself.
#[tokio::test]
async fn integ_post_issue_batches_report_progress_only_after_a_batch_landed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/agile/1.0/sprint/55/issue"))
        .respond_with(ResponseTemplate::new(204))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/rest/agile/1.0/sprint/55/issue"))
        .respond_with(ResponseTemplate::new(400).set_body_string("sprint closed"))
        .mount(&server)
        .await;
    let client = mock_client(server.uri());

    let issues: Vec<String> = (0..120).map(|i| format!("MDW-{i}")).collect();
    let err = move_issues_to_sprint(55, &issues, &client)
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("50/120"), "{err:#}");

    let err = move_issues_to_sprint(55, &["A-1".to_string()], &client)
        .await
        .unwrap_err();
    assert!(!format!("{err:#}").contains("already processed"), "{err:#}");
    assert!(format!("{err:#}").contains("sprint closed"), "{err:#}");
}

/// Epic membership is the issue's `parent`, edited one issue at a time,
/// which is the one form Jira honours in team-managed and company-managed
/// projects alike. A cleared parent is read back, and one Jira kept despite
/// answering 204 is a failure that names how far the edit got.
#[tokio::test]
async fn integ_epic_membership_edits_each_issue_parent() {
    let server = MockServer::start().await;
    for issue in ["A-1", "A-2"] {
        Mock::given(method("PUT"))
            .and(path(format!("/rest/api/3/issue/{issue}")))
            .and(body_json(
                json!({ "fields": { "parent": { "key": "E-1" } } }),
            ))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
    }
    for (issue, read_back) in [
        ("A-3", json!({ "id": "3", "key": "A-3" })),
        (
            "A-4",
            json!({ "id": "4", "key": "A-4", "fields": { "parent": { "key": "E-1" } } }),
        ),
    ] {
        Mock::given(method("PUT"))
            .and(path(format!("/rest/api/3/issue/{issue}")))
            .and(body_json(json!({ "fields": { "parent": null } })))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/rest/api/3/issue/{issue}")))
            .and(query_param("fields", "parent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(read_back))
            .expect(1)
            .mount(&server)
            .await;
    }
    let client = mock_client(server.uri());

    let assigned = ["A-1".to_string(), "A-2".to_string()];
    assert_eq!(
        assign_issues_to_epic("E-1", &assigned, &client)
            .await
            .unwrap(),
        json!({})
    );

    let unassigned = ["A-3".to_string(), "A-4".to_string()];
    let err = unassign_issues_from_epic(&unassigned, &client)
        .await
        .unwrap_err();
    let message = format!("{err:#}");
    assert!(message.contains("at A-4 after 1/2"), "{message}");
    assert!(message.contains("still under E-1"), "{message}");
}

/// A clear Jira accepted but the read-back could not show fails under the
/// read-back's own operation and status, not as a refused edit, and the issue
/// is named even when it is the first.
#[tokio::test]
async fn integ_epic_unassign_reports_an_unconfirmed_clear_apart_from_a_refused_edit() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/rest/api/3/issue/A-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/A-1"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let client = mock_client(server.uri());

    let err = unassign_issues_from_epic(&["A-1".to_string()], &client)
        .await
        .unwrap_err();
    let api = err.downcast_ref::<crate::client::ApiError>().unwrap();
    assert_eq!(api.operation, "read back the issue parent");
    assert_eq!(api.status, 401);
    let message = format!("{err:#}");
    assert!(message.contains("at A-1 after 0/1"), "{message}");
    assert!(
        message.contains("accepted clearing the parent of A-1"),
        "{message}"
    );
}

#[tokio::test]
async fn integ_move_to_sprint_rejects_empty_issues() {
    let server = MockServer::start().await;
    let client = mock_client(server.uri());
    let err = move_issues_to_sprint(55, &[], &client).await.unwrap_err();
    assert!(err.to_string().contains("no issues provided"));
}

#[tokio::test]
async fn integ_path_segment_encoding_handles_unsafe_chars() {
    let server = MockServer::start().await;
    // %20 is the encoding for space — verify the issue_key is percent-encoded.
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/MDW%20207/comment"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "comments": [], "total": 0 })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_comments("MDW 207", false, &client).await;
    assert!(result.is_ok(), "{:?}", result.err());
}

#[tokio::test]
async fn integ_get_comments_reads_past_the_first_page() {
    let server = MockServer::start().await;
    // The endpoint reports how many comments exist and hands them over a
    // page at a time; reading only the first page drops the rest of a busy
    // issue's discussion without saying so.
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .and(query_param("startAt", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "startAt": 0,
            "total": 3,
            "comments": [{ "id": "1" }, { "id": "2" }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .and(query_param("startAt", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "startAt": 2,
            "total": 3,
            "comments": [{ "id": "3" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_comments("ABC-1", false, &client).await.unwrap();
    let ids: Vec<&str> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["1", "2", "3"]);
}

#[tokio::test]
async fn integ_get_comments_bails_when_the_server_stops_short_of_its_own_total() {
    let server = MockServer::start().await;
    // `total` is the whole contract here, so a page that adds nothing while
    // items remain is the server declining to hand them over. Returning the
    // short list would report a truncated discussion as the entire one.
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .and(query_param("startAt", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total": 9,
            "comments": [{ "id": "1" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .and(query_param("startAt", "1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "total": 9, "comments": [] })),
        )
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_comments("ABC-1", false, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("stopped returning them at 1"), "got: {err}");
}

#[tokio::test]
async fn integ_get_comments_accepts_a_total_that_shrank_mid_walk() {
    let server = MockServer::start().await;
    // Comments deleted while the walk is in flight lower the count the
    // server reports. The walk has everything that still exists, so this is
    // the end of the collection — not the server withholding items.
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .and(query_param("startAt", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total": 5,
            "comments": [{ "id": "1" }, { "id": "2" }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .and(query_param("startAt", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "total": 2, "comments": [] })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let result = get_comments("ABC-1", false, &client).await.unwrap();
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn integ_get_comments_bails_without_a_total() {
    let server = MockServer::start().await;
    // A 2xx that omits the count cannot say whether more comments exist.
    Mock::given(method("GET"))
        .and(path("/rest/api/3/issue/ABC-1/comment"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "comments": [{ "id": "1" }] })),
        )
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = get_comments("ABC-1", false, &client)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("missing `total` count"), "got: {err}");
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
async fn integ_update_issue_preserves_empty_null_and_omitted_descriptions() {
    let server = MockServer::start().await;
    let client = mock_client(server.uri());
    for (input, expected) in [
        (
            json!({ "description": "" }),
            json!({ "description": {
                "type": "doc", "version": 1,
                "content": [{ "type": "paragraph", "content": [] }]
            } }),
        ),
        (
            json!({ "description": null }),
            json!({ "description": null }),
        ),
        (json!({ "summary": "new" }), json!({ "summary": "new" })),
        (
            json!({ "description": " \t\n" }),
            json!({ "description": {
                "type": "doc", "version": 1,
                "content": [{ "type": "paragraph", "content": [
                    { "type": "text", "text": " \t\n" }
                ] }]
            } }),
        ),
    ] {
        Mock::given(method("PUT"))
            .and(path("/rest/api/3/issue/MDW-1"))
            .and(body_json(json!({ "fields": expected })))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        assert_eq!(
            update_issue("MDW-1", input, &client).await.unwrap(),
            json!({})
        );
    }
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

#[tokio::test]
async fn search_preserves_scope_and_quoted_sorting_on_the_wire() {
    use crate::test_utils::mock_client_with_config;
    use wiremock::matchers::body_partial_json;
    let server = MockServer::start().await;
    let client = mock_client_with_config(
        server.uri(),
        create_test_config(vec!["DEFAULT".into()], None),
    );
    for (input, expected) in [
        ("\"project\" = OTHER", "\"project\" = OTHER"),
        ("status=Open&&!project=OTHER", "status=Open&&!project=OTHER"),
        (
            "\"project.key\" = OTHER",
            "project IN (\"DEFAULT\") AND (\"project.key\" = OTHER)",
        ),
        (
            "status = Open ORDER BY\"created\"",
            "project IN (\"DEFAULT\") AND (status = Open) ORDER BY\"created\"",
        ),
    ] {
        Mock::given(method("POST"))
            .and(path("/rest/api/3/search/jql"))
            .and(body_partial_json(json!({"jql":expected})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"issues":[{"key":"OTHER-1"}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let result = search(input, 1, None, false, &client).await.unwrap();
        assert_eq!(result["items"][0]["key"], "OTHER-1");
    }
}

/// The query is the caller's text, so it travels as one parameter value even
/// when it carries `&`, and the bare array the endpoint answers with becomes the
/// list envelope with the usual noise filtered out.
#[tokio::test]
async fn integ_search_users_sends_the_query_as_one_value_and_lists_the_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/user/search"))
        .and(query_param("query", "Kim & Lee"))
        .and(query_param("maxResults", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "accountId": "5b10a2844c20165700ede21g",
            "displayName": "Kim Lee",
            "active": true,
            "avatarUrls": { "48x48": "https://avatar.example" }
        }])))
        .expect(1)
        .mount(&server)
        .await;

    let result = search_users("Kim & Lee", 5, &mock_client(server.uri()))
        .await
        .unwrap();

    assert_eq!(
        result,
        json!({ "items": [{
            "accountId": "5b10a2844c20165700ede21g",
            "displayName": "Kim Lee",
            "active": true
        }] })
    );
}
