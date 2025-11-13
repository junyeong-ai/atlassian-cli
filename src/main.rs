use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "atlassian", version, about = "CLI for Atlassian Jira and Confluence", long_about = None)]
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
    },
    Search {
        jql: String,
        #[arg(long, default_value = "20")]
        limit: u32,
        #[arg(long, value_delimiter = ',')]
        fields: Option<Vec<String>>,
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
    },
    Get {
        page_id: String,
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
                cli.domain.as_ref(),
                cli.email.as_ref(),
                cli.token.as_ref(),
            )?;

            let result = handle_jira(cmd, &config).await?;
            output_json(&result, cli.pretty);
            Ok(())
        }
        Command::Confluence(cmd) => {
            let config = atlassian_cli::Config::load(
                cli.config.as_ref(),
                cli.profile.as_ref(),
                cli.domain.as_ref(),
                cli.email.as_ref(),
                cli.token.as_ref(),
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
            let config = atlassian_cli::Config::load(None, None, None, None, None)?;

            let mut display = config.clone();
            if let Some(ref token) = display.token {
                let mask_len = 4.min(token.len());
                display.token = Some(format!("{}***", &token[..mask_len]));
            }

            println!("{}", toml::to_string_pretty(&display)?);
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
                atlassian_cli::Config::project_config_path()
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
        JiraSubcommand::Get { issue_key } => jira::get_issue(&issue_key, config).await,
        JiraSubcommand::Search { jql, limit, fields } => {
            jira::search(&jql, limit, fields, config).await
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

    match cmd.subcommand {
        ConfluenceSubcommand::Search { query, limit } => {
            confluence::search(&query, limit, None, None, config).await
        }
        ConfluenceSubcommand::Get { page_id } => {
            confluence::get_page(&page_id, None, None, config).await
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
