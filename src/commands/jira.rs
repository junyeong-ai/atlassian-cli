use crate::cli::{
    BoardAction, CommentAction, EpicAction, JiraCommand, JiraFormat, JiraSubcommand, LinkAction,
    ListAction, SprintAction, TransitionAction, UserAction, WatcherAction, WorklogAction,
};
use crate::commands::read_body;
use anyhow::Result;
use atlassian_cli::markdown::markdown_to_adf;
use serde_json::Value;

/// A rich-text body as the Jira API layer takes it: a string it normalizes to
/// ADF itself, or the document Markdown converts to.
fn rich_text(text: String, format: JiraFormat) -> Result<Value> {
    match format {
        JiraFormat::Adf => Ok(Value::String(text)),
        JiraFormat::Markdown => markdown_to_adf(&text),
    }
}

/// Convert a string `description` among JSON fields under Markdown. A JSON
/// object there is already a document and is left to ADF validation.
fn convert_description(fields: &mut Value, format: JiraFormat) -> Result<()> {
    if let JiraFormat::Markdown = format
        && let Some(Value::String(text)) = fields.get("description")
    {
        fields["description"] = markdown_to_adf(text)?;
    }
    Ok(())
}

pub(crate) async fn run(cmd: JiraCommand, client: &atlassian_cli::ApiClient) -> Result<Value> {
    use atlassian_cli::jira;

    match cmd.subcommand {
        JiraSubcommand::Get {
            issue_key,
            fields,
            format,
        } => {
            let as_markdown = matches!(format, JiraFormat::Markdown);
            jira::get_issue(&issue_key, fields, as_markdown, client).await
        }
        JiraSubcommand::Search {
            jql,
            limit,
            all,
            stream,
            fields,
            format,
        } => {
            let as_markdown = matches!(format, JiraFormat::Markdown);
            if all {
                jira::search_all(&jql, fields, stream, as_markdown, client).await
            } else {
                jira::search(&jql, limit, fields, as_markdown, client).await
            }
        }
        JiraSubcommand::Create {
            project,
            summary,
            issue_type,
            description,
            parent,
            fields,
            format,
        } => {
            let desc = match description {
                Some(text) => rich_text(read_body(text).await?, format)?,
                None => Value::Null,
            };
            let extra = fields
                .map(|raw| {
                    let mut extra = serde_json::from_str::<Value>(&raw).map_err(|e| {
                        anyhow::anyhow!(
                            "Invalid JSON for --fields: {}. Example: {{\"labels\":[\"api\"]}}",
                            e
                        )
                    })?;
                    convert_description(&mut extra, format)?;
                    anyhow::Ok(extra)
                })
                .transpose()?;
            jira::create_issue(
                &project,
                &summary,
                &issue_type,
                desc,
                parent.as_deref(),
                extra,
                client,
            )
            .await
        }
        JiraSubcommand::Update {
            issue_key,
            fields,
            format,
        } => {
            let mut fields_value: Value =
                serde_json::from_str(&read_body(fields).await?).map_err(|e| {
                    anyhow::anyhow!(
                        "Invalid JSON for update fields: {}. Example: {{\"summary\":\"New title\"}}",
                        e
                    )
                })?;
            convert_description(&mut fields_value, format)?;
            jira::update_issue(&issue_key, fields_value, client).await
        }
        JiraSubcommand::Delete {
            issue_key,
            yes,
            delete_subtasks,
        } => {
            if !yes {
                anyhow::bail!(
                    "Deleting {} is irreversible (Jira has no recycle bin for issues). Re-run with --yes to confirm.",
                    issue_key
                );
            }
            jira::delete_issue(&issue_key, delete_subtasks, client).await
        }
        JiraSubcommand::Comment { action } => match action {
            CommentAction::Add {
                issue_key,
                text,
                format,
            } => {
                let body = rich_text(read_body(text).await?, format)?;
                jira::add_comment(&issue_key, body, client).await
            }
            CommentAction::Update {
                issue_key,
                comment_id,
                text,
                format,
            } => {
                let body = rich_text(read_body(text).await?, format)?;
                jira::update_comment(&issue_key, &comment_id, body, client).await
            }
            CommentAction::List { issue_key, format } => {
                let as_markdown = matches!(format, JiraFormat::Markdown);
                jira::get_comments(&issue_key, as_markdown, client).await
            }
            CommentAction::Delete {
                issue_key,
                comment_id,
            } => jira::delete_comment(&issue_key, &comment_id, client).await,
        },
        JiraSubcommand::Transition { action } => match action {
            TransitionAction::Apply {
                issue_key,
                transition_id,
            } => jira::transition_issue(&issue_key, &transition_id, client).await,
            TransitionAction::List { issue_key } => jira::get_transitions(&issue_key, client).await,
        },
        JiraSubcommand::Link { action } => match action {
            LinkAction::Types => jira::get_link_types(client).await,
            LinkAction::Add {
                source,
                target,
                link_type,
                comment,
            } => {
                let comment_val = comment.map(Value::String).unwrap_or(Value::Null);
                jira::add_link(&source, &target, &link_type, comment_val, client).await
            }
            LinkAction::Remove {
                source,
                target,
                link_type,
                id,
            } => match (id, source, target) {
                (Some(id), _, _) => jira::remove_link_by_id(&id, client).await,
                (None, Some(source), Some(target)) => {
                    jira::remove_link(&source, &target, link_type.as_deref(), client).await
                }
                // Clap requires the pair wherever `--id` is absent and refuses
                // the parse otherwise. Said again here because a parse that
                // stopped enforcing it would otherwise reach a delete with one
                // end of the link unnamed.
                (None, ..) => anyhow::bail!(
                    "Name the two issues the link runs between, or --id to remove one link by \
                     its own id."
                ),
            },
            LinkAction::List { issue_key } => jira::get_links(&issue_key, client).await,
        },
        JiraSubcommand::Worklog { action } => match action {
            WorklogAction::Add {
                issue_key,
                time_spent,
                comment,
                started,
            } => {
                let comment_val = comment.map(Value::String).unwrap_or(Value::Null);
                jira::add_worklog(
                    &issue_key,
                    &time_spent,
                    comment_val,
                    started.as_deref(),
                    client,
                )
                .await
            }
            WorklogAction::List { issue_key } => jira::get_worklogs(&issue_key, client).await,
            WorklogAction::Update {
                issue_key,
                worklog_id,
                time_spent,
                comment,
            } => {
                let comment_val = comment.map(Value::String).unwrap_or(Value::Null);
                jira::update_worklog(&issue_key, &worklog_id, &time_spent, comment_val, client)
                    .await
            }
            WorklogAction::Remove {
                issue_key,
                worklog_id,
            } => jira::remove_worklog(&issue_key, &worklog_id, client).await,
        },
        JiraSubcommand::Watcher { action } => match action {
            WatcherAction::Add { issue_key } => jira::add_watcher(&issue_key, client).await,
            WatcherAction::Remove { issue_key } => jira::remove_watcher(&issue_key, client).await,
            WatcherAction::List { issue_key } => jira::get_watchers(&issue_key, client).await,
        },
        JiraSubcommand::User { action } => match action {
            UserAction::Search { query, limit } => jira::search_users(&query, limit, client).await,
        },
        JiraSubcommand::List { action } => match action {
            ListAction::Types => jira::get_issue_types(client).await,
            ListAction::Priorities => jira::get_priorities(client).await,
            ListAction::Statuses => jira::get_statuses(client).await,
            ListAction::Labels => jira::get_labels(client).await,
        },
        JiraSubcommand::Board { action } => match action {
            BoardAction::List { project } => jira::get_boards(&project, client).await,
        },
        JiraSubcommand::Sprint { action } => match action {
            SprintAction::List {
                board,
                project,
                state,
            } => {
                let board_id = match board {
                    Some(id) => id,
                    None => {
                        let project_key = project.ok_or_else(|| {
                            anyhow::anyhow!("Either --board or --project is required")
                        })?;
                        jira::resolve_board_id(&project_key, client).await?
                    }
                };
                jira::get_sprints(board_id, &state, client).await
            }
            SprintAction::Move { sprint_id, issues } => {
                jira::move_issues_to_sprint(sprint_id, &issues, client).await
            }
            SprintAction::Backlog { issues } => jira::move_issues_to_backlog(&issues, client).await,
        },
        JiraSubcommand::Epic { action } => match action {
            EpicAction::Assign { epic_key, issues } => {
                jira::assign_issues_to_epic(&epic_key, &issues, client).await
            }
            EpicAction::Unassign { issues } => {
                jira::unassign_issues_from_epic(&issues, client).await
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn markdown_converts_only_a_string_description() {
        let mut fields = json!({ "summary": "**kept**", "description": "**bold**" });
        convert_description(&mut fields, JiraFormat::Markdown).unwrap();
        assert_eq!(fields["summary"], "**kept**");
        assert_eq!(fields["description"], markdown_to_adf("**bold**").unwrap());

        for untouched in [
            json!({ "type": "doc", "version": 1, "content": [] }),
            Value::Null,
        ] {
            let mut fields = json!({ "description": untouched.clone() });
            convert_description(&mut fields, JiraFormat::Markdown).unwrap();
            assert_eq!(fields["description"], untouched);
        }

        let mut fields = json!({ "description": "**bold**" });
        convert_description(&mut fields, JiraFormat::Adf).unwrap();
        assert_eq!(fields["description"], "**bold**");
    }
}
