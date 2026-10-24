use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// How Jira rich-text fields are written and read on the command line.
#[derive(Clone, Copy, Default, ValueEnum)]
pub(crate) enum JiraFormat {
    /// Atlassian Document Format JSON, as Jira stores it; writes also take plain text
    #[default]
    #[value(alias = "html")]
    Adf,
    /// Markdown, converted to and from ADF
    Markdown,
}

/// How Confluence page and comment bodies are written and read on the command line.
#[derive(Clone, Copy, Default, ValueEnum)]
pub(crate) enum ConfluenceFormat {
    /// Storage format, the XHTML Confluence stores
    #[default]
    #[value(alias = "html")]
    Storage,
    /// Markdown, converted to and from storage format
    Markdown,
}

#[derive(Parser)]
#[command(name = "atlassian-cli", version, about = "CLI for Atlassian Jira and Confluence", long_about = None)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,

    #[arg(long, help = "Config file path")]
    pub(crate) config: Option<PathBuf>,

    #[arg(long, help = "Profile name")]
    pub(crate) profile: Option<String>,

    #[arg(long, env = "ATLASSIAN_DOMAIN", hide_env_values = true)]
    pub(crate) domain: Option<String>,

    #[arg(long, env = "ATLASSIAN_EMAIL", hide_env_values = true)]
    pub(crate) email: Option<String>,

    #[arg(long, env = "ATLASSIAN_API_TOKEN", hide_env_values = true)]
    pub(crate) token: Option<String>,

    #[arg(long, env = "ATLASSIAN_CLIENT_ID", hide_env_values = true)]
    pub(crate) client_id: Option<String>,

    #[arg(long, env = "ATLASSIAN_CLIENT_SECRET", hide_env_values = true)]
    pub(crate) client_secret: Option<String>,

    #[arg(long, env = "ATLASSIAN_CLOUD_ID", hide_env_values = true)]
    pub(crate) cloud_id: Option<String>,

    #[arg(long, help = "Pretty-print JSON output")]
    pub(crate) pretty: bool,

    #[arg(short, long, action = clap::ArgAction::Count, help = "Verbose logging")]
    pub(crate) verbose: u8,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    #[command(about = "Issues, comments, links, worklogs, watchers, and agile boards")]
    Jira(JiraCommand),
    #[command(about = "Pages, comment threads, labels, properties, spaces, and attachments")]
    Confluence(ConfluenceCommand),
    #[command(about = "Create, inspect, or validate the configuration this tool reads")]
    Config(ConfigCommand),
    #[command(about = "Sign in, sign out, and report or refresh the stored session")]
    Auth(AuthCommand),
    #[command(name = "self", about = "Inspect, update, or remove this installation")]
    Selfcmd(SelfCommand),
    #[command(about = "Generate a shell completion script on stdout")]
    Completions {
        #[arg(value_enum, help = "Target shell")]
        shell: clap_complete::Shell,
    },
}

#[derive(Parser)]
pub(crate) struct SelfCommand {
    #[command(subcommand)]
    pub(crate) subcommand: SelfSubcommand,
}

#[derive(Subcommand)]
pub(crate) enum SelfSubcommand {
    /// Report what this installation is and where its files are
    Status,
    /// Replace this binary with a published release
    Update {
        /// Install this version instead of the latest. Named explicitly, so it may go back.
        #[arg(long, value_name = "VER")]
        version: Option<String>,
        /// Replace the binary even when it already reports the target version
        #[arg(long)]
        force: bool,
        /// Additionally hold the archive to the build provenance the release attested
        #[arg(long)]
        verify_attestations: bool,
    },
    /// Install or remove the Claude Code skill this binary carries
    Skill {
        #[command(subcommand)]
        action: SelfSkillAction,
    },
    /// Remove this binary, the skill it deployed, and the tokens it stored
    Uninstall {
        /// Confirm the removal
        #[arg(long)]
        yes: bool,
        /// Leave the deployed skill in place
        #[arg(long)]
        keep_skill: bool,
        /// Leave stored OAuth tokens in the keychain and the credentials file
        #[arg(long)]
        keep_credentials: bool,
        /// Also remove the global configuration directory
        #[arg(long)]
        purge_config: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum SelfSkillAction {
    /// Write the skill this binary carries into ~/.claude/skills
    Install,
    /// Remove the deployed skill
    Remove {
        /// Confirm the removal
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Parser)]
pub(crate) struct AuthCommand {
    #[command(subcommand)]
    pub(crate) subcommand: AuthSubcommand,
}

#[derive(Subcommand)]
pub(crate) enum AuthSubcommand {
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
pub(crate) struct JiraCommand {
    #[command(subcommand)]
    pub(crate) subcommand: JiraSubcommand,
}

#[derive(Subcommand)]
pub(crate) enum JiraSubcommand {
    /// Fetch a single issue by key (fields filtered, ADF rendered)
    Get {
        issue_key: String,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Fields to return (default: essential set + configured custom fields; use *all for the full issue)"
        )]
        fields: Option<Vec<String>>,
        /// Format of rich-text fields in the output
        #[arg(long, value_enum, default_value_t)]
        format: JiraFormat,
    },
    /// Search issues with JQL; the configured project filter is auto-injected
    Search {
        jql: String,
        #[arg(
            long,
            default_value = "100",
            help = "Results per page without --all; --all requests 100 per page"
        )]
        limit: u32,
        #[arg(long, help = "Fetch all results via token pagination")]
        all: bool,
        #[arg(long, requires = "all", help = "Stream as JSONL (requires --all)")]
        stream: bool,
        #[arg(long, value_delimiter = ',', help = "Fields to return")]
        fields: Option<Vec<String>>,
        /// Format of rich-text fields in the output; markdown also returns the description
        #[arg(long, value_enum, default_value_t)]
        format: JiraFormat,
    },
    /// Create an issue
    Create {
        project: String,
        summary: String,
        issue_type: String,
        /// Issue description; `-` reads it from standard input
        #[arg(long, allow_hyphen_values = true)]
        description: Option<String>,
        /// Parent issue key — required by every sub-task type, and how a
        /// team-managed project puts an issue under its epic
        #[arg(long)]
        parent: Option<String>,
        /// Any other field the create screen requires, as a JSON object —
        /// '{"components":[{"name":"api"}],"customfield_10010":"X"}'
        #[arg(long)]
        fields: Option<String>,
        /// Format of the description, given with --description or in --fields
        #[arg(long, value_enum, default_value_t)]
        format: JiraFormat,
    },
    /// Update an issue's fields from a JSON object (e.g. '{"summary":"..."}')
    Update {
        issue_key: String,
        /// Field values as a JSON object; `-` reads it from standard input
        fields: String,
        /// Format of a string `description` among the fields
        #[arg(long, value_enum, default_value_t)]
        format: JiraFormat,
    },
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
    /// Find users and the account ids that assignee fields, JQL and CQL take
    User {
        #[command(subcommand)]
        action: UserAction,
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
pub(crate) enum CommentAction {
    /// Add a comment to an issue
    Add {
        issue_key: String,
        /// Comment text; `-` reads it from standard input
        #[arg(allow_hyphen_values = true)]
        text: String,
        /// Format of the comment text
        #[arg(long, value_enum, default_value_t)]
        format: JiraFormat,
    },
    /// Update an existing comment
    Update {
        issue_key: String,
        comment_id: String,
        /// Comment text; `-` reads it from standard input
        #[arg(allow_hyphen_values = true)]
        text: String,
        /// Format of the comment text
        #[arg(long, value_enum, default_value_t)]
        format: JiraFormat,
    },
    /// List comments on an issue
    List {
        issue_key: String,
        /// Format of comment bodies in the output
        #[arg(long, value_enum, default_value_t)]
        format: JiraFormat,
    },
    /// Delete a comment by id
    Delete {
        issue_key: String,
        comment_id: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum TransitionAction {
    /// Apply a transition to an issue
    Apply {
        issue_key: String,
        transition_id: String,
    },
    /// List available transitions for an issue
    List { issue_key: String },
}

#[derive(Subcommand)]
pub(crate) enum LinkAction {
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
    /// Remove a link — by the two issues it runs between, or by its own id
    Remove {
        /// Source issue key
        #[arg(required_unless_present = "id", conflicts_with = "id")]
        source: Option<String>,
        /// Target issue key
        #[arg(required_unless_present = "id", conflicts_with = "id")]
        target: Option<String>,
        /// Link type (required when multiple link types exist between the pair)
        #[arg(long = "type", conflicts_with = "id")]
        link_type: Option<String>,
        /// Remove the link with this id, as `jira link list` reports it
        #[arg(long)]
        id: Option<String>,
    },
    /// List links on an issue
    List {
        /// Issue key
        issue_key: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum WorklogAction {
    /// Add a worklog entry to an issue
    Add {
        /// Issue key
        issue_key: String,
        /// Time spent (e.g., "2h 30m", "1d", "45m")
        time_spent: String,
        /// Comment describing the work
        #[arg(long)]
        comment: Option<String>,
        /// When the work started, in the shape Jira takes it —
        /// 2026-08-22T09:30:00.000+0900, with the milliseconds and an offset
        /// carrying no colon. Omitted, Jira records now
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
pub(crate) enum WatcherAction {
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
pub(crate) enum UserAction {
    /// Search users whose display name or email matches the query
    Search {
        query: String,
        /// Maximum number of users to return
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
}

#[derive(Subcommand)]
pub(crate) enum ListAction {
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
pub(crate) enum BoardAction {
    /// List boards for a project
    List {
        /// Project key or ID
        #[arg(long)]
        project: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum SprintAction {
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
pub(crate) enum EpicAction {
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
pub(crate) struct ConfluenceCommand {
    #[command(subcommand)]
    pub(crate) subcommand: ConfluenceSubcommand,
}

#[derive(Subcommand)]
pub(crate) enum ConfluenceSubcommand {
    /// Search pages with CQL; the configured space filter is auto-injected
    Search {
        query: String,
        #[arg(
            long,
            default_value = "50",
            help = "Results per page (capped at 50 by the body-expanding search API). With --all, controls first-page batch size"
        )]
        limit: u32,
        #[arg(long, help = "Fetch all results via cursor pagination")]
        all: bool,
        #[arg(long, requires = "all", help = "Stream as JSONL (requires --all)")]
        stream: bool,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Expand fields (e.g., body.storage,ancestors)"
        )]
        expand: Option<Vec<String>>,
        /// Format of page bodies in the output
        #[arg(long, value_enum, default_value_t)]
        format: ConfluenceFormat,
    },
    /// Fetch a single page by ID
    Get {
        page_id: String,
        /// Format of the page body in the output
        #[arg(long, value_enum, default_value_t)]
        format: ConfluenceFormat,
    },
    /// Create a page
    Create {
        space: String,
        title: String,
        /// Page body; `-` reads it from standard input
        #[arg(allow_hyphen_values = true)]
        content: String,
        /// Parent page id to nest under (omit to create at the space root)
        #[arg(long)]
        parent: Option<String>,
        /// Format of the page body
        #[arg(long, value_enum, default_value_t)]
        format: ConfluenceFormat,
    },
    /// Replace a page's title and body
    Update {
        page_id: String,
        title: String,
        /// Page body; `-` reads it from standard input
        #[arg(allow_hyphen_values = true)]
        content: String,
        /// Parent page id to re-parent under (omit to keep the current parent)
        #[arg(long)]
        parent: Option<String>,
        /// Format of the page body
        #[arg(long, value_enum, default_value_t)]
        format: ConfluenceFormat,
        /// Version the new body was derived from, as `get` reported it; the
        /// update is refused when the page has been saved since
        #[arg(long, value_name = "VERSION")]
        base_version: Option<u64>,
    },
    /// List a page's direct children: pages, folders, whiteboards, databases and embeds
    Children { page_id: String },
    /// Read a page's comments and threads, or write footer comments
    Comment {
        #[command(subcommand)]
        action: ConfluenceCommentAction,
    },
    /// List, add, or remove labels on a page
    Label {
        #[command(subcommand)]
        action: ConfluenceLabelAction,
    },
    /// List, set, or delete content properties (structured JSON metadata) on a page
    Property {
        #[command(subcommand)]
        action: ConfluencePropertyAction,
    },
    /// List spaces, or fetch a single space by key
    Space {
        #[command(subcommand)]
        action: ConfluenceSpaceAction,
    },
    /// List attachments on a page, or upload a file
    Attachment {
        #[command(subcommand)]
        action: ConfluenceAttachmentAction,
    },
    /// Move a page to the trash (recoverable — requires --yes)
    Delete {
        page_id: String,
        /// Confirm the deletion
        #[arg(long)]
        yes: bool,
    },
}

/// Which of Confluence's two comment families an id or a listing refers to.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum CommentLocation {
    Footer,
    Inline,
}

impl From<CommentLocation> for atlassian_cli::confluence::CommentFamily {
    fn from(location: CommentLocation) -> Self {
        use atlassian_cli::confluence::CommentFamily;
        match location {
            CommentLocation::Footer => CommentFamily::Footer,
            CommentLocation::Inline => CommentFamily::Inline,
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum ConfluenceCommentAction {
    /// List comments on a page, replies included
    List {
        page_id: String,
        #[arg(
            long,
            value_enum,
            help = "Comment family to list (default: both footer and inline)"
        )]
        location: Option<CommentLocation>,
        /// List only top-level comments, leaving their replies unfetched
        #[arg(long)]
        roots_only: bool,
        /// Format of comment bodies in the output
        #[arg(long, value_enum, default_value_t)]
        format: ConfluenceFormat,
    },
    /// Fetch a single comment by id
    Get {
        comment_id: String,
        #[arg(
            long,
            value_enum,
            default_value = "footer",
            help = "Comment family the id belongs to"
        )]
        location: CommentLocation,
        /// Format of the comment body in the output
        #[arg(long, value_enum, default_value_t)]
        format: ConfluenceFormat,
    },
    /// List every reply below a comment
    Replies {
        comment_id: String,
        #[arg(
            long,
            value_enum,
            default_value = "footer",
            help = "Comment family the id belongs to"
        )]
        location: CommentLocation,
        /// Format of reply bodies in the output
        #[arg(long, value_enum, default_value_t)]
        format: ConfluenceFormat,
    },
    /// Add a footer comment to a page
    Add {
        page_id: String,
        /// Comment body; `-` reads it from standard input
        #[arg(allow_hyphen_values = true)]
        body: String,
        /// Reply to an existing comment instead of posting a top-level one
        #[arg(long = "reply-to")]
        reply_to: Option<String>,
        /// Format of the comment body
        #[arg(long, value_enum, default_value_t)]
        format: ConfluenceFormat,
    },
    /// Update a footer comment's body
    Update {
        comment_id: String,
        /// New comment body; `-` reads it from standard input
        #[arg(allow_hyphen_values = true)]
        body: String,
        /// Format of the comment body
        #[arg(long, value_enum, default_value_t)]
        format: ConfluenceFormat,
    },
    /// Delete a footer comment by id
    Delete { comment_id: String },
}

#[derive(Subcommand)]
pub(crate) enum ConfluenceLabelAction {
    /// List labels on a page
    List { page_id: String },
    /// Add a label to a page
    Add { page_id: String, label: String },
    /// Remove a label from a page
    Remove { page_id: String, label: String },
}

#[derive(Subcommand)]
pub(crate) enum ConfluencePropertyAction {
    /// List content properties on a page
    List { page_id: String },
    /// Create or update a content property (value is a JSON literal)
    Set {
        page_id: String,
        key: String,
        /// Property value as a JSON literal (e.g. '{"state":"done"}', '42', '"text"');
        /// `-` reads it from standard input
        value: String,
    },
    /// Delete a content property by key
    Delete { page_id: String, key: String },
}

#[derive(Subcommand)]
pub(crate) enum ConfluenceSpaceAction {
    /// List spaces visible to you
    List,
    /// Get a single space by key
    Get { space_key: String },
}

#[derive(Subcommand)]
pub(crate) enum ConfluenceAttachmentAction {
    /// List attachments on a page
    List { page_id: String },
    /// Upload a local file as an attachment (creates, or versions by filename)
    Upload {
        page_id: String,
        /// Path to the local file to upload
        file: String,
        /// Optional version comment recorded with the upload
        #[arg(long)]
        comment: Option<String>,
        /// Mark as a minor edit (suppresses watcher notifications on re-upload)
        #[arg(long)]
        minor: bool,
        /// Override the Content-Type (default: mapped from the file extension)
        #[arg(long = "content-type")]
        content_type: Option<String>,
    },
}

#[derive(Parser)]
pub(crate) struct ConfigCommand {
    #[command(subcommand)]
    pub(crate) subcommand: ConfigSubcommand,
}

#[derive(Subcommand)]
pub(crate) enum ConfigSubcommand {
    /// Create a starter config at --config, the global or project location.
    Init {
        #[arg(
            long,
            help = "Write the global config instead of --config or ./.atlassian.toml"
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
            help = "Edit the global config instead of --config or the project config"
        )]
        global: bool,
    },
    /// Print the path of the active config file.
    Path {
        #[arg(
            long,
            help = "Print the global config path instead of --config or the project config"
        )]
        global: bool,
    },
    /// Validate configured credentials against Atlassian auth/API endpoints.
    Validate,
}
