use super::list_envelope;
use crate::client::{ApiClient, Service};
use crate::http_utils::encode_path_segment;
use crate::jira::adf;
use crate::response::require_array;
use anyhow::{Context, Result};
use serde_json::{Value, json};

pub async fn get_link_types(client: &ApiClient) -> Result<Value> {
    let request = client
        .get(Service::Jira, "/rest/api/3/issueLinkType")
        .await?;
    let response = client.execute("get link types", request).await?;

    let data: Value = response.json().await?;
    // The envelope contract is `{"items": [...]}`; a 2xx that lost the array
    // would hand a `jq` pipeline `null` and a caller "there are none". Read
    // before filtering, so the error can only mean the response lacked it.
    Ok(list_envelope(
        require_array(&data, "/issueLinkTypes", "get link types")?,
        client,
    ))
}

pub async fn add_link(
    source_key: &str,
    target_key: &str,
    link_type: &str,
    comment: Value,
    client: &ApiClient,
) -> Result<Value> {
    let mut body = json!({
        "type": { "name": link_type },
        "outwardIssue": { "key": source_key },
        "inwardIssue": { "key": target_key }
    });

    if !comment.is_null() {
        let comment_adf = adf::process_comment_input(comment)?;
        body["comment"] = json!({ "body": comment_adf });
    }

    let request = client
        .post(Service::Jira, "/rest/api/3/issueLink")
        .await?
        .header("Content-Type", "application/json")
        .json(&body);
    client.execute("create link", request).await?;

    Ok(json!({}))
}

/// Whether naming some type would leave one match and nothing unread — the
/// state this command removes in, and so the only state the advice may promise.
///
/// A name is a way out where exactly one match carries it. Fewer than all of
/// them is not enough: `Blocks, Blocks, Relates, Relates` is a dead end under
/// either name. A match carrying no name is one no `--type` selects, and it
/// makes every name a dead end with it.
///
/// An entry the walk could not read does not rule the advice out by itself.
/// The type exclusion runs before anything else is read of an entry, so naming
/// a type takes out every unread one carrying a different name. Two do survive
/// it: one carrying no name, which no `--type` reads, and one carrying the very
/// name that would be named, which stays unread beside the match it leaves.
fn a_type_would_narrow_to_one(matching: &[&Value], unread: &[(&str, Option<&str>)]) -> bool {
    let mut names = Vec::with_capacity(matching.len());
    for link in matching {
        match link["type"]["name"].as_str() {
            None => return false,
            Some(name) => names.push(name),
        }
    }
    names.iter().any(|name| {
        names.iter().filter(|other| *other == name).count() == 1
            && unread
                .iter()
                .all(|(_, unread_name)| unread_name.is_some_and(|n| n != *name))
    })
}

pub async fn remove_link(
    source_key: &str,
    target_key: &str,
    link_type: Option<&str>,
    client: &ApiClient,
) -> Result<Value> {
    let items = fetch_links(source_key, client).await?;

    // A link names the issue at its other end, its direction by which side
    // names it, and its type when one was asked for. Any of those that is
    // present and disagrees excludes the entry outright; among what is left,
    // the answer is decided in that order, so none of them rests on a field
    // that was never there and none is withheld over one that no longer
    // matters.
    let mut matching: Vec<&Value> = Vec::new();
    // What each entry left open, not merely how many did: the reasons differ,
    // and a fixed one would be describing entries it never saw. The type it
    // carried rides along, because naming a type takes out an unread entry
    // carrying a different one.
    let mut unreadable: Vec<(&'static str, Option<&str>)> = Vec::new();
    let mut reverse = 0usize;
    for link in &items {
        let type_name = link["type"]["name"].as_str();

        // A type that is there and is not the one asked for settles the entry
        // before anything else is read of it: whatever the rest of the entry
        // turns out to be, it is not a link of that type.
        if link_type.is_some_and(|t| type_name.is_some_and(|n| n != t)) {
            continue;
        }

        // An outward side that named a third issue settles the entry the same
        // way, whatever the rest of it turns out to be: read as this issue's
        // other end it is that issue's link, read absolutely its pair holds
        // that issue, and read as an inward end it runs the other way. None of
        // those is the link asked for, so nothing below needs reading.
        if link["outwardIssue"]["key"]
            .as_str()
            .is_some_and(|k| k != target_key && k != source_key)
        {
            continue;
        }

        // A side is an issue or it is not there. Anything else — a string, a
        // number, a list — is a shape this code has no reading of, and since
        // carrying a side is what settles direction below, taking one for a
        // side would settle direction on something never read.
        let side_unreadable = |side: &Value| !side.is_null() && !side.is_object();
        if side_unreadable(&link["outwardIssue"]) || side_unreadable(&link["inwardIssue"]) {
            unreadable.push(("carries a side that is not an issue", type_name));
            continue;
        }

        // Which sides the entry carries, and separately what they say. A side
        // that is there with an unreadable key still tells us the entry has
        // that end; reading only the key would take it for a side that is not
        // there at all, and the two mean opposite things here.
        let has_outward = link["outwardIssue"].is_object();
        let has_inward = link["inwardIssue"].is_object();
        let outward_key = link["outwardIssue"]["key"].as_str();
        let inward_key = link["inwardIssue"]["key"].as_str();

        // The same for the issues it names, but only where every side it
        // carries said which issue it is. An entry naming two issues is
        // excluded when neither of them is the one asked about: the pair is
        // what the request names, and either side carrying it makes the entry
        // relevant.
        let keys_readable =
            (!has_outward || outward_key.is_some()) && (!has_inward || inward_key.is_some());
        let names_target = outward_key == Some(target_key) || inward_key == Some(target_key);
        if keys_readable && (has_outward || has_inward) && !names_target {
            continue;
        }

        // An issue's own links carry the other end and only the other end, so
        // which side carries it is the direction. An entry carrying both is
        // the shape the standalone link resource has, where the sides are
        // absolute rather than relative to this issue: taking one of them as
        // "the other end" is a guess, and this guess decides a delete.
        //
        // Two readings of it could still be the link asked for — the outward
        // side is this issue's other end and it is the target, or the sides
        // are absolute and they are this issue and the target. The third, the
        // inward side being the other end, runs the other way and never is.
        // What ruled the entry out under all three has already gone above.
        if has_outward && has_inward {
            unreadable.push(("carries both ends", type_name));
            continue;
        }

        // Direction is part of the match, not a tie-break. `add` puts the
        // source on the outward side, so on this issue the link it created
        // carries the other one as `outwardIssue`; an entry with only an
        // inward side runs the other way and is removed from the issue it
        // starts at. Carrying that side settles the entry whatever the rest of
        // it says, so nothing below is read for it — and it joins the links
        // running the other way only where it said which issue it is between,
        // which is the only form that count is claimed in.
        if has_inward {
            if inward_key == Some(target_key) {
                reverse += 1;
            }
            continue;
        }

        // An outward side that did not say which issue, or no side at all —
        // and with no side there is no direction to read either. Either way
        // the entry may be the second link from here to the issue asked
        // about, which is what would make the removal a guess.
        if !names_target {
            unreadable.push(("names no issue", type_name));
            continue;
        }

        // Outward, to the issue asked about, and the type asked for is not in
        // the response: this may be a second link of that type. The command
        // refuses two it can read and cannot tell apart, so one it cannot read
        // leaves that same question open.
        if link_type.is_some() && type_name.is_none() {
            unreadable.push(("names no type", type_name));
            continue;
        }

        matching.push(link);
    }

    // The way through any refusal below, worked out once because the rule is
    // the same for all of them: an entry that went unread is a candidate whose
    // id this walk never saw, so wherever there is one the ids it did read are
    // not the whole of what to choose between.
    let choose_by_id = by_id_advice(
        source_key,
        unreadable.is_empty().then_some(matching.as_slice()),
    );

    // Where two the walk could read already match, an entry it could not read
    // changes nothing: the command cannot choose among the ones it has, and
    // saying the question cannot be told would be saying it of a question
    // already answered.
    if !unreadable.is_empty() && matching.len() < 2 {
        let mut reasons: Vec<&str> = Vec::new();
        for (reason, _) in &unreadable {
            if !reasons.contains(reason) {
                reasons.push(reason);
            }
        }
        anyhow::bail!(
            "{} of the links on {} left the question open ({}), so {}. {}",
            unreadable.len(),
            source_key,
            reasons.join("; "),
            if matching.is_empty() {
                format!(
                    "whether one from {source_key} to {target_key} is among them cannot be told"
                )
            } else {
                format!(
                    "whether one of them is a second link from {source_key} to {target_key} \
                     cannot be told, and this command removes only where there is one"
                )
            },
            choose_by_id
        );
    }

    // The count is of what the walk could read. An entry it could not may be
    // another of them, and where one is there the answer says how many it got
    // to rather than how many there are.
    let found = if unreadable.is_empty() {
        format!("Found {} links", matching.len())
    } else {
        format!("Found at least {} links", matching.len())
    };

    match matching.len() {
        // What this counted is links running the other way that the requested
        // type did not rule out — not every link between the two issues, and
        // not every link of that type either, since one may have gone unread.
        // A total stated here would be either, and naming one of them to
        // remove would claim a uniqueness the walk did not establish. What it
        // did establish is that the request belongs on the other issue.
        0 if reverse > 0 => anyhow::bail!(
            "No link runs from {} to {}{}, but at least one link between them runs the other \
             way. A link is removed from the issue it starts at, so run this on {}.",
            source_key,
            target_key,
            link_type
                .map(|t| format!(" with type '{}'", t))
                .unwrap_or_default(),
            target_key
        ),
        0 => anyhow::bail!(
            "No link runs from {} to {}{}. Links name the key an issue has now, so a key \
             left over from a renamed project will not match one.",
            source_key,
            target_key,
            link_type
                .map(|t| format!(" with type '{}'", t))
                .unwrap_or_default()
        ),
        1 => {}
        // Without a type, naming one is the way out. With one, the pair and the
        // type are everything this command takes, so it has nothing left to
        // choose by and says so rather than picking.
        // Naming a type is the way out only where one of them would be left
        // alone by it, and only where nothing was left unread: an entry this
        // walk could not read is one no `--type` reads either, and it withholds
        // the removal once the matches are down to one. Anywhere else the
        // advice narrows to a set this same refusal answers again.
        _ if link_type.is_none() && a_type_would_narrow_to_one(&matching, &unreadable) => {
            anyhow::bail!(
                "{} from {} to {}. Specify --type to disambiguate.",
                found,
                source_key,
                target_key
            )
        }
        _ => match link_type {
            None => anyhow::bail!(
                "{} from {} to {}, and no type tells them apart. This command removes by \
                     issue pair and type, so it cannot choose between them. {}",
                found,
                source_key,
                target_key,
                choose_by_id
            ),
            Some(t) => anyhow::bail!(
                "{} from {} to {} with type '{}'. This command removes by issue pair and \
                     type, so it cannot choose between them. {}",
                found,
                source_key,
                target_key,
                t,
                choose_by_id
            ),
        },
    }

    let link_id = matching[0]["id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Link ID missing from response"))?;

    remove_link_by_id(link_id, client).await
}

/// The way through a refusal that could not settle on one link.
///
/// `among` is the set the caller may offer a choice from, where there is one.
/// Their ids are named only where every one of them carried a readable id — a
/// list short by the match whose id could not be read offers a choice that
/// leaves that one out, the same way one short by an unread entry would.
fn by_id_advice(source_key: &str, among: Option<&[&Value]>) -> String {
    let ids = among.and_then(|links| {
        links
            .iter()
            .map(|link| link["id"].as_str())
            .collect::<Option<Vec<_>>>()
    });
    match ids {
        Some(ids) => format!(
            "Pass --id to remove one of them by its own id ({}).",
            ids.join(", ")
        ),
        None => format!(
            "Pass --id to remove one by its own id, which `jira link list {source_key}` reports."
        ),
    }
}

/// Remove the link an id names, and no other.
///
/// The id is the link's whole address: the endpoint takes nothing else, and it
/// is a field every refusal above could read off entries it could not classify.
/// Confirming it against the issue pair would put this back behind the walk
/// those refusals are the way out of, so nothing is read before the delete.
pub async fn remove_link_by_id(link_id: &str, client: &ApiClient) -> Result<Value> {
    let url = format!("/rest/api/3/issueLink/{}", encode_path_segment(link_id));
    let request = client.delete(Service::Jira, &url).await?;
    client.execute("remove link", request).await?;

    Ok(json!({}))
}

/// The links on an issue, as the API gave them.
///
/// Unfiltered, because `remove_link` decides which link to delete by reading
/// `type.name` and the other issue's `key` off these: `response_exclude_fields`
/// is a cosmetic setting for reads, and a name in it matching one of those
/// would have the match fail and the command report no such link on an issue
/// that has one. `get links` filters the items it returns instead, which is
/// where filtering was always for.
///
/// A link-free issue still carries `[]`; the field goes missing only where the
/// site has issue linking turned off, which is what the error says — reading
/// that as "no links on this issue" would have `remove_link` report nothing to
/// remove for the same reason.
async fn fetch_links(issue_key: &str, client: &ApiClient) -> Result<Vec<Value>> {
    let url = format!(
        "/rest/api/3/issue/{}?fields=issuelinks",
        encode_path_segment(issue_key)
    );

    let request = client.get(Service::Jira, &url).await?;
    let response = client.execute("get links", request).await?;

    let data: Value = response.json().await?;
    require_array(&data, "/fields/issuelinks", "get links")
        .context("a site with issue linking turned off carries no 'issuelinks' field")
}

pub async fn get_links(issue_key: &str, client: &ApiClient) -> Result<Value> {
    Ok(list_envelope(fetch_links(issue_key, client).await?, client))
}
