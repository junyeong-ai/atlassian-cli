use crate::cli::{
    ConfluenceAttachmentAction, ConfluenceCommand, ConfluenceCommentAction, ConfluenceFormat,
    ConfluenceLabelAction, ConfluencePropertyAction, ConfluenceSpaceAction, ConfluenceSubcommand,
};
use crate::commands::read_body;
use anyhow::Result;
use atlassian_cli::markdown::markdown_to_confluence;

/// A body argument as storage format, the representation every write sends.
async fn storage_body(argument: String, format: ConfluenceFormat) -> Result<String> {
    let body = read_body(argument).await?;
    match format {
        ConfluenceFormat::Storage => Ok(body),
        ConfluenceFormat::Markdown => markdown_to_confluence(&body),
    }
}

pub(crate) async fn run(
    cmd: ConfluenceCommand,
    client: &atlassian_cli::ApiClient,
) -> Result<serde_json::Value> {
    use atlassian_cli::confluence;

    match cmd.subcommand {
        ConfluenceSubcommand::Search {
            query,
            limit,
            all,
            stream,
            expand,
            format,
        } => {
            let as_markdown = matches!(format, ConfluenceFormat::Markdown);
            if all {
                confluence::search_all(&query, limit, None, expand, stream, as_markdown, client)
                    .await
            } else {
                confluence::search(&query, limit, None, expand, as_markdown, client).await
            }
        }
        ConfluenceSubcommand::Get { page_id, format } => {
            let as_markdown = matches!(format, ConfluenceFormat::Markdown);
            confluence::get_page(&page_id, None, None, as_markdown, client).await
        }
        ConfluenceSubcommand::Create {
            space,
            title,
            content,
            parent,
            format,
        } => {
            let content = storage_body(content, format).await?;
            confluence::create_page(&space, &title, &content, parent.as_deref(), client).await
        }
        ConfluenceSubcommand::Update {
            page_id,
            title,
            content,
            parent,
            format,
            base_version,
        } => {
            let content = storage_body(content, format).await?;
            confluence::update_page(
                &page_id,
                &title,
                &content,
                parent.as_deref(),
                base_version,
                client,
            )
            .await
        }
        ConfluenceSubcommand::Children { page_id } => {
            confluence::get_page_children(&page_id, client).await
        }
        ConfluenceSubcommand::Comment { action } => match action {
            ConfluenceCommentAction::List {
                page_id,
                location,
                roots_only,
                format,
            } => {
                let families = match location {
                    Some(location) => vec![location.into()],
                    None => confluence::CommentFamily::ALL.to_vec(),
                };
                confluence::get_comments(
                    &page_id,
                    &families,
                    !roots_only,
                    matches!(format, ConfluenceFormat::Markdown),
                    client,
                )
                .await
            }
            ConfluenceCommentAction::Get {
                comment_id,
                location,
                format,
            } => {
                confluence::get_comment(
                    &comment_id,
                    location.into(),
                    matches!(format, ConfluenceFormat::Markdown),
                    client,
                )
                .await
            }
            ConfluenceCommentAction::Replies {
                comment_id,
                location,
                format,
            } => {
                confluence::get_comment_replies(
                    &comment_id,
                    location.into(),
                    matches!(format, ConfluenceFormat::Markdown),
                    client,
                )
                .await
            }
            ConfluenceCommentAction::Add {
                page_id,
                body,
                reply_to,
                format,
            } => {
                let body = storage_body(body, format).await?;
                confluence::add_comment(&page_id, &body, reply_to.as_deref(), client).await
            }
            ConfluenceCommentAction::Update {
                comment_id,
                body,
                format,
            } => {
                let body = storage_body(body, format).await?;
                confluence::update_comment(&comment_id, &body, client).await
            }
            ConfluenceCommentAction::Delete { comment_id } => {
                confluence::delete_comment(&comment_id, client).await
            }
        },
        ConfluenceSubcommand::Label { action } => match action {
            ConfluenceLabelAction::List { page_id } => {
                confluence::get_labels(&page_id, client).await
            }
            ConfluenceLabelAction::Add { page_id, label } => {
                confluence::add_label(&page_id, &label, client).await
            }
            ConfluenceLabelAction::Remove { page_id, label } => {
                confluence::remove_label(&page_id, &label, client).await
            }
        },
        ConfluenceSubcommand::Property { action } => match action {
            ConfluencePropertyAction::List { page_id } => {
                confluence::get_properties(&page_id, client).await
            }
            ConfluencePropertyAction::Set {
                page_id,
                key,
                value,
            } => {
                let parsed: serde_json::Value = serde_json::from_str(&read_body(value).await?)
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "value must be valid JSON (quote strings, e.g. '\"done\"'): {}",
                            e
                        )
                    })?;
                confluence::set_property(&page_id, &key, parsed, client).await
            }
            ConfluencePropertyAction::Delete { page_id, key } => {
                confluence::delete_property(&page_id, &key, client).await
            }
        },
        ConfluenceSubcommand::Space { action } => match action {
            ConfluenceSpaceAction::List => confluence::get_spaces(client).await,
            ConfluenceSpaceAction::Get { space_key } => {
                confluence::get_space(&space_key, client).await
            }
        },
        ConfluenceSubcommand::Attachment { action } => match action {
            ConfluenceAttachmentAction::List { page_id } => {
                confluence::get_attachments(&page_id, client).await
            }
            ConfluenceAttachmentAction::Upload {
                page_id,
                file,
                comment,
                minor,
                content_type,
            } => {
                confluence::upload_attachment(
                    &page_id,
                    &file,
                    comment.as_deref(),
                    minor,
                    content_type.as_deref(),
                    client,
                )
                .await
            }
        },
        ConfluenceSubcommand::Delete { page_id, yes } => {
            if !yes {
                anyhow::bail!(
                    "Deleting page {} moves it to the trash. Re-run with --yes to confirm.",
                    page_id
                );
            }
            confluence::delete_page(&page_id, client).await
        }
    }
}
