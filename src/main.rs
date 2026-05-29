use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, Default, ValueEnum)]
enum OutputFormat {
    #[default]
    Html,
    Markdown,
}

#[derive(Parser)]
#[command(name = "atlassian-cli", version, about = "CLI for Atlassian Jira and Confluence", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, help = "Config file path")]
    config: Option<PathBuf>,

    #[arg(long, help = "Profile name")]
    profile: Option<String>,

    #[arg(long, env = "ATLASSIAN_DOMAIN")]
    domain: Option<String>,

    #[arg(long, env = "ATLASSIAN_EMAIL")]
    email: Option<String>,

    #[arg(long, env = "ATLASSIAN_API_TOKEN")]
    token: Option<String>,

    #[arg(long, env = "ATLASSIAN_CLIENT_ID")]
    client_id: Option<String>,

    #[arg(long, env = "ATLASSIAN_CLIENT_SECRET")]
    client_secret: Option<String>,

    #[arg(long, env = "ATLASSIAN_CLOUD_ID")]
    cloud_id: Option<String>,

    #[arg(long, help = "Pretty-print JSON output")]
    pretty: bool,

    #[arg(short, long, action = clap::ArgAction::Count, help = "Verbose logging")]
    verbose: u8,
}

impl Cli {
    fn to_overrides(&self) -> atlassian_cli::CliOverrides {
        atlassian_cli::CliOverrides {
            domain: self.domain.clone(),
            email: self.email.clone(),
            token: self.token.clone(),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            cloud_id: self.cloud_id.clone(),
        }
    }
}

#[derive(Subcommand)]
enum Command {
    Jira(JiraCommand),
    Confluence(ConfluenceCommand),
    Config(ConfigCommand),
    Auth(AuthCommand),
}

#[derive(Parser)]
struct AuthCommand {
    #[command(subcommand)]
    subcommand: AuthSubcommand,
}

#[derive(Subcommand)]
enum AuthSubcommand {
    /// Start the OAuth 3LO flow and persist tokens for the active profile.
    Login {
        #[arg(long, help = "Print the authorize URL instead of opening a browser")]
        no_browser: bool,
    },
    /// Discard stored OAuth tokens for the active profile.
    Logout,
    /// Show the active profile's stored token status (identity, expiry, scopes).
    Status,
    /// Force-refresh the access_token using the stored refresh_token.
    Refresh,
}

#[derive(Parser)]
struct JiraCommand {
    #[command(subcommand)]
    subcommand: JiraSubcommand,
}

#[derive(Subcommand)]
enum JiraSubcommand {
    /// Fetch a single issue by key (fields filtered, ADF rendered)
    Get {
        issue_key: String,
        #[arg(long, value_enum, default_value = "html", help = "ADF content format")]
        format: OutputFormat,
    },
    /// Search issues with JQL; the configured project filter is auto-injected
    Search {
        jql: String,
        #[arg(long, default_value = "100", help = "Results per page")]
        limit: u32,
        #[arg(long, help = "Fetch all results via token pagination")]
        all: bool,
        #[arg(long, help = "Stream as JSONL (requires --all)")]
        stream: bool,
        #[arg(long, value_delimiter = ',', help = "Fields to return")]
        fields: Option<Vec<String>>,
        #[arg(long, value_enum, default_value = "html", help = "ADF content format")]
        format: OutputFormat,
    },
    /// Create an issue (plain-text description auto-converts to ADF)
    Create {
        project: String,
        summary: String,
        issue_type: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Update an issue's fields from a JSON object (e.g. '{"summary":"..."}')
    Update { issue_key: String, fields: String },
    /// Permanently delete an issue (irreversible — requires --yes)
    Delete {
        issue_key: String,
        /// Confirm the irreversible deletion
        #[arg(long)]
        yes: bool,
        /// Also delete subtasks (Jira rejects the call otherwise when present)
        #[arg(long)]
        delete_subtasks: bool,
    },
    /// Add, update, or list comments on an issue
    Comment {
        #[command(subcommand)]
        action: CommentAction,
    },
    /// Apply or list workflow transitions for an issue
    Transition {
        #[command(subcommand)]
        action: TransitionAction,
    },
    /// Create, remove, or list issue links (and list link types)
    Link {
        #[command(subcommand)]
        action: LinkAction,
    },
    /// Add, update, list, or remove worklog (time-tracking) entries
    Worklog {
        #[command(subcommand)]
        action: WorklogAction,
    },
    /// Start watching, stop watching, or list watchers on an issue
    Watcher {
        #[command(subcommand)]
        action: WatcherAction,
    },
    /// Query global metadata (issue types, priorities, statuses, labels)
    List {
        #[command(subcommand)]
        action: ListAction,
    },
    /// List agile boards for a project
    Board {
        #[command(subcommand)]
        action: BoardAction,
    },
    /// List sprints, or move issues between a sprint and the backlog
    Sprint {
        #[command(subcommand)]
        action: SprintAction,
    },
    /// Assign issues to an epic, or remove them from their epics
    Epic {
        #[command(subcommand)]
        action: EpicAction,
    },
}

#[derive(Subcommand)]
enum CommentAction {
    /// Add a comment to an issue
    Add { issue_key: String, text: String },
    /// Update an existing comment
    Update {
        issue_key: String,
        comment_id: String,
        text: String,
    },
    /// List comments on an issue
    List {
        issue_key: String,
        #[arg(long, value_enum, default_value = "html", help = "ADF content format")]
        format: OutputFormat,
    },
    /// Delete a comment by id
    Delete {
        issue_key: String,
        comment_id: String,
    },
}

#[derive(Subcommand)]
enum TransitionAction {
    /// Apply a transition to an issue
    Apply {
        issue_key: String,
        transition_id: String,
    },
    /// List available transitions for an issue
    List { issue_key: String },
}

#[derive(Subcommand)]
enum LinkAction {
    /// List available link types
    Types,
    /// Create a link between two issues
    Add {
        /// Source issue key (outward side: "A blocks B" → A)
        source: String,
        /// Target issue key (inward side: "A blocks B" → B)
        target: String,
        /// Link type name
        #[arg(long = "type", default_value = "Relates")]
        link_type: String,
        /// Comment to add with the link
        #[arg(long)]
        comment: Option<String>,
    },
    /// Remove a link between two issues
    Remove {
        /// Source issue key
        source: String,
        /// Target issue key
        target: String,
        /// Link type (required when multiple link types exist between the pair)
        #[arg(long = "type")]
        link_type: Option<String>,
    },
    /// List links on an issue
    List {
        /// Issue key
        issue_key: String,
    },
}

#[derive(Subcommand)]
enum WorklogAction {
    /// Add a worklog entry to an issue
    Add {
        /// Issue key
        issue_key: String,
        /// Time spent (e.g., "2h 30m", "1d", "45m")
        time_spent: String,
        /// Comment describing the work
        #[arg(long)]
        comment: Option<String>,
        /// Start time in ISO 8601 format (defaults to now)
        #[arg(long)]
        started: Option<String>,
    },
    /// List worklog entries on an issue
    List {
        /// Issue key
        issue_key: String,
    },
    /// Update a worklog entry
    Update {
        /// Issue key
        issue_key: String,
        /// Worklog ID
        worklog_id: String,
        /// New time spent
        time_spent: String,
        /// Updated comment
        #[arg(long)]
        comment: Option<String>,
    },
    /// Remove a worklog entry
    Remove {
        /// Issue key
        issue_key: String,
        /// Worklog ID
        worklog_id: String,
    },
}

#[derive(Subcommand)]
enum WatcherAction {
    /// Start watching an issue (adds current user)
    Add {
        /// Issue key
        issue_key: String,
    },
    /// Stop watching an issue (removes current user)
    Remove {
        /// Issue key
        issue_key: String,
    },
    /// List watchers on an issue
    List {
        /// Issue key
        issue_key: String,
    },
}

#[derive(Subcommand)]
enum ListAction {
    /// List available issue types
    Types,
    /// List available priorities
    Priorities,
    /// List available statuses
    Statuses,
    /// List available labels
    Labels,
}

#[derive(Subcommand)]
enum BoardAction {
    /// List boards for a project
    List {
        /// Project key or ID
        #[arg(long)]
        project: String,
    },
}

#[derive(Subcommand)]
enum SprintAction {
    /// List sprints on a board
    List {
        /// Board ID
        #[arg(long, group = "board_source")]
        board: Option<u64>,
        /// Project key (auto-resolves board)
        #[arg(long, group = "board_source")]
        project: Option<String>,
        /// Sprint state filter
        #[arg(long, default_value = "active,future")]
        state: String,
    },
    /// Move issues to a sprint
    Move {
        /// Sprint ID
        sprint_id: u64,
        /// Issue keys to move
        #[arg(required = true)]
        issues: Vec<String>,
    },
    /// Move issues to the backlog
    Backlog {
        /// Issue keys to move
        #[arg(required = true)]
        issues: Vec<String>,
    },
}

#[derive(Subcommand)]
enum EpicAction {
    /// Assign issues to an epic
    Assign {
        /// Epic issue key
        epic_key: String,
        /// Issue keys to assign
        #[arg(required = true)]
        issues: Vec<String>,
    },
    /// Remove issues from their epics
    Unassign {
        /// Issue keys to unassign
        #[arg(required = true)]
        issues: Vec<String>,
    },
}

#[derive(Parser)]
struct ConfluenceCommand {
    #[command(subcommand)]
    subcommand: ConfluenceSubcommand,
}

#[derive(Subcommand)]
enum ConfluenceSubcommand {
    /// Search pages with CQL; the configured space filter is auto-injected
    Search {
        query: String,
        #[arg(
            long,
            default_value = "10",
            help = "Results per page (capped at 50 by the body-expanding search API). With --all, controls first-page batch size"
        )]
        limit: u32,
        #[arg(long, help = "Fetch all results via cursor pagination")]
        all: bool,
        #[arg(long, help = "Stream as JSONL (requires --all)")]
        stream: bool,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Expand fields (e.g., body.storage,ancestors)"
        )]
        expand: Option<Vec<String>>,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
    },
    /// Fetch a single page by ID (body rendered as HTML or markdown)
    Get {
        page_id: String,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
    },
    /// Create a page from storage-format HTML content
    Create {
        space: String,
        title: String,
        content: String,
    },
    /// Update a page's title and storage-format HTML content
    Update {
        page_id: String,
        title: String,
        content: String,
    },
    /// List the direct child pages of a page (metadata only)
    Children { page_id: String },
    /// List comments on a page
    Comments {
        page_id: String,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
    },
    /// Move a page to the trash (recoverable — requires --yes)
    Delete {
        page_id: String,
        /// Confirm the deletion
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Parser)]
struct ConfigCommand {
    #[command(subcommand)]
    subcommand: ConfigSubcommand,
}

#[derive(Subcommand)]
enum ConfigSubcommand {
    /// Create a starter config file at the global or project location.
    Init {
        #[arg(
            long,
            help = "Write to ~/.config/atlassian-cli/config.toml instead of ./.atlassian.toml"
        )]
        global: bool,
    },
    /// Print the resolved config (secrets masked).
    Show,
    /// List config file paths and environment variable status.
    List,
    /// Open the active config file in $EDITOR.
    Edit {
        #[arg(
            long,
            help = "Edit the global config even when a project config exists"
        )]
        global: bool,
    },
    /// Print the path of the active config file.
    Path {
        #[arg(
            long,
            help = "Print the global config path even when a project config exists"
        )]
        global: bool,
    },
    /// Validate configured credentials against Atlassian auth/API endpoints.
    Validate,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .with_writer(std::io::stderr)
        .init();

    let overrides = cli.to_overrides();
    let config_path = cli.config.clone();
    let profile = cli.profile.clone();

    match cli.command {
        Command::Config(cmd) => {
            handle_config(cmd, config_path.as_ref(), profile.as_ref(), overrides).await
        }
        Command::Auth(cmd) => handle_auth(cmd, config_path, profile, overrides).await,
        Command::Jira(cmd) => {
            let config =
                atlassian_cli::Config::load(config_path.as_ref(), profile.as_ref(), overrides)?;

            let client = atlassian_cli::ApiClient::new(config).await?;
            let result = handle_jira(cmd, &client).await?;
            output_json(&result, cli.pretty);
            Ok(())
        }
        Command::Confluence(cmd) => {
            let config =
                atlassian_cli::Config::load(config_path.as_ref(), profile.as_ref(), overrides)?;

            let client = atlassian_cli::ApiClient::new(config).await?;
            let result = handle_confluence(cmd, &client).await?;
            output_json(&result, cli.pretty);
            Ok(())
        }
    }
}

async fn handle_config(
    cmd: ConfigCommand,
    config_path: Option<&PathBuf>,
    profile: Option<&String>,
    overrides: atlassian_cli::CliOverrides,
) -> Result<()> {
    match cmd.subcommand {
        ConfigSubcommand::Init { global } => {
            let path = atlassian_cli::Config::init_config(global)?;
            println!("Created config file: {:?}", path);
            println!("Edit it and add your credentials.");
            Ok(())
        }
        ConfigSubcommand::Show => {
            // Respect --config, --profile, and CLI overrides for accurate "resolved" view.
            let config =
                atlassian_cli::Config::load_without_validation(config_path, profile, overrides)?;
            print_resolved_config(&config);
            Ok(())
        }
        ConfigSubcommand::List => {
            println!("Configuration files (in precedence order):\n");

            if let Some(global) = atlassian_cli::Config::global_config_path() {
                let status = if global.exists() { "✓" } else { "✗" };
                println!("Global:  {:?} {}", global, status);
            }

            if let Some(project) = atlassian_cli::Config::project_config_path() {
                println!("Project: {:?} ✓", project);
            } else {
                println!("Project: (none)");
            }

            println!("\nEnvironment variables:");
            let env_vars = [
                ("ATLASSIAN_DOMAIN", false),
                ("ATLASSIAN_AUTH_METHOD", false),
                ("ATLASSIAN_EMAIL", false),
                ("ATLASSIAN_API_TOKEN", true),
                ("ATLASSIAN_CLIENT_ID", false),
                ("ATLASSIAN_CLIENT_SECRET", true),
                ("ATLASSIAN_CLOUD_ID", false),
            ];
            for (key, mask) in env_vars {
                let value = std::env::var(key)
                    .ok()
                    .map(|v| if mask { "***".to_string() } else { v });
                println!(
                    "  {}: {}",
                    key,
                    value.unwrap_or_else(|| "(not set)".to_string())
                );
            }

            Ok(())
        }
        ConfigSubcommand::Path { global } => {
            let path = if global {
                atlassian_cli::Config::global_config_path()
            } else {
                atlassian_cli::Config::project_config_path()
                    .or_else(atlassian_cli::Config::global_config_path)
            };

            if let Some(p) = path {
                println!("{}", p.display());
            } else {
                anyhow::bail!("Config file not found");
            }
            Ok(())
        }
        ConfigSubcommand::Edit { global } => {
            let path = if global {
                atlassian_cli::Config::global_config_path()
            } else {
                atlassian_cli::Config::project_config_path()
                    .or_else(atlassian_cli::Config::global_config_path)
            };

            let path = path.ok_or_else(|| anyhow::anyhow!("Config file not found"))?;

            if !path.exists() {
                anyhow::bail!(
                    "Config file does not exist: {:?}\nRun 'atlassian-cli config init{}' to create it.",
                    path,
                    if global { " --global" } else { "" }
                );
            }

            let editor = std::env::var("EDITOR").unwrap_or_else(|_| {
                if cfg!(target_os = "macos") {
                    "open".to_string()
                } else if cfg!(target_os = "windows") {
                    "notepad".to_string()
                } else {
                    "vi".to_string()
                }
            });

            let status = std::process::Command::new(&editor).arg(&path).status()?;

            if !status.success() {
                anyhow::bail!("Failed to open editor");
            }

            println!("Config file edited: {:?}", path);
            Ok(())
        }
        ConfigSubcommand::Validate => {
            let config = atlassian_cli::Config::load(config_path, profile, overrides)?;

            // ApiClient::new() performs each strategy's credential check
            // (token fetch, cloud_id discovery, stored-token load). Any
            // failure here means credentials are invalid.
            let client = atlassian_cli::ApiClient::new(config).await?;
            let method = client.strategy().method();
            let identity = client.strategy().probe_identity(&client).await?;

            println!("✓ {} credentials valid", method);
            if let Some(domain) = client.config().domain.as_ref() {
                println!("  Domain: {}", domain);
            }
            if let Some(cid) = client.cloud_id() {
                println!("  Cloud ID: {}", cid);
            }
            match identity {
                Some(id) => {
                    println!("  User: {}", id.display_name);
                    if let Some(email) = id.email {
                        println!("  Email: {}", email);
                    }
                }
                None => {
                    // Non-probing principal (e.g. service_account) — credentials
                    // are already verified via the strategy's own check.
                    println!("  Identity: {}", client.strategy().identity_label());
                    println!(
                        "  Note: individual operations still require matching OAuth scopes and product permissions."
                    );
                }
            }
            Ok(())
        }
    }
}

async fn handle_jira(
    cmd: JiraCommand,
    client: &atlassian_cli::ApiClient,
) -> Result<serde_json::Value> {
    use atlassian_cli::jira;

    match cmd.subcommand {
        JiraSubcommand::Get { issue_key, format } => {
            let as_markdown = matches!(format, OutputFormat::Markdown);
            jira::get_issue(&issue_key, as_markdown, client).await
        }
        JiraSubcommand::Search {
            jql,
            limit,
            all,
            stream,
            fields,
            format,
        } => {
            if stream && !all {
                anyhow::bail!("--stream requires --all flag");
            }
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
        } => {
            let desc = description
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null);
            jira::create_issue(&project, &summary, &issue_type, desc, client).await
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
            } => jira::remove_link(&source, &target, link_type.as_deref(), client).await,
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

async fn handle_confluence(
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
            if stream && !all {
                anyhow::bail!("--stream requires --all flag");
            }
            let as_markdown = matches!(format, OutputFormat::Markdown);
            if all {
                confluence::search_all(&query, limit, None, expand, stream, as_markdown, client)
                    .await
            } else {
                confluence::search(&query, limit, None, expand, as_markdown, client).await
            }
        }
        ConfluenceSubcommand::Get { page_id, format } => {
            let as_markdown = matches!(format, OutputFormat::Markdown);
            confluence::get_page(&page_id, None, None, as_markdown, client).await
        }
        ConfluenceSubcommand::Create {
            space,
            title,
            content,
        } => confluence::create_page(&space, &title, &content, None, None, client).await,
        ConfluenceSubcommand::Update {
            page_id,
            title,
            content,
        } => confluence::update_page(&page_id, &title, &content, None, None, client).await,
        ConfluenceSubcommand::Children { page_id } => {
            confluence::get_page_children(&page_id, client).await
        }
        ConfluenceSubcommand::Comments { page_id, format } => {
            let as_markdown = matches!(format, OutputFormat::Markdown);
            confluence::get_comments(&page_id, as_markdown, client).await
        }
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

fn output_json(value: &serde_json::Value, pretty: bool) {
    // Null is a sentinel used by streaming commands that have already
    // written to stdout — emitting "null" would corrupt that output.
    if value.is_null() {
        return;
    }
    if pretty {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    } else {
        println!("{}", serde_json::to_string(value).unwrap());
    }
}

/// Print the resolved config as TOML for the active profile. Secrets are
/// masked via each `AuthConfig` variant's `display_lines`. Output is
/// copy-pasteable after replacing redactions with real secrets.
fn print_resolved_config(config: &atlassian_cli::Config) {
    let profile = &config.profile;
    println!("[{profile}]");
    match &config.domain {
        Some(d) => println!("domain = {:?}", d),
        None => println!("# domain = (not set)"),
    }

    println!();
    match &config.auth {
        Some(auth) => {
            println!("[{profile}.auth]");
            for line in auth.display_lines() {
                println!("{}", line);
            }
        }
        None => {
            println!("# [{profile}.auth] (not configured — set ATLASSIAN_AUTH_METHOD)");
        }
    }

    println!();
    println!("[{profile}.jira]");
    println!("projects_filter = {:?}", config.jira.projects_filter);
    if let Some(ref fields) = config.jira.search_default_fields {
        println!("search_default_fields = {:?}", fields);
    }
    if !config.jira.search_custom_fields.is_empty() {
        println!(
            "search_custom_fields = {:?}",
            config.jira.search_custom_fields
        );
    }

    println!();
    println!("[{profile}.confluence]");
    println!("spaces_filter = {:?}", config.confluence.spaces_filter);

    println!();
    println!("[{profile}.performance]");
    println!(
        "request_timeout_ms = {}",
        config.performance.request_timeout_ms
    );
    println!(
        "rate_limit_delay_ms = {}",
        config.performance.rate_limit_delay_ms
    );

    if let Some(ref excludes) = config.optimization.response_exclude_fields {
        println!();
        println!("[{profile}.optimization]");
        println!("response_exclude_fields = {:?}", excludes);
    }
}

async fn handle_auth(
    cmd: AuthCommand,
    config_path: Option<PathBuf>,
    profile: Option<String>,
    overrides: atlassian_cli::CliOverrides,
) -> Result<()> {
    use atlassian_cli::auth::{AuthMethod, OAuthStrategy, TokenStore};

    match cmd.subcommand {
        AuthSubcommand::Login { no_browser } => {
            // Validation-light load: the user is about to log in, so OAuth
            // tokens are absent and domain may be unset.
            let config = atlassian_cli::Config::load_without_validation(
                config_path.as_ref(),
                profile.as_ref(),
                overrides,
            )?;
            let params = config.oauth_params()?;
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()?;

            let outcome = OAuthStrategy::login(params, &config.profile, &http, !no_browser).await?;

            println!("✓ Logged in (profile: {})", config.profile);
            if let Some(cid) = outcome.tokens.cloud_id.as_deref() {
                println!("  Cloud ID: {}", cid);
            }
            if !outcome.authorized_sites.is_empty() {
                println!("  Accessible sites:");
                for site in &outcome.authorized_sites {
                    let name = site.name.as_deref().unwrap_or("");
                    println!("    - {} ({}) {}", site.url, site.id, name);
                }
            }
            println!("  Scopes: {}", outcome.tokens.scopes.join(", "));
            Ok(())
        }
        AuthSubcommand::Logout => {
            // Only proceed if the profile is OAuth — basic / service_account
            // have no stored session, and silently succeeding would mislead.
            let config = atlassian_cli::Config::load_without_validation(
                config_path.as_ref(),
                profile.as_ref(),
                overrides,
            )?;
            match config.auth.as_ref().map(|a| a.method()) {
                Some(AuthMethod::OAuth) => {
                    TokenStore::new(&config.profile)?.delete().await?;
                    println!("✓ OAuth tokens cleared for profile '{}'", config.profile);
                }
                Some(method) => println!(
                    "Profile '{}' uses '{}' auth — nothing to log out (no stored session).",
                    config.profile, method
                ),
                None => println!(
                    "Profile '{}' has no auth configured — nothing to log out.",
                    config.profile
                ),
            }
            Ok(())
        }
        AuthSubcommand::Status => {
            let profile_name = profile.as_deref().unwrap_or("default");
            let store = TokenStore::new(profile_name)?;
            match store.load().await? {
                Some(loaded) => {
                    let t = &loaded.tokens;
                    println!("✓ Logged in (profile: {})", profile_name);
                    println!("  Storage: {}", loaded.backend);
                    if let Some(cid) = &t.cloud_id {
                        println!("  Cloud ID: {}", cid);
                    }
                    println!("  Scopes: {}", t.scopes.join(", "));
                    let delta = t.seconds_until_expiry();
                    if delta > 0 {
                        println!("  Access token expires in: {}s ({}m)", delta, delta / 60);
                    } else {
                        println!("  Access token: EXPIRED ({}s ago)", -delta);
                    }
                    println!(
                        "  Refresh token: {}",
                        if t.refresh_token.is_some() {
                            "present"
                        } else {
                            "(none — re-login on expiry)"
                        }
                    );
                }
                None => println!(
                    "Not logged in (profile: {}). Run `atlassian-cli auth login`.",
                    profile_name
                ),
            }
            Ok(())
        }
        AuthSubcommand::Refresh => {
            let config =
                atlassian_cli::Config::load(config_path.as_ref(), profile.as_ref(), overrides)?;
            let params = config.oauth_params()?;
            let strategy = OAuthStrategy::resume(params, &config.profile).await?;
            let refreshed = strategy.force_refresh().await?;
            println!("✓ Token refreshed (profile: {})", config.profile);
            println!(
                "  Access token now expires in: {}s",
                refreshed.seconds_until_expiry()
            );
            Ok(())
        }
    }
}
