use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, Default, ValueEnum)]
pub enum OutputFormat {
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

    #[arg(long, help = "Pretty-print JSON output")]
    pretty: bool,

    #[arg(short, long, action = clap::ArgAction::Count, help = "Verbose logging")]
    verbose: u8,
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
        #[arg(
            long,
            value_enum,
            default_value = "html",
            help = "Output format for description/comments"
        )]
        format: OutputFormat,
    },
    Search {
        jql: String,
        #[arg(long, default_value = "20")]
        limit: u32,
        #[arg(long, value_delimiter = ',')]
        fields: Option<Vec<String>>,
        #[arg(
            long,
            value_enum,
            default_value = "html",
            help = "Output format for description/comments"
        )]
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
        #[arg(long, default_value = "10")]
        limit: u32,
        #[arg(long, help = "Fetch all pages using cursor-based pagination")]
        all: bool,
        #[arg(long, help = "Stream results as JSONL (requires --all)")]
        stream: bool,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Fields to expand (e.g., body.storage,ancestors)"
        )]
        expand: Option<Vec<String>>,
        #[arg(
            long,
            value_enum,
            default_value = "html",
            help = "Output format for body content"
        )]
        format: OutputFormat,
    },
    Get {
        page_id: String,
        #[arg(
            long,
            value_enum,
            default_value = "html",
            help = "Output format for body content"
        )]
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
    },
}

#[derive(Parser)]
struct ConfigCommand {
    #[command(subcommand)]
    subcommand: ConfigSubcommand,
}

#[derive(Subcommand)]
enum ConfigSubcommand {
    Init {
        #[arg(long)]
        global: bool,
    },
    Show,
    List,
    Edit {
        #[arg(long)]
        global: bool,
    },
    Path {
        #[arg(long)]
        global: bool,
    },
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

    match cli.command {
        Command::Config(cmd) => handle_config(cmd).await,
        Command::Jira(cmd) => {
            let config = atlassian_cli::Config::load(
                cli.config.as_ref(),
                cli.profile.as_ref(),
                cli.domain,
                cli.email,
                cli.token,
            )?;

            let result = handle_jira(cmd, &config).await?;
            output_json(&result, cli.pretty);
            Ok(())
        }
        Command::Confluence(cmd) => {
            let config = atlassian_cli::Config::load(
                cli.config.as_ref(),
                cli.profile.as_ref(),
                cli.domain,
                cli.email,
                cli.token,
            )?;

            let result = handle_confluence(cmd, &config).await?;
            output_json(&result, cli.pretty);
            Ok(())
        }
    }
}

async fn handle_config(cmd: ConfigCommand) -> Result<()> {
    match cmd.subcommand {
        ConfigSubcommand::Init { global } => {
            let path = atlassian_cli::Config::init_config(global)?;
            println!("Created config file: {:?}", path);
            println!("Edit it and add your credentials.");
            Ok(())
        }
        ConfigSubcommand::Show => {
            let config =
                atlassian_cli::Config::load_without_validation(None, None, None, None, None)?;

            // Display credentials (not in Config struct's TOML serialization)
            println!("[default]");
            if let Some(ref domain) = config.domain {
                println!("domain = {:?}", domain);
            } else {
                println!("# domain = (not set)");
            }
            if let Some(ref email) = config.email {
                println!("email = {:?}", email);
            } else {
                println!("# email = (not set)");
            }
            if let Some(ref token) = config.token {
                let mask_len = 4.min(token.len());
                println!("token = \"{}***\"", &token[..mask_len]);
            } else {
                println!("# token = (not set)");
            }
            println!();

            // Display rest of config via TOML serialization
            let toml_str = toml::to_string_pretty(&config)?;
            // Skip the empty [default] section if present
            for line in toml_str.lines() {
                if line.trim().is_empty() || line.trim() == "[default]" {
                    continue;
                }
                println!("{}", line);
            }
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
            for (key, value) in [
                ("ATLASSIAN_DOMAIN", std::env::var("ATLASSIAN_DOMAIN").ok()),
                ("ATLASSIAN_EMAIL", std::env::var("ATLASSIAN_EMAIL").ok()),
                (
                    "ATLASSIAN_API_TOKEN",
                    std::env::var("ATLASSIAN_API_TOKEN")
                        .ok()
                        .map(|_| "***".to_string()),
                ),
            ] {
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
                // Try project config first, fall back to global
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
                // Try project config first, fall back to global
                atlassian_cli::Config::project_config_path()
                    .or_else(atlassian_cli::Config::global_config_path)
            };

            let path = path.ok_or_else(|| anyhow::anyhow!("Config file not found"))?;

            if !path.exists() {
                anyhow::bail!(
                    "Config file does not exist: {:?}\nRun 'atlassian config init{}' to create it.",
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
    }
}

async fn handle_jira(
    cmd: JiraCommand,
    config: &atlassian_cli::Config,
) -> Result<serde_json::Value> {
    use atlassian_cli::jira;

    match cmd.subcommand {
        JiraSubcommand::Get { issue_key, format } => {
            let as_markdown = matches!(format, OutputFormat::Markdown);
            jira::get_issue(&issue_key, as_markdown, config).await
        }
        JiraSubcommand::Search {
            jql,
            limit,
            fields,
            format,
        } => {
            let as_markdown = matches!(format, OutputFormat::Markdown);
            jira::search(&jql, limit, fields, as_markdown, config).await
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
            jira::create_issue(&project, &summary, &issue_type, desc, config).await
        }
        JiraSubcommand::Update { issue_key, fields } => {
            let fields_value: serde_json::Value = serde_json::from_str(&fields)?;
            jira::update_issue(&issue_key, fields_value, config).await
        }
        JiraSubcommand::Comment { action } => match action {
            CommentAction::Add { issue_key, text } => {
                jira::add_comment(&issue_key, serde_json::Value::String(text), config).await
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
                    config,
                )
                .await
            }
        },
        JiraSubcommand::Transition {
            issue_key,
            transition_id,
        } => jira::transition_issue(&issue_key, &transition_id, config).await,
        JiraSubcommand::Transitions { issue_key } => {
            jira::get_transitions(&issue_key, config).await
        }
    }
}

async fn handle_confluence(
    cmd: ConfluenceCommand,
    config: &atlassian_cli::Config,
) -> Result<serde_json::Value> {
    use atlassian_cli::confluence;

    let as_markdown = matches!(
        cmd.subcommand,
        ConfluenceSubcommand::Search {
            format: OutputFormat::Markdown,
            ..
        } | ConfluenceSubcommand::Get {
            format: OutputFormat::Markdown,
            ..
        }
    );

    match cmd.subcommand {
        ConfluenceSubcommand::Search {
            query,
            limit,
            all,
            stream,
            expand,
            format: _,
        } => {
            if all {
                confluence::search_all(&query, None, expand, stream, as_markdown, config).await
            } else {
                confluence::search(&query, limit, None, expand, as_markdown, config).await
            }
        }
        ConfluenceSubcommand::Get { page_id, format: _ } => {
            confluence::get_page(&page_id, None, None, as_markdown, config).await
        }
        ConfluenceSubcommand::Create {
            space,
            title,
            content,
        } => confluence::create_page(&space, &title, &content, None, None, config).await,
        ConfluenceSubcommand::Update {
            page_id,
            title,
            content,
        } => confluence::update_page(&page_id, &title, &content, None, None, config).await,
        ConfluenceSubcommand::Children { page_id } => {
            confluence::get_page_children(&page_id, None, None, config).await
        }
        ConfluenceSubcommand::Comments { page_id } => {
            confluence::get_comments(&page_id, None, None, config).await
        }
    }
}

fn output_json(value: &serde_json::Value, pretty: bool) {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    } else {
        println!("{}", serde_json::to_string(value).unwrap());
    }
}
