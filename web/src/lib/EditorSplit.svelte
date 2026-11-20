<script>
  import ArchPreviewCanvas from './ArchPreviewCanvas.svelte';
  import ConcurrentEditBanner from './ConcurrentEditBanner.svelte';
  import SpecChatInput from './SpecChatInput.svelte';
  import SpecConflictDialog from './SpecConflictDialog.svelte';
  import SpecDiffView from './SpecDiffView.svelte';
  import SpecSuggestion from './SpecSuggestion.svelte';
  import { api } from './api.js';
  import { toastError, toastSuccess, toastInfo } from './toast.svelte.js';
  import { t } from 'svelte-i18n';
  import { lineDiff, diffSummary } from './lineDiff.js';
  import { applyDiffOps, streamAssist, suggestionText } from './specAssist.js';
  import { normalizeGhosts } from './ghostOverlay.js';

  /**
   * EditorSplit — full-width editor + architecture preview split layout
   * (ui-layout.md §2 Editor Split).
   *
   * Used by DetailPanel (spec editing pop-out) and MetaSpecs (meta-spec editing,
   * §9 preview loop). Left panel: spec textarea + inline LLM chat. Right panel:
   * architectural impact preview with Architecture (default) and Code Diff tabs.
   *
   * Spec ref: ui-layout.md §2 Editor Split,
   *           §3 LLM-Assisted Spec Editing (Accept/Edit/Dismiss),
   *           §9 Meta-specs Preview Loop (Editing → Preview Running → Preview Complete)
   *
   * Props:
   *   content       — string — current text content (bindable)
   *   onChange       — (newContent: string) => void — called on every change
   *   repoId        — string | null
   *   specPath      — string | null — e.g. 'specs/system/auth.md'
   *   ghostOverlays — array of { nodeId, type: 'new'|'modified'|'removed' }
   *   onClose       — () => void — called when user dismisses (Back or Esc)
   *   context       — 'spec' | 'meta-spec' — display label
   *   workspaceId   — string | null — workspace scope for the presence banner (HSI §7)
   *   wsStore       — WebSocket store ({ send, sessionId, onMessage }) for live presence
   *   selfUserId    — string | null — current user id (excludes own sessions)
   *   baseSha       — string | null — spec current_sha at load (optimistic concurrency)
   *   targetSpecs   — array of { path, repo_id } — selectable preview targets
   *                   (§9 State 1). When provided (meta-spec context) the right
   *                   panel shows the target spec selector in the editing state;
   *                   repo_id is required — preview agents spawn per target repo
   *                   (meta-spec-reconciliation.md §5).
   *   draftKind     — meta-spec kind sent as the draft's `kind` in the preview
   *                   POST body (§5: { draft: { kind, content }, targets }).
   *   onpublish     — () => void — Publish hook (meta-spec approval flow, §9 State 3)
   *   baselineContent — string | null — the published baseline of the content
   *                   (e.g. the meta-spec registry's stored prompt). While a
   *                   preview runs, the locked left pane renders the diff of
   *                   pending changes against this baseline (§9 State 2
   *                   "shows diff of changes"); null → verbatim draft.
   *   onPreviewStateChange — (state: 'editing' | 'preview_running' | 'preview_complete') => void
   *                   — notified on every preview state transition so the embedding
   *                   page (e.g. MetaSpecs) can lock its own controls while a preview
   *                   runs (§9 State 2 "locked during preview").
   */
  let {
    content = $bindable(''),
    onChange = undefined,
    repoId = null,
    specPath = null,
    ghostOverlays = [],
    onClose = undefined,
    context = 'spec',
    workspaceId = null,
    wsStore = null,
    selfUserId = null,
    baseSha = null,
    targetSpecs = [],
    draftKind = 'meta:persona',
    onpublish = undefined,
    baselineContent = null,
    onPreviewStateChange = undefined,
  } = $props();

  // 409 conflict body from the last save attempt, or null (HSI §7).
  let specConflict = $state(null);

  // ── Graph data (lazy-loaded) ───────────────────────────────────────────────
  // The fast preview combines the repo's current graph (real nodes/edges from
  // the knowledge graph) with graphPredict ghost overlays (ui-layout.md §2
  // Editor Split — Phase 1 fast preview; Phase 2 thorough preview computes a
  // real ArchitecturalDelta server-side).
  let graphNodes = $state([]);
  let graphEdges = $state([]);
  let graphLoading = $state(false);
  let graphLoaded = $state(false);
  // Ghost overlays derived from graphPredict predictions (system-explorer.md
  // §3 fast tier): normalized via the shared ghostOverlay.js model so the
  // canvas renders the full ghost visual language (action colors,
  // confidence opacity, tooltips, dotted edges) — the same normalization
  // DetailPanel uses.
  let derivedGhosts = $state([]);
  // Instant tier (§3 "Link graph impact"): connected specs, repos
  // implementing this spec, governed node count — computed from real link
  // + graph data the moment editing begins, shown in the impact bar.
  let impact = $state(null);

  // Effective overlays: caller-supplied ghosts (e.g. DetailPanel arch tab)
  // take precedence; otherwise the locally predicted ones. Both shapes
  // normalize through the shared model.
  let overlays = $derived(
    normalizeGhosts(ghostOverlays.length ? ghostOverlays : derivedGhosts),
  );

  // ── Preview state machine (ui-layout.md §2/§9) ─────────────────────────────
  // editing → preview_running → preview_complete (Iterate returns to editing
  // with results still visible, §9 State 3). A preview that fails, times out,
  // or is unavailable never presents State 3 — it returns to editing (the
  // Architecture tab keeps the fast graphPredict overlays) with an error banner.
  let previewState = $state('editing');
  let previewError = $state(null);
  let selectedSpecKeys = $state([]);

  // Targets are {repo_id, spec_path} pairs: two workspace repos can expose
  // the same spec path, so paths alone are not unique keys (keyed each
  // blocks would collide). Keys and selection use the composite.
  function specKey(repoId, path) { return `${repoId ?? ''}:${path ?? ''}`; }
  let previewPollTimer = null;
  let previewProgress = $state([]);
  let codeDiffFiles = $state([]); // [{ path, diff: [{op,text}] }]
  let previewRunning = $state(false);
  let activeImpactTab = $state('architecture');
  // Agent spawned by the thorough preview (§3 thorough tier) — used to
  // resolve the completion MR and its real code diff.
  let previewAgentId = $state(null);
  // Last meta-spec preview status (real §5 shape: preview_id, state,
  // agents: [{agent_id, repo_id, spec_path, branch, status, diff}]) rendered
  // in the right pane.
  let metaPreviewResult = $state(null);

  // Pending-changes diff for the locked editor (§9 State 2 "shows diff of
  // changes"): the draft against the published baseline. Only meaningful
  // while locked; null when no baseline is available (verbatim fallback).
  let lockedDiffRows = $derived.by(() => {
    if (previewState !== 'preview_running') return null;
    // No baseline (null/undefined/empty) or an unchanged draft → verbatim.
    if (!baselineContent || baselineContent === content) return null;
    return lineDiff(baselineContent, content);
  });
  let lockedDiffStats = $derived(
    lockedDiffRows ? diffSummary(lockedDiffRows) : null,
  );

  const selectableSpecs = $derived(
    Array.isArray(targetSpecs) ? targetSpecs.filter((s) => s?.path) : [],
  );

  $effect(() => {
    // Load graph once when repoId is available
    if (repoId && !graphLoaded && !graphLoading) {
      loadGraph();
    }
  });

  // Instant tier (§3 "Link graph impact", milliseconds): the moment spec
  // editing begins, compute the link-graph impact — connected specs (spec
  // links API), repos implementing this spec (inbound implements links +
  // the repos whose graphs govern this spec's nodes), governed node count.
  // Rendered as the compact info bar above the editor. Best-effort: a
  // failing segment degrades to the others, never blocks editing.
  $effect(() => {
    if (repoId && specPath) loadImpact(repoId, specPath);
  });

  // ── Fast tier: debounced structural prediction (§3 "Fast (2-5 seconds)") ──
  // As the user edits, predictions are debounced (PREDICT_DEBOUNCE_MS) and
  // fired against POST /repos/:id/graph/predict with the current draft;
  // the response's predictions render as ghost overlays on the canvas.
  // Suppressed while a thorough preview runs (editor locked, §9 State 2)
  // and after a save (editing ended — the overlay cleared below).
  const PREDICT_DEBOUNCE_MS = 1200;
  let predicting = $state(false);
  let lastPredictedContent = null;

  $effect(() => {
    const draft = content;
    const rid = repoId;
    const path = specPath;
    if (!rid || !path) return;
    if (previewState !== 'editing') return;
    if (draft === lastPredictedContent) return;
    const timer = setTimeout(() => {
      runPrediction(rid, path, draft);
    }, PREDICT_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  async function runPrediction(rid, path, draft) {
    predicting = true;
    try {
      const result = await api.graphPredict(rid, {
        spec_path: path,
        draft_content: draft || undefined,
      });
      const predictions = result?.predictions ?? result?.overlays ?? [];
      derivedGhosts = normalizeGhosts(predictions);
      lastPredictedContent = draft;
    } catch {
      // Best-effort (§3: predictions are probabilistic feedback, not a
      // gate): keep the previous overlay, clear the loading indicator.
      lastPredictedContent = draft;
    } finally {
      predicting = false;
    }
  }

  async function loadImpact(rid, path) {
    try {
      // Connected specs: every spec link touching this path (either
      // direction), from the spec links API.
      let connectedSpecs = [];
      let reposImplementing = [];
      try {
        const links = await api.specLinks(path, rid);
        const connected = new Set();
        const implementing = new Set();
        for (const link of links ?? []) {
          const other =
            link?.source_path === path ? link?.target_path : link?.source_path;
          if (other) connected.add(other);
          const lt = String(link?.link_type ?? '').toLowerCase();
          if (lt === 'implements' && link?.target_path === path && link?.source_repo_id) {
            implementing.add(link.source_repo_id);
          }
        }
        connectedSpecs = [...connected].sort();
        reposImplementing = [...implementing];
      } catch {
        // links unavailable — governed-node counts below still render
      }

      // Repos implementing this spec + governed node count, from the
      // loaded graph(s): the current repo's nodes filtered by spec_path.
      let governedNodes = null;
      try {
        const graph = await api.repoGraph(rid);
        const specNodes = (graph?.nodes ?? []).filter(
          (n) => n.spec_path === path && !n.deleted_at,
        );
        governedNodes = specNodes.length;
        if (specNodes.length && !reposImplementing.includes(rid)) {
          reposImplementing.push(rid);
        }
      } catch {
        // graph unavailable — links segment still renders
      }

      impact = { connectedSpecs, reposImplementing, governedNodes };
    } catch {
      impact = null;
    }
  }

  // Stop any preview poll when the component is destroyed.
  $effect(() => {
    return () => stopPreviewPoll();
  });

  // Notify the embedding page of preview state transitions (e.g. MetaSpecs
  // locks its persona selector while a preview runs — §9 State 2).
  $effect(() => {
    onPreviewStateChange?.(previewState);
  });

  async function loadGraph() {
    if (!repoId) return;
    graphLoading = true;
    try {
      // Base graph: real nodes/edges from the knowledge graph. The spec-path
      // filter mirrors the DetailPanel architecture tab: nodes governed by
      // this spec plus edges between them. Falls back to the full graph when
      // no node carries this spec_path (e.g. new/unlinked specs).
      let nodes = [];
      let edges = [];
      try {
        const graph = await api.repoGraph(repoId);
        const allNodes = graph?.nodes ?? [];
        const allEdges = graph?.edges ?? [];
        const specNodes = specPath ? allNodes.filter((n) => n.spec_path === specPath) : allNodes;
        if (specNodes.length) {
          const specNodeIds = new Set(specNodes.map((n) => n.id));
          edges = allEdges.filter(
            (e) => specNodeIds.has(e.source_id ?? e.source) && specNodeIds.has(e.target_id ?? e.target),
          );
        } else {
          specNodes.push(...allNodes);
          edges = allEdges;
        }
        nodes = specNodes;
      } catch {
        // graceful: fall through to an empty canvas — the debounced
        // graphPredict ghosts still render via the ghost overlay layer
      }

      graphNodes = nodes;
      graphEdges = edges;
      graphLoaded = true;
    } finally {
      graphLoading = false;
    }
  }

  // ── Preview workflow (§2/§9) ────────────────────────────────────────────────

  function toggleSpec(repoId, path) {
    const key = specKey(repoId, path);
    if (selectedSpecKeys.includes(key)) {
      selectedSpecKeys = selectedSpecKeys.filter((k) => k !== key);
    } else {
      selectedSpecKeys = [...selectedSpecKeys, key];
    }
  }

  function selectAllSpecs() {
    selectedSpecKeys = selectableSpecs.map((s) => specKey(s.repo_id, s.path));
  }

  function clearAllSpecs() {
    selectedSpecKeys = [];
  }

  const canRunPreview = $derived.by(() => {
    if (previewState !== 'editing' || previewRunning) return false;
    // Meta-spec context: at least one target spec with a repo must be selected
    // (§9 State 1) — the §5 preview endpoint spawns one agent per { repo_id,
    // spec_path } target, so a target without repo_id can never run.
    if (context === 'meta-spec')
      return selectableSpecs.some((s) => selectedSpecKeys.includes(specKey(s.repo_id, s.path)) && s.repo_id);
    // Spec context: needs a repo + spec path.
    return Boolean(repoId && specPath);
  });

  /**
   * Whether the §3/§9 LLM chat is usable: the repo-scoped specs/assist in
   * the spec context (needs a repo), or the workspace-scoped
   * meta-specs/assist in the §9 meta-spec context (needs a workspace —
   * the MetaSpecs workspace view always provides one).
   */
  const llmChatEnabled = $derived(
    Boolean(repoId || (context === 'meta-spec' && workspaceId)),
  );

  /**
   * Whether the right pane renders the Architecture/Code Diff tabs.
   * Always true in the spec context and in §9 State 3; in the meta-spec
   * editing state only when §9 Iterate retained the last preview result —
   * "Iterate: returns to State 1 with the preview results still visible"
   * (ui-layout.md §9) — so the tabs stack below the target-spec selector.
   */
  const showResultTabs = $derived(
    context !== 'meta-spec' || previewState !== 'editing' || Boolean(metaPreviewResult),
  );

  /**
   * Start the thorough preview: the server commits the draft to a throwaway
   * branch, agents implement, and the resulting graph delta + code diff are
   * reported (ui-layout.md §2 Editor Split right panel).
   */
  async function runPreview() {
    if (!canRunPreview) return;
    previewRunning = true;
    previewError = null;
    stopPreviewPoll();
    try {
      if (context === 'meta-spec') {
        // Meta-spec preview loop (§9): real agent runs on throwaway preview
        // branches (meta-spec-reconciliation.md §5) — one agent per selected
        // target, no gates/MR/provenance. Response 202: { preview_id, agents:
        // [{ agent_id, repo_id, spec_path, branch }] }.
        const targets = selectableSpecs
          .filter((s) => selectedSpecKeys.includes(specKey(s.repo_id, s.path)) && s.repo_id)
          .map((s) => ({ repo_id: s.repo_id, spec_path: s.path }));
        let res;
        try {
          res = await api.previewMetaSpec({
            draft: { kind: draftKind, content },
            targets,
          });
        } catch (e) {
          // Preview service unavailable — return to editing; never present
          // §9 State 3 without a real result.
          failPreview($t('editor_split.preview_unavailable'), e);
          return;
        }
        if (res?.preview_id && Array.isArray(res.agents)) {
          pollPreviewRun(res.preview_id, res);
        } else {
          failPreview($t('editor_split.preview_unavailable'));
        }
      } else if (repoId && specPath) {
        // Spec preview: thorough preview on a throwaway branch — the server
        // creates a preview task and spawns an implementation agent
        // (§3 thorough tier); we poll the task and render the resulting MR's
        // real code diff.
        const result = await api.thoroughPreview(repoId, {
          spec_path: specPath,
          draft_content: content,
        });
        previewAgentId = result?.agent_id ?? null;
        if (result?.task_id) {
          pollPreviewTask(result.task_id);
        } else if (result) {
          applyPreviewResult(result);
          finishPreview();
        } else {
          failPreview($t('editor_split.preview_fallback'));
        }
      }
    } catch (e) {
      // Thorough preview unavailable — back to editing; the Architecture tab
      // keeps the fast graphPredict overlays (Phase 1, ui-layout.md §2).
      failPreview($t('editor_split.preview_fallback'), e);
    } finally {
      previewRunning = false;
    }
  }

  function pollPreviewTask(taskId) {
    previewState = 'preview_running';
    previewProgress = selectedSpecKeys.length
      ? selectableSpecs
          .filter((s) => selectedSpecKeys.includes(specKey(s.repo_id, s.path)))
          .map((s) => ({ path: s.path, repo_id: s.repo_id, status: 'running' }))
      : [{ path: specPath ?? '', repo_id: repoId ?? null, status: 'running' }];
    let elapsed = 0;
    previewPollTimer = setInterval(async () => {
      elapsed += 10000;
      try {
        const status = await api.taskStatus(taskId);
        // A preview task completes when its agent finishes: the task lands
        // in Review (agent called complete → MR created) or Done. The wire
        // vocabulary is exactly what GET /api/v1/tasks/{id} serializes via
        // task_status_str: backlog/in_progress/review/done/blocked/
        // cancelled — any other value keeps polling until the timeout.
        const s = status?.status;
        if (s === 'review' || s === 'done') {
          applyPreviewResult(status);
          // §3 thorough tier: the agent really implemented the spec on a
          // throwaway branch — resolve its completion MR and render the
          // real line-level diff in the Code Diff tab.
          await loadPreviewMrDiff(taskId);
          finishPreview();
          return;
        }
        if (s === 'blocked' || s === 'cancelled') {
          failPreview(
            $t('editor_split.preview_failed', { values: { error: status?.error ?? s ?? 'agent execution failed' } }),
          );
          return;
        }
      } catch {
        // transient poll error — keep polling
      }
      if (elapsed >= 300000) {
        failPreview($t('editor_split.preview_timeout'));
      }
    }, 10000);
  }

  /**
   * Resolve the MR the completed preview agent created (author agent or
   * linked task) and map its real diff — files + hunks from
   * GET /merge-requests/:id/diff — onto the Code Diff tab's
   * [{ path, diff: [{op,text}] }] shape. Best-effort: a preview result
   * that already carries code_diff (or no resolvable MR) keeps what the
   * task status reported.
   */
  async function loadPreviewMrDiff(taskId) {
    try {
      const mrs = await api.mergeRequests({ repository_id: repoId });
      const mr = (mrs ?? []).find(
        (m) =>
          m?.task_id === taskId ||
          (previewAgentId && m?.author_agent_id === previewAgentId),
      );
      if (!mr) return;
      const diff = await api.mrDiff(mr.id);
      const files = (diff?.files ?? [])
        .map((f) => ({
          path: f.path ?? '',
          diff: (f.hunks ?? []).flatMap((h) =>
            (h.lines ?? []).map((l) => ({
              op: l.line_type === 'delete' ? 'remove' : l.line_type === 'add' ? 'add' : 'context',
              text: l.content ?? '',
            })),
          ),
        }))
        .filter((f) => f.path);
      if (files.length) codeDiffFiles = files;
    } catch {
      // MR diff unavailable — the completed state still renders whatever
      // the task status reported; never fabricate a diff.
    }
  }

  // Real agent runs take minutes, not seconds (§5 iteration cycle: "minutes,
  // not hours") — poll patiently with a bounded wait instead of the 30s cap
  // that only fit the old instantly-complete estimate endpoint.
  const META_PREVIEW_POLL_MS = 2000;
  const META_PREVIEW_TIMEOUT_MS = 600000;

  /** Per-agent status label (§5 status endpoint: running|complete|failed|stopped|dead|unknown). */
  function agentStatusLabel(status) {
    if (status === 'complete') return $t('editor_split.status_complete');
    if (status === 'running') return $t('editor_split.agent_implementing');
    if (status === 'failed') return $t('editor_split.status_failed');
    if (status === 'stopped') return $t('editor_split.status_stopped');
    if (status === 'dead') return $t('editor_split.status_dead');
    return $t('editor_split.status_unknown');
  }

  function agentStatusIcon(status) {
    if (status === 'complete' || status === 'stopped') return '✓';
    if (status === 'running') return '◐';
    return '✗';
  }

  function agentStatusDone(status) {
    return status === 'complete' || status === 'stopped';
  }

  function pollPreviewRun(previewId, initial) {
    previewState = 'preview_running';
    previewProgress = (Array.isArray(initial?.agents) ? initial.agents : []).map((a) => ({
      path: a?.spec_path ?? '',
      repo_id: a?.repo_id ?? null,
      status: 'running',
    }));
    metaPreviewResult = initial ?? null;
    let elapsed = 0;
    previewPollTimer = setInterval(async () => {
      elapsed += META_PREVIEW_POLL_MS;
      try {
        const status = await api.previewMetaSpecStatus(previewId);
        if (Array.isArray(status?.agents)) {
          previewProgress = status.agents.map((a) => ({
            path: a?.spec_path ?? '',
            repo_id: a?.repo_id ?? null,
            status: a?.status ?? 'unknown',
          }));
          metaPreviewResult = status;
          // §5 iteration step 4: "The diff viewer updates as each preview
          // agent completes" — surface each finished agent's diff on the
          // Code Diff tab DURING the run, not only after every agent is
          // terminal (a slow second agent must not hide a finished first
          // agent's diff for the rest of the run).
          const midRunFiles = metaAgentDiffFiles(status);
          if (midRunFiles.length) codeDiffFiles = midRunFiles;
        }
        if (status?.state === 'complete') {
          applyPreviewResult(status);
          finishPreview();
          return;
        }
      } catch (e) {
        // Status endpoint failing mid-poll is a failed preview, not a
        // completed one — back to editing.
        failPreview($t('editor_split.preview_unavailable'), e);
        return;
      }
      if (elapsed >= META_PREVIEW_TIMEOUT_MS) {
        failPreview($t('editor_split.preview_timeout'));
      }
    }, META_PREVIEW_POLL_MS);
  }

  /**
   * Map the §5 preview status's per-agent diffs onto the Code Diff tab's
   * [{ path, diff: [{op,text}] }] shape. Shared by the mid-run poll updates
   * and the final applyPreviewResult so both render the same files.
   */
  function metaAgentDiffFiles(result) {
    return Array.isArray(result?.agents)
      ? result.agents
          .filter((a) => Array.isArray(a?.diff?.patches))
          .map((a) => ({
            path: a.spec_path ?? '',
            repo_id: a.repo_id ?? null,
            diff: a.diff.patches
              .flatMap((p) => String(p?.patch ?? '').split('\n'))
              .filter((line) => line.length > 0)
              .map((line) => ({
                op:
                  line.startsWith('+') && !line.startsWith('+++')
                    ? 'add'
                    : line.startsWith('-') && !line.startsWith('---')
                      ? 'remove'
                      : 'context',
                text: line,
              })),
          }))
      : [];
  }

  /**
   * Map a completed preview result onto the Architecture (ghost overlays /
   * node delta) and Code Diff tabs. Accepts the shapes returned by
   * thoroughPreview/taskStatus ({predictions, code_diff}) and the real §5
   * preview status ({preview_id, state, agents: [{agent_id, repo_id,
   * spec_path, branch, status, diff}]}) — the meta-spec result is kept whole
   * for the right-pane summary (§9 State 3).
   */
  function applyPreviewResult(result) {
    if (!result) return;
    // Meta-spec preview results (real server shape) are stored for the
    // Architecture pane summary; each agent's diff.patches carry the
    // unified-diff text its preview branch produced (§5 iteration step 4).
    if (result.preview_id) {
      metaPreviewResult = result;
      const agentFiles = metaAgentDiffFiles(result);
      if (agentFiles.length) codeDiffFiles = agentFiles;
    }
    // Structural delta → ghost overlays on the Architecture tab.
    const preds = result.predictions ?? [];
    if (Array.isArray(preds) && preds.length) {
      // §3 ghost model: normalized predictions keep action, confidence and
      // reason so the overlay renders the full visual language. The ghost
      // layer positions prediction-only nodes itself (near the predicted
      // parent or in free space) — no synthetic graph nodes needed.
      derivedGhosts = normalizeGhosts(preds);
    }
    // Line-level diff → Code Diff tab.
    const files = result.code_diff ?? [];
    if (Array.isArray(files) && files.length) {
      codeDiffFiles = files.map((f) => ({
        path: f.path ?? f.spec_path ?? '',
        diff: Array.isArray(f.diff)
          ? f.diff
          : [{ op: 'context', text: String(f.diff ?? '') }],
      }));
    }
    if (Array.isArray(result.agents)) {
      previewProgress = result.agents.map((a) => ({
        path: a?.spec_path ?? '',
        repo_id: a?.repo_id ?? null,
        status: a?.status ?? 'unknown',
      }));
    }
  }

  function finishPreview() {
    stopPreviewPoll();
    previewState = 'preview_complete';
  }

  /**
   * A failed/unavailable/timed-out preview: back to the editing state with an
   * error banner in the right pane. The Architecture tab keeps whatever the
   * fast graphPredict pass loaded — §9 State 3 is never presented without a
   * real completed result.
   */
  function failPreview(message, err) {
    stopPreviewPoll();
    previewState = 'editing';
    previewProgress = [];
    previewError = err
      ? `${message} (${err?.message ?? err})`
      : message;
    if (err) console.warn('[EditorSplit] preview failed:', err);
  }

  function cancelPreview() {
    stopPreviewPoll();
    previewState = 'editing';
    previewProgress = [];
  }

  /** Iterate (§9 State 3): back to editing with results still visible. */
  function iteratePreview() {
    stopPreviewPoll();
    previewState = 'editing';
  }

  function stopPreviewPoll() {
    if (previewPollTimer) {
      clearInterval(previewPollTimer);
      previewPollTimer = null;
    }
  }

  // ── LLM chat (§3 LLM-Assisted Spec Editing) ────────────────────────────────
  let llmStreaming = $state(false);
  let llmExplanation = $state('');
  // Multiple suggestions can be requested (§3 "Multiple suggestions..."):
  // every completed assist request appends one block; the human curates
  // them — the LLM never writes directly.
  let llmSuggestions = $state([]); // [{ id, diff: [...], explanation }]
  let suggestionSeq = 0;
  let saving = $state(false);

  async function sendLlmInstruction(instruction) {
    if (!instruction || llmStreaming) return;
    if (!llmChatEnabled) return;
    llmStreaming = true;
    llmExplanation = '';

    try {
      // Repo context: repo-scoped specs/assist. §9 meta-spec context
      // (no repo): workspace-scoped meta-specs/assist against the in-editor
      // draft — same SSE {diff, explanation} wire protocol, so the shared
      // parser and Accept/Edit/Dismiss suggestion blocks work unchanged.
      const resp = repoId
        ? await api.specsAssist(repoId, {
            spec_path: specPath,
            instruction,
            draft_content: content || undefined,
          })
        : await api.metaSpecsAssist(workspaceId, {
            instruction,
            draft_content: content || undefined,
            meta_spec_id: specPath || undefined,
          });
      // Shared SSE consumer (specAssist.js): `partial` events stream the
      // explanation progressively; `complete` carries the suggestion diff;
      // `error` throws. A truncated stream must never masquerade as done.
      await streamAssist(resp, {
        onPartial: (text) => {
          llmExplanation += text;
        },
        onComplete: ({ diff, explanation }) => {
          llmSuggestions = [
            ...llmSuggestions,
            {
              id: `suggestion-${++suggestionSeq}`,
              diff,
              explanation: explanation || llmExplanation,
            },
          ];
        },
      });
    } catch (e) {
      toastError($t('editor_split.llm_assist_failed', { values: { error: e.message } }));
    } finally {
      llmStreaming = false;
    }
  }

  /**
   * Accept (§3 step 4): apply this suggestion's diff ops to the editor
   * content — in-memory only, the human must still Save. Shared
   * `applyDiffOps` (specAssist.js) resolves header paths with the
   * "L15-L22" line-range fallback.
   */
  function acceptSuggestion(suggestion) {
    const c = applyDiffOps(content, suggestion.diff);
    content = c;
    onChange?.(c);
    removeSuggestion(suggestion);
  }

  /**
   * Edit (§3 step 5): copy the suggested text into the editor for manual
   * refinement.
   */
  function editSuggestion(suggestion) {
    const text = suggestionText(suggestion);
    if (text) {
      const c = content + '\n\n' + text;
      content = c;
      onChange?.(c);
    }
    removeSuggestion(suggestion);
  }

  /** Dismiss (§3 step 6): remove the suggestion block. */
  function dismissSuggestion(suggestion) {
    removeSuggestion(suggestion);
  }

  function removeSuggestion(suggestion) {
    llmSuggestions = llmSuggestions.filter((s) => s.id !== suggestion.id);
  }

  async function saveSpec({ overwrite = false } = {}) {
    if (!repoId || !specPath || saving) return;
    saving = true;
    try {
      const result = await api.specsSave(repoId, {
        spec_path: specPath,
        content,
        message: `Update ${specPath} via editor split`,
        base_sha: baseSha ?? undefined,
        overwrite,
      });
      if (result?.conflict) {
        // Another editor's change landed since load — surface the conflict dialog.
        specConflict = result.conflict;
        return;
      }
      specConflict = null;
      // §3: "Ghost overlay clears when the spec is saved" — editing ended;
      // suppress re-prediction of the unchanged draft until the next edit.
      derivedGhosts = [];
      lastPredictedContent = content;
      toastSuccess($t('editor_split.spec_saved', { values: { mr_id: result.mr_id } }));
    } catch (e) {
      toastError($t('editor_split.save_failed', { values: { error: e.message } }));
    } finally {
      saving = false;
    }
  }

  // Conflict dialog resolvers (HSI §7).
  function conflictOverwrite() {
    saveSpec({ overwrite: true });
  }

  function conflictDiscard() {
    // Take the server's current version, dropping local edits.
    const serverContent = specConflict?.current_content ?? '';
    content = serverContent;
    onChange?.(serverContent);
    specConflict = null;
  }

  function conflictClose() {
    specConflict = null;
  }

  let textareaEl = $state(null);

  // Markdown toolbar (ui-layout.md §2 "standard markdown editor with
  // toolbar"): each action wraps or prefixes the textarea selection.
  const TOOLBAR_BUTTONS = [
    { id: 'bold', label: 'B', wrap: '**' },
    { id: 'italic', label: 'I', wrap: '_' },
    { id: 'heading', label: 'H', wrap: '## ', prefix: true },
    { id: 'list', label: '•', wrap: '- ', prefix: true },
    { id: 'code', label: '</>', wrap: '`' },
    { id: 'link', label: '🔗', wrap: null, template: '[title](url)' },
    { id: 'quote', label: '"', wrap: null, template: '> ' },
    { id: 'undo', label: '⤺', action: 'undo' },
    { id: 'redo', label: '⤻', action: 'redo' },
  ];

  /**
   * Apply a toolbar action to the textarea selection and update content.
   * Boring by design: wrap/prefix/template only, no parsing.
   */
  function applyToolbarAction(btn) {
    if (!textareaEl) return;
    const { selectionStart: start, selectionEnd: end } = textareaEl;
    if (btn.action === 'undo') {
      textareaEl.focus();
      document.execCommand('undo');
      return;
    }
    if (btn.action === 'redo') {
      textareaEl.focus();
      document.execCommand('redo');
      return;
    }
    if (btn.template) {
      const insert = btn.template;
      const next = content.slice(0, start) + insert + content.slice(end);
      content = next;
      onChange?.(next);
      queueMicrotask(() => {
        textareaEl.focus();
        textareaEl.setSelectionRange(start + insert.length, start + insert.length);
      });
      return;
    }
    const wrap = btn.wrap ?? '';
    const prefixMode = Boolean(btn.prefix);
    let next;
    let selStart;
    let selEnd;
    if (prefixMode) {
      // Insert at line start(s): walk back to the line start (or each
      // selected line's start) and prepend.
      const lineStart = content.lastIndexOf('\n', start - 1) + 1;
      const selected = content.slice(lineStart, end);
      const prefixed = selected.replace(/^/gm, wrap);
      next = content.slice(0, lineStart) + prefixed + content.slice(end);
      selStart = start + wrap.length;
      selEnd = end + wrap.length * (selected.split('\n').length);
    } else {
      const selected = content.slice(start, end);
      next = content.slice(0, start) + wrap + selected + wrap + content.slice(end);
      selStart = start + wrap.length;
      selEnd = selStart + selected.length;
    }
    content = next;
    onChange?.(next);
    queueMicrotask(() => {
      textareaEl.focus();
      textareaEl.setSelectionRange(selStart, selEnd);
    });
  }

  function handleContentInput(e) {
    content = e.target.value;
    onChange?.(content);
  }

  function handleKeydown(e) {
    if (e.key !== 'Escape') return;
    // While the 409 conflict dialog is open it owns Esc (HSI §7): the dialog
    // closes, the editor stays. The dialog's own window listener handles it.
    if (specConflict) return;
    // While a preview is running, Esc cancels the preview (§9 State 2
    // "[Cancel Preview]") rather than closing the split — in every context,
    // including the DetailPanel pop-out.
    if (previewState === 'preview_running') {
      e.preventDefault();
      cancelPreview();
      return;
    }
    // No dismiss surface (e.g. embedded in the MetaSpecs page rather than a
    // pop-out) — Esc does nothing.
    if (!onClose) return;
    e.preventDefault();
    onClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="editor-split" role="region" aria-label={$t('editor_split.editor_split_view')}>
  <!-- Back button -->
  <div class="split-header">
    <button class="back-btn" onclick={() => onClose?.()} aria-label={$t('editor_split.close_editor')}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <path d="M19 12H5M12 19l-7-7 7-7"/>
      </svg>
      {$t('editor_split.back')}
    </button>
    <span class="split-label">{context === 'meta-spec' ? $t('editor_split.meta_spec_editor') : $t('editor_split.spec_editor')}{specPath ? ` — ${specPath}` : ''}</span>
    <div class="header-actions">
      {#if previewState === 'preview_complete'}
        <button class="preview-btn" onclick={iteratePreview} data-testid="iterate-btn">{$t('editor_split.iterate')}</button>
        {#if context === 'meta-spec' && onpublish}
          <button class="save-btn" onclick={() => onpublish?.()} data-testid="publish-btn">{$t('editor_split.publish')}</button>
        {/if}
      {:else if previewState !== 'preview_running'}
        {#if context === 'meta-spec' || (repoId && specPath)}
          <button
            class="preview-btn"
            onclick={runPreview}
            disabled={!canRunPreview || previewRunning}
            aria-busy={previewRunning}
            data-testid="preview-btn"
          >
            {previewRunning ? $t('editor_split.preview_starting') : $t('editor_split.preview')}
          </button>
        {/if}
        {#if context === 'meta-spec' && onpublish}
          <button class="save-btn" onclick={() => onpublish?.()} data-testid="publish-btn">{$t('editor_split.publish')}</button>
        {/if}
      {/if}
      {#if repoId && specPath && context === 'spec'}
        <button
          class="save-btn"
          onclick={() => saveSpec()}
          disabled={saving || !content.trim()}
          aria-busy={saving}
        >
          {saving ? $t('editor_split.saving') : $t('editor_split.save_create_mr')}
        </button>
      {/if}
    </div>
  </div>

  <!-- Split panes -->
  <div class="split-panes">
    <!-- Left: Editor + LLM chat -->
    <div class="pane pane-left">
      {#if context === 'spec' && specPath && workspaceId}
        <ConcurrentEditBanner {specPath} {workspaceId} {wsStore} {selfUserId} />
      {/if}
      {#if context === 'spec' && specPath}
        <!-- Instant tier (§3 "Link graph impact"): connected specs, repos
             implementing this spec, governed node count — computed the
             moment editing begins, before any prediction returns. -->
        <div
          class="impact-bar"
          role="status"
          aria-label={$t('editor_split.impact_bar_aria')}
          data-testid="impact-bar">
          {#if impact}
            <span class="impact-seg" data-testid="impact-connected-specs">
              {$t('editor_split.impact_connected_specs', { values: { count: impact.connectedSpecs.length } })}
            </span>
            <span class="impact-seg" data-testid="impact-repos">
              {$t('editor_split.impact_repos_implementing', { values: { count: impact.reposImplementing.length } })}
            </span>
            <span class="impact-seg" data-testid="impact-governed-nodes">
              {#if impact.governedNodes == null}
                {$t('editor_split.impact_governed_nodes_unknown')}
              {:else}
                {$t('editor_split.impact_governed_nodes', { values: { count: impact.governedNodes } })}
              {/if}
            </span>
          {:else}
            <span class="impact-seg impact-loading">{$t('editor_split.impact_loading')}</span>
          {/if}
        </div>
      {/if}
      {#if previewState === 'preview_running'}
        <!-- Editor locked during preview (§9 State 2). With a baseline
             available the pane shows the diff of pending changes (draft vs.
             published); without one, the draft verbatim. -->
        <div class="locked-editor" role="region" aria-label={$t('editor_split.editor_locked_aria')} data-testid="editor-locked">
          {#if lockedDiffRows}
            <div class="locked-diff" data-testid="locked-diff">
              <div class="locked-diff-header">
                <span class="locked-diff-title">{$t('editor_split.pending_changes')}</span>
                <span class="locked-diff-summary" data-testid="locked-diff-summary" aria-live="polite">
                  +{lockedDiffStats.added} -{lockedDiffStats.removed}
                </span>
              </div>
              {#each lockedDiffRows as row}
                <div class="locked-line locked-diff-line locked-diff-{row.op}">
                  <span class="locked-diff-marker" aria-hidden="true">{row.op === 'add' ? '+' : row.op === 'remove' ? '-' : ' '}</span>
                  <span class="locked-diff-text">{row.text}</span>
                </div>
              {/each}
            </div>
          {:else}
            {#each content.split('\n') as line}
              <div class="locked-line">{line}</div>
            {/each}
          {/if}
        </div>
      {:else}
        <div class="md-toolbar" role="toolbar" aria-label={$t('editor_split.md_toolbar_aria')} data-testid="md-toolbar">
          {#each TOOLBAR_BUTTONS as btn (btn.id)}
            <button
              class="md-btn"
              type="button"
              onclick={() => applyToolbarAction(btn)}
              aria-label={$t(`editor_split.md_${btn.id}_aria`)}
              title={$t(`editor_split.md_${btn.id}_aria`)}
              data-testid={`md-${btn.id}`}
            >{btn.label}</button>
          {/each}
        </div>
        <textarea
          class="split-textarea"
          value={content}
          oninput={handleContentInput}
          placeholder={$t('editor_split.spec_placeholder')}
          aria-label={$t('editor_split.spec_editor')}
          spellcheck="false"
          bind:this={textareaEl}
          data-testid="editor-split-textarea"
        ></textarea>
      {/if}

      <!-- Each suggestion renders as its own inline "Suggested Change"
           diff block (§3 "Multiple suggestions... Each appears as a separate
           inline diff block") with the Accept/Edit/Dismiss curation actions.
           Accept applies in-memory only; the human saves explicitly. -->
      {#each llmSuggestions as suggestion (suggestion.id)}
        <SpecSuggestion
          {suggestion}
          onaccept={() => acceptSuggestion(suggestion)}
          onedit={() => editSuggestion(suggestion)}
          ondismiss={() => dismissSuggestion(suggestion)}
        />
      {/each}

      <!-- Shared §3 chat input: "Edit spec: ..." recipient indicator,
           progressive `partial` explanation rendering, loading state. -->
      <SpecChatInput
        recipient={`${$t('editor_split.edit_label')} ${context === 'meta-spec' ? $t('editor_split.meta_spec') : $t('editor_split.spec')}${specPath ? `: "${specPath}" ▸` : ' ▸'}`}
        enabled={llmChatEnabled}
        streaming={llmStreaming}
        streamingText={llmExplanation}
        placeholder={$t('editor_split.llm_placeholder')}
        instructionAria={$t('editor_split.llm_instruction')}
        sendAria={$t('editor_split.send_to_llm')}
        thinking={$t('editor_split.thinking')}
        hint={llmChatEnabled ? $t('editor_split.llm_hint') : null}
        warnHint={llmChatEnabled ? null : $t('editor_split.llm_requires_repo')}
        onsubmit={sendLlmInstruction}
      />
    </div>

    <!-- Divider -->
    <div class="split-divider" aria-hidden="true"></div>

    <!-- Right: Architecture preview / preview states -->
    <div class="pane pane-right" data-testid="arch-preview-pane">
      {#if previewError}
        <!-- A failed/unavailable/timed-out preview surfaces here in EVERY
             right-pane state (including the meta-spec selector branch the
             component returns to) — the error is never silently dropped. -->
        <div class="preview-error" role="alert" data-testid="preview-error">
          <strong>{$t('editor_split.preview_error_label')}:</strong> {previewError}
        </div>
      {/if}
      {#if previewState === 'preview_running'}
        <!-- Preview Running (§9 State 2): per-agent progress indicators,
             rendered as a strip ABOVE the Architecture/Code Diff tabs (not
             instead of them) so the §3 ghost overlays stay on the canvas
             while the agent runs — pulsing borders (GhostOverlay
             mode="previewing"). Statuses are the real §5 per-agent states
             (running|complete|failed|stopped|dead|unknown). -->
        <div class="preview-progress" data-testid="preview-running" aria-live="polite">
          <div class="progress-header" role="status">{$t('editor_split.preview_running')}</div>
          <div class="progress-list">
            {#each previewProgress as item (specKey(item.repo_id, item.path))}
              <div class="progress-item">
                <span class="progress-icon" aria-hidden="true">{agentStatusIcon(item.status)}</span>
                <span class="progress-path">{item.path}</span>
                <span class="progress-status">{agentStatusLabel(item.status)}</span>
              </div>
            {/each}
          </div>
          <div class="progress-summary">{$t('editor_split.progress_label', { values: { done: previewProgress.filter((p) => agentStatusDone(p.status)).length, total: previewProgress.length } })}</div>
          <button class="dismiss-btn-sm cancel-preview" onclick={cancelPreview} data-testid="cancel-preview">{$t('editor_split.cancel_preview')}</button>
        </div>
      {/if}
      {#if context === 'meta-spec' && previewState === 'editing'}
        <!-- Target spec selector (§9 State 1) — always shown in the meta-spec
             editing state, with an empty-state message when the workspace
             has no non-meta specs. After §9 Iterate the last preview result
             is retained, so the selector stacks above the Architecture /
             Code Diff tabs (showResultTabs) instead of replacing them. -->
        <div class="spec-selector" class:stacked={Boolean(metaPreviewResult)} data-testid="spec-selector">
          <div class="spec-selector-header">
            <span class="pane-title">{$t('editor_split.preview_against')}</span>
            <div class="spec-selector-shortcuts">
              <button class="link-btn" onclick={selectAllSpecs}>{$t('editor_split.select_all')}</button>
              <button class="link-btn" onclick={clearAllSpecs}>{$t('editor_split.clear')}</button>
            </div>
          </div>
          <div class="spec-checklist">
            {#each selectableSpecs as spec (specKey(spec.repo_id, spec.path))}
              <label class="spec-check-item">
                <input
                  type="checkbox"
                  checked={selectedSpecKeys.includes(specKey(spec.repo_id, spec.path))}
                  onchange={() => toggleSpec(spec.repo_id, spec.path)}
                />
                <span class="spec-check-path">{spec.path}</span>
              </label>
            {:else}
              <p class="empty-specs">{$t('editor_split.no_specs_available')}</p>
            {/each}
          </div>
        </div>
      {/if}
      {#if showResultTabs}
        <!-- Preview Complete / spec context, or the retained results after
             §9 Iterate: Architecture (default) + Code Diff tabs -->
        <div class="pane-header">
          <div class="impact-tabs" role="tablist" aria-label={$t('editor_split.impact_view_aria')}>
            <button
              class="impact-tab"
              role="tab"
              id="editor-split-tab-arch"
              aria-controls="editor-split-panel-arch"
              aria-selected={activeImpactTab === 'architecture'}
              class:active={activeImpactTab === 'architecture'}
              tabindex={activeImpactTab === 'architecture' ? 0 : -1}
              onclick={() => (activeImpactTab = 'architecture')}
              data-testid="tab-architecture"
            >{$t('editor_split.architecture')}</button>
            <button
              class="impact-tab"
              role="tab"
              id="editor-split-tab-diff"
              aria-controls="editor-split-panel-diff"
              aria-selected={activeImpactTab === 'code-diff'}
              class:active={activeImpactTab === 'code-diff'}
              tabindex={activeImpactTab === 'code-diff' ? 0 : -1}
              onclick={() => (activeImpactTab = 'code-diff')}
              data-testid="tab-code-diff"
            >{$t('editor_split.code_diff')}</button>
          </div>
          {#if graphLoading}
            <span class="loading-chip" aria-live="polite">{$t('editor_split.loading_graph')}</span>
          {:else if predicting}
            <!-- Fast tier in flight (§3): the debounced graphPredict call is
                 running — subtle indicator, never blocking. -->
            <span class="loading-chip" aria-live="polite" data-testid="predicting-chip">{$t('editor_split.predicting')}</span>
          {:else if overlays.length}
            <span class="overlay-chip" data-testid="overlay-chip">{$t('editor_split.predicted_changes', { values: { count: overlays.length } })}</span>
          {/if}
        </div>
        {#if activeImpactTab === 'architecture'}
          <div
            class="canvas-wrap"
            role="tabpanel"
            id="editor-split-panel-arch"
            aria-labelledby="editor-split-tab-arch"
          >
            {#if context === 'meta-spec' && metaPreviewResult}
              <!-- Meta-spec preview (§9 State 3): the real §5 status — one
                   agent per target spec, its preview branch and produced
                   diff stats. -->
              <div class="meta-impact" data-testid="meta-impact">
                <div class="meta-impact-section">
                  <h4 class="meta-impact-title">{$t('editor_split.preview_agents_title')}</h4>
                  <ul class="meta-repo-list">
                    {#each metaPreviewResult.agents ?? [] as a (a.agent_id ?? specKey(a.repo_id, a.spec_path))}
                      <li class="meta-repo-item">
                        <span class="meta-repo-id">{a.spec_path}</span>
                        <span class="meta-repo-reason">
                          {agentStatusLabel(a.status)}{a.branch ? ` · ${a.branch}` : ''}{#if a.diff && typeof a.diff.files_changed === 'number'} · +{a.diff.insertions ?? 0}/-{a.diff.deletions ?? 0} ({a.diff.files_changed} {$t('editor_split.diff_files')}){/if}
                        </span>
                      </li>
                    {/each}
                  </ul>
                </div>
              </div>
            {:else}
              <ArchPreviewCanvas
                nodes={graphNodes}
                edges={graphEdges}
                ghostOverlays={overlays}
                ghostMode={previewState === 'preview_running'
                  ? 'previewing'
                  : previewState === 'preview_complete'
                    ? 'confirmed'
                    : 'predicted'}
                size="full"
              />
            {/if}
          </div>
        {:else}
          <div
            class="code-diff-wrap"
            role="tabpanel"
            id="editor-split-panel-diff"
            aria-labelledby="editor-split-tab-diff"
            data-testid="code-diff-panel"
          >
            {#if codeDiffFiles.length}
              {#each codeDiffFiles as file (specKey(file.repo_id, file.path))}
                <div class="code-diff-file">
                  <div class="code-diff-path">{file.path}</div>
                  <SpecDiffView diff={file.diff} />
                </div>
              {/each}
            {:else if context === 'meta-spec' && metaPreviewResult}
              <!-- No agent produced a diff for this preview — list the
                   per-agent statuses so the outcome is still visible. -->
              {#if metaPreviewResult?.agents?.length}
                <div class="meta-spec-statuses" data-testid="meta-spec-statuses">
                  {#each metaPreviewResult.agents as s (s.agent_id ?? specKey(s.repo_id, s.spec_path))}
                    <div class="meta-spec-status">
                      <span class="meta-spec-path">{s.spec_path}</span>
                      <span class="meta-spec-state">{agentStatusLabel(s.status)}</span>
                    </div>
                  {/each}
                </div>
                <p class="code-diff-empty">{$t('editor_split.no_line_diff_available')}</p>
              {:else}
                <p class="code-diff-empty">{$t('editor_split.no_code_diff')}</p>
              {/if}
            {:else if previewState === 'preview_complete'}
              <p class="code-diff-empty">{$t('editor_split.no_code_diff')}</p>
            {:else}
              <p class="code-diff-empty">{$t('editor_split.run_preview_for_diff')}</p>
            {/if}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

<SpecConflictDialog
  conflict={specConflict}
  onOverwrite={conflictOverwrite}
  onDiscard={conflictDiscard}
  onClose={conflictClose}
/>

<style>
  .editor-split {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-surface);
    overflow: hidden;
  }

  .split-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
    min-height: 44px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .back-btn:hover {
    color: var(--color-text);
    border-color: var(--color-text-muted);
  }

  .back-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .split-label {
    flex: 1;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .preview-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    white-space: nowrap;
    transition: border-color var(--transition-fast);
  }

  .preview-btn:hover:not(:disabled) { border-color: var(--color-primary); }
  .preview-btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .preview-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .save-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    font-weight: 500;
    white-space: nowrap;
    transition: background var(--transition-fast);
  }

  .save-btn:hover:not(:disabled) { background: var(--color-primary-hover); }
  .save-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  .save-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .split-panes {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .pane {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
  }

  .pane-left {
    flex: 1;
    border-right: none;
  }

  .pane-right {
    flex: 1;
    background: var(--color-surface, #0f172a);
  }

  .split-divider {
    width: 1px;
    background: var(--color-border);
    flex-shrink: 0;
  }

  .split-textarea {
    flex: 1;
    width: 100%;
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border: none;
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    resize: none;
    box-sizing: border-box;
    min-height: 200px;
  }

  .split-textarea:focus:not(:focus-visible) { outline: none; }
  .split-textarea:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* Editor locked during preview (§9 State 2) */
  .locked-editor {
    flex: 1;
    overflow: auto;
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    min-height: 200px;
  }

  .locked-line { white-space: pre-wrap; color: var(--color-text-secondary); }

  /* Pending-changes diff in the locked editor (§9 State 2) */
  .locked-diff { display: flex; flex-direction: column; min-height: 100%; }
  .locked-diff-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-2);
    margin-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }
  .locked-diff-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .locked-diff-summary { font-family: var(--font-mono, monospace); font-size: var(--text-xs); color: var(--color-text-muted); }
  .locked-diff-line { display: flex; gap: var(--space-2); }
  .locked-diff-marker { flex-shrink: 0; width: 1ch; font-family: var(--font-mono, monospace); }
  .locked-diff-text { min-width: 0; }
  .locked-diff-add .locked-diff-marker { color: var(--color-success, #2e9e5b); font-weight: 600; }
  .locked-diff-add .locked-diff-text { color: var(--color-success, #2e9e5b); }
  .locked-diff-remove .locked-diff-marker { color: var(--color-danger, #cc3340); font-weight: 600; }
  .locked-diff-remove .locked-diff-text { color: var(--color-danger, #cc3340); text-decoration: line-through; }

  /* Instant tier impact bar (§3 "Link graph impact") */
  .impact-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    background: color-mix(in srgb, var(--color-primary, #60a5fa) 4%, transparent);
    flex-shrink: 0;
    min-height: 26px;
  }

  .impact-seg {
    font-size: 10px;
    color: var(--color-text-muted);
    font-family: var(--font-mono, monospace);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .impact-loading {
    font-style: italic;
  }

  /* Markdown toolbar (ui-layout.md §2) */
  .md-toolbar {
    display: flex;
    gap: 2px;
    align-items: center;
    padding: 4px var(--space-3);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-1, transparent);
  }
  .md-btn {
    min-width: 26px;
    height: 24px;
    padding: 0 6px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--color-text-secondary);
    font-size: 12px;
    font-family: var(--font-mono, monospace);
    cursor: pointer;
  }
  .md-btn:hover { background: var(--color-surface-3, rgba(127,127,127,.15)); color: var(--color-text); }
  .md-btn:active { transform: translateY(1px); }

  /* Preview error banner (right pane) */
  .preview-error {
    margin: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-danger, #cc3340);
    border-radius: 6px;
    background: rgba(204, 51, 64, 0.08);
    color: var(--color-text);
    font-size: 12px;
  }

  /* Meta-spec preview impact summary (real PreviewResponse fields) */
  .meta-impact {
    overflow: auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .meta-impact-title { margin: 0 0 var(--space-2); font-size: 12px; font-weight: 600; color: var(--color-text-secondary); text-transform: uppercase; letter-spacing: 0.04em; }
  .meta-repo-list { margin: 0; padding: 0; list-style: none; display: flex; flex-direction: column; gap: var(--space-2); }
  .meta-repo-item { display: flex; flex-direction: column; gap: 2px; font-size: 13px; }
  .meta-repo-id { font-family: var(--font-mono, monospace); }
  .meta-repo-reason { color: var(--color-text-secondary); font-size: 12px; }
  .meta-spec-statuses { display: flex; flex-direction: column; gap: var(--space-2); padding: var(--space-3); }
  .meta-spec-status { display: flex; justify-content: space-between; gap: var(--space-3); font-size: 13px; }
  .meta-spec-path { font-family: var(--font-mono, monospace); }
  .meta-spec-state { color: var(--color-text-secondary); }

  /* Suggestion / LLM-chat visuals live in the shared SpecSuggestion and
     SpecChatInput components (task-185). Only the small ghost button used
     by the preview progress list stays here. */
  .dismiss-btn-sm {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
  }

  .dismiss-btn-sm:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* Right pane */
  .pane-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .impact-tabs {
    display: flex;
    gap: var(--space-1);
  }

  .impact-tab {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    font-weight: 500;
  }

  .impact-tab:hover { color: var(--color-text); }
  .impact-tab.active {
    color: var(--color-primary);
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
  }
  .impact-tab:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .pane-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .loading-chip {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .overlay-chip {
    font-size: 10px;
    padding: 1px 5px;
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent);
    border-radius: var(--radius-sm);
    color: var(--color-warning);
    font-family: var(--font-mono);
  }

  .canvas-wrap {
    flex: 1;
    overflow: hidden;
    min-height: 0;
  }

  /* Target spec selector (§9 State 1) */
  .spec-selector {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
    padding: var(--space-3);
    gap: var(--space-2);
  }

  /* §9 Iterate: with a retained preview result the selector stacks above
     the Architecture/Code Diff tabs instead of taking the whole pane. */
  .spec-selector.stacked {
    flex: 0 1 auto;
    max-height: 40%;
    border-bottom: 1px solid var(--color-border);
  }

  .spec-selector-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .spec-selector-shortcuts {
    display: flex;
    gap: var(--space-2);
  }

  .link-btn {
    background: none;
    border: none;
    color: var(--color-primary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    padding: 0;
  }

  .link-btn:hover { text-decoration: underline; }
  .link-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .spec-checklist {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    overflow: auto;
    padding: var(--space-1) 0;
  }

  .spec-check-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text);
  }

  .spec-check-item:hover { background: var(--color-surface-elevated); }

  .empty-specs {
    margin: 0;
    padding: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  /* Preview Running (§9 State 2) */
  .preview-progress {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3);
    overflow: auto;
    /* §3: a strip above the Architecture/Code Diff tabs — the canvas (and
       its pulsing ghost overlays) stays visible below while the preview
       agent runs. */
    flex-shrink: 0;
    max-height: 40%;
    border-bottom: 1px solid var(--color-border);
  }

  .progress-header {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .progress-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .progress-item {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .progress-icon { width: 1em; flex-shrink: 0; }
  .progress-path { font-family: var(--font-mono); color: var(--color-text); }
  .progress-status { color: var(--color-text-muted); }

  .progress-summary {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .cancel-preview { align-self: flex-start; }

  /* Code Diff tab */
  .code-diff-wrap {
    flex: 1;
    overflow: auto;
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-height: 0;
  }

  .code-diff-file {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .code-diff-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text);
  }

  .code-diff-empty {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .back-btn, .save-btn { transition: none; }
  }
</style>
