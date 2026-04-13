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
                            let short_sha = if sha.len() > 10 { &sha[..10] } else { sha };
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
    }

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
