mod client;
mod config;
mod tui;
mod ws;

use anyhow::Result;
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use gyre_common::WsMessage;
use tokio_tungstenite::tungstenite::Message;
use tracing::info;

const DEFAULT_SERVER: &str = "ws://localhost:3000/ws";
const DEFAULT_TOKEN: &str = "gyre-dev-token";

#[derive(Parser)]
#[command(name = "gyre", about = "Gyre platform CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Connect to the Gyre server and stay connected
    Connect {
        #[arg(long, default_value = DEFAULT_SERVER)]
        server: String,
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Send a ping and print the round-trip time
    Ping {
        #[arg(long, default_value = DEFAULT_SERVER)]
        server: String,
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Check server health via HTTP
    Health {
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
    },
    /// Launch the TUI dashboard
    Tui {
        #[arg(long, default_value = DEFAULT_SERVER)]
        server: String,
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Register this CLI as a Gyre agent and save credentials to ~/.gyre/config
    Init {
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Agent name to register
        #[arg(long)]
        name: String,
        /// Use this token to authenticate the registration call (dev/system token)
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Clone a Gyre-hosted repository
    Clone {
        /// Repository in "project/repo" format, or a full Gyre git URL
        repo: String,
        /// Local directory to clone into (default: repo name)
        #[arg(long)]
        dir: Option<String>,
    },
    /// Push current branch to the Gyre server
    Push {
        /// Git remote name (default: origin)
        #[arg(long, default_value = "origin")]
        remote: String,
    },
    /// Merge request operations
    Mr {
        #[command(subcommand)]
        command: MrCommands,
    },
    /// Task operations
    Tasks {
        #[command(subcommand)]
        command: TaskCommands,
    },
    /// Show this agent's status and current task
    Status,
    /// Release automation: compute next version and generate changelog
    Release {
        #[command(subcommand)]
        command: ReleaseCommands,
    },
}

#[derive(Subcommand)]
enum ReleaseCommands {
    /// Compute next semver version and generate changelog from conventional commits
    Prepare {
        /// Repository ID to analyze
        #[arg(long)]
        repo_id: String,
        /// Branch to analyze (default: repo's default branch)
        #[arg(long)]
        branch: Option<String>,
        /// Override "from" tag/ref for changelog range
        #[arg(long)]
        from: Option<String>,
        /// Create a release MR after computing the changelog
        #[arg(long)]
        create_mr: bool,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
        /// Output changelog markdown to stdout instead of summary
        #[arg(long)]
        markdown: bool,
    },
}

#[derive(Subcommand)]
enum MrCommands {
    /// Create a merge request for the current branch
    Create {
        /// MR title
        #[arg(long)]
        title: String,
        /// Target branch (default: main)
        #[arg(long, default_value = "main")]
        target: String,
        /// Repository ID (required)
        #[arg(long)]
        repo_id: String,
        /// Source branch (default: current git branch)
        #[arg(long)]
        source: Option<String>,
    },
}

#[derive(Subcommand)]
enum TaskCommands {
    /// List tasks
    List {
        /// Filter by status (backlog, in_progress, review, done, blocked)
        #[arg(long)]
        status: Option<String>,
        /// Only show tasks assigned to me
        #[arg(long)]
        mine: bool,
    },
    /// Assign a task to this agent and mark it in_progress
    Take {
        /// Task ID
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Connect { server, token } => {
            info!("Connecting to {server}");
            let client = ws::WsClient::new(server.clone(), token);
            let mut ws = client.connect_and_auth().await?;
            println!("Connected to {server}. Listening for messages (Ctrl-C to quit)...");
            while let Some(frame) = ws.next().await {
                match frame? {
                    Message::Text(text) => {
                        let msg: Result<WsMessage, _> = serde_json::from_str(&text);
                        match msg {
                            Ok(m) => println!("{m:?}"),
                            Err(_) => println!("Raw: {text}"),
                        }
                    }
                    Message::Close(_) => {
                        println!("Server closed connection");
                        break;
                    }
                    _ => {}
                }
            }
        }

        Commands::Ping { server, token } => {
            info!("Pinging {server}");
            let client = ws::WsClient::new(server.clone(), token);
            let mut ws = client.connect_and_auth().await?;
            let rtt = client.ping(&mut ws).await?;
            println!("Pong from {server}: RTT {rtt}ms");
        }

        Commands::Health { server } => {
            let url = if server.starts_with("ws://") {
                server.replacen("ws://", "http://", 1)
            } else if server.starts_with("wss://") {
                server.replacen("wss://", "https://", 1)
            } else {
                server
            };
            let health_url = format!("{url}/health");
            info!("Checking health at {health_url}");
            let resp = reqwest::get(&health_url)
                .await
                .map_err(|e| anyhow::anyhow!("HTTP request failed: {e}"))?;
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            println!("HTTP {status}: {body}");
        }

        Commands::Tui { server, token } => {
            tui::run(server, token).await?;
        }

        Commands::Init {
            server,
            name,
            token,
        } => {
            let api = client::GyreClient::new(server.clone(), token);
            println!("Registering agent '{name}' with {server}...");
            let resp = api.register_agent(&name).await?;
            let cfg = config::Config {
                server,
                token: Some(resp.auth_token.clone()),
                agent_id: Some(resp.id.clone()),
                agent_name: Some(resp.name.clone()),
            };
            cfg.save()?;
            let path = config::Config::path();
            println!("Agent registered!");
            println!("  ID:     {}", resp.id);
            println!("  Name:   {}", resp.name);
            println!("  Status: {}", resp.status);
            println!("Config saved to {}", path.display());
        }

        Commands::Clone { repo, dir } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;

            // Build git URL: if "project/repo", construct from server; otherwise use as-is
            let git_url = if repo.starts_with("http://") || repo.starts_with("https://") {
                repo.clone()
            } else {
                // Expect "project/repo" or "project/repo.git"
                let normalized = if repo.ends_with(".git") {
                    repo.clone()
                } else {
                    format!("{repo}.git")
                };
                format!("{}/git/{normalized}", cfg.server)
            };

            // Local directory: use last path segment without .git
            let local_dir = dir.unwrap_or_else(|| {
                git_url
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("repo")
                    .trim_end_matches(".git")
                    .to_string()
            });

            println!("Cloning {git_url} into {local_dir}/");
            let status = std::process::Command::new("git")
                .args([
                    "-c",
                    &format!("http.extraHeader=Authorization: Bearer {token}"),
                    "clone",
                    &git_url,
                    &local_dir,
                ])
                .status()
                .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;
            if !status.success() {
                anyhow::bail!("git clone failed");
            }
        }

        Commands::Push { remote } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;

            let status = std::process::Command::new("git")
                .args([
                    "-c",
                    &format!("http.extraHeader=Authorization: Bearer {token}"),
                    "push",
                    &remote,
                ])
                .status()
                .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;
            if !status.success() {
                anyhow::bail!("git push failed");
            }
        }

        Commands::Mr {
            command:
                MrCommands::Create {
                    title,
                    target,
                    repo_id,
                    source,
                },
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let agent_id = cfg.agent_id.as_deref();

            // Detect current branch if --source not given
            let source_branch = match source {
                Some(b) => b,
                None => {
                    let out = std::process::Command::new("git")
                        .args(["rev-parse", "--abbrev-ref", "HEAD"])
                        .output()
                        .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;
                    if !out.status.success() {
                        anyhow::bail!("could not detect current branch; use --source");
                    }
                    String::from_utf8_lossy(&out.stdout).trim().to_string()
                }
            };

            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());
            println!("Creating MR: '{title}' ({source_branch} → {target})");
            let mr = api
                .create_mr(&repo_id, &title, &source_branch, &target, agent_id)
                .await?;
            println!("MR created!");
            println!("  ID:     {}", mr.id);
            println!("  Title:  {}", mr.title);
            println!("  Branch: {} → {}", mr.source_branch, mr.target_branch);
            println!("  Status: {}", mr.status);
        }

        Commands::Tasks {
            command: TaskCommands::List { status, mine },
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let assigned_to = if mine { cfg.agent_id.as_deref() } else { None };
            let tasks = api.list_tasks(status.as_deref(), assigned_to).await?;

            if tasks.is_empty() {
                println!("No tasks found.");
            } else {
                println!("{:<20} {:<12} {:<10} TITLE", "ID", "STATUS", "PRIORITY");
                println!("{}", "-".repeat(70));
                for t in &tasks {
                    println!(
                        "{:<20} {:<12} {:<10} {}",
                        t.id, t.status, t.priority, t.title
                    );
                }
            }
        }

        Commands::Tasks {
            command: TaskCommands::Take { id },
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let agent_id = cfg.require_agent_id()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let task = api.assign_task(&id, agent_id).await?;
            println!("Task assigned to you: {}", task.id);

            // Also transition to in_progress (best-effort; task may already be in_progress)
            match api.transition_task_status(&id, "in_progress").await {
                Ok(t) => println!("Status: {} → {}", task.status, t.status),
                Err(e) => println!("Note: could not transition status: {e}"),
            }
        }

        Commands::Status => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let agent_id = cfg.require_agent_id()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let agent = api.get_agent(agent_id).await?;
            println!("Agent Status");
            println!("  ID:           {}", agent.id);
            println!("  Name:         {}", agent.name);
            println!("  Status:       {}", agent.status);
            if let Some(task_id) = &agent.current_task_id {
                println!("  Current Task: {task_id}");
            } else {
                println!("  Current Task: (none)");
            }
            if let Some(hb) = agent.last_heartbeat {
                println!("  Last Heartbeat: {hb}");
            }
        }

        Commands::Release {
            command:
                ReleaseCommands::Prepare {
                    repo_id,
                    branch,
                    from,
                    create_mr,
                    server,
                    token,
                    markdown: show_markdown,
                },
        } => {
            let api = client::GyreClient::new(server.clone(), token.clone());
            let result = api
                .release_prepare(&repo_id, branch.as_deref(), from.as_deref(), create_mr)
                .await?;

            if show_markdown {
                if let Some(md) = result["changelog"].as_str() {
                    print!("{md}");
                }
            } else {
                let current = result["current_tag"].as_str().unwrap_or("(none)");
                let next = result["next_version"].as_str().unwrap_or("unknown");
                let bump = result["bump_type"].as_str().unwrap_or("none");
                let count = result["commit_count"].as_u64().unwrap_or(0);
                let has_release = result["has_release"].as_bool().unwrap_or(false);

                println!("Release Preparation");
                println!("  Current tag:   {current}");
                println!("  Next version:  {next}");
                println!("  Bump type:     {bump}");
                println!("  Commits since: {count}");
                println!("  Has release:   {has_release}");
                if let Some(mr_id) = result["mr_id"].as_str() {
                    println!("  Release MR:    {mr_id}");
                }
                println!();

                if let Some(sections) = result["sections"].as_array() {
                    if sections.is_empty() {
                        println!("No releasable changes found.");
                    } else {
                        for section in sections {
                            let title = section["title"].as_str().unwrap_or("Other");
                            println!("--- {title} ---");
                            if let Some(entries) = section["entries"].as_array() {
                                for e in entries {
                                    let desc = e["description"].as_str().unwrap_or("");
                                    let scope = e["scope"]
                                        .as_str()
                                        .map(|s| format!("({s}) "))
                                        .unwrap_or_default();
                                    let agent = e["agent_name"]
                                        .as_str()
                                        .or_else(|| e["agent_id"].as_str())
                                        .unwrap_or("");
                                    let task = e["task_id"].as_str().unwrap_or("");
                                    let mut attrs = Vec::new();
                                    if !agent.is_empty() {
                                        attrs.push(agent.to_string());
                                    }
                                    if !task.is_empty() {
                                        attrs.push(task.to_string());
                                    }
                                    let attr_str = if attrs.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" [{}]", attrs.join(", "))
                                    };
                                    println!("  - {scope}{desc}{attr_str}");
                                }
                            }
                            println!();
                        }
                    }
                }
                println!("Run with --markdown to output full changelog markdown.");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_connect_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "connect",
            "--server",
            "ws://host:3000/ws",
            "--token",
            "tok",
        ]);
        assert!(args.is_ok());
        let cli = args.unwrap();
        if let Commands::Connect { server, token } = cli.command {
            assert_eq!(server, "ws://host:3000/ws");
            assert_eq!(token, "tok");
        } else {
            panic!("Expected Connect");
        }
    }

    #[test]
    fn cli_ping_parses() {
        let args = Cli::try_parse_from(["gyre", "ping"]);
        assert!(args.is_ok());
        if let Commands::Ping { server, token } = args.unwrap().command {
            assert_eq!(server, DEFAULT_SERVER);
            assert_eq!(token, DEFAULT_TOKEN);
        } else {
            panic!("Expected Ping");
        }
    }

    #[test]
    fn cli_health_parses() {
        let args = Cli::try_parse_from(["gyre", "health", "--server", "http://myhost:8080"]);
        assert!(args.is_ok());
        if let Commands::Health { server } = args.unwrap().command {
            assert_eq!(server, "http://myhost:8080");
        } else {
            panic!("Expected Health");
        }
    }

    #[test]
    fn cli_tui_parses() {
        let args = Cli::try_parse_from(["gyre", "tui"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_init_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "init",
            "--server",
            "http://localhost:3333",
            "--name",
            "ralph",
        ]);
        assert!(args.is_ok());
        if let Commands::Init {
            server,
            name,
            token,
        } = args.unwrap().command
        {
            assert_eq!(server, "http://localhost:3333");
            assert_eq!(name, "ralph");
            assert_eq!(token, DEFAULT_TOKEN);
        } else {
            panic!("Expected Init");
        }
    }

    #[test]
    fn cli_clone_parses() {
        let args = Cli::try_parse_from(["gyre", "clone", "myproject/myrepo"]);
        assert!(args.is_ok());
        if let Commands::Clone { repo, dir } = args.unwrap().command {
            assert_eq!(repo, "myproject/myrepo");
            assert!(dir.is_none());
        } else {
            panic!("Expected Clone");
        }
    }

    #[test]
    fn cli_clone_with_dir_parses() {
        let args = Cli::try_parse_from(["gyre", "clone", "proj/repo", "--dir", "/tmp/myrepo"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_push_parses() {
        let args = Cli::try_parse_from(["gyre", "push"]);
        assert!(args.is_ok());
        if let Commands::Push { remote } = args.unwrap().command {
            assert_eq!(remote, "origin");
        } else {
            panic!("Expected Push");
        }
    }

    #[test]
    fn cli_push_custom_remote_parses() {
        let args = Cli::try_parse_from(["gyre", "push", "--remote", "gyre"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_mr_create_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "mr",
            "create",
            "--title",
            "My PR",
            "--repo-id",
            "repo-123",
        ]);
        assert!(args.is_ok());
        if let Commands::Mr {
            command:
                MrCommands::Create {
                    title,
                    target,
                    repo_id,
                    source,
                },
        } = args.unwrap().command
        {
            assert_eq!(title, "My PR");
            assert_eq!(target, "main");
            assert_eq!(repo_id, "repo-123");
            assert!(source.is_none());
        } else {
            panic!("Expected Mr Create");
        }
    }

    #[test]
    fn cli_tasks_list_parses() {
        let args = Cli::try_parse_from(["gyre", "tasks", "list"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_tasks_list_with_filter_parses() {
        let args =
            Cli::try_parse_from(["gyre", "tasks", "list", "--status", "in_progress", "--mine"]);
        assert!(args.is_ok());
        if let Commands::Tasks {
            command: TaskCommands::List { status, mine },
        } = args.unwrap().command
        {
            assert_eq!(status.as_deref(), Some("in_progress"));
            assert!(mine);
        } else {
            panic!("Expected Tasks List");
        }
    }

    #[test]
    fn cli_tasks_take_parses() {
        let args = Cli::try_parse_from(["gyre", "tasks", "take", "task-001"]);
        assert!(args.is_ok());
        if let Commands::Tasks {
            command: TaskCommands::Take { id },
        } = args.unwrap().command
        {
            assert_eq!(id, "task-001");
        } else {
            panic!("Expected Tasks Take");
        }
    }

    #[test]
    fn cli_status_parses() {
        let args = Cli::try_parse_from(["gyre", "status"]);
        assert!(args.is_ok());
    }

    #[test]
    fn cli_verify() {
        #[test]
        fn cli_release_prepare_parses() {
            let args = Cli::try_parse_from(["gyre", "release", "prepare", "--repo-id", "repo-123"]);
            assert!(args.is_ok());
            if let Commands::Release {
                command:
                    ReleaseCommands::Prepare {
                        repo_id,
                        branch,
                        from,
                        create_mr,
                        markdown,
                        ..
                    },
            } = args.unwrap().command
            {
                assert_eq!(repo_id, "repo-123");
                assert!(branch.is_none());
                assert!(from.is_none());
                assert!(!create_mr);
                assert!(!markdown);
            } else {
                panic!("Expected Release Prepare");
            }
        }

        #[test]
        fn cli_release_prepare_with_options_parses() {
            let args = Cli::try_parse_from([
                "gyre",
                "release",
                "prepare",
                "--repo-id",
                "repo-456",
                "--branch",
                "main",
                "--from",
                "v1.2.3",
                "--create-mr",
                "--markdown",
            ]);
            assert!(args.is_ok());
            if let Commands::Release {
                command:
                    ReleaseCommands::Prepare {
                        repo_id,
                        branch,
                        from,
                        create_mr,
                        markdown,
                        ..
                    },
            } = args.unwrap().command
            {
                assert_eq!(repo_id, "repo-456");
                assert_eq!(branch.as_deref(), Some("main"));
                assert_eq!(from.as_deref(), Some("v1.2.3"));
                assert!(create_mr);
                assert!(markdown);
            } else {
                panic!("Expected Release Prepare with options");
            }
        }

        Cli::command().debug_assert();
    }
}
