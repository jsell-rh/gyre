mod bootstrap;
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
    /// Bootstrap a fresh Gyre platform: tenant, workspace, repo, personas, gates,
    /// and a repo orchestrator, all in one run (platform-model.md §8)
    Bootstrap {
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Token used to authenticate bootstrap API calls. Defaults to
        /// $GYRE_AUTH_TOKEN when set, else the static dev token.
        #[arg(long)]
        token: Option<String>,
        /// Tenant display name (e.g. "Acme Corp")
        #[arg(long)]
        tenant: Option<String>,
        /// Workspace display name (e.g. "Platform Team")
        #[arg(long)]
        workspace: Option<String>,
        /// Repository name to register
        #[arg(long)]
        repo: Option<String>,
        /// Local path to the repo checkout (used for gate + spec detection,
        /// starter kit target)
        #[arg(long)]
        repo_path: Option<String>,
        /// Admin username to create (non-dev mode)
        #[arg(long)]
        admin_user: Option<String>,
        /// OIDC issuer URL for the tenant
        #[arg(long)]
        oidc_issuer: Option<String>,
        /// Dev mode: skip OIDC and admin user, use static tokens, tenant "dev",
        /// workspace "default"
        #[arg(long)]
        dev: bool,
        /// Create a starter spec structure in the repo path (or --repo-path)
        #[arg(long)]
        starter_kit: bool,
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
    /// Display workspace briefing narrative
    Briefing {
        /// Workspace slug (optional — if omitted, shows briefings for all accessible workspaces)
        #[arg(long)]
        workspace: Option<String>,
        /// Only show activity since this Unix epoch
        #[arg(long)]
        since: Option<u64>,
    },
    /// List and manage notifications (bare invocation lists all)
    Inbox {
        /// Workspace slug to filter by (applies to bare invocation)
        #[arg(long)]
        workspace: Option<String>,
        /// Priority range, e.g. "1-5" (applies to bare invocation)
        #[arg(long)]
        priority: Option<String>,
        #[command(subcommand)]
        command: Option<InboxCommands>,
    },
    /// Search the knowledge graph for a concept
    Explore {
        /// Concept name to search for
        concept: String,
        /// Repository name to scope the search (workspace inferred from git remote if --workspace omitted)
        #[arg(long)]
        repo: Option<String>,
        /// Workspace slug to scope the search
        #[arg(long)]
        workspace: Option<String>,
    },

    /// Full-text search across all entities (specs, tasks, MRs, commits, agents)
    Search {
        /// Search query — supports quoted phrases and facet:value syntax
        /// (e.g. "merge queue" type:spec status:approved)
        query: Option<String>,
        /// Filter by entity type (spec, task, mr, commit, agent)
        #[arg(long, short = 't')]
        r#type: Option<String>,
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
        /// Filter by workspace slug
        #[arg(long, short = 'w')]
        workspace: Option<String>,
        /// Only results since this time (e.g., 7d, 2026-03-01)
        #[arg(long)]
        since: Option<String>,
        /// Autocomplete mode — return suggestions for the given prefix
        #[arg(long)]
        suggest: Option<String>,
        /// Maximum results to return
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Show system trace for a merge request
    Trace {
        /// Merge request ID
        mr_id: String,
    },
    /// Spec operations
    Spec {
        #[command(subcommand)]
        command: SpecCommands,
    },
    /// Show divergence (conflicting interpretation) alerts
    Divergence {
        /// Workspace slug to filter by
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Dependency graph operations
    Deps {
        #[command(subcommand)]
        command: DepsCommands,
    },
    /// Repository operations (status, revert, merge queue control)
    Repo {
        #[command(subcommand)]
        command: RepoCommands,
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
enum RepoCommands {
    /// Show repo status: main health (green/broken) and merge queue state
    Status {
        /// Repository ID
        #[arg(long)]
        repo_id: String,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Manually revert a merged MR
    Revert {
        /// Merge request ID
        mr_id: String,
        /// Repository ID
        #[arg(long)]
        repo_id: String,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Merge queue control (pause/resume)
    Queue {
        #[command(subcommand)]
        command: QueueCommands,
    },
}

#[derive(Subcommand)]
enum QueueCommands {
    /// Manually pause the merge queue
    Pause {
        /// Repository ID
        #[arg(long)]
        repo_id: String,
        /// Reason for the pause
        #[arg(long)]
        reason: Option<String>,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
    },
    /// Manually resume the merge queue
    Resume {
        /// Repository ID
        #[arg(long)]
        repo_id: String,
        /// Gyre server base URL
        #[arg(long, default_value = "http://localhost:3000")]
        server: String,
        /// Auth token
        #[arg(long, default_value = DEFAULT_TOKEN)]
        token: String,
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

#[derive(Subcommand)]
enum InboxCommands {
    /// List notifications (same as bare `gyre inbox`)
    List {
        /// Workspace slug to filter by
        #[arg(long)]
        workspace: Option<String>,
        /// Priority range (e.g., "1-5")
        #[arg(long)]
        priority: Option<String>,
    },
    /// Dismiss a notification
    Dismiss {
        /// Notification ID
        id: String,
    },
    /// Resolve a notification
    Resolve {
        /// Notification ID
        id: String,
    },
}

#[derive(Subcommand)]
enum SpecCommands {
    /// Get LLM-suggested edits for a spec file
    Assist {
        /// Spec file path within the repository
        path: String,
        /// Instruction describing what to change
        instruction: String,
        /// Repository name (optional — inferred from git remote if omitted)
        #[arg(long)]
        repo: Option<String>,
        /// Workspace slug (optional — inferred from git remote if omitted)
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Show all links for a spec (outbound and inbound)
    Links {
        /// Spec file path (e.g., system/identity-security.md)
        path: String,
    },
    /// Show specs that depend on the given spec
    Dependents {
        /// Spec file path (e.g., system/source-control.md)
        path: String,
    },
    /// Display the tenant-wide spec dependency graph
    Graph {
        /// Output format: "text" (default) or "dot" (Graphviz DOT)
        #[arg(long)]
        format: Option<String>,
    },
    /// List all stale links across the tenant
    StaleLinks,
    /// List all active conflicts
    Conflicts,
}

#[derive(Subcommand)]
enum DepsCommands {
    /// Show dependencies and dependents
    Show {
        /// Show workspace-wide dependency graph
        #[arg(long)]
        workspace: bool,
        /// Show tenant-wide dependency graph
        #[arg(long)]
        tenant: bool,
    },
    /// Output dependency graph in DOT format
    Graph {
        /// Output format (dot)
        #[arg(long, default_value = "dot")]
        format: String,
    },
    /// Show blast radius (all transitive dependents) for a repo
    Impact {
        /// Repository name
        repo: String,
    },
    /// List all stale dependencies
    Stale,
    /// List unacknowledged breaking changes
    Breaking,
    /// Add a manual dependency from this repo to another
    Add {
        /// Target repository name
        #[arg(long)]
        target: String,
        /// Dependency type (code, spec, api, schema, manual)
        #[arg(long, rename_all = "snake_case")]
        r#type: String,
    },
    /// Acknowledge a breaking change
    Acknowledge {
        /// Breaking change ID
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

        Commands::Bootstrap {
            server,
            token,
            tenant,
            workspace,
            repo,
            repo_path,
            admin_user,
            oidc_issuer,
            dev,
            starter_kit,
        } => {
            // GYRE_AUTH_TOKEN pattern (server-config.md): explicit flag >
            // env > static dev token.
            let token = token
                .or_else(|| std::env::var("GYRE_AUTH_TOKEN").ok())
                .unwrap_or_else(|| DEFAULT_TOKEN.to_string());
            run_bootstrap(BootstrapArgs {
                server,
                token,
                tenant,
                workspace,
                repo,
                repo_path,
                admin_user,
                oidc_issuer,
                dev,
                starter_kit,
            })
            .await?;
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

        Commands::Briefing { workspace, since } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            // If --workspace is given, show that workspace's briefing.
            // Otherwise, list all accessible workspaces and show briefings for each.
            let workspace_ids: Vec<(String, String)> = if let Some(slug) = &workspace {
                let wid = api.resolve_workspace_slug(slug).await?;
                vec![(slug.clone(), wid)]
            } else {
                let workspaces = api.list_workspaces().await?;
                workspaces
                    .iter()
                    .filter_map(|w| {
                        let id = w["id"].as_str()?;
                        let slug = w["slug"].as_str().or_else(|| w["name"].as_str())?;
                        Some((slug.to_string(), id.to_string()))
                    })
                    .collect()
            };

            if workspace_ids.is_empty() {
                println!("No accessible workspaces found.");
            }

            for (i, (slug, wid)) in workspace_ids.iter().enumerate() {
                if i > 0 {
                    println!();
                    println!("{}", "=".repeat(80));
                    println!();
                }

                let briefing = api.get_briefing(wid, since).await?;

                println!("Workspace Briefing: {slug}");
                println!();

                print_briefing(&briefing);
            }
        }

        Commands::Inbox {
            workspace,
            priority,
            command,
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            match command {
                None => {
                    // Bare `gyre inbox` — list notifications using top-level flags
                    print_notifications(&api, workspace.as_deref(), priority.as_deref()).await?;
                }
                Some(InboxCommands::List {
                    workspace: sub_ws,
                    priority: sub_pri,
                }) => {
                    // `gyre inbox list` — subcommand flags take precedence
                    let ws = sub_ws.as_deref().or(workspace.as_deref());
                    let pri = sub_pri.as_deref().or(priority.as_deref());
                    print_notifications(&api, ws, pri).await?;
                }
                Some(InboxCommands::Dismiss { id }) => {
                    api.dismiss_notification(&id).await?;
                    println!("Notification {id} dismissed.");
                }
                Some(InboxCommands::Resolve { id }) => {
                    api.resolve_notification(&id).await?;
                    println!("Notification {id} resolved.");
                }
            }
        }

        Commands::Explore {
            concept,
            repo,
            workspace,
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            // Determine which concept search endpoints to call.
            // If --repo is given (with --workspace), search that single repo.
            // If --repo is given without --workspace, infer workspace from git remote.
            // If --workspace is given (without --repo), search across all repos in the workspace.
            // If neither is given, search across all accessible workspaces.
            let results: Vec<serde_json::Value> = match (&workspace, &repo) {
                (Some(slug), Some(repo_name)) => {
                    let wid = api.resolve_workspace_slug(slug).await?;
                    let rid = api.resolve_repo_name(&wid, repo_name).await?;
                    vec![api.get_graph_concept(&concept, Some(&rid), None).await?]
                }
                (Some(slug), None) => {
                    let wid = api.resolve_workspace_slug(slug).await?;
                    vec![api.get_graph_concept(&concept, None, Some(&wid)).await?]
                }
                (None, Some(repo_name)) => {
                    // Infer workspace from git remote, then resolve repo name
                    let (ws_slug, _) = infer_repo_from_git_remote().ok_or_else(|| {
                        anyhow::anyhow!(
                            "could not infer workspace from git remote. \
                             Use --workspace <slug> --repo <name> to specify."
                        )
                    })?;
                    let wid = api.resolve_workspace_slug(&ws_slug).await?;
                    let rid = api.resolve_repo_name(&wid, repo_name).await?;
                    vec![api.get_graph_concept(&concept, Some(&rid), None).await?]
                }
                (None, None) => {
                    // Search across all accessible workspaces
                    let workspaces = api.list_workspaces().await?;
                    let mut all = Vec::new();
                    for ws in &workspaces {
                        if let Some(wid) = ws["id"].as_str() {
                            match api.get_graph_concept(&concept, None, Some(wid)).await {
                                Ok(r) => all.push(r),
                                Err(_) => continue, // skip inaccessible workspaces
                            }
                        }
                    }
                    all
                }
            };

            // Collect all nodes from all results
            let all_nodes: Vec<&serde_json::Value> = results
                .iter()
                .filter_map(|r| r["nodes"].as_array())
                .flatten()
                .collect();

            if all_nodes.is_empty() {
                println!("No matching graph nodes found for '{concept}'.");
            } else {
                println!(
                    "{:<12} {:<30} {:<50} {:<10} SPEC",
                    "TYPE", "NAME", "QUALIFIED_NAME", "CONFIDENCE"
                );
                println!("{}", "-".repeat(120));
                for n in &all_nodes {
                    let ntype = n["node_type"].as_str().unwrap_or("");
                    let name = n["name"].as_str().unwrap_or("");
                    let qname = n["qualified_name"].as_str().unwrap_or("");
                    let confidence = n["spec_confidence"].as_str().unwrap_or("None");
                    let spec = n["spec_path"].as_str().unwrap_or("-");
                    println!("{ntype:<12} {name:<30} {qname:<50} {confidence:<10} {spec}");
                }
            }
        }

        Commands::Search {
            query,
            r#type,
            status,
            workspace,
            since,
            suggest,
            limit,
        } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let status_filter = status.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let since_cutoff = match since.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Some(parse_since(s).ok_or_else(|| {
                    anyhow::anyhow!(
                        "invalid --since value '{s}' (expected <N><s|m|h|d|w> such as 7d, or a date like 2026-03-01)"
                    )
                })?),
                None => None,
            };

            // Autocomplete: the dedicated /search/suggest endpoint
            // (search.md §API, task-153) is not implemented yet; fall back
            // to a regular search over the prefix and keep title-prefix
            // matches (see `collect_suggestions`).
            let q = match suggest.as_deref() {
                Some(p) if !p.trim().is_empty() => p.trim().to_string(),
                _ => query.as_deref().unwrap_or_default().trim().to_string(),
            };

            if q.is_empty() {
                println!("No search query given. Usage: gyre search <query> [--type spec] [--status approved] [--workspace slug] [--since 7d] [--suggest prefix] [--limit N]");
            } else {
                let workspace_id = match &workspace {
                    Some(slug) => Some(api.resolve_workspace_slug(slug).await?),
                    None => None,
                };
                // `--type`/`--workspace` are enforced server-side
                // (entity_type / workspace_id params on GET /api/v1/search).
                // `--status`/`--since` filter client-side against live
                // entity state, so over-fetch candidates before filtering;
                // the server caps any limit at 100, which bounds how many
                // candidates a filtered search can scan.
                let filtering = status_filter.is_some() || since_cutoff.is_some();
                let fetch_limit = if filtering { limit.max(100) } else { limit };
                let entity_type = r#type.as_deref().map(str::trim).filter(|t| !t.is_empty());
                let mut response = api
                    .search(&q, entity_type, workspace_id.as_deref(), fetch_limit)
                    .await?;

                if filtering {
                    // Search index facets freeze at create time (the index
                    // write sites run only on entity creation), so resolve
                    // live status/recency per result before filtering.
                    let mut kept: Vec<client::SearchResult> = Vec::new();
                    let mut unresolvable = 0usize;
                    for r in &response.results {
                        let live = live_entity_state(&api, r).await;
                        if live.unavailable {
                            unresolvable += 1;
                        }
                        if result_matches_filters(status_filter, since_cutoff, &live) {
                            kept.push(r.clone());
                        }
                    }
                    if unresolvable > 0 {
                        eprintln!(
                            "note: {unresolvable} result(s) excluded — live status/recency \
                             could not be resolved for their entity type (spec/commit have \
                             no detail endpoint yet, or the record was deleted)"
                        );
                    }
                    response.total = kept.len();
                    kept.truncate(limit);
                    response.results = kept;
                }

                if let Some(prefix) = suggest.as_deref() {
                    print_search_suggestions(prefix, &response);
                } else {
                    print_search_results(&response);
                }
            }
        }

        Commands::Trace { mr_id } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let result = api.get_mr_trace(&mr_id).await?;

            let commit_sha = result["commit_sha"].as_str().unwrap_or("unknown");
            let gate_run_id = result["gate_run_id"].as_str().unwrap_or("unknown");
            let span_count = result["span_count"].as_u64().unwrap_or(0);
            let captured_at = result["captured_at"].as_u64().unwrap_or(0);

            println!("Trace for MR {mr_id}");
            println!("  commit:     {commit_sha}");
            println!("  gate_run:   {gate_run_id}");
            println!("  captured:   {}", format_timestamp(captured_at));
            println!("  spans:      {span_count}");
            println!();

            let root_spans = result["root_spans"].as_array();
            if let Some(roots) = root_spans {
                let root_ids: Vec<&str> = roots.iter().filter_map(|v| v.as_str()).collect();
                println!("Root spans: {}", root_ids.join(", "));
                println!();
            }

            let spans = result["spans"].as_array();
            match spans {
                Some(items) if !items.is_empty() => {
                    println!(
                        "{:<20} {:<20} {:<30} {:>10} {:<8}",
                        "SPAN_ID", "SERVICE", "OPERATION", "DURATION", "STATUS"
                    );
                    println!("{}", "-".repeat(92));
                    for span in items {
                        let span_id = span["span_id"].as_str().unwrap_or("");
                        let service = span["service_name"].as_str().unwrap_or("");
                        let operation = span["operation_name"].as_str().unwrap_or("");
                        let duration_us = span["duration_us"].as_u64().unwrap_or(0);
                        let status = span["status"].as_str().unwrap_or("");
                        let duration_str = if duration_us >= 1_000_000 {
                            format!("{:.1}s", duration_us as f64 / 1_000_000.0)
                        } else if duration_us >= 1_000 {
                            format!("{:.1}ms", duration_us as f64 / 1_000.0)
                        } else {
                            format!("{duration_us}us")
                        };
                        println!(
                            "{:<20} {:<20} {:<30} {:>10} {:<8}",
                            span_id, service, operation, duration_str, status
                        );
                    }
                }
                _ => println!("No trace spans."),
            }
        }

        Commands::Spec { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            match command {
                SpecCommands::Assist {
                    path,
                    instruction,
                    repo,
                    workspace,
                } => {
                    // Resolve repo ID: from explicit flags, or infer from git remote
                    let (ws_slug, repo_name) = match (workspace, repo) {
                        (Some(ws), Some(r)) => (ws, r),
                        (Some(ws), None) => {
                            let (_, rn) = infer_repo_from_git_remote().ok_or_else(|| {
                                anyhow::anyhow!(
                                    "could not infer repository from git remote. \
                                     Use --repo <name> to specify."
                                )
                            })?;
                            (ws, rn)
                        }
                        (None, Some(repo_name)) => {
                            let (ws, _) = infer_repo_from_git_remote().ok_or_else(|| {
                                anyhow::anyhow!(
                                    "could not infer workspace from git remote. \
                                     Use --workspace <slug> --repo <name> to specify."
                                )
                            })?;
                            (ws, repo_name)
                        }
                        (None, None) => infer_repo_from_git_remote().ok_or_else(|| {
                            anyhow::anyhow!(
                                "could not infer repository from git remote. \
                                 Use --workspace <slug> --repo <name> to specify, \
                                 or run from a gyre-cloned repository."
                            )
                        })?,
                    };

                    let workspace_id = api.resolve_workspace_slug(&ws_slug).await?;
                    let repo_id = api.resolve_repo_name(&workspace_id, &repo_name).await?;

                    println!("Requesting spec assist for {path}...");
                    let ops = api.spec_assist(&repo_id, &path, &instruction).await?;

                    if ops.is_empty() {
                        println!("No suggestions returned.");
                    } else {
                        for op in &ops {
                            if let Some(error_msg) = op["error"].as_str() {
                                eprintln!("Error: {error_msg}");
                                continue;
                            }
                            if let Some(explanation) = op["explanation"].as_str() {
                                println!();
                                println!("Explanation: {explanation}");
                            }
                            if let Some(diff) = op["diff"].as_array() {
                                println!();
                                for d in diff {
                                    let op_type = d["op"].as_str().unwrap_or("unknown");
                                    let path = d["path"].as_str().unwrap_or("");
                                    let content = d["content"].as_str().unwrap_or("");
                                    println!("  [{op_type}] {path}");
                                    for line in content.lines() {
                                        println!("    {line}");
                                    }
                                }
                            }
                        }
                    }
                }

                SpecCommands::Links { path } => {
                    let links = api.get_spec_links(&path).await?;
                    if links.is_empty() {
                        println!("No links found for {path}.");
                    } else {
                        println!("Links for {path}");
                        println!();
                        print_spec_links_table(&links);
                    }
                }

                SpecCommands::Dependents { path } => {
                    let deps = api.get_spec_dependents(&path).await?;
                    if deps.is_empty() {
                        println!("No specs depend on {path}.");
                    } else {
                        println!("Specs that depend on {path}");
                        println!();
                        print_spec_links_table(&deps);
                    }
                }

                SpecCommands::Graph { format } => {
                    let graph = api.get_spec_graph().await?;
                    let fmt = format.as_deref().unwrap_or("text");
                    match fmt {
                        "dot" => print_spec_dot_graph(&graph),
                        "text" => print_spec_graph_text(&graph),
                        other => {
                            anyhow::bail!("unsupported format '{other}': use 'text' or 'dot'");
                        }
                    }
                }

                SpecCommands::StaleLinks => {
                    let links = api.get_stale_spec_links().await?;
                    if links.is_empty() {
                        println!("No stale spec links.");
                    } else {
                        print_spec_links_table(&links);
                    }
                }

                SpecCommands::Conflicts => {
                    let links = api.get_spec_conflicts().await?;
                    if links.is_empty() {
                        println!("No active conflicts.");
                    } else {
                        print_spec_links_table(&links);
                    }
                }
            }
        }

        Commands::Deps { command } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            match command {
                DepsCommands::Show { workspace, tenant } => {
                    if tenant {
                        // Tenant-wide graph
                        let graph = api.get_dependency_graph().await?;
                        print_dependency_graph(&graph);
                    } else if workspace {
                        // Workspace-scoped graph — infer workspace from git remote
                        let (ws_slug, _) = infer_repo_from_git_remote().ok_or_else(|| {
                            anyhow::anyhow!(
                                "could not infer workspace from git remote. \
                                 Run from a gyre-cloned repository."
                            )
                        })?;
                        let ws_id = api.resolve_workspace_slug(&ws_slug).await?;
                        let repos = api.list_workspace_repos(&ws_id).await?;
                        let ws_repo_ids: std::collections::HashSet<String> = repos
                            .iter()
                            .filter_map(|r| r["id"].as_str().map(|s| s.to_string()))
                            .collect();

                        let graph = api.get_dependency_graph().await?;
                        print_dependency_graph_filtered(&graph, &ws_repo_ids);
                    } else {
                        // This repo's dependencies and dependents
                        let (ws_slug, repo_name) =
                            infer_repo_from_git_remote().ok_or_else(|| {
                                anyhow::anyhow!(
                                    "could not infer repository from git remote. \
                                     Run from a gyre-cloned repository."
                                )
                            })?;
                        let ws_id = api.resolve_workspace_slug(&ws_slug).await?;
                        let repo_id = api.resolve_repo_name(&ws_id, &repo_name).await?;

                        let deps = api.list_dependencies(&repo_id).await?;
                        let dependents = api.list_dependents(&repo_id).await?;

                        println!("Dependencies for {repo_name}");
                        println!();
                        if deps.is_empty() {
                            println!("No outgoing dependencies.");
                        } else {
                            println!(
                                "{:<36} {:<12} {:<10} {:<12} TARGET",
                                "ID", "TYPE", "STATUS", "METHOD"
                            );
                            println!("{}", "-".repeat(90));
                            for d in &deps {
                                let id = d["id"].as_str().unwrap_or("");
                                let dtype = d["dependency_type"].as_str().unwrap_or("");
                                let status = d["status"].as_str().unwrap_or("");
                                let method = d["detection_method"].as_str().unwrap_or("");
                                let target = d["target_repo_id"].as_str().unwrap_or("");
                                println!(
                                    "{:<36} {:<12} {:<10} {:<12} {}",
                                    id, dtype, status, method, target
                                );
                            }
                        }

                        println!();
                        if dependents.is_empty() {
                            println!("No incoming dependents.");
                        } else {
                            println!("Dependents (repos that depend on {repo_name}):");
                            println!(
                                "{:<36} {:<12} {:<10} {:<12} SOURCE",
                                "ID", "TYPE", "STATUS", "METHOD"
                            );
                            println!("{}", "-".repeat(90));
                            for d in &dependents {
                                let id = d["id"].as_str().unwrap_or("");
                                let dtype = d["dependency_type"].as_str().unwrap_or("");
                                let status = d["status"].as_str().unwrap_or("");
                                let method = d["detection_method"].as_str().unwrap_or("");
                                let source = d["source_repo_id"].as_str().unwrap_or("");
                                println!(
                                    "{:<36} {:<12} {:<10} {:<12} {}",
                                    id, dtype, status, method, source
                                );
                            }
                        }
                    }
                }

                DepsCommands::Graph { format } => {
                    if format != "dot" {
                        anyhow::bail!("unsupported format '{format}': only 'dot' is supported");
                    }
                    let graph = api.get_dependency_graph().await?;
                    print_dot_graph(&graph);
                }

                DepsCommands::Impact { repo } => {
                    // Resolve repo name to ID via git remote context
                    let (ws_slug, _) = infer_repo_from_git_remote().ok_or_else(|| {
                        anyhow::anyhow!(
                            "could not infer workspace from git remote. \
                             Run from a gyre-cloned repository."
                        )
                    })?;
                    let ws_id = api.resolve_workspace_slug(&ws_slug).await?;
                    let repo_id = api.resolve_repo_name(&ws_id, &repo).await?;

                    let result = api.get_blast_radius(&repo_id).await?;

                    let direct = result["direct_dependents"].as_array();
                    let transitive = result["transitive_dependents"].as_array();
                    let total = result["total"].as_u64().unwrap_or(0);

                    println!("Blast radius for {repo}");
                    println!("  Total dependents: {total}");
                    println!();

                    if let Some(items) = direct {
                        if !items.is_empty() {
                            println!("Direct dependents:");
                            for item in items {
                                let rid = item.as_str().unwrap_or("");
                                println!("  ├── {rid}");
                            }
                        }
                    }

                    if let Some(items) = transitive {
                        if !items.is_empty() {
                            println!("Transitive dependents:");
                            for item in items {
                                let rid = item.as_str().unwrap_or("");
                                println!("  └── {rid}");
                            }
                        }
                    }

                    if total == 0 {
                        println!("No dependents found.");
                    }
                }

                DepsCommands::Stale => {
                    let stale = api.list_stale_dependencies(None).await?;
                    if stale.is_empty() {
                        println!("No stale dependencies.");
                    } else {
                        println!(
                            "{:<36} {:<36} {:<12} {:>6} PINNED → CURRENT",
                            "SOURCE", "TARGET", "TYPE", "DRIFT"
                        );
                        println!("{}", "-".repeat(110));
                        for d in &stale {
                            let source = d["source_repo_id"].as_str().unwrap_or("");
                            let target = d["target_repo_id"].as_str().unwrap_or("");
                            let dtype = d["dependency_type"].as_str().unwrap_or("");
                            let drift = d["version_drift"].as_u64().unwrap_or(0);
                            let pinned = d["version_pinned"].as_str().unwrap_or("-");
                            let current = d["target_version_current"].as_str().unwrap_or("-");
                            println!(
                                "{:<36} {:<36} {:<12} {:>6} {} → {}",
                                source, target, dtype, drift, pinned, current
                            );
                        }
                    }
                }

                DepsCommands::Breaking => {
                    let changes = api.list_breaking_changes().await?;
                    if changes.is_empty() {
                        println!("No unacknowledged breaking changes.");
                    } else {
                        println!(
                            "{:<36} {:<36} {:<12} {:<20} DESCRIPTION",
                            "ID", "SOURCE_REPO", "COMMIT", "DETECTED"
                        );
                        println!("{}", "-".repeat(120));
                        for bc in &changes {
                            let id = bc["id"].as_str().unwrap_or("");
                            let source = bc["source_repo_id"].as_str().unwrap_or("");
                            // dependency_edge_id: internal reference, not user-facing
                            // acknowledged / acknowledged_by / acknowledged_at: always false/None for unacknowledged list
                            let sha = bc["commit_sha"].as_str().unwrap_or("");
                            let short_sha: String = sha.chars().take(10).collect();
                            let detected = bc["detected_at"].as_u64().unwrap_or(0);
                            let desc = bc["description"].as_str().unwrap_or("");
                            println!(
                                "{:<36} {:<36} {:<12} {:<20} {}",
                                id,
                                source,
                                short_sha,
                                format_timestamp(detected),
                                desc
                            );
                        }
                    }
                }

                DepsCommands::Add { target, r#type } => {
                    let (ws_slug, repo_name) = infer_repo_from_git_remote().ok_or_else(|| {
                        anyhow::anyhow!(
                            "could not infer repository from git remote. \
                             Run from a gyre-cloned repository."
                        )
                    })?;
                    let ws_id = api.resolve_workspace_slug(&ws_slug).await?;
                    let source_repo_id = api.resolve_repo_name(&ws_id, &repo_name).await?;
                    let target_repo_id = api.resolve_repo_name(&ws_id, &target).await?;

                    let result = api
                        .add_dependency(&source_repo_id, &target_repo_id, &r#type)
                        .await?;

                    let id = result["id"].as_str().unwrap_or("unknown");
                    println!(
                        "Dependency added: {repo_name} → {target} (type: {})",
                        r#type
                    );
                    println!("  ID: {id}");
                }

                DepsCommands::Acknowledge { id } => {
                    api.acknowledge_breaking_change(&id).await?;
                    println!("Breaking change {id} acknowledged.");
                }
            }
        }

        Commands::Divergence { workspace } => {
            let cfg = config::Config::load()?;
            let token = cfg.require_token()?;
            let api = client::GyreClient::new(cfg.server.clone(), token.to_string());

            let workspace_id = if let Some(slug) = &workspace {
                Some(api.resolve_workspace_slug(slug).await?)
            } else {
                None
            };

            let result = api
                .get_notifications(
                    workspace_id.as_deref(),
                    None,
                    None,
                    Some("ConflictingInterpretations"),
                )
                .await?;

            let notifications = result["notifications"].as_array();
            match notifications {
                Some(items) if !items.is_empty() => {
                    println!("Divergence Alerts");
                    println!("{}", "-".repeat(80));
                    for n in items {
                        let id = n["id"].as_str().unwrap_or("");
                        let title = n["title"].as_str().unwrap_or("");
                        let desc = n["body"].as_str().unwrap_or("");
                        let pri = n["priority"].as_u64().unwrap_or(0);
                        let entity_ref = n["entity_ref"].as_str().unwrap_or("");
                        println!("[P{pri}] {title}");
                        println!("  ID: {id}");
                        if !entity_ref.is_empty() {
                            println!("  Spec: {entity_ref}");
                        }
                        if !desc.is_empty() {
                            println!("  {desc}");
                        }
                        println!();
                    }
                }
                _ => println!("No divergence alerts."),
            }
        }

        Commands::Repo { command } => match command {
            RepoCommands::Status {
                repo_id,
                server,
                token,
            } => {
                let api = client::GyreClient::new(server, token);
                let status = api.repo_status(&repo_id).await?;

                let paused = status["queue_paused"].as_bool().unwrap_or(false);
                let main_green = status["main_green"].as_bool();
                let gates = status["post_merge_gates"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();

                println!("Repo {repo_id}");
                println!();
                match main_green {
                    Some(true) => println!("Main: green"),
                    Some(false) => println!("Main: broken (post-merge validation failing)"),
                    None => println!("Main: unknown (no post-merge gates configured)"),
                }
                if paused {
                    let reason = status["pause_reason"].as_str().unwrap_or("unknown");
                    println!("Merge queue: paused ({reason})");
                } else {
                    println!("Merge queue: running");
                }
                if gates.is_empty() {
                    println!("Post-merge gates: none configured");
                } else {
                    println!("Post-merge gates:");
                    for g in &gates {
                        let name = g["name"].as_str().unwrap_or("?");
                        let cmd = g["command"].as_str().unwrap_or("?");
                        let required = g["required"].as_bool().unwrap_or(true);
                        println!("  {name}: {cmd} (required: {required})");
                    }
                }
            }
            RepoCommands::Revert {
                mr_id,
                repo_id,
                server,
                token,
            } => {
                let api = client::GyreClient::new(server, token);
                let result = api.revert_mr(&repo_id, &mr_id).await?;
                let sha = result["revert_commit_sha"].as_str().unwrap_or("?");
                println!("Reverted MR {mr_id} (revert commit: {sha})");
            }
            RepoCommands::Queue { command } => match command {
                QueueCommands::Pause {
                    repo_id,
                    reason,
                    server,
                    token,
                } => {
                    let api = client::GyreClient::new(server, token);
                    api.pause_queue(&repo_id, reason.as_deref()).await?;
                    println!("Merge queue for repo {repo_id} paused.");
                }
                QueueCommands::Resume {
                    repo_id,
                    server,
                    token,
                } => {
                    let api = client::GyreClient::new(server, token);
                    api.resume_queue(&repo_id).await?;
                    println!("Merge queue for repo {repo_id} resumed.");
                }
            },
        },
    }

    Ok(())
}

// ── Bootstrap (platform-model.md §8) ──────────────────────────────────────────

struct BootstrapArgs {
    server: String,
    token: String,
    tenant: Option<String>,
    workspace: Option<String>,
    repo: Option<String>,
    repo_path: Option<String>,
    admin_user: Option<String>,
    oidc_issuer: Option<String>,
    dev: bool,
    starter_kit: bool,
}

/// One bootstrap step, named for the failure report (spec §8: "on failure,
/// report which steps succeeded, which failed, how to resume").
struct StepTracker {
    done: Vec<&'static str>,
    current: &'static str,
}

impl StepTracker {
    fn new(current: &'static str) -> Self {
        Self {
            done: Vec::new(),
            current,
        }
    }

    fn advance(&mut self, next: &'static str) {
        self.done.push(self.current);
        self.current = next;
    }

    fn fail(&self, err: anyhow::Error) -> anyhow::Error {
        let succeeded = if self.done.is_empty() {
            "none".to_string()
        } else {
            self.done.join(", ")
        };
        anyhow::anyhow!(
            "bootstrap failed at step '{}' (completed: {succeeded}): {err}\n\
             Resume hint: re-run `gyre bootstrap` after fixing the issue; \
             already-created resources keep their IDs and slugs, so \
             re-running skips or fails gracefully on duplicates.",
            self.current
        )
    }
}

async fn run_bootstrap(args: BootstrapArgs) -> Result<()> {
    // ── Flag validation ──
    if args.dev {
        if args.tenant.is_some() {
            anyhow::bail!("--dev cannot be combined with --tenant (dev mode uses tenant \"dev\")");
        }
        if args.workspace.is_some() {
            anyhow::bail!(
                "--dev cannot be combined with --workspace (dev mode uses workspace \"default\")"
            );
        }
        if args.admin_user.is_some() {
            anyhow::bail!("--dev cannot be combined with --admin-user (dev mode uses static tokens)");
        }
        if args.oidc_issuer.is_some() {
            anyhow::bail!("--dev cannot be combined with --oidc-issuer (dev mode skips OIDC)");
        }
    } else if args.tenant.is_none() {
        anyhow::bail!("--tenant is needed to name the tenant (or pass --dev for defaults)");
    }

    let repo_name = args.repo.clone().unwrap_or_else(|| "main".to_string());
    let tenant_name = args.tenant.clone().unwrap_or_else(|| "dev".to_string());
    let workspace_name = args.workspace.clone().unwrap_or_else(|| "default".to_string());
    let repo_path = args.repo_path.as_deref().map(std::path::PathBuf::from);

    let api = client::GyreClient::new(args.server.clone(), args.token.clone());
    let mut summary = bootstrap::BootstrapSummary {
        server_url: args.server.clone(),
        repo_name: repo_name.clone(),
        tenant_name: tenant_name.clone(),
        workspace_name: workspace_name.clone(),
        ..Default::default()
    };

    // ── Step 0: health check ──
    println!("Checking server health at {}...", args.server);
    let health = api.health().await?;
    if health["status"].as_str() != Some("ok") {
        anyhow::bail!("server health check returned unexpected status: {health}");
    }
    println!("  Server healthy (version {}).", health["version"].as_str().unwrap_or("?"));

    let mut step = StepTracker::new("create tenant");

    // ── Step 1: create tenant (resume: reuse existing by slug) ──
    let tenant_slug = bootstrap::derive_slug(&tenant_name);
    let tenant = match api
        .create_tenant(&tenant_name, &tenant_slug, args.oidc_issuer.as_deref())
        .await
    {
        Ok(t) => {
            println!("  Tenant '{}' created ({})", t.name, t.id);
            t
        }
        Err(create_err) => match api.find_tenant_by_slug(&tenant_slug).await {
            Ok(Some(existing)) => {
                println!("  Tenant '{}' already exists ({}) - reusing", existing.name, existing.id);
                existing
            }
            _ => return Err(step.fail(create_err)),
        },
    };
    summary.tenant_id = tenant.id.clone();

    // ── Step 2: create admin user + API key (skipped in dev mode) ──
    step.advance("create admin user");
    let mut client_api = api;
    if !args.dev {
        let username = args.admin_user.as_deref().unwrap_or("admin");
        match client_api.create_user(username).await {
            Ok(created) => {
                summary.admin_username = Some(created.user.username.clone());
                summary.api_key = Some(created.api_key.key.clone());
                // Save credentials to ~/.gyre/config for subsequent CLI calls.
                let cfg = config::Config {
                    server: args.server.clone(),
                    token: Some(created.api_key.key.clone()),
                    agent_id: Some(created.user.id.clone()),
                    agent_name: Some(created.user.username.clone()),
                };
                cfg.save()?;
                println!("  Admin user '{}' created; credentials saved to {}",
                    created.user.username,
                    config::Config::path().display());
                // Continue authenticating as the new admin via its API key.
                client_api =
                    client::GyreClient::new(args.server.clone(), created.api_key.key.clone());
            }
            Err(e) => {
                // Resume path: the user already exists from a prior run. The
                // API key minted then was saved to ~/.gyre/config — reuse it.
                let saved = config::Config::load().ok();
                let reuse = saved
                    .filter(|c| c.server == args.server)
                    .and_then(|c| c.token)
                    .filter(|t| !t.is_empty());
                match reuse {
                    Some(token) => {
                        println!("  Admin user '{username}' already exists - reusing saved credentials");
                        summary.admin_username = Some(username.to_string());
                        client_api = client::GyreClient::new(args.server.clone(), token);
                    }
                    None => return Err(step.fail(e)),
                }
            }
        }
    }

    // ── Step 3: create workspace (resume: reuse existing by name) ──
    step.advance("create workspace");
    let workspace = match client_api
        .create_workspace(&summary.tenant_id, &workspace_name)
        .await
    {
        Ok(ws) => {
            println!("  Workspace '{}' created ({})", ws.name, ws.id);
            ws
        }
        Err(create_err) => {
            match client_api
                .find_workspace_by_name(&summary.tenant_id, &workspace_name)
                .await
            {
                Ok(Some(existing)) => {
                    println!(
                        "  Workspace '{}' already exists ({}) - reusing",
                        existing.name, existing.id
                    );
                    existing
                }
                _ => return Err(step.fail(create_err)),
            }
        }
    };
    summary.workspace_id = workspace.id.clone();
    step.advance("register repo");

    // ── Step 4: add repo (resume: reuse existing by name) ──
    let repo = match client_api.create_repo(&summary.workspace_id, &repo_name).await {
        Ok(r) => {
            println!("  Repo '{}' created ({})", r.name, r.id);
            r
        }
        Err(create_err) => match client_api
            .find_repo_by_name(&summary.workspace_id, &repo_name)
            .await
        {
            Ok(Some(existing)) => {
                println!("  Repo '{}' already exists ({}) - reusing", existing.name, existing.id);
                existing
            }
            _ => return Err(step.fail(create_err)),
        },
    };
    summary.repo_id = repo.id.clone();
    summary.clone_url = repo.clone_url.clone();
    step.advance("register personas");

    // ── Step 5: register built-in personas (pre-approved; resume: skip existing) ──
    for persona in bootstrap::BUILTIN_PERSONAS {
        match client_api.find_persona_by_slug(persona.slug).await? {
            Some(_) => {
                println!("  Persona '{}' already registered - skipping", persona.slug);
            }
            None => {
                let created = client_api
                    .create_persona(
                        persona.name,
                        persona.slug,
                        &summary.tenant_id,
                        persona.prompt,
                        persona.capabilities,
                        persona.protocols,
                    )
                    .await
                    .map_err(|e| step.fail(e))?;
                client_api.approve_persona(&created.id).await.map_err(|e| step.fail(e))?;
            }
        }
        summary.personas_registered.push(persona.slug.to_string());
    }
    println!(
        "  {} personas registered and pre-approved",
        bootstrap::BUILTIN_PERSONAS.len()
    );
    step.advance("init spec registry");

    // ── Step 6: spec registry (report-only; sync happens on push) ──
    match &repo_path {
        Some(path) if path.join("specs").join("manifest.yaml").exists() => {
            println!("  Spec manifest found at {} - ledger syncs on push to the default branch",
                path.join("specs").join("manifest.yaml").display());
        }
        _ => println!("  No spec manifest found - spec registry stays empty until specs are pushed"),
    }
    if args.starter_kit {
        let target = repo_path
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from(&repo_name));
        bootstrap::write_starter_kit(&target)?;
        println!("  Starter kit written to {}", target.display());
    }
    step.advance("configure gates");

    // ── Step 7: configure default gates ──
    if let Some(path) = &repo_path {
        for gate in bootstrap::detect_default_gates(path) {
            client_api
                .create_gate(&summary.repo_id, gate.name, gate.gate_type, gate.command)
                .await
                .map_err(|e| step.fail(e))?;
            summary.gates_configured.push(gate.name.to_string());
        }
    }
    if summary.gates_configured.is_empty() {
        println!("  No default gates detected (no Cargo.toml or check-arch.sh in repo path)");
    } else {
        println!("  Gates configured: {}", summary.gates_configured.join(", "));
    }
    step.advance("spawn repo orchestrator");

    // ── Step 8: spawn repo orchestrator ──
    // Task-093 endpoint: repo-tier orchestrator with exactly-one-live
    // semantics, repo-scoped JWT, and restart-on-failure — no synthetic task.
    let orchestrator_name = format!("{repo_name}-orchestrator");
    match client_api
        .spawn_repo_orchestrator(&summary.repo_id, Some(&orchestrator_name))
        .await
    {
        Ok(client::SpawnRepoOrchestratorOutcome::Spawned(spawned)) => {
            summary.orchestrator_agent_id = Some(spawned.agent.id.clone());
        }
        Ok(client::SpawnRepoOrchestratorOutcome::AlreadyLive) => {
            println!("  A repo orchestrator is already active for this repo - keeping it");
        }
        Err(e) => return Err(step.fail(e)),
    }
    step.advance("done");

    // ── Step 9: summary ──
    println!();
    print!("{}", summary.render());
    Ok(())
}

/// Infer workspace slug and repo name from the git remote URL.
/// Gyre git URLs have the form: {server}/git/{workspace_slug}/{repo_name}.git
fn infer_repo_from_git_remote() -> Option<(String, String)> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Parse: .../git/{workspace_slug}/{repo_name}.git
    let parts: Vec<&str> = url.trim_end_matches(".git").rsplit('/').collect();
    if parts.len() >= 2 {
        let repo_name = parts[0].to_string();
        let workspace_slug = parts[1].to_string();
        // Verify this looks like a gyre git URL (has /git/ in it)
        if url.contains("/git/") {
            return Some((workspace_slug, repo_name));
        }
    }
    None
}

/// List notifications, resolving workspace slug and parsing priority range.
async fn print_notifications(
    api: &client::GyreClient,
    workspace_slug: Option<&str>,
    priority: Option<&str>,
) -> Result<()> {
    let workspace_id = if let Some(slug) = workspace_slug {
        Some(api.resolve_workspace_slug(slug).await?)
    } else {
        None
    };

    let (min_pri, max_pri) = parse_priority_range(priority)?;

    let result = api
        .get_notifications(workspace_id.as_deref(), min_pri, max_pri, None)
        .await?;

    let notifications = result["notifications"].as_array();
    match notifications {
        Some(items) if !items.is_empty() => {
            println!(
                "{:<8} {:<36} {:<5} {:<28} TITLE",
                "PRI", "ID", "TYPE", "AGE"
            );
            println!("{}", "-".repeat(100));
            for n in items {
                let id = n["id"].as_str().unwrap_or("");
                let pri = n["priority"].as_u64().unwrap_or(0);
                let ntype = n["notification_type"].as_str().unwrap_or("");
                let title = n["title"].as_str().unwrap_or("");
                let created = n["created_at"].as_u64().unwrap_or(0);
                let age = format_age(created);
                println!("P{pri:<7} {id:<36} {ntype:<5} {age:<28} {title}");
            }
        }
        _ => println!("No notifications."),
    }
    Ok(())
}

/// Print a briefing JSON response in human-readable format.
fn print_briefing(briefing: &serde_json::Value) {
    if let Some(summary) = briefing["summary"].as_str() {
        if !summary.is_empty() {
            println!("{summary}");
            println!();
        }
    }

    // Completed
    if let Some(items) = briefing["completed"].as_array() {
        if !items.is_empty() {
            println!("--- Completed ---");
            for item in items {
                print_briefing_item(item, "  - ");
            }
            println!();
        }
    }

    // Completed Agents (HSI §9 — agent decisions and uncertainties)
    if let Some(agents) = briefing["completed_agents"].as_array() {
        if !agents.is_empty() {
            println!("--- Completed Agents ---");
            for agent in agents {
                let agent_id = agent["agent_id"].as_str().unwrap_or("unknown");
                let spec_ref = agent["spec_ref"].as_str().unwrap_or("");
                if spec_ref.is_empty() {
                    println!("  Agent: {agent_id}");
                } else {
                    println!("  Agent: {agent_id} (spec: {spec_ref})");
                }
                if let Some(decisions) = agent["decisions"].as_array() {
                    for decision in decisions {
                        if let Some(text) = decision.as_str() {
                            println!("    Decision: {text}");
                        } else if let Some(obj) = decision.as_object() {
                            let reasoning =
                                obj.get("reasoning").and_then(|v| v.as_str()).unwrap_or("");
                            let confidence =
                                obj.get("confidence").and_then(|v| v.as_str()).unwrap_or("");
                            if !reasoning.is_empty() && !confidence.is_empty() {
                                println!("    Decision: {reasoning} (confidence: {confidence})");
                            } else if !reasoning.is_empty() {
                                println!("    Decision: {reasoning}");
                            }
                        }
                    }
                }
                if let Some(uncertainties) = agent["uncertainties"].as_array() {
                    for u in uncertainties {
                        if let Some(text) = u.as_str() {
                            println!("    Uncertainty: {text}");
                        }
                    }
                }
                if let Some(sha) = agent["conversation_sha"].as_str() {
                    if !sha.is_empty() {
                        println!("    Conversation: {sha}");
                    }
                }
                if let Some(completed_at) = agent["completed_at"].as_u64() {
                    println!("    Completed at: {}", format_timestamp(completed_at));
                }
            }
            println!();
        }
    }

    // In Progress
    if let Some(items) = briefing["in_progress"].as_array() {
        if !items.is_empty() {
            println!("--- In Progress ---");
            for item in items {
                print_briefing_item(item, "  - ");
            }
            println!();
        }
    }

    // Cross-Workspace
    if let Some(items) = briefing["cross_workspace"].as_array() {
        if !items.is_empty() {
            println!("--- Cross-Workspace ---");
            for item in items {
                print_briefing_item(item, "  ↔ ");
            }
            println!();
        }
    }

    // Exceptions
    if let Some(items) = briefing["exceptions"].as_array() {
        if !items.is_empty() {
            println!("--- Exceptions ---");
            for item in items {
                print_briefing_item(item, "  ! ");
            }
            println!();
        }
    }

    // Metrics
    if let Some(metrics) = briefing.get("metrics") {
        let mrs = metrics["mrs_merged"].as_u64().unwrap_or(0);
        let gates = metrics["gate_runs"].as_u64().unwrap_or(0);
        let budget = metrics["budget_spent_usd"].as_f64().unwrap_or(0.0);
        let pct = metrics["budget_pct"].as_u64().unwrap_or(0);
        println!("--- Metrics ---");
        println!("  MRs merged:    {mrs}");
        println!("  Gate runs:     {gates}");
        println!("  Budget spent:  ${budget:.2} ({pct}%)");
    }
}

/// Print a single briefing item with its details (description, spec_path, entity_type, entity_id, timestamp).
fn print_briefing_item(item: &serde_json::Value, prefix: &str) {
    let title = item["title"].as_str().unwrap_or("");
    println!("{prefix}{title}");
    if let Some(desc) = item["description"].as_str() {
        if !desc.is_empty() {
            println!("      {desc}");
        }
    }
    if let Some(spec) = item["spec_path"].as_str() {
        if !spec.is_empty() {
            println!("      (spec: {spec})");
        }
    }
    if let Some(etype) = item["entity_type"].as_str() {
        if !etype.is_empty() {
            let eid = item["entity_id"].as_str().unwrap_or("");
            if !eid.is_empty() {
                println!("      [{etype}: {eid}]");
            } else {
                println!("      [{etype}]");
            }
        }
    }
    if let Some(ts) = item["timestamp"].as_u64() {
        if ts > 0 {
            println!("      ({})", format_timestamp(ts));
        }
    }
    if let Some(ws_slug) = item["source_workspace_slug"].as_str() {
        if !ws_slug.is_empty() {
            println!("      (workspace: {ws_slug})");
        }
    }
    if let Some(actions) = item["actions"].as_array() {
        if !actions.is_empty() {
            let labels: Vec<&str> = actions.iter().filter_map(|a| a.as_str()).collect();
            println!("      Actions: {}", labels.join(" | "));
        }
    }
}

/// Print a dependency graph as a table (nodes + edges).
fn print_dependency_graph(graph: &serde_json::Value) {
    let nodes = graph["nodes"].as_array();
    let edges = graph["edges"].as_array();

    if let Some(nodes) = nodes {
        if nodes.is_empty() {
            println!("No dependencies in the graph.");
            return;
        }
        println!("Repos ({} nodes):", nodes.len());
        for n in nodes {
            let repo_id = n["repo_id"].as_str().unwrap_or("");
            let name = n["name"].as_str().unwrap_or("");
            println!("  {name} ({repo_id})");
        }
    }

    println!();

    if let Some(edges) = edges {
        if edges.is_empty() {
            println!("No dependency edges.");
        } else {
            println!("{:<30} {:<30} {:<10} STATUS", "SOURCE", "TARGET", "TYPE");
            println!("{}", "-".repeat(80));
            for e in edges {
                let source = e["source"].as_str().unwrap_or("");
                let target = e["target"].as_str().unwrap_or("");
                let etype = e["type"].as_str().unwrap_or("");
                let status = e["status"].as_str().unwrap_or("");
                println!("{:<30} {:<30} {:<10} {}", source, target, etype, status);
            }
        }
    }
}

/// Print a dependency graph filtered to a set of repo IDs.
fn print_dependency_graph_filtered(
    graph: &serde_json::Value,
    repo_ids: &std::collections::HashSet<String>,
) {
    let nodes = graph["nodes"].as_array();
    let edges = graph["edges"].as_array();

    let filtered_nodes: Vec<&serde_json::Value> = nodes
        .map(|ns| {
            ns.iter()
                .filter(|n| {
                    n["repo_id"]
                        .as_str()
                        .map(|id| repo_ids.contains(id))
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();

    if filtered_nodes.is_empty() {
        println!("No dependencies in this workspace.");
        return;
    }

    println!("Repos ({} nodes):", filtered_nodes.len());
    for n in &filtered_nodes {
        let repo_id = n["repo_id"].as_str().unwrap_or("");
        let name = n["name"].as_str().unwrap_or("");
        println!("  {name} ({repo_id})");
    }
    println!();

    let filtered_edges: Vec<&serde_json::Value> = edges
        .map(|es| {
            es.iter()
                .filter(|e| {
                    let src = e["source"].as_str().unwrap_or("");
                    let tgt = e["target"].as_str().unwrap_or("");
                    repo_ids.contains(src) || repo_ids.contains(tgt)
                })
                .collect()
        })
        .unwrap_or_default();

    if filtered_edges.is_empty() {
        println!("No dependency edges in this workspace.");
    } else {
        println!("{:<30} {:<30} {:<10} STATUS", "SOURCE", "TARGET", "TYPE");
        println!("{}", "-".repeat(80));
        for e in &filtered_edges {
            let source = e["source"].as_str().unwrap_or("");
            let target = e["target"].as_str().unwrap_or("");
            let etype = e["type"].as_str().unwrap_or("");
            let status = e["status"].as_str().unwrap_or("");
            println!("{:<30} {:<30} {:<10} {}", source, target, etype, status);
        }
    }
}

/// Render a dependency graph in Graphviz DOT format.
fn print_dot_graph(graph: &serde_json::Value) {
    let mut stdout = std::io::stdout();
    write_dot_graph(graph, &mut stdout).expect("failed to write DOT output");
}

fn write_dot_graph(graph: &serde_json::Value, w: &mut dyn std::io::Write) -> std::io::Result<()> {
    writeln!(w, "digraph dependencies {{")?;
    writeln!(w, "  rankdir=LR;")?;
    writeln!(w, "  node [shape=box, style=filled, fillcolor=lightblue];")?;

    if let Some(nodes) = graph["nodes"].as_array() {
        for n in nodes {
            let repo_id = n["repo_id"].as_str().unwrap_or("");
            let name = n["name"].as_str().unwrap_or(repo_id);
            let safe_name = name.replace('"', "\\\"");
            writeln!(w, "  \"{}\" [label=\"{}\"];", repo_id, safe_name)?;
        }
    }

    if let Some(edges) = graph["edges"].as_array() {
        for e in edges {
            let source = e["source"].as_str().unwrap_or("");
            let target = e["target"].as_str().unwrap_or("");
            let etype = e["type"].as_str().unwrap_or("manual");
            let color = match etype {
                "code" => "blue",
                "spec" => "green",
                "api" => "orange",
                "schema" => "purple",
                _ => "gray",
            };
            writeln!(
                w,
                "  \"{}\" -> \"{}\" [color={}, label=\"{}\"];",
                source, target, color, etype
            )?;
        }
    }

    writeln!(w, "}}")?;
    Ok(())
}

/// Display a list of SpecLinkResponse items as a table.
fn print_spec_links_table(links: &[serde_json::Value]) {
    println!(
        "{:<14} {:<40} {:<40} {:<8} STALE SINCE",
        "TYPE", "SOURCE", "TARGET", "STATUS"
    );
    println!("{}", "-".repeat(120));
    for link in links {
        let link_type = link["link_type"].as_str().unwrap_or("");
        let source = link["source_path"].as_str().unwrap_or("");
        let target = link["target_path"].as_str().unwrap_or("");
        let target_display = link["target_display"].as_str().unwrap_or(target);
        let status = link["status"].as_str().unwrap_or("");
        // id: internal UUID, not user-facing
        // target_repo_id: internal UUID, not user-facing
        // target_sha: internal hash, not user-facing
        // reason: shown inline below if present
        // created_at: creation time less relevant than staleness
        let stale_since = link["stale_since"]
            .as_u64()
            .map(format_timestamp)
            .unwrap_or_else(|| "-".to_string());
        let display_target = if target_display != target {
            target_display
        } else {
            target
        };
        println!(
            "{:<14} {:<40} {:<40} {:<8} {}",
            link_type, source, display_target, status, stale_since
        );
        if let Some(reason) = link["reason"].as_str() {
            if !reason.is_empty() {
                println!("             reason: {reason}");
            }
        }
    }
}

/// Render search results as a readable table (search.md §CLI):
/// one line per result with entity type, title, id, and snippet.
fn print_search_results(results: &client::SearchResponse) {
    if results.results.is_empty() {
        println!("No results for '{}'.", results.query);
        return;
    }
    println!(
        "Search results for '{}' ({} shown of {} total):",
        results.query,
        results.results.len(),
        results.total
    );
    println!("{}", "-".repeat(80));
    for r in &results.results {
        let snippet = r.snippet.replace(['\n', '\r'], " ");
        println!("[{}] {} ({})", r.entity_type, r.title, r.entity_id);
        if !snippet.is_empty() {
            println!("      {snippet}");
        }
    }
}

/// Current UNIX time in seconds.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Days from civil date to days since the UNIX epoch (Howard Hinnant's
/// algorithm — no chrono dependency needed for one date parse).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = (m + 9) % 12; // [0, 11]
    let doy = (153 * mp as u64 + 2) / 5 + d as u64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe as i64 - 719468
}

/// Parse a relative `--since` duration (`7d`, `12h`, `30m`, `2w`, `45s`)
/// against `now` (UNIX seconds), returning the cutoff. `now` is a
/// parameter so tests are deterministic.
fn parse_since_relative(s: &str, now: u64) -> Option<u64> {
    let s = s.trim();
    let mut chars = s.chars();
    // Relative: <count><unit> where unit ∈ {s, m, h, d, w}. Split on the
    // last char, not a byte index — a multibyte unit must not panic.
    let unit = chars.next_back()?;
    let digits = chars.as_str();
    let count: u64 = digits.parse().ok()?;
    let secs = match unit {
        's' => count,
        'm' => count.checked_mul(60)?,
        'h' => count.checked_mul(3600)?,
        'd' => count.checked_mul(86_400)?,
        'w' => count.checked_mul(604_800)?,
        _ => return None,
    };
    now.checked_sub(secs)
}

/// Parse a `--since` argument (search.md §Query Language `since:` facet):
/// either a relative duration or an ISO date, against `now`.
fn parse_since_at(s: &str, now: u64) -> Option<u64> {
    parse_since_relative(s, now).or_else(|| parse_since_date(s))
}

/// ISO date branch of `--since` (e.g. `2026-03-01` → UTC midnight).
/// Kept separate from the relative parser so each stays testable in
/// isolation; `parse_since_at` dispatches on shape.
fn parse_since_date(s: &str) -> Option<u64> {
    let s = s.trim();
    let mut parts = s.splitn(3, '-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    // Strictly zero-padded ISO 8601 calendar date: YYYY-MM-DD. Variable
    // widths (2026-3-1) are rejected rather than guessed at.
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    if !y.bytes().all(|b| b.is_ascii_digit())
        || !m.bytes().all(|b| b.is_ascii_digit())
        || !d.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let y: i64 = y.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    let d: u32 = d.parse().ok()?;
    if !is_valid_civil_date(y, m, d) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400)
}

/// True when (y, m, d) is a real calendar date (rejects Feb 30, Apr 31,
/// month 13, ...). Years before 1970 are out of domain for `--since`.
fn is_valid_civil_date(y: i64, m: u32, d: u32) -> bool {
    if y < 1970 || !(1..=12).contains(&m) {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let max_d = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            // February.
            if leap {
                29
            } else {
                28
            }
        }
    };
    (1..=max_d).contains(&d)
}

/// Parse a `--since` value against the current clock: relative (`7d`)
/// or ISO date (`2026-03-01`).
fn parse_since(s: &str) -> Option<u64> {
    parse_since_at(s, now_secs())
}

/// Live status/recency of a search result, resolved from the entity's
/// detail endpoint. The search index is written only at entity creation,
/// so its `status` facet freezes at the create-time value; `--status` and
/// `--since` must consult the live record.
#[derive(Debug, Clone, PartialEq)]
struct LiveEntityState {
    /// Current status string (task/mr/agent states; empty when unknown).
    status: String,
    /// Most recent activity timestamp (UNIX secs; 0 when unknown).
    updated_at: u64,
    /// True when neither field could be resolved for this entity type.
    unavailable: bool,
}

/// Fetch the live state behind one search result. Entity types the CLI
/// cannot query (spec/commit — no detail endpoint on the current API)
/// return `unavailable`, and the filter treats them conservatively
/// (see `result_matches_filters`).
async fn live_entity_state(api: &client::GyreClient, r: &client::SearchResult) -> LiveEntityState {
    match r.entity_type.as_str() {
        "task" => match api.get_task(&r.entity_id).await {
            Ok(t) => LiveEntityState {
                status: t.status,
                updated_at: t.updated_at,
                unavailable: false,
            },
            // The entity may have been deleted after indexing; a missing
            // record can no longer match a status/recency filter.
            Err(_) => LiveEntityState {
                status: String::new(),
                updated_at: 0,
                unavailable: true,
            },
        },
        "mr" => match api.get_mr(&r.entity_id).await {
            Ok(mr) => LiveEntityState {
                status: mr.status,
                updated_at: mr.updated_at,
                unavailable: false,
            },
            Err(_) => LiveEntityState {
                status: String::new(),
                updated_at: 0,
                unavailable: true,
            },
        },
        "agent" => match api.get_agent(&r.entity_id).await {
            Ok(a) => {
                // Agents carry no updated_at; recency is the later of
                // spawn time and last heartbeat.
                let updated_at = a.spawned_at.max(a.last_heartbeat.unwrap_or(0));
                LiveEntityState {
                    status: a.status,
                    updated_at,
                    unavailable: false,
                }
            }
            Err(_) => LiveEntityState {
                status: String::new(),
                updated_at: 0,
                unavailable: true,
            },
        },
        // spec/commit: no detail endpoint to resolve live state from.
        _ => LiveEntityState {
            status: String::new(),
            updated_at: 0,
            unavailable: true,
        },
    }
}

/// Decide whether one search result passes the `--status`/`--since`
/// filters, given its live state.
///
/// Semantics:
/// - `--status S`: keep results whose live status equals `S`
///   (case-insensitive). Results whose live status cannot be resolved
///   (entity type has no detail endpoint, or the record is gone) are
///   dropped — an unknown status is not a match.
/// - `--since T`: keep results with a known last-activity timestamp
///   at or after `T`. Unknown timestamps drop only when a timestamp
///   filter is active.
fn result_matches_filters(
    status_filter: Option<&str>,
    since_cutoff: Option<u64>,
    live: &LiveEntityState,
) -> bool {
    // Unresolvable live state (no detail endpoint for the entity type, or
    // the record is gone) can never satisfy a status or recency claim.
    if (status_filter.is_some() || since_cutoff.is_some()) && live.unavailable {
        return false;
    }
    if let Some(want) = status_filter {
        if live.status.to_lowercase() != want.to_lowercase() {
            return false;
        }
    }
    if let Some(cutoff) = since_cutoff {
        if live.updated_at < cutoff {
            return false;
        }
    }
    true
}

/// Collect autocomplete suggestions for `prefix`: results whose title starts
/// with the prefix (case-insensitive). The server's search endpoint matches
/// by substring anywhere in the title or body, so the prefix filter for
/// autocomplete is applied client-side.
fn collect_suggestions<'a>(
    prefix: &str,
    results: &'a client::SearchResponse,
) -> Vec<(&'a str, &'a str, &'a str)> {
    let p = prefix.to_lowercase();
    results
        .results
        .iter()
        .filter(|r| r.title.to_lowercase().starts_with(&p))
        .map(|r| (r.entity_type.as_str(), r.title.as_str(), r.entity_id.as_str()))
        .collect()
}

/// Render autocomplete suggestions for `prefix` in `type  title  (id)` form.
fn print_search_suggestions(prefix: &str, results: &client::SearchResponse) {
    let matches = collect_suggestions(prefix, results);
    if matches.is_empty() {
        println!("No suggestions for '{}'.", prefix);
        return;
    }
    for (entity_type, title, entity_id) in &matches {
        println!("{entity_type}    {title}    ({entity_id})");
    }
}

/// Print a SpecGraphResponse as a text summary.
fn print_spec_graph_text(graph: &serde_json::Value) {
    let nodes = graph["nodes"].as_array();
    let edges = graph["edges"].as_array();

    if let Some(nodes) = nodes {
        if nodes.is_empty() {
            println!("No specs in the graph.");
            return;
        }
        println!("Specs ({} nodes):", nodes.len());
        for n in nodes {
            let path = n["path"].as_str().unwrap_or("");
            let title = n["title"].as_str().unwrap_or("");
            let approval = n["approval_status"].as_str().unwrap_or("");
            if title.is_empty() {
                println!("  {path} [{approval}]");
            } else {
                println!("  {path} — {title} [{approval}]");
            }
        }
    }

    println!();

    if let Some(edges) = edges {
        if edges.is_empty() {
            println!("No spec links.");
        } else {
            println!("{:<40} {:<14} {:<40} STATUS", "SOURCE", "TYPE", "TARGET");
            println!("{}", "-".repeat(100));
            for e in edges {
                let source = e["source"].as_str().unwrap_or("");
                let target = e["target"].as_str().unwrap_or("");
                let link_type = e["link_type"].as_str().unwrap_or("");
                let status = e["status"].as_str().unwrap_or("");
                // reason: omitted in summary view
                println!("{:<40} {:<14} {:<40} {}", source, link_type, target, status);
            }
        }
    }
}

/// Print a SpecGraphResponse as Graphviz DOT format.
fn print_spec_dot_graph(graph: &serde_json::Value) {
    let mut stdout = std::io::stdout();
    write_spec_dot_graph(graph, &mut stdout).expect("failed to write DOT output");
}

/// Write a SpecGraphResponse as Graphviz DOT format.
fn write_spec_dot_graph(
    graph: &serde_json::Value,
    w: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    writeln!(w, "digraph specs {{")?;
    writeln!(w, "  rankdir=LR;")?;
    writeln!(w, "  node [shape=box, style=filled];")?;

    if let Some(nodes) = graph["nodes"].as_array() {
        for n in nodes {
            let path = n["path"].as_str().unwrap_or("");
            let title = n["title"].as_str().unwrap_or(path);
            let approval = n["approval_status"].as_str().unwrap_or("");
            let fillcolor = match approval {
                "approved" | "Approved" => "palegreen",
                "rejected" | "Rejected" => "lightcoral",
                "pending" | "Pending" => "lightyellow",
                _ => "lightblue",
            };
            let safe_title = title.replace('"', "\\\"");
            let safe_path = path.replace('"', "\\\"");
            writeln!(
                w,
                "  \"{}\" [label=\"{}\\n[{}]\", fillcolor={}];",
                safe_path, safe_title, approval, fillcolor
            )?;
        }
    }

    if let Some(edges) = graph["edges"].as_array() {
        for e in edges {
            let source = e["source"].as_str().unwrap_or("");
            let target = e["target"].as_str().unwrap_or("");
            let link_type = e["link_type"].as_str().unwrap_or("");
            let status = e["status"].as_str().unwrap_or("");
            // reason: not shown in DOT graph (visual medium uses color/style instead)
            let (color, style) = match link_type {
                "implements" => ("blue", "solid"),
                "depends_on" => ("green", "solid"),
                "supersedes" => ("gray", "solid"),
                "conflicts_with" => ("red", "solid"),
                "extends" => ("orange", "solid"),
                "references" => ("gray", "dotted"),
                _ => ("gray", "solid"),
            };
            let penwidth = if status == "stale" { "2.0" } else { "1.0" };
            let edge_color = if status == "stale" { "gold" } else { color };
            let safe_source = source.replace('"', "\\\"");
            let safe_target = target.replace('"', "\\\"");
            writeln!(
                w,
                "  \"{}\" -> \"{}\" [color={}, style={}, penwidth={}, label=\"{}\"];",
                safe_source, safe_target, edge_color, style, penwidth, link_type
            )?;
        }
    }

    writeln!(w, "}}")?;
    Ok(())
}

/// Parse a priority range string like "1-5" into (min, max).
fn parse_priority_range(range: Option<&str>) -> Result<(Option<u8>, Option<u8>)> {
    match range {
        None => Ok((None, None)),
        Some(s) => {
            let parts: Vec<&str> = s.split('-').collect();
            if parts.len() != 2 {
                anyhow::bail!(
                    "invalid priority range '{s}': expected format 'MIN-MAX' (e.g., '1-5')"
                );
            }
            let min: u8 = parts[0]
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid min priority '{}'", parts[0]))?;
            let max: u8 = parts[1]
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid max priority '{}'", parts[1]))?;
            Ok((Some(min), Some(max)))
        }
    }
}

/// Format a Unix epoch timestamp as a human-readable age string.
fn format_age(epoch_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if epoch_secs == 0 || epoch_secs > now {
        return "just now".to_string();
    }
    let diff = now - epoch_secs;
    if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

/// Format a Unix epoch timestamp as ISO-ish string.
fn format_timestamp(epoch_secs: u64) -> String {
    // Simple UTC formatting without chrono dependency
    let secs = epoch_secs;
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Approximate date from days since epoch (1970-01-01)
    // Good enough for display — not calendar-precise for leap seconds
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's chrono-compatible date library
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
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
    fn cli_bootstrap_parses_with_defaults() {
        let args = Cli::try_parse_from(["gyre", "bootstrap", "--tenant", "Acme Corp"]);
        assert!(args.is_ok());
        if let Commands::Bootstrap {
            server,
            token,
            tenant,
            workspace,
            repo,
            repo_path,
            admin_user,
            oidc_issuer,
            dev,
            starter_kit,
        } = args.unwrap().command
        {
            assert_eq!(token.as_deref(), None);
            assert_eq!(server, "http://localhost:3000");
            assert_eq!(tenant.as_deref(), Some("Acme Corp"));
            assert!(workspace.is_none());
            assert!(repo.is_none());
            assert!(repo_path.is_none());
            assert!(admin_user.is_none());
            assert!(oidc_issuer.is_none());
            assert!(!dev);
            assert!(!starter_kit);
        } else {
            panic!("Expected Bootstrap");
        }
    }

    #[test]
    fn cli_bootstrap_parses_all_flags() {
        let args = Cli::try_parse_from([
            "gyre",
            "bootstrap",
            "--server",
            "http://boothost:9100",
            "--token",
            "boot-tok",
            "--tenant",
            "Acme Corp",
            "--workspace",
            "Platform Team",
            "--repo",
            "gyre",
            "--repo-path",
            "/tmp/checkout",
            "--admin-user",
            "jsell",
            "--oidc-issuer",
            "https://sso.acme.test",
            "--starter-kit",
        ]);
        assert!(args.is_ok());
        if let Commands::Bootstrap {
            server,
            token,
            tenant,
            workspace,
            repo,
            repo_path,
            admin_user,
            oidc_issuer,
            dev,
            starter_kit,
        } = args.unwrap().command
        {
            assert_eq!(server, "http://boothost:9100");
            assert_eq!(token.as_deref(), Some("boot-tok"));
            assert_eq!(tenant.as_deref(), Some("Acme Corp"));
            assert_eq!(workspace.as_deref(), Some("Platform Team"));
            assert_eq!(repo.as_deref(), Some("gyre"));
            assert_eq!(repo_path.as_deref(), Some("/tmp/checkout"));
            assert_eq!(admin_user.as_deref(), Some("jsell"));
            assert_eq!(oidc_issuer.as_deref(), Some("https://sso.acme.test"));
            assert!(!dev);
            assert!(starter_kit);
        } else {
            panic!("Expected Bootstrap");
        }
    }

    #[test]
    fn cli_bootstrap_dev_mode_parses() {
        let args = Cli::try_parse_from(["gyre", "bootstrap", "--dev"]);
        assert!(args.is_ok());
        if let Commands::Bootstrap { dev, .. } = args.unwrap().command {
            assert!(dev);
        } else {
            panic!("Expected Bootstrap");
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
        Cli::command().debug_assert();
    }

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

    // ── Briefing command tests ───────────────────────────────────────────────

    #[test]
    fn cli_briefing_parses() {
        let args = Cli::try_parse_from(["gyre", "briefing", "--workspace", "platform"]);
        assert!(args.is_ok());
        if let Commands::Briefing { workspace, since } = args.unwrap().command {
            assert_eq!(workspace.as_deref(), Some("platform"));
            assert!(since.is_none());
        } else {
            panic!("Expected Briefing");
        }
    }

    #[test]
    fn cli_briefing_with_since_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "briefing",
            "--workspace",
            "platform",
            "--since",
            "1700000000",
        ]);
        assert!(args.is_ok());
        if let Commands::Briefing { workspace, since } = args.unwrap().command {
            assert_eq!(workspace.as_deref(), Some("platform"));
            assert_eq!(since, Some(1700000000));
        } else {
            panic!("Expected Briefing");
        }
    }

    #[test]
    fn cli_briefing_without_workspace_parses() {
        let args = Cli::try_parse_from(["gyre", "briefing"]);
        assert!(args.is_ok());
        if let Commands::Briefing { workspace, since } = args.unwrap().command {
            assert!(workspace.is_none());
            assert!(since.is_none());
        } else {
            panic!("Expected Briefing");
        }
    }

    // ── Inbox command tests ─────────────────────────────────────────────────

    #[test]
    fn cli_inbox_bare_parses() {
        // Bare `gyre inbox` should parse (defaults to list behavior)
        let args = Cli::try_parse_from(["gyre", "inbox"]);
        assert!(args.is_ok());
        if let Commands::Inbox {
            workspace,
            priority,
            command,
        } = args.unwrap().command
        {
            assert!(workspace.is_none());
            assert!(priority.is_none());
            assert!(command.is_none());
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_bare_with_filters_parses() {
        let args =
            Cli::try_parse_from(["gyre", "inbox", "--workspace", "myws", "--priority", "1-5"]);
        assert!(args.is_ok());
        if let Commands::Inbox {
            workspace,
            priority,
            command,
        } = args.unwrap().command
        {
            assert_eq!(workspace.as_deref(), Some("myws"));
            assert_eq!(priority.as_deref(), Some("1-5"));
            assert!(command.is_none());
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_list_subcommand_parses() {
        let args = Cli::try_parse_from(["gyre", "inbox", "list"]);
        assert!(args.is_ok());
        if let Commands::Inbox { command, .. } = args.unwrap().command {
            assert!(matches!(command, Some(InboxCommands::List { .. })));
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_list_with_filters_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "inbox",
            "list",
            "--workspace",
            "myws",
            "--priority",
            "1-5",
        ]);
        assert!(args.is_ok());
        if let Commands::Inbox { command, .. } = args.unwrap().command {
            if let Some(InboxCommands::List {
                workspace,
                priority,
            }) = command
            {
                assert_eq!(workspace.as_deref(), Some("myws"));
                assert_eq!(priority.as_deref(), Some("1-5"));
            } else {
                panic!("Expected Inbox List subcommand");
            }
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_dismiss_parses() {
        let args = Cli::try_parse_from(["gyre", "inbox", "dismiss", "notif-123"]);
        assert!(args.is_ok());
        if let Commands::Inbox { command, .. } = args.unwrap().command {
            if let Some(InboxCommands::Dismiss { id }) = command {
                assert_eq!(id, "notif-123");
            } else {
                panic!("Expected Inbox Dismiss");
            }
        } else {
            panic!("Expected Inbox");
        }
    }

    #[test]
    fn cli_inbox_resolve_parses() {
        let args = Cli::try_parse_from(["gyre", "inbox", "resolve", "notif-456"]);
        assert!(args.is_ok());
        if let Commands::Inbox { command, .. } = args.unwrap().command {
            if let Some(InboxCommands::Resolve { id }) = command {
                assert_eq!(id, "notif-456");
            } else {
                panic!("Expected Inbox Resolve");
            }
        } else {
            panic!("Expected Inbox");
        }
    }

    // ── Explore command tests ───────────────────────────────────────────────

    #[test]
    fn cli_explore_parses() {
        let args = Cli::try_parse_from(["gyre", "explore", "UserRepository"]);
        assert!(args.is_ok());
        if let Commands::Explore {
            concept,
            repo,
            workspace,
        } = args.unwrap().command
        {
            assert_eq!(concept, "UserRepository");
            assert!(repo.is_none());
            assert!(workspace.is_none());
        } else {
            panic!("Expected Explore");
        }
    }

    #[test]
    fn cli_explore_with_repo_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "explore",
            "AuthMiddleware",
            "--repo",
            "my-service",
            "--workspace",
            "platform",
        ]);
        assert!(args.is_ok());
        if let Commands::Explore {
            concept,
            repo,
            workspace,
        } = args.unwrap().command
        {
            assert_eq!(concept, "AuthMiddleware");
            assert_eq!(repo.as_deref(), Some("my-service"));
            assert_eq!(workspace.as_deref(), Some("platform"));
        } else {
            panic!("Expected Explore");
        }
    }

    #[test]
    fn cli_explore_repo_without_workspace_parses() {
        // --repo without --workspace is valid: workspace is inferred from git remote at runtime
        let args =
            Cli::try_parse_from(["gyre", "explore", "AuthMiddleware", "--repo", "my-service"]);
        assert!(args.is_ok());
        if let Commands::Explore {
            concept,
            repo,
            workspace,
        } = args.unwrap().command
        {
            assert_eq!(concept, "AuthMiddleware");
            assert_eq!(repo.as_deref(), Some("my-service"));
            assert!(workspace.is_none());
        } else {
            panic!("Expected Explore");
        }
    }

    #[test]
    fn cli_explore_with_workspace_parses() {
        let args =
            Cli::try_parse_from(["gyre", "explore", "HttpServer", "--workspace", "platform"]);
        assert!(args.is_ok());
        if let Commands::Explore {
            concept,
            repo,
            workspace,
        } = args.unwrap().command
        {
            assert_eq!(concept, "HttpServer");
            assert!(repo.is_none());
            assert_eq!(workspace.as_deref(), Some("platform"));
        } else {
            panic!("Expected Explore");
        }
    }

    // ── Search command tests ────────────────────────────────────────────────

    #[test]
    fn cli_search_parses() {
        let args = Cli::try_parse_from(["gyre", "search", "identity security"]);
        assert!(args.is_ok());
        if let Commands::Search {
            query,
            r#type,
            status,
            workspace,
            since,
            suggest,
            limit,
        } = args.unwrap().command
        {
            assert_eq!(query.as_deref(), Some("identity security"));
            assert!(r#type.is_none());
            assert!(status.is_none());
            assert!(workspace.is_none());
            assert!(since.is_none());
            assert!(suggest.is_none());
            assert_eq!(limit, 20);
        } else {
            panic!("Expected Search");
        }
    }

    #[test]
    fn cli_search_faceted_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "search",
            "--type",
            "spec",
            "--status",
            "approved",
            "--workspace",
            "platform-team",
            "--since",
            "7d",
            "--limit",
            "5",
            "ABAC",
        ]);
        assert!(args.is_ok());
        if let Commands::Search {
            query,
            r#type,
            status,
            workspace,
            since,
            suggest,
            limit,
        } = args.unwrap().command
        {
            assert_eq!(query.as_deref(), Some("ABAC"));
            assert_eq!(r#type.as_deref(), Some("spec"));
            assert_eq!(status.as_deref(), Some("approved"));
            assert_eq!(workspace.as_deref(), Some("platform-team"));
            assert_eq!(since.as_deref(), Some("7d"));
            assert!(suggest.is_none());
            assert_eq!(limit, 5);
        } else {
            panic!("Expected Search");
        }
    }

    #[test]
    fn cli_search_suggest_parses() {
        let args = Cli::try_parse_from(["gyre", "search", "--suggest", "iden"]);
        assert!(args.is_ok());
        if let Commands::Search {
            query,
            suggest,
            ..
        } = args.unwrap().command
        {
            assert!(query.is_none());
            assert_eq!(suggest.as_deref(), Some("iden"));
        } else {
            panic!("Expected Search");
        }
    }

    #[test]
    fn parse_since_relative_units() {
        // Fixed `now` keeps the arithmetic observable.
        let now = 1_000_000_000u64;
        assert_eq!(parse_since_at("45s", now), Some(now - 45));
        assert_eq!(parse_since_at("30m", now), Some(now - 1_800));
        assert_eq!(parse_since_at("12h", now), Some(now - 43_200));
        assert_eq!(parse_since_at("7d", now), Some(now - 604_800));
        assert_eq!(parse_since_at("2w", now), Some(now - 1_209_600));
    }

    #[test]
    fn parse_since_relative_rejects_junk() {
        let now = 1_000_000_000u64;
        // Unknown unit, empty, bare unit, negative, float, and overflow
        // (a count whose seconds exceed `now` itself) all fail.
        assert_eq!(parse_since_at("7x", now), None);
        assert_eq!(parse_since_at("", now), None);
        assert_eq!(parse_since_at("d", now), None);
        assert_eq!(parse_since_at("-3d", now), None);
        assert_eq!(parse_since_at("1.5d", now), None);
        assert_eq!(parse_since_at("999999999999999999999d", now), None);
    }

    #[test]
    fn parse_since_multibyte_unit_does_not_panic() {
        // Regression: a byte-index split panicked on multibyte input
        // (`--since é`). It must be a clean parse error, not a crash.
        let now = 1_000_000_000u64;
        assert_eq!(parse_since_at("é", now), None);
        assert_eq!(parse_since_at("7é", now), None);
        assert_eq!(parse_since_at("café", now), None);
    }

    #[test]
    fn parse_since_relative_before_epoch_fails() {
        // A duration longer than elapsed time since the epoch cannot be
        // represented as a cutoff; it must be an error, not a wraparound.
        assert_eq!(parse_since_at("999999w", 1_000_000_000), None);
    }

    #[test]
    fn parse_since_iso_date() {
        // 2026-03-01T00:00:00Z == 1772323200 (verified independently).
        assert_eq!(parse_since_at("2026-03-01", 0), Some(1_772_323_200));
        // Leap-year day is a valid date.
        assert_eq!(parse_since_at("2024-02-29", 0), Some(1_709_164_800));
    }

    #[test]
    fn parse_since_iso_date_rejects_invalid() {
        assert_eq!(parse_since_at("2026-02-30", 0), None); // Feb has 28 days
        assert_eq!(parse_since_at("2026-13-01", 0), None); // month 13
        assert_eq!(parse_since_at("2026-04-31", 0), None); // Apr has 30 days
        assert_eq!(parse_since_at("2023-02-29", 0), None); // not a leap year
        assert_eq!(parse_since_at("1969-12-31", 0), None); // before epoch
        assert_eq!(parse_since_at("2026-3-1", 0), None); // not zero-padded
    }

    #[test]
    fn result_matches_status_filter() {
        let live = LiveEntityState {
            status: "in_progress".into(),
            updated_at: 1_000,
            unavailable: false,
        };
        // Exact and case-insensitive matches pass; other statuses fail.
        assert!(result_matches_filters(Some("in_progress"), None, &live));
        assert!(result_matches_filters(Some("In_Progress"), None, &live));
        assert!(!result_matches_filters(Some("done"), None, &live));
        // No filter passes everything.
        assert!(result_matches_filters(None, None, &live));
    }

    #[test]
    fn result_matches_since_filter() {
        let live = LiveEntityState {
            status: String::new(),
            updated_at: 1_000,
            unavailable: false,
        };
        assert!(result_matches_filters(None, Some(1_000), &live)); // boundary: at cutoff
        assert!(result_matches_filters(None, Some(999), &live));
        assert!(!result_matches_filters(None, Some(1_001), &live));
    }

    #[test]
    fn result_matches_filters_compose_with_and() {
        let live = LiveEntityState {
            status: "approved".into(),
            updated_at: 5_000,
            unavailable: false,
        };
        assert!(result_matches_filters(Some("approved"), Some(4_000), &live));
        // Status matches but recency does not → dropped.
        assert!(!result_matches_filters(Some("approved"), Some(6_000), &live));
        // Recency matches but status does not → dropped.
        assert!(!result_matches_filters(Some("open"), Some(4_000), &live));
    }

    #[test]
    fn result_matches_filters_drop_unavailable_state() {
        // Unresolvable live state (no detail endpoint / deleted entity)
        // can never satisfy a status or recency claim.
        let live = LiveEntityState {
            status: String::new(),
            updated_at: 0,
            unavailable: true,
        };
        assert!(!result_matches_filters(Some("approved"), None, &live));
        assert!(!result_matches_filters(None, Some(1), &live));
        // Without filters, nothing consults live state.
        assert!(result_matches_filters(None, None, &live));
    }
    #[test]
    fn suggest_filters_to_title_prefix() {
        let results = client::SearchResponse {
            query: "iden".into(),
            total: 3,
            results: vec![
                client::SearchResult {
                    entity_type: "spec".into(),
                    entity_id: "system/identity-security.md".into(),
                    title: "Identity & Security".into(),
                    snippet: String::new(),
                    score: 3.0,
                    facets: Default::default(),
                },
                client::SearchResult {
                    entity_type: "task".into(),
                    entity_id: "task-042".into(),
                    title: "Coincidental iden-tifier cleanup".into(),
                    snippet: String::new(),
                    score: 1.0,
                    facets: Default::default(),
                },
                client::SearchResult {
                    entity_type: "spec".into(),
                    entity_id: "system/identity.md".into(),
                    title: "identity model".into(),
                    snippet: String::new(),
                    score: 2.0,
                    facets: Default::default(),
                },
            ],
        };
        let suggestions = collect_suggestions("iden", &results);
        assert_eq!(
            suggestions,
            vec![
                ("spec", "Identity & Security", "system/identity-security.md"),
                ("spec", "identity model", "system/identity.md"),
            ]
        );
    }

    #[test]
    fn suggest_prefix_is_case_insensitive() {
        let results = client::SearchResponse {
            query: "IDEN".into(),
            total: 1,
            results: vec![client::SearchResult {
                entity_type: "spec".into(),
                entity_id: "system/identity.md".into(),
                title: "identity model".into(),
                snippet: String::new(),
                score: 2.0,
                facets: Default::default(),
            }],
        };
        assert_eq!(
            collect_suggestions("IDEN", &results),
            vec![("spec", "identity model", "system/identity.md")]
        );
    }

    #[test]
    fn suggest_no_prefix_match_reports_none() {
        let results = client::SearchResponse {
            query: "xyz".into(),
            total: 1,
            results: vec![client::SearchResult {
                entity_type: "task".into(),
                entity_id: "task-001".into(),
                title: "unrelated".into(),
                snippet: String::new(),
                score: 1.0,
                facets: Default::default(),
            }],
        };
        assert!(collect_suggestions("xyz", &results).is_empty());
    }

    // ── Trace command tests ─────────────────────────────────────────────────

    #[test]
    fn cli_trace_parses() {
        let args = Cli::try_parse_from(["gyre", "trace", "mr-789"]);
        assert!(args.is_ok());
        if let Commands::Trace { mr_id } = args.unwrap().command {
            assert_eq!(mr_id, "mr-789");
        } else {
            panic!("Expected Trace");
        }
    }

    // ── Spec assist command tests ───────────────────────────────────────────

    #[test]
    fn cli_spec_assist_parses() {
        // Minimal: just path and instruction (repo inferred from git remote)
        let args = Cli::try_parse_from([
            "gyre",
            "spec",
            "assist",
            "specs/auth.md",
            "add RBAC section",
        ]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command:
                SpecCommands::Assist {
                    path,
                    instruction,
                    repo,
                    workspace,
                },
        } = args.unwrap().command
        {
            assert_eq!(path, "specs/auth.md");
            assert_eq!(instruction, "add RBAC section");
            assert!(repo.is_none());
            assert!(workspace.is_none());
        } else {
            panic!("Expected Spec Assist");
        }
    }

    #[test]
    fn cli_spec_assist_with_explicit_repo_parses() {
        let args = Cli::try_parse_from([
            "gyre",
            "spec",
            "assist",
            "specs/auth.md",
            "add RBAC section",
            "--repo",
            "my-service",
            "--workspace",
            "platform",
        ]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command:
                SpecCommands::Assist {
                    path,
                    instruction,
                    repo,
                    workspace,
                },
        } = args.unwrap().command
        {
            assert_eq!(path, "specs/auth.md");
            assert_eq!(instruction, "add RBAC section");
            assert_eq!(repo.as_deref(), Some("my-service"));
            assert_eq!(workspace.as_deref(), Some("platform"));
        } else {
            panic!("Expected Spec Assist");
        }
    }

    // ── Divergence command tests ────────────────────────────────────────────

    #[test]
    fn cli_divergence_parses() {
        let args = Cli::try_parse_from(["gyre", "divergence"]);
        assert!(args.is_ok());
        if let Commands::Divergence { workspace } = args.unwrap().command {
            assert!(workspace.is_none());
        } else {
            panic!("Expected Divergence");
        }
    }

    #[test]
    fn cli_divergence_with_workspace_parses() {
        let args = Cli::try_parse_from(["gyre", "divergence", "--workspace", "platform"]);
        assert!(args.is_ok());
        if let Commands::Divergence { workspace } = args.unwrap().command {
            assert_eq!(workspace.as_deref(), Some("platform"));
        } else {
            panic!("Expected Divergence");
        }
    }

    // ── Deps command tests ───────────────────────────────────────────────────

    #[test]
    fn cli_deps_show_bare_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "show"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Show { workspace, tenant },
        } = args.unwrap().command
        {
            assert!(!workspace);
            assert!(!tenant);
        } else {
            panic!("Expected Deps Show");
        }
    }

    #[test]
    fn cli_deps_show_workspace_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "show", "--workspace"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Show { workspace, tenant },
        } = args.unwrap().command
        {
            assert!(workspace);
            assert!(!tenant);
        } else {
            panic!("Expected Deps Show with --workspace");
        }
    }

    #[test]
    fn cli_deps_show_tenant_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "show", "--tenant"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Show { workspace, tenant },
        } = args.unwrap().command
        {
            assert!(!workspace);
            assert!(tenant);
        } else {
            panic!("Expected Deps Show with --tenant");
        }
    }

    #[test]
    fn cli_deps_graph_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "graph"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Graph { format },
        } = args.unwrap().command
        {
            assert_eq!(format, "dot");
        } else {
            panic!("Expected Deps Graph");
        }
    }

    #[test]
    fn cli_deps_graph_custom_format_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "graph", "--format", "dot"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Graph { format },
        } = args.unwrap().command
        {
            assert_eq!(format, "dot");
        } else {
            panic!("Expected Deps Graph with --format dot");
        }
    }

    #[test]
    fn cli_deps_impact_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "impact", "repo-b"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Impact { repo },
        } = args.unwrap().command
        {
            assert_eq!(repo, "repo-b");
        } else {
            panic!("Expected Deps Impact");
        }
    }

    #[test]
    fn cli_deps_stale_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "stale"]);
        assert!(args.is_ok());
        assert!(matches!(
            args.unwrap().command,
            Commands::Deps {
                command: DepsCommands::Stale
            }
        ));
    }

    #[test]
    fn cli_deps_breaking_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "breaking"]);
        assert!(args.is_ok());
        assert!(matches!(
            args.unwrap().command,
            Commands::Deps {
                command: DepsCommands::Breaking
            }
        ));
    }

    #[test]
    fn cli_deps_add_parses() {
        let args =
            Cli::try_parse_from(["gyre", "deps", "add", "--target", "repo-b", "--type", "api"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Add { target, r#type },
        } = args.unwrap().command
        {
            assert_eq!(target, "repo-b");
            assert_eq!(r#type, "api");
        } else {
            panic!("Expected Deps Add");
        }
    }

    #[test]
    fn cli_deps_acknowledge_parses() {
        let args = Cli::try_parse_from(["gyre", "deps", "acknowledge", "breaking-123"]);
        assert!(args.is_ok());
        if let Commands::Deps {
            command: DepsCommands::Acknowledge { id },
        } = args.unwrap().command
        {
            assert_eq!(id, "breaking-123");
        } else {
            panic!("Expected Deps Acknowledge");
        }
    }

    #[test]
    fn dot_output_produces_valid_syntax() {
        let graph = serde_json::json!({
            "nodes": [
                {"repo_id": "repo-a", "name": "service-a"},
                {"repo_id": "repo-b", "name": "service-b"},
            ],
            "edges": [
                {"source": "repo-a", "target": "repo-b", "type": "code", "status": "active"},
            ]
        });

        let mut buf = Vec::new();
        write_dot_graph(&graph, &mut buf).unwrap();
        let dot = String::from_utf8(buf).unwrap();

        assert!(dot.starts_with("digraph dependencies {"));
        assert!(dot.trim_end().ends_with('}'));
        assert!(dot.contains("\"repo-a\" -> \"repo-b\""));
        assert!(dot.contains("color=blue"));
        assert!(dot.contains("label=\"service-a\""));
        assert!(dot.contains("label=\"service-b\""));
        assert!(dot.contains("rankdir=LR;"));
        assert!(dot.contains("node [shape=box, style=filled, fillcolor=lightblue];"));
    }

    #[test]
    fn dot_output_edge_colors() {
        let graph = serde_json::json!({
            "nodes": [
                {"repo_id": "a", "name": "a"},
                {"repo_id": "b", "name": "b"},
            ],
            "edges": [
                {"source": "a", "target": "b", "type": "code", "status": "active"},
                {"source": "a", "target": "b", "type": "spec", "status": "active"},
                {"source": "a", "target": "b", "type": "api", "status": "active"},
                {"source": "a", "target": "b", "type": "schema", "status": "active"},
                {"source": "a", "target": "b", "type": "manual", "status": "active"},
            ]
        });

        let mut buf = Vec::new();
        write_dot_graph(&graph, &mut buf).unwrap();
        let dot = String::from_utf8(buf).unwrap();

        assert!(dot.contains("color=blue"), "code type should be blue");
        assert!(dot.contains("color=green"), "spec type should be green");
        assert!(dot.contains("color=orange"), "api type should be orange");
        assert!(dot.contains("color=purple"), "schema type should be purple");
        assert!(dot.contains("color=gray"), "manual type should be gray");
    }

    // ── Helper function tests ───────────────────────────────────────────────

    #[test]
    fn parse_priority_range_none() {
        let (min, max) = parse_priority_range(None).unwrap();
        assert!(min.is_none());
        assert!(max.is_none());
    }

    #[test]
    fn parse_priority_range_valid() {
        let (min, max) = parse_priority_range(Some("1-5")).unwrap();
        assert_eq!(min, Some(1));
        assert_eq!(max, Some(5));
    }

    #[test]
    fn parse_priority_range_invalid_format() {
        assert!(parse_priority_range(Some("invalid")).is_err());
    }

    #[test]
    fn parse_priority_range_invalid_number() {
        assert!(parse_priority_range(Some("abc-5")).is_err());
    }

    #[test]
    fn format_age_zero_is_just_now() {
        assert_eq!(format_age(0), "just now");
    }

    #[test]
    fn format_age_future_is_just_now() {
        assert_eq!(format_age(u64::MAX), "just now");
    }

    #[test]
    fn format_timestamp_epoch() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00Z");
    }

    #[test]
    fn format_timestamp_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(format_timestamp(1704067200), "2024-01-01 00:00:00Z");
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-01 is day 19723 since epoch
        assert_eq!(days_to_ymd(19723), (2024, 1, 1));
    }

    // ── Spec link CLI tests ──────────────────────────────────────────────────

    #[test]
    fn cli_spec_links_parses() {
        let args = Cli::try_parse_from(["gyre", "spec", "links", "system/identity-security.md"]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command: SpecCommands::Links { path },
        } = args.unwrap().command
        {
            assert_eq!(path, "system/identity-security.md");
        } else {
            panic!("Expected Spec Links");
        }
    }

    #[test]
    fn cli_spec_dependents_parses() {
        let args = Cli::try_parse_from(["gyre", "spec", "dependents", "system/source-control.md"]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command: SpecCommands::Dependents { path },
        } = args.unwrap().command
        {
            assert_eq!(path, "system/source-control.md");
        } else {
            panic!("Expected Spec Dependents");
        }
    }

    #[test]
    fn cli_spec_graph_text_default() {
        let args = Cli::try_parse_from(["gyre", "spec", "graph"]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command: SpecCommands::Graph { format },
        } = args.unwrap().command
        {
            assert_eq!(format, None);
        } else {
            panic!("Expected Spec Graph");
        }
    }

    #[test]
    fn cli_spec_graph_dot_format() {
        let args = Cli::try_parse_from(["gyre", "spec", "graph", "--format", "dot"]);
        assert!(args.is_ok());
        if let Commands::Spec {
            command: SpecCommands::Graph { format },
        } = args.unwrap().command
        {
            assert_eq!(format.as_deref(), Some("dot"));
        } else {
            panic!("Expected Spec Graph with dot format");
        }
    }

    #[test]
    fn cli_spec_stale_links_parses() {
        let args = Cli::try_parse_from(["gyre", "spec", "stale-links"]);
        assert!(args.is_ok());
        assert!(matches!(
            args.unwrap().command,
            Commands::Spec {
                command: SpecCommands::StaleLinks
            }
        ));
    }

    #[test]
    fn cli_spec_conflicts_parses() {
        let args = Cli::try_parse_from(["gyre", "spec", "conflicts"]);
        assert!(args.is_ok());
        assert!(matches!(
            args.unwrap().command,
            Commands::Spec {
                command: SpecCommands::Conflicts
            }
        ));
    }

    #[test]
    fn write_spec_dot_graph_basic() {
        let graph = serde_json::json!({
            "nodes": [
                {"path": "system/auth.md", "title": "Authentication", "approval_status": "approved"},
                {"path": "system/api.md", "title": "API Layer", "approval_status": "pending"}
            ],
            "edges": [
                {"source": "system/api.md", "target": "system/auth.md", "link_type": "depends_on", "status": "active"},
                {"source": "system/api.md", "target": "system/auth.md", "link_type": "references", "status": "stale"}
            ]
        });

        let mut buf = Vec::new();
        write_spec_dot_graph(&graph, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("digraph specs {"));
        assert!(output.contains("rankdir=LR;"));
        // Nodes with approval-based coloring
        assert!(output.contains("system/auth.md"));
        assert!(output.contains("Authentication"));
        assert!(output.contains("fillcolor=palegreen"));
        assert!(output.contains("fillcolor=lightyellow"));
        // Edges with type-based coloring
        assert!(output.contains("color=green"));
        assert!(output.contains("style=solid"));
        assert!(output.contains("label=\"depends_on\""));
        // Stale link highlighted in gold
        assert!(output.contains("color=gold"));
        assert!(output.contains("style=dotted"));
        assert!(output.contains("label=\"references\""));
        assert!(output.ends_with("}\n"));
    }

    #[test]
    fn write_spec_dot_graph_empty() {
        let graph = serde_json::json!({"nodes": [], "edges": []});
        let mut buf = Vec::new();
        write_spec_dot_graph(&graph, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("digraph specs {"));
        assert!(output.ends_with("}\n"));
    }

    #[test]
    fn write_spec_dot_graph_all_link_types() {
        let graph = serde_json::json!({
            "nodes": [
                {"path": "a.md", "title": "A", "approval_status": "approved"},
                {"path": "b.md", "title": "B", "approval_status": "rejected"},
                {"path": "c.md", "title": "C", "approval_status": "other"}
            ],
            "edges": [
                {"source": "a.md", "target": "b.md", "link_type": "implements", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "depends_on", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "supersedes", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "conflicts_with", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "extends", "status": "active"},
                {"source": "a.md", "target": "b.md", "link_type": "references", "status": "active"}
            ]
        });

        let mut buf = Vec::new();
        write_spec_dot_graph(&graph, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();

        // Node colors
        assert!(output.contains("fillcolor=palegreen"));
        assert!(output.contains("fillcolor=lightcoral"));
        assert!(output.contains("fillcolor=lightblue"));

        // Edge colors per link type
        assert!(output.contains("color=blue")); // implements
        assert!(output.contains("color=green")); // depends_on
        assert!(output.contains("color=gray")); // supersedes
        assert!(output.contains("color=red")); // conflicts_with
        assert!(output.contains("color=orange")); // extends
                                                  // references: gray + dotted
        let has_dotted = output
            .lines()
            .any(|l| l.contains("references") && l.contains("dotted"));
        assert!(has_dotted, "references edge should use dotted style");
    }

    #[test]
    fn write_spec_dot_graph_escapes_quotes() {
        let graph = serde_json::json!({
            "nodes": [
                {"path": "a\"b.md", "title": "Test \"quotes\"", "approval_status": "pending"}
            ],
            "edges": []
        });

        let mut buf = Vec::new();
        write_spec_dot_graph(&graph, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains(r#"a\"b.md"#));
        assert!(output.contains(r#"Test \"quotes\""#));
    }
}
