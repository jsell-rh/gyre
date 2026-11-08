//! Rust Pass 2 call-graph extraction via the `rust-analyzer` LSP subprocess.
//!
//! Implements `specs/system/lsp-call-graph.md` §5 "Rust: rust-analyzer",
//! Option B (LSP subprocess): spawn `rust-analyzer` in LSP mode, send
//! `textDocument/references` for every function definition that Pass 1
//! could have turned into a graph node, resolve the enclosing function at
//! each reference site, and emit `Calls` edges. No `ra_ap_*` crates are
//! involved - the type checker runs out of process and speaks JSON-RPC over
//! stdio, so this module owns a minimal LSP client (framing, request/response
//! correlation, server-request replies).
//!
//! Why subprocess instead of the `ra_ap_*` library: the spec recommends it
//! (no ~50-crate dependency, simpler integration); performance is acceptable
//! because Pass 2 is a background job (spec §9).
//!
//! # Qualified-name contract
//!
//! [`CallEdge`]s are raw qualified-name pairs; the domain layer resolves them
//! to Pass 1 node IDs (`gyre_domain::call_graph_resolve`). Pass 1
//! (`gyre_domain::rust_extractor`) names a function node
//! `<module_qname>::<fn_name>` where `<module_qname>` comes from
//! `crate_and_module_from_path` (path-derived, see [`module_qname_for_path`]).
//! This module mirrors that construction exactly; the
//! `fake_server_edges_resolve_against_real_pass1_nodes` test pins the two
//! sides together so a divergence on either end fails CI instead of silently
//! dropping every Pass 2 edge.
//!
//! # Graceful degradation
//!
//! The port contract requires Pass 2 to be best-effort: a missing
//! `rust-analyzer` binary (checked before spawning, with an
//! `RUST_ANALYZER_BIN` override for tests and explicit configuration), a
//! binary that fails to initialize (e.g. a rustup shim whose component is
//! not installed), a workspace that never finishes loading, or per-request
//! timeouts all degrade to fewer or zero edges - never an error. The whole
//! extraction is bounded by [`OVERALL_BUDGET`] so a wedged server can never
//! stall the background Pass 2 task.

use anyhow::{bail, Context, Result};
use gyre_common::call_graph::CallEdge;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentParams, InitializeParams, Location, Position,
    ReferenceContext, ReferenceParams, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use syn::spanned::Spanned;
use syn::visit::Visit;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, watch, Mutex};
use walkdir::WalkDir;

/// Maximum time for the `initialize` handshake (rust-analyzer responds
/// before workspace loading begins; this only bounds a wedged binary).
const INIT_TIMEOUT: Duration = Duration::from_secs(60);
/// Time reserved for reference queries at the end of the overall budget:
/// after the readiness wait consumes the rest, queries get at least this
/// much of the budget.
const QUERY_RESERVE: Duration = Duration::from_secs(30);
/// Maximum time per `textDocument/references` request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum time for the shutdown handshake.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// Hard budget for the whole extraction. The spec's release-binary estimate
/// for a workspace the size of Gyre is ~20s total (§9), and its stated
/// worst case for Pass 2 is "within a minute"; this bound is deliberately
/// looser so large-workspace loads that exceed the spec's estimate still
/// complete instead of degrading to an empty graph, while a wedged server
/// can never stall the background Pass 2 task indefinitely. Partial edges
/// are returned if the budget runs out mid-run.
const OVERALL_BUDGET: Duration = Duration::from_secs(180);

/// Directories never scanned for Rust sources (build output and VCS data).
const SKIP_DIRS: [&str; 3] = ["target", ".git", ".cargo"];

// ── Public entry point ──────────────────────────────────────────────────────

/// Extract caller -> callee edges for the Rust project at `repo_path` by
/// driving `rust-analyzer` over LSP.
///
/// Best-effort per the port contract: every failure mode degrades to fewer or
/// zero edges, never an error.
pub async fn extract_rust_call_edges(repo_path: &Path) -> Result<Vec<CallEdge>> {
    extract_with_binary(repo_path, find_rust_analyzer()).await
}

/// [`extract_rust_call_edges`] with the server binary supplied by the caller
/// (`None` models "rust-analyzer is not installed"). Split so tests can pin
/// each degradation branch without mutating process-global PATH state.
async fn extract_with_binary(repo_path: &Path, bin: Option<PathBuf>) -> Result<Vec<CallEdge>> {
    let deadline = tokio::time::Instant::now() + OVERALL_BUDGET;
    // Canonicalize so the file:// URI round-trips against the URIs the
    // server reports for reference sites (symlinked roots otherwise miss).
    let repo_path = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());

    // Binary first: if rust-analyzer is not installed there is no point
    // indexing the tree.
    let Some(bin) = bin else {
        tracing::warn!(
            "rust Pass 2: rust-analyzer binary not found (set RUST_ANALYZER_BIN to override); \
             skipping Pass 2 for Rust"
        );
        return Ok(Vec::new());
    };

    // Discover and parse .rs files into per-file function indexes.
    let mut files: Vec<FileIndex> = Vec::new();
    for path in discover_rs_files(&repo_path) {
        match build_file_index(&repo_path, &path) {
            Ok(index) => files.push(index),
            Err(e) => {
                tracing::warn!(
                    file = %path.display(),
                    error = %e,
                    "rust Pass 2: failed to index Rust file; skipping it"
                );
            }
        }
    }
    if files.is_empty() {
        tracing::info!("rust Pass 2: no indexable .rs files; skipping");
        return Ok(Vec::new());
    }

    let query_targets: Vec<(&FileIndex, &FnDef)> = files
        .iter()
        .flat_map(|f| f.fns.iter().map(move |d| (f, d)))
        .filter(|(_, d)| d.is_query_target)
        .collect();
    if query_targets.is_empty() {
        tracing::info!("rust Pass 2: no Pass 1 node candidates found; skipping");
        return Ok(Vec::new());
    }

    let mut client = match LspClient::spawn(&bin, &repo_path).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "rust Pass 2: failed to start rust-analyzer LSP subprocess; skipping"
            );
            return Ok(Vec::new());
        }
    };
    let root_uri = file_uri(&repo_path)?;
    if let Err(e) = client.initialize(&root_uri).await {
        tracing::warn!(
            error = %e,
            "rust Pass 2: rust-analyzer failed to initialize (is the component installed?); skipping"
        );
        client.shutdown().await;
        return Ok(Vec::new());
    }

    // Wait for the workspace-ready signal (rust-analyzer's
    // `experimental/serverStatus` quiescent=true on current builds,
    // `rust-analyzer/workspaceLoaded` on older ones) until QUERY_RESERVE
    // remains in the budget. References sent before the workspace is loaded
    // block on the server's analysis lock and time out, so waiting is
    // strictly better than querying early; the reserve guarantees queries
    // still run even if the signal never arrives.
    let ready_until = deadline - QUERY_RESERVE;
    if !client.wait_workspace_loaded(ready_until).await {
        tracing::warn!(
            "rust Pass 2: no workspace-ready signal from rust-analyzer; querying anyway"
        );
    }

    // didOpen the files we indexed so the server sees the exact content we
    // parsed (also covers manifest-less directories where rust-analyzer only
    // analyzes open documents).
    for file in &files {
        client.did_open(&file.uri, &file.text);
    }
    // Look up reference sites by path; also index canonical paths so symlinked
    // roots still hit (the server may canonicalize).
    let mut by_path: HashMap<PathBuf, usize> = HashMap::new();
    let mut by_canonical: HashMap<PathBuf, usize> = HashMap::new();
    for (i, file) in files.iter().enumerate() {
        by_path.insert(file.path.clone(), i);
        if let Ok(c) = file.path.canonicalize() {
            by_canonical.insert(c, i);
        }
    }

    let total = query_targets.len();
    let mut edges: Vec<CallEdge> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut failed_queries = 0u32;

    for (i, (file, def)) in query_targets.iter().enumerate() {
        if tokio::time::Instant::now() >= deadline {
            tracing::warn!(
                budget_secs = OVERALL_BUDGET.as_secs(),
                completed = i,
                total,
                "rust Pass 2: overall budget exhausted; returning partial call edges"
            );
            break;
        }
        let locations = match client
            .references(&file.uri, def.ident_line, def.ident_character)
            .await
        {
            Some(locs) => locs,
            None => {
                failed_queries += 1;
                continue;
            }
        };
        for loc in locations {
            let Some(ref_path) = uri_to_path(loc.uri.as_str()) else {
                continue; // not a file:// location (macro expansion, etc.)
            };
            let idx = by_path.get(&ref_path).copied().or_else(|| {
                ref_path
                    .canonicalize()
                    .ok()
                    .and_then(|c| by_canonical.get(&c).copied())
            });
            let Some(ref_file) = idx.map(|i| &files[i]) else {
                continue; // reference outside the indexed tree (sysroot, deps)
            };
            // LSP lines are 0-based; FnDef lines are 1-based.
            let ref_line = loc.range.start.line as usize + 1;
            let Some(enclosing) = find_enclosing_fn(ref_file, ref_line) else {
                continue; // reference at module level (no caller function)
            };
            if seen.insert((enclosing.qname.clone(), def.qname.clone())) {
                edges.push(CallEdge {
                    from: enclosing.qname.clone(),
                    to: def.qname.clone(),
                });
            }
        }
    }
    if failed_queries > 0 {
        tracing::warn!(
            failed_queries,
            total,
            "rust Pass 2: some references queries failed or timed out; call graph may be partial"
        );
    }

    client.shutdown().await;
    tracing::info!(
        files = files.len(),
        targets = total,
        edges = edges.len(),
        "rust Pass 2: rust-analyzer call graph extraction complete"
    );
    Ok(edges)
}

// ── Binary discovery ────────────────────────────────────────────────────────

/// Locate the `rust-analyzer` binary.
///
/// Search order:
/// 1. `RUST_ANALYZER_BIN` environment variable (explicit override; also used
///    by tests to substitute a fake LSP server)
/// 2. `rust-analyzer` on `PATH`
fn find_rust_analyzer() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("RUST_ANALYZER_BIN") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    which_on_path("rust-analyzer")
}

/// Resolve an executable name on `PATH`.
fn which_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

// ── File discovery and function indexing ────────────────────────────────────

/// A single function-like definition (module-level fn, impl/trait method, or
/// local fn) with the extent and identifier position needed to (a) query
/// references at the identifier and (b) attribute reference sites to it.
#[derive(Debug)]
struct FnDef {
    /// `<module_qname>::<fn_name>`, matching Pass 1's node naming (and Pass
    /// 1's `CallCollector` caller naming for methods, which have no nodes).
    qname: String,
    /// Whether references should be queried for this definition: mirrors
    /// Pass 1's node policy (module-level `pub fn` or `#[test]` fn - those
    /// are the only functions that become `Function` nodes).
    is_query_target: bool,
    /// Extent of the whole item, 1-based inclusive. Used to attribute
    /// reference sites to their enclosing function.
    line_start: usize,
    line_end: usize,
    /// Identifier position for the LSP query, 0-based (LSP convention).
    ident_line: u32,
    ident_character: u32,
}

/// Parsed index of one `.rs` file.
struct FileIndex {
    path: PathBuf,
    uri: Uri,
    text: String,
    fns: Vec<FnDef>,
}

/// Walk `repo_root` for `.rs` files, skipping build/VCS directories (mirrors
/// Pass 1's `WalkDir` walk, which filters `target`).
fn discover_rs_files(repo_root: &Path) -> Vec<PathBuf> {
    WalkDir::new(repo_root)
        .into_iter()
        .filter_entry(|e| !SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
        .map(|e| e.into_path())
        .collect()
}

/// Derive the module qualified name Pass 1 assigns to a file's contents.
///
/// Mirrors `gyre_domain::rust_extractor::crate_and_module_from_path` plus
/// `ExtractionContext::extract_rs_file`'s `module_qname` assembly:
/// - `crates/<crate>/src/a/b.rs` -> `<crate>::a::b` (`lib.rs`/`main.rs` stems
///   drop out, leaving `<crate>`)
/// - `crates/<crate>/src/lib.rs` -> `<crate>`
/// - any other layout -> the file stem (`src/foo.rs` -> `foo`,
///   `tests/it.rs` -> `it`)
///
/// Kept in sync with the domain by contract test
/// `fake_server_edges_resolve_against_real_pass1_nodes`.
fn module_qname_for_path(rel_path: &str) -> String {
    let parts: Vec<&str> = rel_path.split('/').collect();

    if let Some(pos) = parts.iter().position(|&p| p == "crates") {
        if let Some(crate_name) = parts.get(pos + 1) {
            let src_idx = pos + 3;
            if parts.get(pos + 2) == Some(&"src") && parts.len() > src_idx {
                let module_parts: Vec<&str> = parts[src_idx..]
                    .iter()
                    .map(|p| p.trim_end_matches(".rs"))
                    .filter(|p| *p != "lib" && *p != "main")
                    .collect();
                let module_prefix = module_parts.join("::");
                return if module_prefix.is_empty() {
                    (*crate_name).to_string()
                } else {
                    format!("{crate_name}::{module_prefix}")
                };
            }
            return (*crate_name).to_string();
        }
    }

    // Fallback: the file stem is the module name.
    parts
        .last()
        .unwrap_or(&"unknown")
        .trim_end_matches(".rs")
        .to_string()
}

/// A function-like item kind. Only `Free` items can be Pass 2 query targets
/// (Pass 1 creates `Function` nodes for module-level `pub fn` / `#[test]`
/// fn only); methods still participate as enclosing functions so references
/// inside them are attributed correctly rather than to a preceding
/// module-level function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FnKind {
    Free,
    Method,
}

struct FnCollector<'a> {
    module_qname: &'a str,
    fns: Vec<FnDef>,
}

impl<'a> FnCollector<'a> {
    fn push(&mut self, ident: &syn::Ident, span: proc_macro2::Span, kind: FnKind, is_target: bool) {
        let start = span.start();
        let end = span.end();
        let ident_pos = ident.span().start();
        // proc-macro2 columns are 0-based byte offsets within the line. The
        // prefix before a fn identifier is Rust keywords and whitespace
        // (`pub fn `, `async fn `, ...), which is ASCII, so the byte column
        // equals the UTF-16 code-unit column LSP expects.
        self.fns.push(FnDef {
            qname: format!("{}::{}", self.module_qname, ident),
            is_query_target: kind == FnKind::Free && is_target,
            line_start: start.line,
            line_end: end.line,
            ident_line: (ident_pos.line - 1) as u32,
            ident_character: ident_pos.column as u32,
        });
    }
}

impl<'ast> Visit<'ast> for FnCollector<'_> {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        // Mirrors Pass 1's node policy: `pub` (exactly `pub`, not `pub(crate)`)
        // or a `#[test]`-style attribute.
        let is_target = matches!(f.vis, syn::Visibility::Public(_)) || has_test_attr(&f.attrs);
        self.push(&f.sig.ident, f.span(), FnKind::Free, is_target);
        syn::visit::visit_item_fn(self, f);
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.push(&f.sig.ident, f.span(), FnKind::Method, false);
        syn::visit::visit_impl_item_fn(self, f);
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        self.push(&f.sig.ident, f.span(), FnKind::Method, false);
        syn::visit::visit_trait_item_fn(self, f);
    }
}

/// Check for a `#[test]` or `#[...]::test` attribute (mirrors Pass 1's
/// `has_test_attr`).
fn has_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        path.is_ident("test") || path.segments.last().is_some_and(|s| s.ident == "test")
    })
}

/// Parse one `.rs` file and index its function-like items.
fn build_file_index(repo_root: &Path, path: &Path) -> Result<FileIndex> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("read error: {}", path.display()))?;
    let rel_path = path
        .strip_prefix(repo_root)
        .ok()
        .and_then(|p| p.to_str())
        .unwrap_or_default()
        .to_string();
    let module_qname = module_qname_for_path(&rel_path);

    let syntax =
        syn::parse_file(&text).with_context(|| format!("syn parse error: {}", path.display()))?;
    let mut collector = FnCollector {
        module_qname: &module_qname,
        fns: Vec::new(),
    };
    collector.visit_file(&syntax);

    Ok(FileIndex {
        path: path.to_path_buf(),
        uri: file_uri(path)?,
        text,
        fns: collector.fns,
    })
}

/// Innermost function containing `line` (1-based), or `None`.
///
/// "Innermost" = the containing definition with the latest start line (and,
/// for definitions starting on the same line, the smallest extent). Methods
/// and local fns start strictly inside their parents, so this picks the
/// closest lexical function without needing column-precise extents.
fn find_enclosing_fn(file: &FileIndex, line: usize) -> Option<&FnDef> {
    file.fns
        .iter()
        .filter(|d| d.line_start <= line && line <= d.line_end)
        // Latest start wins; on ties the smaller extent (innermost) wins.
        .max_by_key(|d| (d.line_start, std::cmp::Reverse(d.line_end)))
}

// ── URI helpers ─────────────────────────────────────────────────────────────

/// `file://` URI for an absolute path.
fn file_uri(path: &Path) -> Result<Uri> {
    let s = url::Url::from_file_path(path)
        .map_err(|_| anyhow::anyhow!("path is not absolute: {}", path.display()))?
        .to_string();
    s.parse()
        .map_err(|e| anyhow::anyhow!("invalid file URI {s}: {e}"))
}

/// Filesystem path for a `file://` URI string, or `None` for other schemes.
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

// ── LSP client ──────────────────────────────────────────────────────────────

/// A minimal LSP client over a child process's stdio.
///
/// One reader task frames incoming messages, correlates responses to pending
/// requests, replies to server-to-client requests, and watches for
/// rust-analyzer's `workspaceLoaded` notification. One writer task serializes
/// all outgoing messages (client requests, notifications, and replies to
/// server requests share the stdin pipe, so writes must not interleave).
struct LspClient {
    child: Child,
    writer_tx: mpsc::UnboundedSender<Value>,
    pending: std::sync::Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    next_id: i64,
    workspace_loaded_rx: watch::Receiver<bool>,
}

impl LspClient {
    /// Spawn the server binary and start the reader/writer/drain tasks.
    async fn spawn(bin: &Path, root: &Path) -> Result<LspClient> {
        let mut child = Command::new(bin)
            .current_dir(root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("failed to spawn rust-analyzer")?;

        let stdin = child.stdin.take().context("child stdin was not piped")?;
        let stdout = child.stdout.take().context("child stdout was not piped")?;
        let stderr = child.stderr.take().context("child stderr was not piped")?;

        // Drain stderr so the server never blocks on a full pipe; log at
        // debug (rust-analyzer is chatty in normal operation).
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "rust_analyzer", "{line}");
            }
        });

        let (writer_tx, writer_rx) = mpsc::unbounded_channel();
        tokio::spawn(writer_loop(stdin, writer_rx));

        let pending = std::sync::Arc::new(Mutex::new(HashMap::new()));
        // watch (not Notify) so a ready signal emitted before the waiter
        // subscribes is not lost.
        let (loaded_tx, loaded_rx) = watch::channel(false);
        tokio::spawn(reader_loop(
            BufReader::new(stdout),
            std::sync::Arc::clone(&pending),
            writer_tx.clone(),
            loaded_tx,
        ));

        Ok(LspClient {
            child,
            writer_tx,
            pending,
            next_id: 0,
            workspace_loaded_rx: loaded_rx,
        })
    }

    /// `initialize` request + `initialized` notification. Fails on timeout,
    /// server error response, or connection loss. The workspace-ready wait
    /// is [`Self::wait_workspace_loaded`], driven by the caller against the
    /// overall budget.
    async fn initialize(&mut self, root_uri: &Uri) -> Result<()> {
        #[allow(deprecated)] // rootUri: deprecated in LSP 3.16 in favor of
        // workspaceFolders, but universally supported by servers including
        // rust-analyzer, and simpler than folder objects for a single root.
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(root_uri.clone()),
            capabilities: ClientCapabilities {
                // Opt in to rust-analyzer's `experimental/serverStatus`
                // notifications; `quiescent: true` is the workspace-ready
                // signal this client waits for (older builds send a custom
                // `rust-analyzer/workspaceLoaded` notification instead).
                experimental: Some(json!({
                    "serverStatusNotification": true
                })),
                ..Default::default()
            },
            ..Default::default()
        };
        let response = self
            .request("initialize", serde_json::to_value(&params)?, INIT_TIMEOUT)
            .await?;
        if let Some(error) = response.get("error") {
            bail!("initialize rejected by server: {error}");
        }
        self.notify("initialized", json!({}));
        Ok(())
    }

    /// Wait for the workspace-ready signal until `until` (an absolute
    /// deadline). `false` if the deadline passed (or the server exited)
    /// without a ready signal.
    async fn wait_workspace_loaded(&self, until: tokio::time::Instant) -> bool {
        let mut rx = self.workspace_loaded_rx.clone();
        if *rx.borrow() {
            return true;
        }
        let now = tokio::time::Instant::now();
        let timeout = until.saturating_duration_since(now);
        match tokio::time::timeout(timeout, rx.changed()).await {
            Ok(Ok(())) => *rx.borrow(),
            // Sender dropped (server exited) / timed out / lagged.
            Ok(Err(_)) | Err(_) => *rx.borrow(),
        }
    }

    /// `textDocument/didOpen` notification for an indexed file.
    fn did_open(&self, uri: &Uri, text: &str) {
        let item = TextDocumentItem {
            uri: uri.clone(),
            language_id: "rust".to_string(),
            version: 1,
            text: text.to_string(),
        };
        let params = DidOpenTextDocumentParams {
            text_document: item,
        };
        self.notify(
            "textDocument/didOpen",
            serde_json::to_value(&params).expect("serializable"),
        );
    }

    /// `textDocument/references` at a position. `None` on timeout, error, or
    /// a null/invalid result (treated as "no references").
    async fn references(&mut self, uri: &Uri, line: u32, character: u32) -> Option<Vec<Location>> {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let response = self
            .request(
                "textDocument/references",
                serde_json::to_value(&params).expect("serializable"),
                REQUEST_TIMEOUT,
            )
            .await
            .ok()?;
        let result = response.get("result")?;
        if result.is_null() {
            return None;
        }
        // Parse locations leniently: rust-analyzer may report synthetic
        // positions for macro-generated code; skip entries that do not
        // deserialize instead of dropping the whole batch.
        let raw = result.as_array()?;
        let locations = raw
            .iter()
            .filter_map(|v| serde_json::from_value::<Location>(v.clone()).ok())
            .collect::<Vec<_>>();
        Some(locations)
    }

    /// Send a request and await its response (the full JSON-RPC response
    /// object). Fails on timeout or connection loss.
    async fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.writer_tx
            .send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .context("LSP writer channel closed")?;

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => Ok(response),
            // Sender dropped: the reader hit EOF (server exited) and drained
            // pending requests.
            Ok(Err(_)) => bail!("LSP server closed the connection"),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                bail!("LSP request '{method}' timed out")
            }
        }
    }

    fn notify(&self, method: &str, params: Value) {
        let _ = self.writer_tx.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    /// `shutdown` request, `exit` notification, then make sure the child is
    /// gone (wait, then kill).
    async fn shutdown(mut self) {
        let _ = self
            .request("shutdown", Value::Null, SHUTDOWN_TIMEOUT)
            .await;
        self.notify("exit", Value::Null);
        if tokio::time::timeout(SHUTDOWN_TIMEOUT, self.child.wait())
            .await
            .is_err()
        {
            tracing::debug!("rust Pass 2: rust-analyzer did not exit after shutdown; killing");
            if let Err(e) = self.child.kill().await {
                tracing::debug!(error = %e, "rust Pass 2: kill failed (process already gone?)");
            }
        }
    }
}

/// Frame and write every outgoing message. Owned by a dedicated task because
/// client requests and replies to server requests both write to stdin and
/// must not interleave mid-packet.
async fn writer_loop(mut stdin: ChildStdin, mut rx: mpsc::UnboundedReceiver<Value>) {
    while let Some(msg) = rx.recv().await {
        let Ok(body) = serde_json::to_string(&msg) else {
            continue;
        };
        let packet = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        if stdin.write_all(packet.as_bytes()).await.is_err() {
            break;
        }
        if stdin.flush().await.is_err() {
            break; // server closed stdin; nothing more can be written
        }
    }
}

/// Read framed messages, route them, and fail all pending requests on EOF.
async fn reader_loop(
    mut reader: BufReader<ChildStdout>,
    pending: std::sync::Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    writer_tx: mpsc::UnboundedSender<Value>,
    workspace_loaded: watch::Sender<bool>,
) {
    while let Some(msg) = read_lsp_message(&mut reader).await {
        let id = msg.get("id").and_then(Value::as_i64);
        let method = msg.get("method").and_then(Value::as_str);
        match (id, method) {
            // Server-to-client request: reply so the server never blocks.
            // `workspace/configuration` must get an array (rust-analyzer
            // treats missing entries as "use the default"); progress/register
            // calls get a null result; anything else gets a standard
            // MethodNotFound error, which LSP servers must tolerate from
            // clients that do not implement the capability.
            (Some(id), Some(method)) => {
                let reply = match method {
                    "workspace/configuration" => {
                        json!({ "jsonrpc": "2.0", "id": id, "result": [] })
                    }
                    "window/workDoneProgress/create"
                    | "client/registerCapability"
                    | "client/unregisterCapability" => {
                        json!({ "jsonrpc": "2.0", "id": id, "result": null })
                    }
                    _ => {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32601,
                                "message": format!("client does not implement '{method}'"),
                            },
                        })
                    }
                };
                let _ = writer_tx.send(reply);
            }
            // Response to one of our requests.
            (Some(id), None) => {
                let tx = pending.lock().await.remove(&id);
                if let Some(tx) = tx {
                    let _ = tx.send(msg);
                }
            }
            // Notification.
            (None, Some(method)) => {
                let ready = match method {
                    // Legacy ready signal.
                    "rust-analyzer/workspaceLoaded" => true,
                    // Current signal: opt-in status notification with
                    // quiescent == true (all workspace/loading queues idle).
                    "experimental/serverStatus" => msg
                        .get("params")
                        .and_then(|p| p.get("quiescent"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    _ => false,
                };
                if ready {
                    let _ = workspace_loaded.send(true);
                }
            }
            (None, None) => {}
        }
    }

    // EOF: the server is gone. Dropping the pending senders fails every
    // in-flight request with "connection closed".
    pending.lock().await.clear();
}

/// Read one Content-Length framed JSON-RPC message, or `None` on EOF or a
/// malformed frame.
async fn read_lsp_message(reader: &mut BufReader<ChildStdout>) -> Option<Value> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await.ok()?;
        if n == 0 {
            return None; // EOF
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break; // end of headers
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = value.trim().parse().ok();
        }
        // Other headers (Content-Type) are ignored.
    }
    let len = content_length?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await.ok()?;
    serde_json::from_slice(&body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::graph::EdgeType;
    use gyre_ports::CallGraphExtractor as _;

    /// Serializes tests that read or write `RUST_ANALYZER_BIN`: `set_var`
    /// mutates process-global state, so binary discovery must not race
    /// across tests that exercise the Rust Pass 2 path.
    static ENV_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

    /// Set an env var for the duration of a scope and restore the previous
    /// value on exit - even on panic. Without the restore, a panicking test
    /// leaks `RUST_ANALYZER_BIN` into concurrently running tests (observed:
    /// the real-rust-analyzer test probed a leftover fake-server wrapper and
    /// extracted against the wrong fixture).
    struct EnvRestore {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvRestore {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            EnvRestore { key, prev }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    /// Write a minimal Rust crate used by every LSP-level test. Layout and
    /// the expected qualified names (derived from the Pass 1 naming rules,
    /// NOT from this module's code under test):
    ///
    /// - `src/alpha.rs` -> module `alpha`
    ///   - `pub fn alpha_target` (query target; the only callee)
    ///   - `fn private_caller` calls it (private: no Pass 1 node)
    /// - `src/beta.rs` -> module `beta`
    ///   - `pub fn beta_caller` calls it (pub: a Pass 1 node)
    ///   - `impl Widget { pub fn spin }` calls it (method: no Pass 1 node)
    fn write_rust_fixture(dir: &Path) {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"fx\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub mod alpha;\npub mod beta;\n").unwrap();
        std::fs::write(
            dir.join("src/alpha.rs"),
            "pub fn alpha_target() {}\n\nfn private_caller() {\n    alpha_target();\n}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/beta.rs"),
            "pub fn beta_caller() {\n    crate::alpha::alpha_target();\n}\n\npub struct Widget;\n\nimpl Widget {\n    pub fn spin(&self) {\n        crate::alpha::alpha_target();\n    }\n}\n",
        )
        .unwrap();
    }

    /// Make a file executable (unix).
    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A shell wrapper that runs the fake LSP server with canned references
    /// and a request log, usable as an `RUST_ANALYZER_BIN` value.
    #[cfg(unix)]
    fn write_fake_server_wrapper(dir: &Path, refs: &serde_json::Value, log: &Path) -> PathBuf {
        let refs_path = dir.join("fake-refs.json");
        std::fs::write(&refs_path, serde_json::to_string(refs).unwrap()).unwrap();
        let script_path = dir.join("fake-rust-analyzer");
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/fake-rust-analyzer.py"
        );
        std::fs::write(
            &script_path,
            format!("#!/bin/sh\nexec python3 {fixture:?} {refs_path:?} {log:?}\n"),
        )
        .unwrap();
        make_executable(&script_path);
        script_path
    }

    fn fixture_uri(path: &Path) -> String {
        url::Url::from_file_path(path.canonicalize().unwrap())
            .unwrap()
            .to_string()
    }

    /// Canned reference data: `alpha_target` (queried at alpha.rs line 0,
    /// character 7) is referenced from `private_caller` (alpha.rs line 3,
    /// 0-based), `beta_caller` (beta.rs line 1) and `Widget::spin`
    /// (beta.rs line 8); `beta_caller` (queried at beta.rs line 0, character
    /// 7) has no references.
    fn fake_references(alpha_uri: &str, beta_uri: &str) -> serde_json::Value {
        let loc = |uri: &str, line: u32| {
            serde_json::json!({
                "uri": uri,
                "range": {
                    "start": {"line": line, "character": 4},
                    "end": {"line": line, "character": 20},
                },
            })
        };
        serde_json::json!({
            format!("{alpha_uri}|0|7"): [
                loc(alpha_uri, 3),
                loc(beta_uri, 1),
                loc(beta_uri, 8),
            ],
            format!("{beta_uri}|0|7"): [],
        })
    }

    /// The three edges the canned reference data must produce. The callers
    /// use Pass 1's caller-naming convention (`module::fn`; methods are
    /// `module::method`, matching Pass 1's `CallCollector`).
    fn expected_fake_edges() -> Vec<CallEdge> {
        vec![
            CallEdge {
                from: "alpha::private_caller".to_string(),
                to: "alpha::alpha_target".to_string(),
            },
            CallEdge {
                from: "beta::beta_caller".to_string(),
                to: "alpha::alpha_target".to_string(),
            },
            CallEdge {
                from: "beta::spin".to_string(),
                to: "alpha::alpha_target".to_string(),
            },
        ]
    }

    #[test]
    fn module_qname_matches_pass1_layouts() {
        // Expected values transcribed from gyre-domain's
        // `crate_and_module_from_path` + `extract_rs_file` naming rules (the
        // same rules `fake_server_edges_resolve_against_real_pass1_nodes`
        // pins end-to-end against the real Pass 1 extractor).
        assert_eq!(
            module_qname_for_path("crates/gyre-adapters/src/call_graph_rust.rs"),
            "gyre-adapters::call_graph_rust"
        );
        // lib.rs / main.rs stems drop out, leaving the bare crate name.
        assert_eq!(
            module_qname_for_path("crates/gyre-domain/src/lib.rs"),
            "gyre-domain"
        );
        assert_eq!(
            module_qname_for_path("crates/gyre-server/src/main.rs"),
            "gyre-server"
        );
        // Fallback layout: the file stem is the module name.
        assert_eq!(module_qname_for_path("src/lib.rs"), "lib");
        assert_eq!(module_qname_for_path("src/alpha.rs"), "alpha");
        assert_eq!(module_qname_for_path("tests/it.rs"), "it");
        // A crate dir without src/: bare crate name.
        assert_eq!(module_qname_for_path("crates/gyre-x/other.rs"), "gyre-x");
    }

    #[tokio::test]
    async fn missing_binary_degrades_to_empty() {
        // No rust-analyzer anywhere: Pass 2 skips with an empty edge set,
        // never an error (port contract).
        let dir = tempfile::TempDir::new().unwrap();
        write_rust_fixture(dir.path());
        let edges = extract_with_binary(dir.path(), None)
            .await
            .expect("Pass 2 must never error");
        assert!(edges.is_empty());
    }

    #[tokio::test]
    async fn binary_that_exits_immediately_degrades_to_empty() {
        // Models the rustup-shim case: `rust-analyzer` resolves on PATH (the
        // spawn succeeds) but the component is not installed, so the process
        // exits before speaking LSP. The initialize handshake must fail fast
        // (EOF on stdout), not hang, and Pass 2 degrades to empty.
        let dir = tempfile::TempDir::new().unwrap();
        write_rust_fixture(dir.path());
        let stub = dir.path().join("broken-rust-analyzer");
        std::fs::write(&stub, "#!/bin/sh\nexit 1\n").unwrap();
        make_executable(&stub);

        let edges = extract_with_binary(dir.path(), Some(stub))
            .await
            .expect("Pass 2 must never error");
        assert!(edges.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_lsp_session_produces_edges_and_correct_requests() {
        let _guard = ENV_LOCK.lock().await;
        let dir = tempfile::TempDir::new().unwrap();
        write_rust_fixture(dir.path());
        let canonical = dir.path().canonicalize().unwrap();
        let alpha_uri = fixture_uri(&canonical.join("src/alpha.rs"));
        let beta_uri = fixture_uri(&canonical.join("src/beta.rs"));
        let log_path = canonical.join("requests.log");

        let fake_bin = write_fake_server_wrapper(
            dir.path(),
            &fake_references(&alpha_uri, &beta_uri),
            &log_path,
        );

        // Drive the real port implementation through the real dispatch
        // (SubprocessCallGraphExtractor selects the rust-analyzer LSP path
        // for Language::Rust).
        let _env = EnvRestore::set("RUST_ANALYZER_BIN", &fake_bin);
        let extractor = crate::SubprocessCallGraphExtractor::new();
        let edges = extractor
            .extract_call_edges(dir.path(), gyre_common::call_graph::Language::Rust)
            .await
            .expect("Pass 2 must never error");
        drop(_env);

        let mut actual: Vec<(String, String)> = edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        actual.sort();
        let mut expected: Vec<(String, String)> = expected_fake_edges()
            .into_iter()
            .map(|e| (e.from, e.to))
            .collect();
        expected.sort();
        assert_eq!(
            actual, expected,
            "edges must map reference sites to Calls edges"
        );

        // Assert the LSP conversation the adapter actually conducted, from
        // the fake server's request log.
        let log: Vec<serde_json::Value> = std::fs::read_to_string(&log_path)
            .unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();

        // initialize with the repo root, then initialized.
        let init = log
            .iter()
            .find(|m| m["method"] == "initialize")
            .expect("initialize request must be sent");
        assert_eq!(init["params"]["rootUri"], fixture_uri(&canonical));
        assert!(log.iter().any(|m| m["method"] == "initialized"));

        // didOpen for every indexed file, as Rust documents.
        let opened: Vec<&str> = log
            .iter()
            .filter(|m| m["method"] == "textDocument/didOpen")
            .map(|m| m["params"]["textDocument"]["uri"].as_str().unwrap())
            .collect();
        let lib_uri = fixture_uri(&canonical.join("src/lib.rs"));
        for expected_uri in [&alpha_uri, &beta_uri, &lib_uri] {
            assert!(
                opened.contains(&expected_uri.as_str()),
                "didOpen missing for {expected_uri}"
            );
        }

        // references queried at the exact identifier positions, and only for
        // Pass 1 node candidates: alpha_target (alpha.rs line 1, col 7) and
        // beta_caller (beta.rs line 1, col 7) - not private_caller, not spin.
        let queries: Vec<(String, u32, u32)> = log
            .iter()
            .filter(|m| m["method"] == "textDocument/references")
            .map(|m| {
                (
                    m["params"]["textDocument"]["uri"]
                        .as_str()
                        .unwrap()
                        .to_string(),
                    m["params"]["position"]["line"].as_u64().unwrap() as u32,
                    m["params"]["position"]["character"].as_u64().unwrap() as u32,
                )
            })
            .collect();
        let mut expected_queries = vec![(alpha_uri.clone(), 0, 7), (beta_uri.clone(), 0, 7)];
        expected_queries.sort();
        let mut queries = queries;
        queries.sort();
        assert_eq!(
            queries, expected_queries,
            "references must be queried at pub fn identifier positions only"
        );

        // Clean shutdown.
        assert!(log.iter().any(|m| m["method"] == "shutdown"));
        assert!(log.iter().any(|m| m["method"] == "exit"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fake_server_edges_resolve_against_real_pass1_nodes() {
        // The qname-format contract test (Go's task-072 F6 class): run the
        // REAL Pass 1 extractor over the same fixture, then the REAL domain
        // resolver over the adapter's edges. Only `beta::beta_caller ->
        // alpha::alpha_target` connects two Pass 1 nodes; the private-fn and
        // method callers have no nodes and must be dropped, not mis-linked.
        let _guard = ENV_LOCK.lock().await;
        let dir = tempfile::TempDir::new().unwrap();
        write_rust_fixture(dir.path());
        let canonical = dir.path().canonicalize().unwrap();
        let alpha_uri = fixture_uri(&canonical.join("src/alpha.rs"));
        let beta_uri = fixture_uri(&canonical.join("src/beta.rs"));
        let log_path = canonical.join("requests.log");

        let fake_bin = write_fake_server_wrapper(
            dir.path(),
            &fake_references(&alpha_uri, &beta_uri),
            &log_path,
        );

        let _env = EnvRestore::set("RUST_ANALYZER_BIN", &fake_bin);
        let extractor = crate::SubprocessCallGraphExtractor::new();
        let edges = extractor
            .extract_call_edges(dir.path(), gyre_common::call_graph::Language::Rust)
            .await
            .expect("Pass 2 must never error");
        drop(_env);
        assert_eq!(edges.len(), 3);

        // Real Pass 1: RustExtractor over the same fixture.
        use gyre_domain::extractor::LanguageExtractor;
        let pass1 = gyre_domain::rust_extractor::RustExtractor.extract(&canonical, "deadbeef");
        assert!(
            pass1.errors.is_empty(),
            "fixture must parse cleanly: {} errors",
            pass1.errors.len()
        );
        let node = |qname: &str| {
            pass1
                .nodes
                .iter()
                .find(|n| n.qualified_name == qname)
                .unwrap_or_else(|| panic!("Pass 1 node {qname} must exist"))
        };
        let alpha_target = node("alpha::alpha_target");
        let beta_caller = node("beta::beta_caller");
        // Pass 1 creates no Function nodes for private fns or impl methods,
        // so those Pass 2 edges must not resolve.
        assert!(pass1
            .nodes
            .iter()
            .all(|n| n.qualified_name != "alpha::private_caller"));
        assert!(pass1.nodes.iter().all(|n| n.qualified_name != "beta::spin"));

        let repo_id = gyre_common::Id::new("repo-fx");
        let resolved = gyre_domain::call_graph_resolve::resolve_call_edges(
            gyre_common::call_graph::Language::Rust,
            &edges,
            &pass1.nodes,
            &[],
            &repo_id,
        );
        assert_eq!(resolved.len(), 1, "only the pub-to-pub edge resolves");
        assert_eq!(resolved[0].source_id, beta_caller.id);
        assert_eq!(resolved[0].target_id, alpha_target.id);
        assert_eq!(resolved[0].edge_type, EdgeType::Calls);
    }

    /// A working `rust-analyzer` (env override first, then PATH), verified by
    /// actually running `--version`: a rustup shim whose component is not
    /// installed exists on PATH but cannot serve LSP, and must not be
    /// mistaken for a working binary.
    fn working_rust_analyzer() -> Option<PathBuf> {
        let bin = find_rust_analyzer()?;
        let ok = std::process::Command::new(&bin)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        ok.then_some(bin)
    }

    #[tokio::test]
    async fn real_rust_analyzer_extracts_known_call_edges() {
        // Probe under the lock: without it, a concurrently running fake-server
        // test could have RUST_ANALYZER_BIN set at probe time (each such test
        // restores it before releasing the lock, but the probe must not race
        // that window).
        let _guard = ENV_LOCK.lock().await;
        let Some(bin) = working_rust_analyzer() else {
            eprintln!("skipping: no working rust-analyzer binary on PATH or RUST_ANALYZER_BIN");
            return;
        };
        let dir = tempfile::TempDir::new().unwrap();
        write_rust_fixture(dir.path());

        let edges = extract_with_binary(dir.path(), Some(bin))
            .await
            .expect("Pass 2 must never error");

        // Real rust-analyzer must report the three reference sites of
        // alpha_target: inside private_caller (alpha.rs), beta_caller
        // (beta.rs) and Widget::spin (beta.rs). The references query is
        // includeDeclaration=false, so the definition itself is excluded.
        let mut actual: Vec<(String, String)> = edges
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        actual.sort();
        let mut expected: Vec<(String, String)> = expected_fake_edges()
            .into_iter()
            .map(|e| (e.from, e.to))
            .collect();
        expected.sort();
        assert_eq!(
            actual, expected,
            "real rust-analyzer must resolve the fixture's call sites"
        );
    }
}
