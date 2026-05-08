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
}

#[derive(Parser)]
struct JiraCommand {
    #[command(subcommand)]
    subcommand: JiraSubcommand,
}

#[derive(Subcommand)]
enum JiraSubcommand {
    Get {
        issue_key: String,
        #[arg(long, value_enum, default_value = "html", help = "ADF content format")]
        format: OutputFormat,
    },
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
    Create {
        project: String,
        summary: String,
        issue_type: String,
        #[arg(long)]
        description: Option<String>,
    },
    Update {
        issue_key: String,
        fields: String,
    },
    Comment {
        #[command(subcommand)]
        action: CommentAction,
    },
    Transition {
        issue_key: String,
        transition_id: String,
    },
    Transitions {
        issue_key: String,
    },
    Comments {
        issue_key: String,
        #[arg(long, value_enum, default_value = "html", help = "ADF content format")]
        format: OutputFormat,
    },
}

#[derive(Subcommand)]
enum CommentAction {
    Add {
        issue_key: String,
        text: String,
    },
    Update {
        issue_key: String,
        comment_id: String,
        text: String,
    },
}

#[derive(Parser)]
struct ConfluenceCommand {
    #[command(subcommand)]
    subcommand: ConfluenceSubcommand,
}

#[derive(Subcommand)]
enum ConfluenceSubcommand {
    Search {
        query: String,
        #[arg(
            long,
            default_value = "10",
            help = "Results per page (max 250). With --all, controls batch size"
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
    Get {
        page_id: String,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
    },
    Create {
        space: String,
        title: String,
        content: String,
    },
    Update {
        page_id: String,
        title: String,
        content: String,
    },
    Children {
        page_id: String,
    },
    Comments {
        page_id: String,
        #[arg(long, value_enum, default_value = "html", help = "Body content format")]
        format: OutputFormat,
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

            // ApiClient::new() performs:
            //   - Basic: credential encoding (offline)
            //   - Service account: token fetch + cloud_id discovery (online)
            // Any failure here means credentials are invalid.
            let client = atlassian_cli::ApiClient::new(config).await?;

            if client.is_service_account() {
                // service account credentials already verified via token fetch and
                // accessible-resources call. Additional /myself may fail due
                // to scope mismatch (e.g. read:jira-work but not read:jira-user),
                // which doesn't indicate a credential problem.
                println!("✓ service account credentials and cloud access valid");
                if let Some(cid) = client.cloud_id() {
                    println!("  Cloud ID: {}", cid);
                }
                println!(
                    "  Note: individual Jira/Confluence operations still require matching OAuth scopes and product permissions."
                );
            } else {
                // Basic auth: call /myself to show user info and verify token.
                let response = client
                    .get(atlassian_cli::Service::Jira, "/rest/api/3/myself")
                    .await?
                    .header("Accept", "application/json")
                    .send()
                    .await?;

                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.text().await.unwrap_or_default();
                    anyhow::bail!("Authentication failed ({}): {}", status, body);
                }

                let data: serde_json::Value = response.json().await?;
                println!("✓ Basic auth credentials valid");
                if let Some(domain) = client.config().domain.as_ref() {
                    println!("  Domain: {}", domain);
                }
                println!(
                    "  User: {}",
                    data["displayName"].as_str().unwrap_or("Unknown")
                );
                println!(
                    "  Email: {}",
                    data["emailAddress"].as_str().unwrap_or("Unknown")
                );
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
        },
        JiraSubcommand::Transition {
            issue_key,
            transition_id,
        } => jira::transition_issue(&issue_key, &transition_id, client).await,
        JiraSubcommand::Transitions { issue_key } => {
            jira::get_transitions(&issue_key, client).await
        }
        JiraSubcommand::Comments { issue_key, format } => {
            let as_markdown = matches!(format, OutputFormat::Markdown);
            jira::get_comments(&issue_key, as_markdown, client).await
        }
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
                confluence::search_all(&query, None, expand, stream, as_markdown, client).await
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

/// Mask a secret by showing only the first 4 characters.
/// Handles short secrets safely (returns "***" if length < 4).
fn mask_secret(secret: &str) -> String {
    if secret.len() < 4 {
        "***".to_string()
    } else {
        format!("{}***", &secret[..4])
    }
}

/// Print the resolved config in valid TOML profile format.
/// Secrets are masked; empty/unspecified sections are omitted.
/// The output is copy-pasteable into a config file (after unmasking secrets).
fn print_resolved_config(config: &atlassian_cli::Config) {
    use atlassian_cli::AuthConfig;

    println!("[default]");
    match &config.domain {
        Some(d) => println!("domain = {:?}", d),
        None => println!("# domain = (not set)"),
    }

    println!();
    match &config.auth {
        Some(AuthConfig::Basic { email, token }) => {
            println!("[default.auth]");
            println!("method = \"basic\"");
            println!("email = {:?}", email);
            if token.is_empty() {
                println!("# token = (not set — provide via ATLASSIAN_API_TOKEN)");
            } else {
                println!("token = \"{}\"", mask_secret(token));
            }
        }
        Some(AuthConfig::ServiceAccount {
            client_id,
            client_secret,
            cloud_id,
        }) => {
            println!("[default.auth]");
            println!("method = \"service_account\"");
            println!("client_id = {:?}", client_id);
            if client_secret.is_empty() {
                println!("# client_secret = (not set — provide via ATLASSIAN_CLIENT_SECRET)");
            } else {
                println!("client_secret = \"{}\"", mask_secret(client_secret));
            }
            if let Some(cid) = cloud_id {
                println!("cloud_id = {:?}", cid);
            } else {
                println!("# cloud_id = (will be auto-discovered)");
            }
        }
        None => {
            println!("# [default.auth] (not configured — set ATLASSIAN_AUTH_METHOD)");
        }
    }

    println!();
    println!("[default.jira]");
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
    println!("[default.confluence]");
    println!("spaces_filter = {:?}", config.confluence.spaces_filter);

    println!();
    println!("[default.performance]");
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
        println!("[default.optimization]");
        println!("response_exclude_fields = {:?}", excludes);
    }
}
