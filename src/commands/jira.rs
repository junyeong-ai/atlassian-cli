use crate::cli::{
    BoardAction, CommentAction, EpicAction, JiraCommand, JiraSubcommand, LinkAction, ListAction,
    OutputFormat, SprintAction, TransitionAction, WatcherAction, WorklogAction,
};
use anyhow::Result;

pub(crate) async fn run(
    cmd: JiraCommand,
    client: &atlassian_cli::ApiClient,
) -> Result<serde_json::Value> {
    use atlassian_cli::jira;

    match cmd.subcommand {
        JiraSubcommand::Get {
            issue_key,
            fields,
            format,
        } => {
            let as_markdown = matches!(format, OutputFormat::Markdown);
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
            let as_markdown = matches!(format, OutputFormat::Markdown);
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
        } => {
            let desc = description
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null);
            let extra = fields
                .map(|raw| {
                    serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| {
                        anyhow::anyhow!(
                            "Invalid JSON for --fields: {}. Example: {{\"labels\":[\"api\"]}}",
                            e
                        )
                    })
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
        JiraSubcommand::Update { issue_key, fields } => {
            let fields_value: serde_json::Value = serde_json::from_str(&fields).map_err(|e| {
                anyhow::anyhow!(
                    "Invalid JSON for update fields: {}. Example: {{\"summary\":\"New title\"}}",
                    e
                )
            })?;
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
            CommentAction::Add { issue_key, text } => {
                jira::add_comment(&issue_key, serde_json::Value::String(text), client).await
            }
            CommentAction::Update {
                issue_key,
                comment_id,
                text,
            } => {
                jira::update_comment(
                    &issue_key,
                    &comment_id,
                    serde_json::Value::String(text),
                    client,
                )
                .await
            }
            CommentAction::List { issue_key, format } => {
                let as_markdown = matches!(format, OutputFormat::Markdown);
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
                let comment_val = comment
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
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
                let comment_val = comment
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
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
                let comment_val = comment
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
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
