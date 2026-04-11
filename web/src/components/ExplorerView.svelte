<script>
  import { getContext, onDestroy } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { entityName } from '../lib/entityNames.svelte.js';
  import { computeTimelineDeltaStats, computeTimelineGhostOverlays } from '../lib/timeline-utils.js';
  import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
  import ExplorerChat from '../lib/ExplorerChat.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import WorkspaceCards from './WorkspaceCards.svelte';
  import NodeDetailPanel from '../lib/NodeDetailPanel.svelte';

  const navigate = getContext('navigate');
  const goToWorkspaceSettings = getContext('goToWorkspaceSettings');

  // scope: { type: 'tenant' | 'workspace' | 'repo', workspaceId?, repoId? }
  // Defaults to tenant scope for backwards compatibility with old App.svelte.
  let { scope = { type: 'tenant' }, onSelectWorkspace = null, workspaceName = null } = $props();

  let scopeType = $derived(scope?.type ?? 'tenant');

  // ── Workspace-scope repo list ──────────────────────────────────────────
  let wsRepos = $state([]);
  let wsReposLoading = $state(true);
  let wsReposError = $state(null);

  // ── Repo-scope graph state ─────────────────────────────────────────────
  let repos = $state([]);
  let selectedRepoId = $state('');
  let graph = $state(null);
  let loading = $state(false);
  let reposLoading = $state(true);
  // selectedNode tracks the currently selected graph node for canvas state
  let selectedNode = $state(null);
  let graphError = $state(null);

  // Explorer always shows architecture view (tabs removed per spec: "one canvas, one conversation, one understanding").

  // Graph data quality warnings (e.g., missing LSP toolchains)
  let graphWarnings = $state([]);
  let graphWarningsDismissed = $state(false);

  // New explorer state
  let explorerCanvasState = $state({ selectedNode: null, zoom: 1, visibleGroups: [], breadcrumb: [], recent_interactions: [] });
  let activeViewQuery = $state(null);
  let explorerFilter = $state('all');
  let explorerLens = $state('structural');
  let explorerSavedViews = $state([]);
  let detailNode = $state(null);
  let highlightedSpanId = $state(null);

  // ── Recent interactions (fed to LLM via canvas_state) ──────────────────
  // ExplorerCanvas tracks canvas-level interactions (click, drill) in
  // canvasState.recent_interactions.  ExplorerView adds higher-level
  // interactions (search, load_view) so the LLM has full context.
  function pushInteraction(label) {
    const existing = explorerCanvasState?.recent_interactions ?? [];
    const updated = [...existing, label].slice(-10);
    explorerCanvasState = { ...explorerCanvasState, recent_interactions: updated };
  }

  // ── Dynamic welcome suggestions (vision.md Principle 6: challenge ceremony) ──
  // Compute context-aware suggestions from the graph so the user doesn't need
  // to figure out what to ask. Shows relevant insights for THIS specific repo.
  let graphHints = $derived.by(() => {
    const nodes = graph?.nodes ?? [];
    const edges = graph?.edges ?? [];
    if (nodes.length === 0) return [];

    const hints = [];
    const endpointCount = nodes.filter(n => n.node_type === 'endpoint').length;
    const typeCount = nodes.filter(n => n.node_type === 'type' || n.node_type === 'interface').length;
    const specPaths = new Set(nodes.filter(n => n.spec_path).map(n => n.spec_path));
    const ungoverned = nodes.filter(n => !n.spec_path && (n.node_type === 'function' || n.node_type === 'type' || n.node_type === 'endpoint'));
    const highComplexity = nodes.filter(n => (n.complexity ?? 0) > 20);
    const highChurn = nodes.filter(n => (n.churn_count_30d ?? 0) > 10);
    const testNodes = nodes.filter(n => n.test_node);

    // Pick the most relevant suggestions based on repo state
    if (endpointCount > 0) {
      hints.push(`Show me the ${endpointCount} API endpoint${endpointCount !== 1 ? 's' : ''} and their handlers`);
    }
    if (ungoverned.length > 5) {
      hints.push(`${ungoverned.length} elements have no governing spec — show me the risk`);
    } else if (highComplexity.length > 0) {
      hints.push(`${highComplexity.length} element${highComplexity.length !== 1 ? 's have' : ' has'} high complexity — which need attention?`);
    }
    if (highChurn.length > 0) {
      hints.push(`What's changing most? ${highChurn.length} element${highChurn.length !== 1 ? 's' : ''} changed 10+ times recently`);
    } else if (typeCount > 3) {
      hints.push(`Show me the dependency graph between the ${typeCount} types`);
    }
    if (testNodes.length > 0 && testNodes.length < nodes.length / 2) {
      hints.push('Which functions have no test coverage?');
    }
    // Warn about incomplete call graph when few call edges exist
    const callEdges = edges.filter(e => (e.edge_type ?? e.type ?? '').toLowerCase() === 'calls');
    const fnCount = nodes.filter(n => n.node_type === 'function' || n.node_type === 'method' || n.node_type === 'endpoint').length;
    if (fnCount > 10 && callEdges.length < fnCount * 0.1) {
      hints.push(`Call graph looks incomplete (${callEdges.length} call edges for ${fnCount} functions) — how do I fix this?`);
    }

    if (hints.length === 0) {
      // Fallback generic suggestions
      hints.push('What are the main boundaries in this architecture?');
      hints.push('Show me the dependency graph');
      hints.push('Which types are most complex?');
    }

    return hints.slice(0, 3);
  });

  // ── Breadcrumb URL deep-linking ──────────────────────────────────────
  // Encode breadcrumb path in URL hash using semantic names for readable URLs.
  // Format: #drill=name1/name2/name3 (human-readable, shareable)
  // Falls back to id:name if name is ambiguous (contains '/').
  $effect(() => {
    const bc = explorerCanvasState?.breadcrumb;
    if (!bc || bc.length === 0) {
      // Only clear hash if it was a drill= hash (don't clear other hashes)
      if (window.location.hash.startsWith('#drill=')) {
        history.replaceState(null, '', window.location.pathname + window.location.search);
      }
      return;
    }
    // Use semantic names for readable URLs: "repo > package > module > type"
    const encoded = bc.map(b => {
      const name = b.name || '?';
      // If name contains path separators, encode the ID too for unambiguous lookup
      if (name.includes('/')) return `${encodeURIComponent(b.id)}:${encodeURIComponent(name)}`;
      return encodeURIComponent(name);
    }).join('/');
    history.replaceState(null, '', `${window.location.pathname}${window.location.search}#drill=${encoded}`);
  });

  // Spec editor state (inline editing with progressive preview, §3)
  let specEditorOpen = $state(false);
  let specEditorPath = $state('');
  let specEditorContent = $state('');
  let specEditorOriginal = $state('');
  let specEditorLoading = $state(false);
  let specEditorError = $state('');
  let predictLoading = $state(false);
  let predictError = $state('');
  let ghostOverlays = $state([]);
  // Spec assertion results (system-explorer.md §9): green checkmark / red X per assertion
  let specAssertionResults = $state([]); // [{ line, assertion_text, passed, explanation }]
  let assertionsLoading = $state(false);
  // View query dry-run result (contains node_metrics for heat map)
  let viewQueryResult = $state(null);
  let dryRunVersion = 0; // Guards against stale async results

  // Fetch dry-run result when a view query with heat emphasis is active
  $effect(() => {
    const q = activeViewQuery;
    const repoId = selectedRepoId;
    if (!q?.emphasis?.heat?.metric || !repoId) {
      viewQueryResult = null;
      return;
    }
    // Capture version to detect if a newer request has been issued
    const thisVersion = ++dryRunVersion;
    const selectedId = explorerCanvasState?.selectedNode?.id ?? null;
    api.graphQueryDryrun(repoId, q, selectedId)
      .then(result => {
        // Only apply if this is still the most recent request
        if (thisVersion === dryRunVersion) viewQueryResult = result;
      })
      .catch(() => {
        if (thisVersion === dryRunVersion) viewQueryResult = null;
      });
  });

  // Evaluative lens: trace data from most recent MR
  let traceData = $state(null);
  let predictAffectedSpecs = $state([]);
  let predictEstimatedCost = $state(null);
  let predictConfidence = $state(null);

  async function openSpecEditor(specPath) {
    if (!specPath || !selectedRepoId) return;
    specEditorOpen = true;
    specEditorPath = specPath;
    specEditorContent = '';
    specEditorOriginal = '';
    specEditorError = '';
    specEditorLoading = true;
    predictError = '';
    predictLoading = false;
    ghostOverlays = [];
    predictAffectedSpecs = [];
    predictEstimatedCost = null;
    predictConfidence = null;
    specAssertionResults = [];
    assertionsLoading = false;
    try {
      const spec = await api.specContent(specPath, selectedRepoId);
      const content = spec?.content ?? spec?.body ?? spec?.text ?? '';
      specEditorContent = content;
      specEditorOriginal = content;
      // Check spec assertions in background (§9: green checkmark / red X inline)
      if (content.includes('gyre:assert')) {
        assertionsLoading = true;
        api.checkSpecAssertions(selectedRepoId, specPath, content)
          .then(result => { specAssertionResults = result?.assertions ?? []; })
          .catch(() => { specAssertionResults = []; })
          .finally(() => { assertionsLoading = false; });
      }
      // Auto-highlight governed code on canvas (spec→code navigation, Vision §3)
      // Sanitize specPath to prevent query injection via quotes, parens, backslashes
      const sanitizedPath = specPath.replace(/[()'"\\]/g, '');
      activeViewQuery = {
        scope: { type: 'filter', computed: `$governed_by('${sanitizedPath}')` },
        emphasis: {
          highlight: { matched: { color: '#22c55e', label: 'Governed' } },
          dim_unmatched: 0.15,
        },
        zoom: 'fit',
        annotation: {
          title: `Spec: ${specPath.split('/').pop()}`,
          description: `Code governed by ${specPath}`,
        },
      };
    } catch (e) {
      specEditorError = e.message ?? 'Failed to load spec';
    } finally {
      specEditorLoading = false;
    }
  }

  function closeSpecEditor() {
    specEditorOpen = false;
    specEditorPath = '';
    specEditorContent = '';
    specEditorOriginal = '';
    specEditorError = '';
    predictError = '';
    ghostOverlays = [];
    predictAffectedSpecs = [];
    predictEstimatedCost = null;
    predictConfidence = null;
    // Clear the spec-governance highlight when closing the editor
    if (activeViewQuery?.annotation?.title?.startsWith('Spec:')) {
      activeViewQuery = null;
    }
  }

  // Instant preview tier: compute link graph impact without LLM (§3 Instant tier)
  let instantImpact = $derived.by(() => {
    if (!specEditorOpen || !graph?.nodes?.length || !graph?.edges?.length) return null;
    // Find the spec node for this path
    const specNode = (graph.nodes ?? []).find(n => n.spec_path === specEditorPath || n.name === specEditorPath);
    const specNodeId = specNode?.id;

    // Find nodes governed by this spec path
    const governedNodeIds = new Set();
    for (const e of (graph.edges ?? [])) {
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et === 'governed_by') {
        const src = e.source_id ?? e.from_node_id ?? e.from;
        const tgtNode = (graph.nodes ?? []).find(n => n.id === (e.target_id ?? e.to_node_id ?? e.to));
        if (tgtNode?.spec_path === specEditorPath || tgtNode?.name === specEditorPath) {
          governedNodeIds.add(src);
        }
      }
    }
    // Also find nodes with spec_path matching
    for (const n of (graph.nodes ?? [])) {
      if (n.spec_path === specEditorPath) governedNodeIds.add(n.id);
    }

    // Count connected specs (specs linked to this spec via edges)
    const connectedSpecIds = new Set();
    if (specNodeId) {
      for (const e of (graph.edges ?? [])) {
        const src = e.source_id ?? e.from_node_id ?? e.from;
        const tgt = e.target_id ?? e.to_node_id ?? e.to;
        if (src === specNodeId || tgt === specNodeId) {
          const otherId = src === specNodeId ? tgt : src;
          const otherNode = (graph.nodes ?? []).find(n => n.id === otherId);
          if (otherNode?.node_type === 'spec') connectedSpecIds.add(otherId);
        }
      }
    }

    // Count implementing repos (distinct repo_id values among governed nodes)
    const repoIds = new Set();
    const affectedNodes = (graph.nodes ?? []).filter(n => governedNodeIds.has(n.id));
    for (const n of affectedNodes) {
      if (n.repo_id) repoIds.add(n.repo_id);
    }

    const affectedTypes = new Map();
    for (const n of affectedNodes) {
      affectedTypes.set(n.node_type ?? 'unknown', (affectedTypes.get(n.node_type ?? 'unknown') ?? 0) + 1);
    }
    // Detect new gyre:assert directives added during editing
    const originalAssertCount = (specEditorOriginal.match(/gyre:assert/g) ?? []).length;
    const currentAssertCount = (specEditorContent.match(/gyre:assert/g) ?? []).length;
    const newAssertions = currentAssertCount - originalAssertCount;

    // Compute transitive blast radius: nodes reachable from governed nodes via calls/implements
    const transitiveIds = new Set(governedNodeIds);
    if (specEditorDirty) {
      // Only compute blast radius when content has actually changed
      let frontier = new Set(governedNodeIds);
      for (let d = 0; d < 3; d++) { // 3 hops to estimate downstream impact
        const nextFrontier = new Set();
        for (const e of (graph.edges ?? [])) {
          const et = (e.edge_type ?? e.type ?? '').toLowerCase();
          if (et !== 'calls' && et !== 'implements' && et !== 'depends_on') continue;
          const src = e.source_id ?? e.from_node_id ?? e.from;
          const tgt = e.target_id ?? e.to_node_id ?? e.to;
          // Incoming callers are what breaks if governed code changes
          if (frontier.has(tgt) && !transitiveIds.has(src)) {
            transitiveIds.add(src);
            nextFrontier.add(src);
          }
        }
        frontier = nextFrontier;
        if (frontier.size === 0) break;
      }
    }

    return {
      governedCount: governedNodeIds.size,
      blastRadius: transitiveIds.size - governedNodeIds.size, // additional nodes affected
      connectedSpecs: connectedSpecIds.size,
      implementingRepos: repoIds.size,
      byType: [...affectedTypes.entries()].map(([t, c]) => `${c} ${t}${c !== 1 ? 's' : ''}`).join(', '),
      newAssertions,
    };
  });

  async function runPrediction() {
    if (!selectedRepoId || !specEditorPath || predictLoading) return;
    predictLoading = true;
    predictError = '';
    ghostOverlays = [];
    predictAffectedSpecs = [];
    predictEstimatedCost = null;
    predictConfidence = null;
    try {
      const result = await api.graphPredict(selectedRepoId, {
        spec_path: specEditorPath,
        draft_content: specEditorContent,
      });

      // Extract prediction-level metadata
      predictAffectedSpecs = result?.affected_specs ?? [];
      predictEstimatedCost = result?.estimated_agent_cost ?? result?.cost ?? null;
      predictConfidence = result?.confidence ?? null;

      // Build ghost overlays with per-node confidence and reason.
      // Validate each item has at minimum a name and action.
      const overlays = [];
      function addOverlay(item, defaultAction) {
        const name = item?.name ?? item?.qualified_name;
        if (!name || typeof name !== 'string') return; // Skip malformed items
        const action = item?.action ?? defaultAction;
        if (!['add', 'change', 'remove'].includes(action)) return; // Skip unknown actions
        overlays.push({
          id: item.id ?? `ghost-${action}-${overlays.length}`,
          name,
          type: item.node_type ?? item.type ?? 'unknown',
          action,
          confidence: item.confidence,
          reason: item.reason,
        });
      }
      for (const item of (result?.added ?? [])) addOverlay(item, 'add');
      for (const item of (result?.changed ?? [])) addOverlay(item, 'change');
      for (const item of (result?.removed ?? [])) addOverlay(item, 'remove');
      // Also check for predictions array (alternative response format)
      for (const item of (result?.predictions ?? [])) addOverlay(item, 'change');
      ghostOverlays = overlays;
    } catch (e) {
      predictError = e.message ?? 'Prediction failed';
    } finally {
      predictLoading = false;
    }
  }

  let specEditorDirty = $derived(specEditorContent !== specEditorOriginal);
  let publishLoading = $state(false);
  let publishError = $state('');
  let thoroughLoading = $state(false);

  // Thorough preview: create throwaway branch, run agents, show real impact (minutes)
  async function runThoroughPreview() {
    if (!selectedRepoId || !specEditorPath || thoroughLoading || !specEditorDirty) return;
    thoroughLoading = true;
    try {
      // Step 1: Save the spec draft to a throwaway branch
      const result = await api.thoroughPreview(selectedRepoId, {
        spec_path: specEditorPath,
        draft_content: specEditorContent,
      });
      // Step 2: Show a toast indicating the preview is running
      showToast('Full preview started — agents are implementing on a throwaway branch. This may take a few minutes.', { type: 'info' });
      // Step 3: Poll for results
      if (result?.task_id) {
        pollThoroughPreview(result.task_id);
      } else if (result?.predictions) {
        // Immediate result (fast path)
        const overlays = [];
        for (const item of (result.predictions ?? [])) {
          overlays.push({
            id: item.id ?? `thorough-${overlays.length}`,
            name: item.name ?? item.qualified_name ?? '?',
            type: item.node_type ?? 'unknown',
            action: item.action ?? 'change',
            confidence: 'high', // Thorough preview = high confidence
            reason: item.reason ?? 'Confirmed by agent execution',
          });
        }
        ghostOverlays = overlays;
        showToast(`Full preview complete: ${overlays.length} confirmed changes.`, { type: 'success' });
      }
    } catch (e) {
      // If thorough preview endpoint doesn't exist, fall back to prediction with a message
      if (e.message?.includes('404') || e.message?.includes('not found')) {
        showToast('Full preview not yet available — using LLM prediction instead.', { type: 'warning' });
        runPrediction();
      } else {
        showToast(`Full preview failed: ${e.message ?? 'Unknown error'}`, { type: 'error' });
      }
    } finally {
      thoroughLoading = false;
    }
  }

  async function pollThoroughPreview(taskId) {
    for (let i = 0; i < 30; i++) { // Poll up to 5 minutes (30 × 10s)
      await new Promise(r => setTimeout(r, 10000));
      try {
        const status = await api.taskStatus(taskId);
        if (status?.status === 'completed') {
          const overlays = (status.predictions ?? []).map((item, idx) => ({
            id: item.id ?? `thorough-${idx}`,
            name: item.name ?? '?',
            type: item.node_type ?? 'unknown',
            action: item.action ?? 'change',
            confidence: 'high',
            reason: item.reason ?? 'Confirmed by agent execution',
          }));
          ghostOverlays = overlays;
          showToast(`Full preview complete: ${overlays.length} confirmed changes.`, { type: 'success' });
          return;
        } else if (status?.status === 'failed') {
          showToast(`Full preview failed: ${status.error ?? 'Agent execution failed'}`, { type: 'error' });
          return;
        }
      } catch { /* Keep polling */ }
    }
    showToast('Full preview timed out — check task status for results.', { type: 'warning' });
  }

  // Debounced auto-prediction and spec assertion re-check:
  // Run predictions 2s after user stops typing per spec §3.2-3.3.
  // Also re-check gyre:assert assertions continuously (§9).
  let predictDebounceTimer = null;
  let predictVersion = 0; // Guards against stale closure race conditions
  $effect(() => {
    // Watch for spec content changes
    const content = specEditorContent;
    const original = specEditorOriginal;
    const dirty = content !== original;
    const path = specEditorPath;
    const repo = selectedRepoId;

    // Clear previous timer
    if (predictDebounceTimer) clearTimeout(predictDebounceTimer);

    // Only auto-predict/assert when editing and we have the necessary data
    if (!path || !repo || !specEditorOpen) return;

    // Capture current version to detect stale closures
    const thisVersion = ++predictVersion;

    // Debounce: 2 seconds after the user stops typing
    predictDebounceTimer = setTimeout(() => {
      // Guard: if the spec editor has changed since this timer was set, skip
      if (thisVersion !== predictVersion) return;

      // Re-check assertions on every edit if content has gyre:assert directives (§9)
      if (content.includes('gyre:assert') && !assertionsLoading) {
        assertionsLoading = true;
        api.checkSpecAssertions(repo, path, content)
          .then(result => {
            if (thisVersion === predictVersion) {
              specAssertionResults = result?.assertions ?? [];
            }
          })
          .catch(() => {
            if (thisVersion === predictVersion) specAssertionResults = [];
          })
          .finally(() => { assertionsLoading = false; });
      }

      // Don't auto-predict if a manual prediction is already running
      if (dirty && !predictLoading) {
        runPrediction();
      }
    }, 2000);

    return () => {
      if (predictDebounceTimer) clearTimeout(predictDebounceTimer);
    };
  });

  // Publish approval confirmation state
  let publishConfirmOpen = $state(false);

  function requestPublish() {
    if (!selectedRepoId || !specEditorPath || !specEditorDirty) return;
    publishConfirmOpen = true;
  }

  async function confirmPublish() {
    publishConfirmOpen = false;
    if (!selectedRepoId || !specEditorPath || !specEditorDirty) return;
    publishLoading = true;
    publishError = '';
    try {
      // Save the spec content first
      await api.updateSpec(specEditorPath, selectedRepoId, specEditorContent);
      specEditorOriginal = specEditorContent; // Mark as saved

      // Submit for approval — the spec enters the approval flow
      // and agents will implement only after approval (vision.md Principle 3)
      try {
        await api.approveSpec(specEditorPath, '');
        showToast('Spec published and submitted for approval. Agents will implement after approval.', { type: 'success' });
      } catch (approvalErr) {
        // Approval endpoint may not exist — show a clear warning so the user
        // knows the save succeeded but approval didn't go through.
        const reason = approvalErr?.message ?? 'Approval endpoint unavailable';
        showToast(`Spec saved, but approval submission failed: ${reason}. You may need to submit for approval separately.`, { type: 'warning' });
      }

      // Trigger spec assertion checking post-publish (vision.md Principle 5: Execute step)
      try {
        await api.checkSpecAssertions(selectedRepoId, specEditorPath, specEditorContent);
      } catch { /* assertion check is best-effort */ }

      closeSpecEditor();
      // Execute→Observe loop (vision.md Principle 5): after publishing,
      // poll for graph changes so ghost overlays transition to real nodes.
      // The agent implements the spec in the background; when the graph
      // updates, we re-fetch and the user sees real nodes replace ghosts.
      pollForGraphUpdate(selectedRepoId, 5, 10000);
    } catch (e) {
      publishError = e.message ?? 'Failed to publish spec';
    } finally {
      publishLoading = false;
    }
  }

  // Poll for graph changes after spec publish (Execute→Observe step).
  // Uses content-based comparison (node IDs + names hash) to detect any change,
  // not just count changes. Polls up to 12 attempts over 2 minutes.
  let publishPolling = $state(false);

  function graphFingerprint(g) {
    if (!g?.nodes?.length) return '';
    // Sort node IDs for deterministic comparison, include names for rename detection
    const entries = g.nodes.map(n => `${n.id}:${n.name ?? ''}`).sort();
    return entries.join('|');
  }

  async function pollForGraphUpdate(repoId, maxAttempts = 12, intervalMs = 10000) {
    if (!repoId) return;
    publishPolling = true;
    const baseline = graphFingerprint(graph);
    for (let i = 0; i < maxAttempts; i++) {
      await new Promise(r => setTimeout(r, intervalMs));
      if (selectedRepoId !== repoId) break; // User navigated away
      try {
        const newGraph = await api.repoGraph(repoId);
        if (graphFingerprint(newGraph) !== baseline) {
          // Graph changed — transition ghosts to real nodes
          graph = newGraph;
          ghostOverlays = [];
          showToast('Architecture updated — agents implemented the spec changes.', { type: 'success' });
          publishPolling = false;
          return;
        }
      } catch { /* graph fetch failed, keep polling */ }
    }
    // Polling exhausted — clear ghosts and notify
    if (ghostOverlays.length > 0) {
      ghostOverlays = [];
      showToast('Prediction timeout — architecture may still be updating.', { type: 'warning' });
    }
    publishPolling = false;
  }

  function cancelPublish() {
    publishConfirmOpen = false;
  }

  function closeSpecEditorWithGuard() {
    if (specEditorDirty) {
      if (!confirm('You have unsaved spec changes. Discard them?')) return;
    }
    closeSpecEditor();
  }

  // Workspace-scope: track when a repo has been selected to show graph canvas
  let showingRepoGraph = $state(false);

  let insightsCollapsed = $state(true);
  let chatCollapsed = $state(false);

  // Reset chatCollapsed when viewport widens past the medium breakpoint,
  // preventing stranded UI state where chat is hidden with no toggle visible.
  $effect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return;
    const mql = window.matchMedia('(min-width: 1025px)');
    const handler = (e) => { if (e.matches) chatCollapsed = false; };
    mql.addEventListener('change', handler);
    // Also reset on mount if already wide
    if (mql.matches) chatCollapsed = false;
    return () => mql.removeEventListener('change', handler);
  });

  // Manual view query editor state
  let queryEditorOpen = $state(false);
  let queryEditorText = $state('');
  let queryEditorError = $state('');

  const queryPresets = [
    {
      label: 'Blast Radius',
      query: {
        scope: { type: 'focus', node: '$clicked', edges: ['calls', 'implements'], direction: 'incoming', depth: 10 },
        emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'], dim_unmatched: 0.12 },
        edges: { filter: ['calls', 'implements'] },
        zoom: 'fit',
        annotation: { title: 'Blast radius: $name', description: '{{count}} transitive callers/implementors' },
      },
    },
    {
      label: 'Test Gaps',
      query: {
        scope: { type: 'test_gaps' },
        emphasis: { highlight: { matched: { color: '#ef4444', label: 'Untested' } }, dim_unmatched: 0.3 },
        annotation: { title: 'Test coverage gaps', description: '{{count}} functions not reachable from any test' },
      },
    },
    {
      label: 'Hot Paths',
      query: {
        scope: { type: 'all' },
        emphasis: { heat: { metric: 'incoming_calls', palette: 'blue-red' } },
        annotation: { title: 'Hot paths', description: 'Nodes colored by incoming call frequency' },
      },
    },
  ];

  function runManualQuery() {
    queryEditorError = '';
    const text = queryEditorText.trim();
    if (!text) {
      queryEditorError = 'Query cannot be empty';
      return;
    }
    try {
      const parsed = JSON.parse(text);
      // Basic schema validation: must have a scope with a type field
      if (!parsed.scope || !parsed.scope.type) {
        queryEditorError = 'Query must have a "scope" object with a "type" field (all, focus, filter, test_gaps, diff, concept)';
        return;
      }
      const validTypes = ['all', 'focus', 'filter', 'test_gaps', 'diff', 'concept'];
      if (!validTypes.includes(parsed.scope.type)) {
        queryEditorError = `Unknown scope type "${parsed.scope.type}". Valid: ${validTypes.join(', ')}`;
        return;
      }
      activeViewQuery = parsed;
    } catch (e) {
      queryEditorError = `Invalid JSON: ${e.message}`;
    }
  }

  function clearManualQuery() {
    queryEditorText = '';
    queryEditorError = '';
    activeViewQuery = null;
  }

  function applyPreset(preset) {
    queryEditorText = JSON.stringify(preset.query, null, 2);
    queryEditorError = '';
  }


  // Concept search state
  let conceptQuery = $state('');
  let conceptLoading = $state(false);
  let conceptNodes = $state(null); // null = no active search
  let conceptEdges = null;
  let debounceTimer = null;

  // Load repos when in workspace/repo scope (graph dropdown)
  $effect(() => {
    if (scopeType !== 'tenant') {
      loadRepos();
    }
    if (scopeType === 'workspace') {
      loadWsRepos();
    }
  });

  // Auto-select repo when scope.repoId is set
  $effect(() => {
    if (scopeType === 'repo' && scope.repoId && scope.repoId !== selectedRepoId) {
      selectedRepoId = scope.repoId;
      clearConceptSearch();
      loadGraph(scope.repoId);
    }
  });

  async function loadWsRepos() {
    wsReposLoading = true;
    wsReposError = null;
    try {
      wsRepos = scope.workspaceId
        ? await api.repos({ workspaceId: scope.workspaceId })
        : await api.allRepos();
    } catch (e) {
      wsReposError = e.message ?? $t('explorer_view.repos_load_failed', { values: { error: '' } });
      wsRepos = [];
    } finally {
      wsReposLoading = false;
    }
  }

  function selectRepo(repo) {
    // In workspace-scope mode, selecting a repo loads its graph in this view
    selectedRepoId = repo.id;
    showingRepoGraph = true;
    clearConceptSearch();
    loadGraph(repo.id);
  }

  function backToRepoList() {
    showingRepoGraph = false;
    selectedRepoId = '';
    graph = null;
    graphError = null;
  }

  async function loadRepos() {
    reposLoading = true;
    try {
      repos = await api.allRepos();
    } catch (e) {
      showToast($t('explorer_view.repos_load_failed', { values: { error: e.message } }), { type: 'error' });
    } finally {
      reposLoading = false;
    }
  }

  async function loadGraph(repoId) {
    if (!repoId) { graph = null; traceData = null; return; }
    loading = true;
    graph = null;
    graphError = null;
    selectedNode = null;
    traceData = null;
    graphWarnings = [];
    graphWarningsDismissed = false;
    try {
      graph = await api.repoGraph(repoId);
      // Surface data quality warnings from the API (e.g., missing LSP toolchains)
      if (graph?.warnings?.length) {
        graphWarnings = graph.warnings;
      }
      // Restore breadcrumb from URL hash if present (deep-link support)
      restoreBreadcrumbFromHash(graph);
      // Load trace data for evaluative lens (best-effort, non-blocking)
      loadTraceData(repoId);
    } catch (e) {
      showToast($t('explorer_view.graph_error', { values: { error: e.message } }), { type: 'error' });
      graphError = e.message;
      graph = { nodes: [], edges: [] };
    } finally {
      loading = false;
    }
  }

  /** Restore breadcrumb drill-down state from URL hash (#drill=name1/name2/name3) */
  function restoreBreadcrumbFromHash(graphData) {
    if (!graphData?.nodes?.length) return;
    const hash = window.location.hash;
    if (!hash.startsWith('#drill=')) return;
    const encoded = hash.slice(7);
    if (!encoded) return;
    const segments = encoded.split('/').map(decodeURIComponent);
    const breadcrumb = [];
    for (const seg of segments) {
      let nodeId, nodeName;
      if (seg.includes(':')) {
        const colonIdx = seg.indexOf(':');
        nodeId = seg.slice(0, colonIdx);
        nodeName = seg.slice(colonIdx + 1);
      } else {
        nodeName = seg;
      }
      const node = graphData.nodes.find(n => {
        if (nodeId) return n.id === nodeId;
        return n.name === nodeName;
      });
      if (node) {
        breadcrumb.push({ id: node.id, name: node.name });
      } else {
        break; // Stop at first unresolvable segment
      }
    }
    if (breadcrumb.length > 0) {
      explorerCanvasState = { ...explorerCanvasState, breadcrumb };
    }
  }

  async function loadTraceData(repoId) {
    try {
      // Find the most recent MR for this repo that has trace data
      const mrs = await api.mergeRequests({ repo_id: repoId, per_page: 5 });
      const mrList = mrs?.merge_requests ?? mrs ?? [];
      for (const mr of mrList) {
        try {
          const trace = await api.mrTrace(mr.id);
          if (trace?.spans?.length > 0) {
            traceData = trace;
            return;
          }
        } catch { /* no trace for this MR, try next */ }
      }
    } catch { /* trace loading is best-effort */ }
  }

  function onRepoChange(e) {
    selectedRepoId = e.target.value;
    clearConceptSearch();
    loadGraph(selectedRepoId);
  }

  function onSelectNode(node) {
    selectedNode = node;
    if (node) {
      pushInteraction(`select:${node.name ?? node.id}(${node.node_type ?? 'unknown'})`);
    }
  }

  function onSearchInput(e) {
    conceptQuery = e.target.value;
    clearTimeout(debounceTimer);
    if (!conceptQuery.trim()) {
      conceptNodes = null;
      conceptEdges = null;
      return;
    }
    debounceTimer = setTimeout(() => doConceptSearch(conceptQuery.trim()), 300);
  }

  function onSearchKeydown(e) {
    if (e.key === 'Enter') {
      clearTimeout(debounceTimer);
      const q = conceptQuery.trim();
      if (q) doConceptSearch(q);
    }
  }

  async function doConceptSearch(q) {
    if (!selectedRepoId) return;
    conceptLoading = true;
    pushInteraction(`search:${q}`);
    try {
      const result = await api.getGraphConcept(selectedRepoId, q);
      conceptNodes = result.nodes ?? [];
      conceptEdges = result.edges ?? [];
    } catch (e) {
      showToast($t('explorer_view.concept_search_failed', { values: { error: e.message } }), { type: 'error' });
      conceptNodes = [];
      conceptEdges = [];
    } finally {
      conceptLoading = false;
    }
  }

  function clearConceptSearch() {
    conceptQuery = '';
    conceptNodes = null;
    conceptEdges = null;
    clearTimeout(debounceTimer);
  }

  let searchInputEl = $state(null);

  function onWindowKeydown(e) {
    const isTyping = e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA' || e.target.tagName === 'SELECT' || e.target.isContentEditable;

    if (e.key === 'Escape' && !isTyping) {
      // Escape cascade: close the most recent overlay first
      if (specEditorOpen) { closeSpecEditorWithGuard(); return; }
      if (queryEditorOpen) { queryEditorOpen = false; return; }
      if (detailNode) { detailNode = null; return; }
      if (activeViewQuery) { activeViewQuery = null; return; }
      return;
    }

    if (e.key === '/' && !isTyping && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      // Focus the chat input if graph is showing, otherwise the concept search input
      const chatInput = document.querySelector('.chat-input');
      if (chatInput) {
        chatInput.focus();
      } else {
        searchInputEl?.focus();
      }
    }
  }

  // Protect unsaved spec edits on page navigation
  function onBeforeUnload(e) {
    if (specEditorDirty) {
      e.preventDefault();
      return 'You have unsaved spec changes.';
    }
  }

  onDestroy(() => {
    clearTimeout(debounceTimer);
    ghostOverlays = [];
  });

  // ── Toolchain warning: detect missing call edges ──────────────────
  // Uses server-side warnings from the API when available, with
  // client-side heuristic fallback (verifier finding #15).
  let missingCallEdgesWarning = $derived.by(() => {
    if (graphWarningsDismissed) return null;
    // Prefer server-provided warnings (more accurate)
    if (graphWarnings.length > 0) {
      return graphWarnings.join(' ');
    }
    // Client-side heuristic: detect missing call edges when no server warnings
    if (!graph?.nodes?.length) return null;
    const fnNodes = graph.nodes.filter(n =>
      (n.node_type === 'function' || n.node_type === 'method' || n.node_type === 'endpoint') && !n.deleted_at
    );
    const callEdges = (graph.edges ?? []).filter(e =>
      (e.edge_type ?? e.type ?? '').toLowerCase() === 'calls' && !e.deleted_at
    );
    // If there are 10+ functions but <10% have call edges, warn
    if (fnNodes.length > 10 && callEdges.length < fnNodes.length * 0.1) {
      return `Call graph is incomplete: ${callEdges.length} call edges for ${fnNodes.length} functions. Install language toolchains (rust-analyzer, pyright, gopls) for accurate blast radius and dependency analysis.`;
    }
    return null;
  });

  // ── Repo dependencies & risk metrics ────────────────────────────────
  let repoDeps = $state(null);
  let repoDepsLoading = $state(false);
  let repoRisks = $state(null);
  let repoRisksLoading = $state(false);
  let graphTypes = $state(null);
  let graphModules = $state(null);
  let graphTimeline = $state(null);

  // Timeline scrubber state: when active, filters graph to show system at selected point
  let timelineScrubActive = $state(false);
  let timelineScrubIndex = $state(-1); // index into graphTimeline array, -1 = current

  // When timeline scrub is active, compute filtered nodes/edges to show state at that point
  let timelineFilteredGraph = $derived.by(() => {
    if (!timelineScrubActive || timelineScrubIndex < 0 || !graphTimeline?.length || !graph?.nodes?.length) return null;
    const delta = graphTimeline[timelineScrubIndex];
    if (!delta?.timestamp) return null;
    const cutoff = typeof delta.timestamp === 'number' ? delta.timestamp : Math.floor(new Date(delta.timestamp).getTime() / 1000);
    // Filter to nodes that existed at or before the cutoff time
    const filteredNodes = graph.nodes.filter(n => {
      // If node has first_seen_at, only include if it was seen before cutoff
      const firstSeen = n.first_seen_at ?? n.created_at ?? 0;
      const ts = typeof firstSeen === 'number' ? firstSeen : Math.floor(new Date(firstSeen).getTime() / 1000);
      return ts <= cutoff;
    });
    const nodeIds = new Set(filteredNodes.map(n => n.id));
    const filteredEdges = graph.edges.filter(e => {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      return nodeIds.has(src) && nodeIds.has(tgt);
    });
    return { nodes: filteredNodes, edges: filteredEdges };
  });

  // The effective graph passed to canvas: either time-filtered or full
  let effectiveGraph = $derived(timelineFilteredGraph ?? graph);

  // Compute delta stats between the scrubber position and the current graph (system-explorer.md §6)
  // Uses imported functions from timeline-utils.js for testability.
  let timelineDeltaStats = $derived.by(() => {
    if (!timelineScrubActive || !timelineFilteredGraph || !graph?.nodes?.length) return null;
    return computeTimelineDeltaStats({
      graph,
      timelineFilteredGraph,
      timeline: graphTimeline,
      scrubIndex: timelineScrubIndex,
    });
  });

  // Compute ghost overlays for historical time travel (system-explorer.md §6)
  // Forward ghosts (green): nodes that exist now but didn't at scrubber time
  // Backward ghosts (red): nodes that existed at scrubber time but have been removed
  // Modified (yellow): nodes that exist at both times but changed
  let timelineGhostOverlays = $derived.by(() => {
    if (!timelineScrubActive || !timelineFilteredGraph || !graph?.nodes?.length) return [];
    return computeTimelineGhostOverlays({
      graph,
      timelineFilteredGraph,
      timeline: graphTimeline,
      scrubIndex: timelineScrubIndex,
    });
  });

  // Merged ghost overlays: spec-edit prediction ghosts + timeline ghosts
  let mergedGhostOverlays = $derived(
    timelineScrubActive && timelineGhostOverlays.length > 0
      ? timelineGhostOverlays
      : ghostOverlays
  );

  // Reset insights on repo change; lazy-load when panel expanded (Vision Principle 2:
  // "Right Context, Not More Context" — don't fetch data the user hasn't asked to see)
  $effect(() => {
    if (!selectedRepoId) return;
    repoDeps = null;
    repoRisks = null;
    graphTypes = null;
    graphModules = null;
    graphTimeline = null;
  });

  // Lazy-load insights data only when the panel is expanded
  let insightsLoadedFor = $state('');
  $effect(() => {
    if (insightsCollapsed || !selectedRepoId) return;
    if (insightsLoadedFor === selectedRepoId) return; // already loaded
    insightsLoadedFor = selectedRepoId;
    repoDepsLoading = true;
    repoRisksLoading = true;
    const currentRepoId = selectedRepoId;
    Promise.all([
      api.repoDependencies(currentRepoId).catch(() => []),
      api.repoDependents(currentRepoId).catch(() => []),
      api.repoGraphRisks(currentRepoId).catch(() => []),
      api.repoGraphTypes(currentRepoId).catch(() => ({ nodes: [] })),
      api.repoGraphModules(currentRepoId).catch(() => ({ nodes: [] })),
      api.repoGraphTimeline(currentRepoId).catch(() => []),
    ]).then(([deps, depts, risks, types, modules, timeline]) => {
      if (selectedRepoId !== currentRepoId) return; // stale
      repoDeps = { dependencies: Array.isArray(deps) ? deps : [], dependents: Array.isArray(depts) ? depts : [] };
      repoRisks = Array.isArray(risks) ? risks : [];
      graphTypes = types?.nodes ?? (Array.isArray(types) ? types : []);
      graphModules = modules?.nodes ?? (Array.isArray(modules) ? modules : []);
      graphTimeline = Array.isArray(timeline) ? timeline : [];
    }).finally(() => { repoDepsLoading = false; repoRisksLoading = false; });
  });
</script>

<svelte:window onkeydown={onWindowKeydown} onbeforeunload={onBeforeUnload} />

{#if scopeType === 'tenant'}
  <!-- Tenant scope: workspace cards grid (S4.4a) -->
  <WorkspaceCards {onSelectWorkspace} />

{:else if scopeType === 'workspace' && !showingRepoGraph}
  <!-- Workspace scope: repo list for graph exploration — S4.4b -->
  <div class="ws-repo-list" aria-busy={wsReposLoading}>
    <div class="ws-repo-header">
      <h1 class="page-title">{$t('explorer_view.workspace_title')}</h1>
      <p class="ws-repo-desc">{$t('explorer_view.workspace_desc')}</p>
    </div>
    {#if wsReposLoading}
      <div class="ws-repo-grid">
        <Skeleton height="80px" />
        <Skeleton height="80px" />
        <Skeleton height="80px" />
      </div>
    {:else if wsReposError}
      <div class="error-banner" role="alert">
        <span>{wsReposError}</span>
        <button onclick={() => { wsReposError = null; loadWsRepos(); }} class="retry-btn">{$t('common.retry')}</button>
      </div>
    {:else if wsRepos.length === 0}
      <EmptyState title={$t('explorer_view.no_repos')} description={$t('explorer_view.no_repos_desc')} />
    {:else}
      <div class="ws-repo-grid">
        {#each wsRepos as repo (repo.id)}
          <button class="ws-repo-card" onclick={() => selectRepo(repo)} aria-label={$t('explorer_view.explore_repo', { values: { name: repo.name } })}>
            <div class="ws-repo-card-left">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" class="ws-repo-icon" aria-hidden="true">
                <path d="M3 3h6l2 3h10a2 2 0 012 2v11a2 2 0 01-2 2H3a2 2 0 01-2-2V5a2 2 0 012-2z"/>
              </svg>
              <div class="ws-repo-info">
                <span class="ws-repo-name">{repo.name}</span>
                {#if repo.description}
                  <span class="ws-repo-description">{repo.description}</span>
                {/if}
              </div>
            </div>
            <span class="ws-repo-explore">{$t('explorer_view.explore_arrow')}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>

{:else}
  <!-- Repo/workspace-repo scope: architecture canvas + chat (S4.4b/c) -->
  <div class="explorer-view">
    <!-- Header -->
    <div class="explorer-header">
      <div class="header-left">
        {#if scopeType === 'workspace' && showingRepoGraph}
          <button class="back-to-repos-btn" onclick={backToRepoList} type="button" aria-label="Back to repositories">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
              <path d="M19 12H5M12 19l-7-7 7-7"/>
            </svg>
            Repos
          </button>
        {/if}
        <h1 class="page-title">{scopeType === 'repo' || showingRepoGraph ? $t('explorer_view.architecture_title') : $t('explorer_view.system_title')}</h1>
        {#if scopeType !== 'repo' && !showingRepoGraph}
          <p class="subtitle">{$t('explorer_view.system_subtitle')}</p>
        {/if}
      </div>
      <div class="header-right">
        <!-- Repo selector — hidden in repo scope (auto-selected from parent) -->
        {#if scopeType !== 'repo' && !showingRepoGraph}
          {#if reposLoading}
            <div class="repo-selector-skeleton">
              <Skeleton lines={1} />
            </div>
          {:else}
            <div class="repo-select-wrap">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" class="repo-icon" aria-hidden="true">
                <path d="M3 3h6l2 3h10a2 2 0 012 2v11a2 2 0 01-2 2H3a2 2 0 01-2-2V5a2 2 0 012-2z"/>
              </svg>
              <select
                class="repo-select"
                value={selectedRepoId}
                onchange={onRepoChange}
                aria-label={$t('explorer_view.select_repo')}
              >
                <option value="">{$t('explorer_view.select_repo')}</option>
                {#each repos as repo}
                  <option value={repo.id}>{repo.name}</option>
                {/each}
              </select>
            </div>
          {/if}
        {/if}

        {#if graph}
          <div class="graph-stats">
            <span class="stat">
              <span class="stat-val">{effectiveGraph.nodes?.length ?? 0}</span>
              <span class="stat-label">{$t('explorer_canvas.nodes')}</span>
              {#if timelineScrubActive}
                <span class="stat-time-travel" title="Time travel active">⏳</span>
              {/if}
            </span>
            <span class="stat-sep">·</span>
            <span class="stat">
              <span class="stat-val">{effectiveGraph.edges?.length ?? 0}</span>
              <span class="stat-label">{$t('explorer_canvas.edges')}</span>
            </span>
            {#if timelineScrubActive && timelineDeltaStats}
              <span class="stat-sep">·</span>
              <span class="stat timeline-delta-summary" title="Between then and now">
                {#each Object.entries(timelineDeltaStats.addedByType ?? {}) as [nodeType, count]}
                  <span class="delta-add">+{count} {count === 1 ? nodeType : nodeType + 's'}</span>
                {/each}
                {#each Object.entries(timelineDeltaStats.removedByType ?? {}) as [nodeType, count]}
                  <span class="delta-remove">-{count} {count === 1 ? nodeType : nodeType + 's'}</span>
                {/each}
                {#each Object.entries(timelineDeltaStats.modifiedByType ?? {}) as [nodeType, count]}
                  <span class="delta-modify">{count} {count === 1 ? nodeType : nodeType + 's'} modified</span>
                {/each}
              </span>
            {/if}
          </div>
        {/if}

        <!-- Lens toggle is in the ExplorerCanvas toolbar to avoid duplication -->
      </div>
    </div>


    <!-- Control bar — concept search + filter toggle, always shown when repo is selected -->
    {#if selectedRepoId}
      <div class="concept-search-bar">
        <!-- Manual query editor toggle -->
        <button
          class="ctrl-btn icon-btn"
          class:active={queryEditorOpen}
          onclick={() => { queryEditorOpen = !queryEditorOpen; }}
          title="Manual view query editor"
          aria-label="Toggle manual view query editor"
          aria-pressed={queryEditorOpen}
          type="button"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>
          </svg>
        </button>

        <!-- Concept search -->
        <div class="search-input-wrap">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" class="search-icon" aria-hidden="true">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
          </svg>
          <input
            type="search"
            class="concept-input"
            placeholder={$t('explorer_view.search_placeholder')}
            disabled={loading}
            value={conceptQuery}
            oninput={onSearchInput}
            onkeydown={onSearchKeydown}
            aria-label={$t('explorer_view.search_placeholder')}
            bind:this={searchInputEl}
          />
        </div>

        <span aria-live="polite" class="sr-only">
          {#if conceptLoading}
            {$t('explorer_view.searching')}
          {:else if conceptNodes !== null && conceptQuery.trim()}
            {#if conceptNodes.length > 0}
              {$t('explorer_view.concepts_found', { values: { count: conceptNodes.length } })}
            {:else}
              {$t('explorer_view.no_concepts')}
            {/if}
          {/if}
        </span>
        {#if conceptLoading}
          <span class="search-loading">
            <span class="spinner" aria-hidden="true"></span>
            {$t('explorer_view.searching')}
          </span>
        {:else if conceptNodes !== null && conceptQuery.trim()}
          {#if conceptNodes.length > 0}
            <span class="concept-chip">
              {$t('explorer_view.nodes_matching', { values: { count: conceptNodes.length, query: conceptQuery.trim() } })}
              <button class="chip-clear" onclick={clearConceptSearch} aria-label={$t('explorer_view.clear_search')}>✕</button>
            </span>
          {:else}
            <span class="concept-chip no-results">
              {$t('explorer_view.no_nodes_matching', { values: { query: conceptQuery.trim() } })}
              <button class="chip-clear" onclick={clearConceptSearch} aria-label={$t('explorer_view.clear_search')}>✕</button>
            </span>
          {/if}
        {/if}
      </div>
    {/if}

    <!-- Main content -->
    <div class="explorer-body">
      <div class="explorer-body-main">
        {#if !selectedRepoId}
          <div class="empty-state-wrap">
            {#if scopeType === 'repo'}
              <!-- Repo scope: repo ID will be set by the auto-select effect -->
              <Skeleton lines={6} />
            {:else}
              <EmptyState
                title={$t('explorer_view.select_repo')}
                description={$t('explorer_view.select_repo_desc')}
              />
              {#if repos.length === 0 && !reposLoading}
                <p class="hint">{$t('explorer_view.no_repos_hint')}</p>
                <button class="go-admin-btn" onclick={() => goToWorkspaceSettings?.()}>{$t('explorer_view.go_to_settings')}</button>
              {/if}
            {/if}
          </div>

        {:else if loading}
          <div class="loading-wrap">
            <Skeleton lines={8} />
            <p class="loading-msg">{$t('explorer_view.fetching_graph')}</p>
          </div>

        {:else if graphError}
          <div class="graph-error" role="alert">
            <p>{$t('explorer_view.graph_error', { values: { error: graphError } })}</p>
            <button onclick={() => loadGraph(selectedRepoId)} aria-label={$t('common.retry')}>{$t('common.retry')}</button>
          </div>

        {:else if graph}
          <div class="explorer-split">
            <div class="explorer-canvas-area">
              {#if missingCallEdgesWarning}
                <div class="toolchain-warning" role="alert">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" style="flex-shrink:0"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
                  <span>{missingCallEdgesWarning}</span>
                  <button class="toolchain-warning-dismiss" onclick={() => { graphWarningsDismissed = true; }} title="Dismiss" type="button" aria-label="Dismiss warning">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                  </button>
                </div>
              {/if}
              <ExplorerCanvas
                repoId={selectedRepoId}
                nodes={effectiveGraph.nodes ?? []}
                edges={effectiveGraph.edges ?? []}
                activeQuery={activeViewQuery}
                filter={explorerFilter}
                lens={explorerLens}
                filters={null}
                bind:canvasState={explorerCanvasState}
                onNodeDetail={(n) => {
                  detailNode = n;
                  highlightedSpanId = null;
                  if (n?._action === 'view_spec' && n.spec_path) {
                    openSpecEditor(n.spec_path);
                  } else if (n?._action === 'create_spec') {
                    // Open spec editor with a template for the uncovered node
                    const suggestedPath = n.suggested_spec_path || `specs/system/${(n.name ?? 'new').toLowerCase().replace(/[^a-z0-9]+/g, '-')}.md`;
                    specEditorOpen = true;
                    specEditorPath = suggestedPath;
                    const template = `# ${n.name ?? 'New Spec'}\n\nStatus: Draft\n\n## Purpose\n\nGoverns the \`${n.qualified_name ?? n.name}\` ${n.node_type ?? 'component'}.\n\n## Requirements\n\n- TODO: Define requirements\n`;
                    specEditorContent = template;
                    specEditorOriginal = '';
                    specEditorError = '';
                  } else if (n?._action === 'view_code' && n.file_path) {
                    // Open in code: show file in detail panel.
                    // The NodeDetailPanel displays Location (file_path:line).
                    // This preserves canvas state unlike opening a new browser tab.
                    detailNode = n;
                  }
                }}
                onInteractiveQuery={(q) => { activeViewQuery = q; }}
                ghostOverlays={mergedGhostOverlays}
                {traceData}
                queryResult={viewQueryResult}
                assertionResults={specAssertionResults}
                assertionSpecPath={specEditorOpen ? specEditorPath : null}
                {highlightedSpanId}
                timeline={graphTimeline ?? []}
                timelineActive={timelineScrubActive}
                timelineScrubIndex={timelineScrubIndex}
                onTimelineToggle={() => {
                  timelineScrubActive = !timelineScrubActive;
                  if (!timelineScrubActive) timelineScrubIndex = -1;
                  else if (graphTimeline?.length) timelineScrubIndex = graphTimeline.length - 1;
                }}
                onTimelineScrub={(idx) => { timelineScrubIndex = idx; }}
                onTimelineMarkerClick={(_delta) => {}}
                {timelineDeltaStats}
              />
              <!-- Architecture Insights — collapsible panel inside canvas area -->
              {#if selectedRepoId && !loading && (repoDeps || repoRisks?.length || graphTypes?.length || graphModules?.length || graphTimeline?.length)}
                <div class="arch-insights-overlay">
                  <div class="arch-insights-toggle">
                    <button
                      class="arch-insights-btn"
                      onclick={() => insightsCollapsed = !insightsCollapsed}
                      aria-expanded={!insightsCollapsed}
                      aria-controls="arch-insights-panel"
                    >
                      <span class="arch-toggle-icon" class:open={!insightsCollapsed}>&#9654;</span>
                      Architecture Insights
                    </button>
                  </div>
                  <div class="arch-insights" id="arch-insights-panel" class:collapsed={insightsCollapsed}>
                    <!-- Graph Types (structs/enums extracted from code) -->
                    {#if graphTypes?.length > 0}
                      <div class="arch-insight-section">
                        <h3 class="arch-insight-title">Types ({graphTypes.length})</h3>
                        <p class="arch-insight-desc">Structs, enums, and type definitions extracted from the codebase.</p>
                        <div class="arch-type-grid">
                          {#each graphTypes.slice(0, 20) as node}
                            <div class="arch-type-card" title={node.qualified_name ?? node.name}>
                              <span class="arch-type-kind">{node.node_type ?? 'type'}</span>
                              <span class="arch-type-name">{node.name ?? node.qualified_name}</span>
                              {#if node.doc_comment}
                                <span class="arch-type-doc">{node.doc_comment.slice(0, 80)}{node.doc_comment.length > 80 ? '...' : ''}</span>
                              {/if}
                            </div>
                          {/each}
                          {#if graphTypes.length > 20}
                            <span class="arch-more">+{graphTypes.length - 20} more</span>
                          {/if}
                        </div>
                      </div>
                    {/if}

                    <!-- Graph Modules -->
                    {#if graphModules?.length > 0}
                      <div class="arch-insight-section">
                        <h3 class="arch-insight-title">Modules ({graphModules.length})</h3>
                        <p class="arch-insight-desc">Module hierarchy extracted from the codebase.</p>
                        <ul class="arch-dep-list">
                          {#each graphModules.slice(0, 15) as mod}
                            <li class="arch-dep-item">
                              <span class="mono">{mod.qualified_name ?? mod.name}</span>
                              {#if mod.doc_comment}
                                <span class="arch-mod-doc">{mod.doc_comment.slice(0, 60)}</span>
                              {/if}
                            </li>
                          {/each}
                        </ul>
                      </div>
                    {/if}

                    {#if repoDeps && (repoDeps.dependencies.length > 0 || repoDeps.dependents.length > 0)}
                      <div class="arch-insight-section">
                        <h3 class="arch-insight-title">Cross-Repo Dependencies</h3>
                        {#if repoDeps.dependencies.length > 0}
                          <div class="arch-dep-group">
                            <span class="arch-dep-label">Depends on ({repoDeps.dependencies.length})</span>
                            <ul class="arch-dep-list">
                              {#each repoDeps.dependencies as dep}
                                <li class="arch-dep-item">{dep.name ?? dep.repo_name ?? entityName('repo', dep.repo_id ?? dep)}</li>
                              {/each}
                            </ul>
                          </div>
                        {/if}
                        {#if repoDeps.dependents.length > 0}
                          <div class="arch-dep-group">
                            <span class="arch-dep-label">Depended on by ({repoDeps.dependents.length})</span>
                            <ul class="arch-dep-list">
                              {#each repoDeps.dependents as dep}
                                <li class="arch-dep-item">{dep.name ?? dep.repo_name ?? entityName('repo', dep.repo_id ?? dep)}</li>
                              {/each}
                            </ul>
                          </div>
                        {/if}
                      </div>
                    {/if}
                    {#if repoRisks?.length > 0}
                      <div class="arch-insight-section">
                        <h3 class="arch-insight-title">
                          Risk Hotspots ({repoRisks.length})
                          <button
                            class="timeline-scrub-toggle"
                            onclick={() => {
                              activeViewQuery = {
                                scope: { type: 'filter', computed: "$where(complexity, '>', 5)" },
                                emphasis: { heat: { metric: 'risk_score', palette: 'blue-red' }, dim_unmatched: 0.3 },
                                zoom: 'fit',
                                annotation: { title: 'Risk Map', description: 'Heat by risk_score: churn × complexity × (1 − test_coverage)' },
                              };
                            }}
                            type="button"
                            title="Apply risk heat map overlay to canvas"
                          >Show on Canvas</button>
                        </h3>
                        <p class="arch-insight-desc">Nodes scored for complexity, coupling, or churn that may warrant attention.</p>
                        <ul class="arch-risk-list">
                          {#each repoRisks.slice(0, 10) as node}
                            <li class="arch-risk-item">
                              <span class="arch-risk-name">{node.qualified_name ?? node.name}</span>
                              <span class="arch-risk-score" title="Risk score">{node.risk_score ?? node.score ?? '\u2014'}</span>
                              {#if node.risk_reason ?? node.reason}
                                <span class="arch-risk-reason">{node.risk_reason ?? node.reason}</span>
                              {/if}
                            </li>
                          {/each}
                        </ul>
                      </div>
                    {/if}

                    <!-- Architecture Timeline with interactive scrubber -->
                    {#if graphTimeline?.length > 0}
                      <div class="arch-insight-section">
                        <h3 class="arch-insight-title">
                          Architecture Timeline ({graphTimeline.length} changes)
                          <button
                            class="timeline-scrub-toggle"
                            class:active={timelineScrubActive}
                            onclick={() => {
                              timelineScrubActive = !timelineScrubActive;
                              if (!timelineScrubActive) timelineScrubIndex = -1;
                              else timelineScrubIndex = graphTimeline.length - 1;
                            }}
                            type="button"
                            title={timelineScrubActive ? 'Exit time travel' : 'Scrub through architectural history'}
                          >{timelineScrubActive ? 'Exit Time Travel' : 'Time Travel'}</button>
                        </h3>
                        {#if timelineScrubActive}
                          <div class="timeline-scrubber">
                            <input
                              type="range"
                              min="0"
                              max={graphTimeline.length - 1}
                              bind:value={timelineScrubIndex}
                              class="timeline-slider"
                              aria-label="Timeline scrubber"
                            />
                            <div class="timeline-scrub-info">
                              {#if timelineScrubIndex >= 0 && graphTimeline[timelineScrubIndex]}
                                {@const delta = graphTimeline[timelineScrubIndex]}
                                <span class="timeline-scrub-date">
                                  {delta.timestamp ? new Date(typeof delta.timestamp === 'number' ? delta.timestamp * 1000 : delta.timestamp).toLocaleDateString() : '\u2014'}
                                </span>
                                <span class="timeline-scrub-event">{delta.change_type ?? delta.event ?? 'change'}</span>
                                {#if delta.added_count || delta.removed_count}
                                  <span class="arch-timeline-stats">
                                    {#if delta.added_count}<span class="diff-ins">+{delta.added_count}</span>{/if}
                                    {#if delta.removed_count}<span class="diff-del">-{delta.removed_count}</span>{/if}
                                  </span>
                                {/if}
                                <span class="timeline-scrub-nodes">{effectiveGraph.nodes?.length ?? 0} nodes</span>
                              {/if}
                            </div>
                          </div>
                        {:else}
                          <p class="arch-insight-desc">How the architecture has evolved over time.</p>
                          <div class="arch-timeline">
                            {#each graphTimeline.slice(0, 10) as delta}
                              <div class="arch-timeline-entry">
                                <span class="arch-timeline-time">{delta.timestamp ? new Date(typeof delta.timestamp === 'number' ? delta.timestamp * 1000 : delta.timestamp).toLocaleDateString() : '\u2014'}</span>
                                <span class="arch-timeline-label">{delta.change_type ?? delta.event ?? 'change'}</span>
                                {#if delta.added_count || delta.removed_count}
                                  <span class="arch-timeline-stats">
                                    {#if delta.added_count}<span class="diff-ins">+{delta.added_count}</span>{/if}
                                    {#if delta.removed_count}<span class="diff-del">-{delta.removed_count}</span>{/if}
                                  </span>
                                {/if}
                                {#if delta.commit_sha}
                                  <code class="mono" style="font-size: var(--text-xs); color: var(--color-text-muted)">{delta.commit_sha.slice(0, 7)}</code>
                                {/if}
                              </div>
                            {/each}
                          </div>
                        {/if}
                      </div>
                    {/if}
                  </div>
                </div>
              {/if}
            </div>
            {#if specEditorOpen}
              <!-- Spec editor renders inline in the detail area, replacing the detail panel -->
            {:else if detailNode}
              <div class="explorer-detail-area">
                <NodeDetailPanel
                  node={detailNode}
                  nodes={graph.nodes ?? []}
                  edges={graph.edges ?? []}
                  onClose={() => { detailNode = null; highlightedSpanId = null; }}
                  onNavigate={(n) => { detailNode = n; }}
                  onInteractiveQuery={(q) => { activeViewQuery = q; }}
                  lens={explorerLens}
                  traceSpans={traceData?.spans ?? []}
                  onSpanSelect={(span) => { highlightedSpanId = span?.span_id ?? null; }}
                />
                {#if detailNode.spec_path}
                  <div class="edit-spec-action">
                    <button
                      class="edit-spec-btn"
                      onclick={() => openSpecEditor(detailNode.spec_path)}
                      type="button"
                    >{$t('explorer_view.edit_spec')}</button>
                  </div>
                {/if}
              </div>
            {/if}
            {#if specEditorOpen}
              <div class="spec-editor-panel" role="complementary" aria-label={$t('explorer_view.spec_editor_title')}>
                <div class="spec-editor-header">
                  <h3 class="spec-editor-title">{$t('explorer_view.spec_editor_title')}</h3>
                  <code class="spec-editor-path">{specEditorPath}</code>
                  <button class="spec-editor-close" onclick={closeSpecEditorWithGuard} aria-label={$t('explorer_view.spec_editor_cancel')} type="button">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
                      <path d="M18 6L6 18M6 6l12 12"/>
                    </svg>
                  </button>
                </div>
                {#if instantImpact}
                  <div class="instant-impact" role="status">
                    <span class="instant-label">Instant impact:</span>
                    {#if instantImpact.connectedSpecs > 0}
                      <span class="instant-count">{instantImpact.connectedSpecs} connected spec{instantImpact.connectedSpecs !== 1 ? 's' : ''}</span>
                      <span class="instant-sep">|</span>
                    {/if}
                    {#if instantImpact.implementingRepos > 0}
                      <span class="instant-count">{instantImpact.implementingRepos} repo{instantImpact.implementingRepos !== 1 ? 's' : ''}</span>
                      <span class="instant-sep">|</span>
                    {/if}
                    <span class="instant-count">{instantImpact.governedCount} governed node{instantImpact.governedCount !== 1 ? 's' : ''}</span>
                    {#if instantImpact.blastRadius > 0}
                      <span class="instant-sep">|</span>
                      <span class="instant-count instant-blast">{instantImpact.blastRadius} downstream affected</span>
                    {/if}
                    {#if instantImpact.byType}
                      <span class="instant-types">({instantImpact.byType})</span>
                    {/if}
                    {#if instantImpact.newAssertions > 0}
                      <span class="instant-sep">|</span>
                      <span class="instant-new-assertion">{instantImpact.newAssertions} new assertion{instantImpact.newAssertions !== 1 ? 's' : ''} detected</span>
                    {/if}
                  </div>
                {/if}
                <div class="spec-editor-body">
                  {#if specEditorLoading}
                    <div class="spec-editor-loading">
                      <span class="spinner" aria-hidden="true"></span>
                      <span>{$t('explorer_view.spec_editor_loading')}</span>
                    </div>
                  {:else if specEditorError}
                    <div class="spec-editor-error" role="alert">
                      <span>{$t('explorer_view.spec_editor_error', { values: { error: specEditorError } })}</span>
                    </div>
                  {:else}
                    <div class="spec-editor-with-lines">
                      <div class="spec-line-numbers" aria-hidden="true">
                        {#each (specEditorContent ?? '').split('\n') as _, i}
                          <div class="spec-line-num">{i + 1}</div>
                        {/each}
                      </div>
                      <textarea
                        class="spec-editor-textarea"
                        bind:value={specEditorContent}
                        spellcheck="false"
                        aria-label="Spec content"
                        onscroll={(e) => {
                          const gutter = e.target.previousElementSibling;
                          if (gutter) gutter.scrollTop = e.target.scrollTop;
                        }}
                      ></textarea>
                    </div>
                  {/if}

                  <!-- Spec Assertion Results (§9: inline green checkmark / red X) -->
                  {#if specAssertionResults.length > 0 || assertionsLoading}
                    <div class="spec-assertions-panel">
                      <h4 class="spec-assertions-title">
                        Assertions
                        {#if assertionsLoading}
                          <span class="assertions-loading">checking...</span>
                        {:else}
                          {@const passed = specAssertionResults.filter(a => a.passed).length}
                          {@const failed = specAssertionResults.filter(a => !a.passed).length}
                          <span class="assertions-summary">
                            {#if failed === 0}
                              <span class="assert-pass-all">{passed}/{specAssertionResults.length} passing</span>
                            {:else}
                              <span class="assert-fail-count">{failed} failing</span>, {passed} passing
                            {/if}
                          </span>
                        {/if}
                      </h4>
                      {#if !assertionsLoading}
                        <ul class="spec-assertions-list">
                          {#each specAssertionResults as result}
                            <li class="spec-assertion-item" class:passed={result.passed} class:failed={!result.passed}>
                              <span class="assert-icon">{result.passed ? '\u2714' : '\u2718'}</span>
                              <span class="assert-text">{result.assertion_text}</span>
                              {#if result.explanation}
                                <span class="assert-explain" title={result.explanation}>{result.explanation}</span>
                              {/if}
                            </li>
                          {/each}
                        </ul>
                      {/if}
                    </div>
                  {/if}
                </div>
                <div class="spec-editor-footer">
                  {#if predictError}
                    <div class="spec-editor-predict-error" role="alert">
                      {$t('explorer_view.spec_editor_predict_error', { values: { error: predictError } })}
                    </div>
                  {/if}
                  {#if ghostOverlays.length > 0}
                    <div class="spec-editor-predict-result" role="status">
                      <span class="predict-summary">
                        {ghostOverlays.length} predicted {ghostOverlays.length === 1 ? 'change' : 'changes'}
                        {#if predictConfidence}
                          <span class="predict-confidence" class:high={predictConfidence === 'high'} class:medium={predictConfidence === 'medium'} class:low={predictConfidence === 'low'}>
                            {predictConfidence} confidence
                          </span>
                        {/if}
                      </span>
                      {#if predictEstimatedCost}
                        <span class="predict-cost" title="Estimated agent cost to implement">
                          ~{typeof predictEstimatedCost === 'number' ? `$${predictEstimatedCost.toFixed(2)}` : predictEstimatedCost} est. cost
                        </span>
                      {/if}
                    </div>
                    {#if ghostOverlays.some(g => g.reason)}
                      <div class="predict-details">
                        {#each ghostOverlays.filter(g => g.reason) as ghost}
                          <div class="predict-detail-item">
                            <span class="predict-detail-action" class:add={ghost.action === 'add'} class:change={ghost.action === 'change'} class:remove={ghost.action === 'remove'}>
                              {ghost.action === 'add' ? '+' : ghost.action === 'remove' ? '\u2212' : '\u0394'}
                            </span>
                            <span class="predict-detail-name">{ghost.name}</span>
                            {#if ghost.confidence}
                              <span class="predict-detail-conf" title="Confidence">{ghost.confidence}</span>
                            {/if}
                            <span class="predict-detail-reason">{ghost.reason}</span>
                          </div>
                        {/each}
                      </div>
                    {/if}
                    {#if predictAffectedSpecs.length > 0}
                      <div class="predict-affected-specs">
                        <span class="predict-affected-label">Affected specs:</span>
                        {#each predictAffectedSpecs as sp}
                          <button
                            class="predict-affected-spec-btn"
                            onclick={() => openSpecEditor(sp)}
                            title="Open {sp}"
                            type="button"
                          >{sp.split('/').pop()}</button>
                        {/each}
                      </div>
                    {/if}
                  {/if}
                  {#if publishError}
                    <div class="spec-editor-predict-error" role="alert">
                      {publishError}
                    </div>
                  {/if}
                  <div class="spec-editor-actions">
                    <button
                      class="spec-editor-cancel-btn"
                      onclick={closeSpecEditorWithGuard}
                      type="button"
                    >{$t('explorer_view.spec_editor_cancel')}</button>
                    <button
                      class="spec-editor-preview-btn"
                      onclick={runPrediction}
                      disabled={predictLoading || !specEditorDirty}
                      type="button"
                      title="LLM prediction: what code changes would this spec produce? (2-5s)"
                    >
                      {#if predictLoading}
                        <span class="spinner" aria-hidden="true"></span>
                        {$t('explorer_view.spec_editor_predicting')}
                      {:else}
                        {$t('explorer_view.spec_editor_preview')}
                      {/if}
                    </button>
                    <button
                      class="spec-editor-thorough-btn"
                      onclick={runThoroughPreview}
                      disabled={thoroughLoading || !specEditorDirty}
                      type="button"
                      title="Full preview: create throwaway branch, run agents, show real downstream impact (minutes)"
                    >
                      {#if thoroughLoading}
                        <span class="spinner" aria-hidden="true"></span>
                        Running full preview...
                      {:else}
                        Full preview
                      {/if}
                    </button>
                    <button
                      class="spec-editor-publish-btn"
                      onclick={requestPublish}
                      disabled={publishLoading || !specEditorDirty}
                      type="button"
                      title="Save spec changes and submit for approval"
                    >
                      {#if publishLoading}
                        <span class="spinner" aria-hidden="true"></span>
                        Publishing...
                      {:else}
                        Publish
                      {/if}
                    </button>
                  </div>
                </div>
              </div>
              {#if publishConfirmOpen}
                <div class="publish-confirm-overlay" role="dialog" aria-label="Confirm publish">
                  <div class="publish-confirm-dialog">
                    <h4 class="publish-confirm-title">Confirm Publish</h4>
                    <p class="publish-confirm-desc">
                      Publishing this spec will save your changes and submit them for approval.
                      After approval, agents will automatically implement the spec changes.
                    </p>
                    {#if instantImpact}
                      <div class="publish-confirm-impact">
                        <span class="publish-impact-label">Impact:</span>
                        <span>{instantImpact.governedCount} governed node{instantImpact.governedCount !== 1 ? 's' : ''}</span>
                        {#if instantImpact.connectedSpecs > 0}
                          <span>, {instantImpact.connectedSpecs} connected spec{instantImpact.connectedSpecs !== 1 ? 's' : ''}</span>
                        {/if}
                        {#if instantImpact.implementingRepos > 0}
                          <span>, {instantImpact.implementingRepos} repo{instantImpact.implementingRepos !== 1 ? 's' : ''}</span>
                        {/if}
                      </div>
                    {/if}
                    <div class="publish-confirm-actions">
                      <button class="publish-confirm-cancel" onclick={cancelPublish} type="button">Cancel</button>
                      <button class="publish-confirm-ok" onclick={confirmPublish} type="button">Confirm &amp; Publish</button>
                    </div>
                  </div>
                </div>
              {/if}
            {/if}
            {#if queryEditorOpen}
              <div class="query-editor-panel" role="complementary" aria-label="Manual view query editor">
                <div class="query-editor-header">
                  <h3 class="query-editor-title">View Query Editor</h3>
                  <button class="query-editor-close" onclick={() => { queryEditorOpen = false; }} aria-label="Close query editor" type="button">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
                      <path d="M18 6L6 18M6 6l12 12"/>
                    </svg>
                  </button>
                </div>
                <div class="query-editor-presets">
                  <span class="query-editor-presets-label">Presets:</span>
                  {#each queryPresets as preset}
                    <button
                      class="query-preset-btn"
                      onclick={() => applyPreset(preset)}
                      type="button"
                    >{preset.label}</button>
                  {/each}
                </div>
                <div class="query-editor-body">
                  <textarea
                    class="query-editor-textarea"
                    bind:value={queryEditorText}
                    placeholder={'{"scope":{"type":"focus","node":"$clicked","edges":["calls"],"depth":5},"zoom":"fit"}'}
                    spellcheck="false"
                    aria-label="View query JSON"
                  ></textarea>
                </div>
                {#if queryEditorError}
                  <div class="query-editor-error" role="alert">
                    {queryEditorError}
                  </div>
                {/if}
                <div class="query-editor-actions">
                  <button
                    class="query-editor-clear-btn"
                    onclick={clearManualQuery}
                    type="button"
                  >Clear</button>
                  <button
                    class="query-editor-run-btn"
                    onclick={runManualQuery}
                    disabled={!queryEditorText.trim()}
                    type="button"
                  >Run Query</button>
                </div>
              </div>
            {/if}
            {#if !chatCollapsed}
              <div class="explorer-chat-area">
                <div class="chat-collapse-bar">
                  <button
                    class="chat-collapse-btn"
                    onclick={() => { chatCollapsed = true; }}
                    title="Collapse chat panel"
                    aria-label="Collapse chat panel"
                    type="button"
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><polyline points="15 18 9 12 15 6"/></svg>
                  </button>
                </div>
                <ExplorerChat
                  repoId={selectedRepoId}
                  canvasState={explorerCanvasState}
                  onViewQuery={(q) => { activeViewQuery = q; pushInteraction(`view:${q?.scope?.type ?? 'query'}`); }}
                  onOpenSpec={(path) => openSpecEditor(path)}
                  savedViews={explorerSavedViews}
                  onSavedViewsUpdate={(views) => { explorerSavedViews = views; }}
                  {graphHints}
                  graphNodes={graph?.nodes ?? []}
                  graphEdges={graph?.edges ?? []}
                />
              </div>
            {/if}
            {#if chatCollapsed}
              <button
                class="chat-expand-btn"
                onclick={() => { chatCollapsed = false; }}
                title="Open chat"
                aria-label="Open chat panel"
                type="button"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="18" height="18"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>
                <span class="chat-expand-label">Chat</span>
              </button>
            {/if}
          </div>
        {/if}

        <!-- Architecture Insights moved inside explorer-canvas-area (see above) -->
      </div>
    </div>
  </div>
{/if}

<style>
  /* ── Workspace scope repo list ──────────────────────────────────────── */
  .ws-repo-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-6);
    overflow-y: auto;
    height: 100%;
  }

  .ws-repo-header .page-title {
    margin: 0 0 var(--space-1);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
  }

  .ws-repo-desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-muted);
  }

  .ws-repo-grid {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .ws-repo-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-4) var(--space-5);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    transition: border-color var(--transition-fast), background var(--transition-fast);
    width: 100%;
  }

  .ws-repo-card:hover {
    border-color: var(--color-focus);
    background: var(--color-surface-elevated);
  }

  .ws-repo-card:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .ws-repo-card-left {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .ws-repo-icon {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .ws-repo-info {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .ws-repo-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .ws-repo-description {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ws-repo-explore {
    font-size: var(--text-sm);
    color: var(--color-link);
    flex-shrink: 0;
    font-weight: 500;
  }

  /* ── Repo scope: graph view styles ───────────────────────────────────── */
  .explorer-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .explorer-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .header-left .page-title {
    margin: 0 0 var(--space-1);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
  }

  .subtitle {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .repo-selector-skeleton {
    width: 200px;
  }

  .repo-select-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
  }

  .repo-icon {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .repo-select {
    background: transparent;
    border: none;
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    min-width: 180px;
    max-width: 280px;
  }

  .repo-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .repo-select option {
    background: var(--color-surface);
    color: var(--color-text);
  }

  .graph-stats {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }

  .stat {
    display: flex;
    align-items: baseline;
    gap: 3px;
  }

  .stat-val {
    font-weight: 700;
    font-family: var(--font-mono);
    color: var(--color-text);
  }

  .stat-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .stat-sep {
    color: var(--color-text-muted);
  }

  .timeline-delta-summary {
    display: inline-flex; gap: 6px; font-size: var(--text-xs);
    font-family: var(--font-mono);
  }
  .delta-add { color: #4ade80; font-weight: 600; }
  .delta-remove { color: #f87171; font-weight: 600; }
  .delta-modify { color: #fbbf24; font-weight: 600; }

  /* ── Back to repos button (workspace scope) ───────────────────────── */
  .back-to-repos-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: none;
    color: var(--color-link);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    border-radius: var(--radius-sm);
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .back-to-repos-btn:hover {
    background: color-mix(in srgb, var(--color-link) 10%, transparent);
    color: var(--color-link-hover);
  }

  .back-to-repos-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .explorer-body {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: row;
  }

  .explorer-body-main {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .empty-state-wrap {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
  }

  .hint {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    text-align: center;
    margin: 0;
  }

  .go-admin-btn {
    background: var(--color-link);
    color: var(--color-text-inverse);
    border: none;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }
  .go-admin-btn:hover { background: var(--color-link-hover); }
  .go-admin-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .loading-wrap {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .loading-msg {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    text-align: center;
    margin: 0;
    font-style: italic;
  }

  /* Concept search bar */
  .concept-search-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .search-input-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    min-width: 220px;
  }

  .search-input-wrap:focus-within {
    border-color: var(--color-focus);
    box-shadow: 0 0 0 2px var(--color-focus);
  }

  .search-icon {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .concept-input {
    background: transparent;
    border: none;
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    outline: none;
    width: 100%;
    min-width: 160px;
  }

  .concept-input::placeholder {
    color: var(--color-text-muted);
  }

  .concept-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }

  /* Remove browser default search cancel button */
  .concept-input::-webkit-search-cancel-button { display: none; }

  .search-loading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .spinner {
    display: inline-block;
    width: 12px;
    height: 12px;
    border: 2px solid var(--color-border-strong);
    border-top-color: var(--color-focus);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner { animation: none; }
    .ws-repo-card,
    .go-admin-btn,
    .chip-clear,
    .graph-error button { transition: none; }
  }

  .concept-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2) var(--space-1) var(--space-3);
    background: color-mix(in srgb, var(--color-focus) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-focus) 30%, transparent);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    color: var(--color-focus);
    font-family: var(--font-mono);
  }

  .concept-chip.no-results {
    background: color-mix(in srgb, var(--color-text-muted) 10%, transparent);
    border-color: var(--color-border-strong);
    color: var(--color-text-muted);
  }

  .chip-clear {
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: var(--text-xs);
    line-height: 1;
    padding: 0 var(--space-1);
    opacity: 0.7;
    transition: opacity var(--transition-fast);
  }

  .chip-clear:hover { opacity: 1; }
  .chip-clear:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* ── Workspace repo error ─────────────────────────────────────────────── */
  .error-banner {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-danger);
    font-size: var(--text-sm);
    padding: var(--space-3) var(--space-4);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .retry-btn {
    background: color-mix(in srgb, var(--color-link) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-link) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-link);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) var(--space-3);
    white-space: nowrap;
  }

  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-link) 25%, transparent);
    border-color: var(--color-link);
  }

  .retry-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Graph error state ────────────────────────────────────────────────── */
  .graph-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-6);
    text-align: center;
    flex: 1;
  }

  .graph-error p {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-danger);
  }

  .graph-error button {
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .graph-error button:hover {
    background: var(--color-surface);
    border-color: var(--color-focus);
  }

  .graph-error button:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Filter toggle button ──────────────────────────────────────────── */
  .ctrl-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast);
    flex-shrink: 0;
  }

  .ctrl-btn:hover {
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .ctrl-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .ctrl-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .icon-btn.active {
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  /* ── Architecture insights (deps + risks) ────────────────────────── */
  .arch-insights-overlay {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    z-index: 10;
    pointer-events: none;
  }

  .arch-insights-overlay > * {
    pointer-events: auto;
  }

  .arch-insights-toggle {
    border-top: 1px solid var(--color-border);
    padding: var(--space-2) var(--space-4);
    flex-shrink: 0;
    background: var(--color-surface);
  }

  .arch-insights-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: none;
    border: none;
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    padding: var(--space-1) 0;
    font-family: var(--font-body);
  }

  .arch-insights-btn:hover {
    color: var(--color-text);
  }

  .arch-toggle-icon {
    display: inline-block;
    font-size: 10px;
    transition: transform 0.15s ease;
  }

  .arch-toggle-icon.open {
    transform: rotate(90deg);
  }

  .arch-insights {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4);
    flex-shrink: 0;
    overflow-y: auto;
    max-height: 300px;
    background: var(--color-surface);
    border-top: 1px solid var(--color-border);
    transition: max-height 0.2s ease, padding 0.2s ease, opacity 0.2s ease;
  }

  .arch-insights.collapsed {
    max-height: 0;
    padding-top: 0;
    padding-bottom: 0;
    opacity: 0;
    pointer-events: none;
  }

  .arch-insight-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .arch-insight-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .arch-insight-desc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .arch-dep-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .arch-dep-label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-muted);
  }

  .arch-dep-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .arch-dep-item {
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-secondary);
  }

  .arch-risk-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .arch-risk-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    font-size: var(--text-xs);
  }

  .arch-risk-name {
    font-family: var(--font-mono);
    color: var(--color-text);
    font-weight: 500;
  }

  .arch-risk-score {
    padding: 1px var(--space-2);
    border-radius: var(--radius-full);
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
    font-weight: 600;
    font-size: var(--text-xs);
  }

  .arch-risk-reason {
    color: var(--color-text-muted);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Graph types grid ──────────────────────────────────────────────── */
  .arch-type-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-2);
  }

  .arch-type-card {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
  }

  .arch-type-kind {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .arch-type-name {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .arch-type-doc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    line-height: 1.3;
  }

  .arch-mod-doc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: var(--space-2);
  }

  .arch-more {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: var(--space-2);
  }

  /* ── Architecture timeline ───────────────────────────────────────────── */
  .arch-timeline {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .arch-timeline-entry {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    font-size: var(--text-xs);
  }

  .arch-timeline-time {
    color: var(--color-text-muted);
    flex-shrink: 0;
    min-width: 80px;
  }

  .arch-timeline-label {
    font-weight: 500;
    color: var(--color-text);
  }

  .arch-timeline-stats {
    display: flex;
    gap: var(--space-1);
    font-family: var(--font-mono);
  }

  .diff-ins { color: var(--color-success); }
  .diff-del { color: var(--color-danger); }

  /* Timeline scrubber (time travel) */
  .timeline-scrub-toggle {
    margin-left: var(--space-2);
    padding: 2px 8px;
    font-size: var(--text-xs);
    font-weight: 500;
    border: 1px solid var(--color-border);
    border-radius: 4px;
    background: transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    transition: all 0.15s;
  }
  .timeline-scrub-toggle:hover { background: var(--color-surface-hover); color: var(--color-text); }
  .timeline-scrub-toggle.active {
    background: var(--color-primary-muted);
    color: var(--color-primary);
    border-color: var(--color-primary);
  }
  .timeline-scrubber {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) 0;
  }
  .timeline-slider {
    width: 100%;
    accent-color: var(--color-primary);
    cursor: pointer;
  }
  .timeline-scrub-info {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-wrap: wrap;
  }
  .timeline-scrub-date { font-weight: 600; color: var(--color-text); }
  .timeline-scrub-event { color: var(--color-text-secondary); }
  .timeline-scrub-nodes { font-family: var(--font-mono); color: var(--color-primary); }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  /* ── Explorer split layout (treemap + chat) ──────────────────────── */
  .explorer-split {
    display: flex;
    flex: 1;
    overflow: hidden;
    min-height: 0;
  }

  .explorer-canvas-area {
    flex: 1;
    overflow: hidden;
    min-width: 0;
    position: relative;
  }

  .toolchain-warning {
    position: absolute; top: 8px; left: 50%; transform: translateX(-50%); z-index: 45;
    display: flex; align-items: center; gap: 8px;
    padding: 6px 14px; background: rgba(120, 53, 15, 0.85); border: 1px solid rgba(245, 158, 11, 0.4);
    border-radius: 8px; font-size: 11px; color: #fbbf24; max-width: 600px;
    backdrop-filter: blur(12px);
  }
  .toolchain-warning-dismiss {
    display: flex; align-items: center; justify-content: center;
    width: 20px; height: 20px; background: transparent; border: none;
    border-radius: 4px; color: #fbbf24; cursor: pointer; flex-shrink: 0;
    opacity: 0.6;
  }
  .toolchain-warning-dismiss:hover { opacity: 1; background: rgba(245, 158, 11, 0.2); }

  .explorer-detail-area {
    width: 320px;
    min-width: 280px;
    max-width: 380px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--color-border);
  }

  .explorer-chat-area {
    width: 360px;
    min-width: 280px;
    max-width: 480px;
    border-left: 1px solid var(--color-border);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  /* ── Edit Spec button (inside detail panel) ─────────────────────── */
  .edit-spec-action {
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .edit-spec-btn {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-primary) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .edit-spec-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 20%, transparent);
    border-color: var(--color-primary);
  }

  .edit-spec-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Spec Editor slide-out panel ───────────────────────────────── */
  .spec-editor-panel {
    width: 420px;
    min-width: 320px;
    max-width: 520px;
    border-left: 1px solid var(--color-border);
    background: var(--color-surface);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .spec-editor-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    background: var(--color-surface-elevated);
  }

  .spec-editor-title {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    white-space: nowrap;
  }

  .spec-editor-path {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .spec-editor-close {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: var(--space-1);
    border-radius: var(--radius-sm);
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .spec-editor-close:hover {
    color: var(--color-text);
    background: var(--color-surface);
  }

  .spec-editor-close:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .spec-editor-body {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .spec-editor-loading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-6);
    justify-content: center;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    font-style: italic;
  }

  .spec-editor-error {
    padding: var(--space-4);
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  .spec-editor-with-lines {
    flex: 1;
    display: flex;
    min-height: 0;
    overflow: hidden;
  }

  .spec-line-numbers {
    width: 44px;
    flex-shrink: 0;
    overflow: hidden;
    background: color-mix(in srgb, var(--color-surface) 70%, var(--color-border));
    border-right: 1px solid var(--color-border);
    padding: var(--space-3) 0;
    user-select: none;
  }

  .spec-line-num {
    text-align: right;
    padding: 0 8px 0 4px;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    color: var(--color-text-muted);
    opacity: 0.5;
  }

  .spec-editor-textarea {
    flex: 1;
    width: 100%;
    resize: none;
    border: none;
    outline: none;
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    tab-size: 2;
    min-height: 0;
  }

  .spec-editor-textarea:focus {
    background: color-mix(in srgb, var(--color-surface-elevated) 50%, var(--color-surface));
  }

  .spec-editor-footer {
    border-top: 1px solid var(--color-border);
    padding: var(--space-3) var(--space-4);
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    background: var(--color-surface-elevated);
  }

  .instant-impact {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: var(--space-2) var(--space-4);
    display: flex;
    gap: 6px;
    align-items: center;
    background: color-mix(in srgb, #60a5fa 6%, var(--color-surface-elevated));
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }
  .instant-label { font-weight: 600; color: #60a5fa; }
  .instant-count { color: var(--color-text); }
  .instant-sep { color: var(--color-text-muted); }
  .instant-types { color: var(--color-text-muted); font-family: 'SF Mono', Menlo, monospace; font-size: 11px; }
  .instant-new-assertion { color: #f59e0b; font-weight: 600; }
  .instant-blast { color: #ef4444; font-weight: 600; }

  .spec-editor-predict-error {
    font-size: var(--text-xs);
    color: var(--color-danger);
    padding: var(--space-1) 0;
  }

  .spec-editor-predict-result {
    font-size: var(--text-xs);
    color: var(--color-success);
    font-weight: 500;
    padding: var(--space-1) 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .predict-summary {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .predict-confidence {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 1px var(--space-2);
    border-radius: var(--radius-full);
  }

  .predict-confidence.high {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
  }

  .predict-confidence.medium {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
  }

  .predict-confidence.low {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
  }

  .predict-cost {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .predict-details {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 120px;
    overflow-y: auto;
  }

  .predict-detail-item {
    display: flex;
    align-items: baseline;
    gap: var(--space-1);
    font-size: var(--text-xs);
    line-height: 1.4;
  }

  .predict-detail-action {
    font-weight: 700;
    font-family: var(--font-mono);
    flex-shrink: 0;
    width: 14px;
    text-align: center;
  }

  .predict-detail-action.add { color: var(--color-success); }
  .predict-detail-action.change { color: var(--color-warning); }
  .predict-detail-action.remove { color: var(--color-danger); }

  .predict-detail-name {
    font-family: var(--font-mono);
    font-weight: 500;
    color: var(--color-text);
    flex-shrink: 0;
  }

  .predict-detail-conf {
    font-size: 9px;
    color: var(--color-text-muted);
    padding: 0 3px;
    border: 1px solid var(--color-border);
    border-radius: 3px;
    flex-shrink: 0;
  }

  .predict-detail-reason {
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .predict-affected-specs {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: wrap;
    padding-top: var(--space-1);
    border-top: 1px solid var(--color-border);
  }

  .predict-affected-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .predict-affected-spec-btn {
    display: inline-flex;
    align-items: center;
    padding: 1px var(--space-2);
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 25%, transparent);
    border-radius: var(--radius-sm);
    color: var(--color-warning);
    font-size: 10px;
    font-family: var(--font-mono);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast);
    white-space: nowrap;
  }

  .predict-affected-spec-btn:hover {
    background: color-mix(in srgb, var(--color-warning) 20%, transparent);
    border-color: var(--color-warning);
  }

  .predict-affected-spec-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Spec Assertions ─────────────────────────────────────── */
  .spec-assertions-panel {
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }
  .spec-assertions-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-1) 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .assertions-loading {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 400;
    font-style: italic;
  }
  .assertions-summary {
    font-size: var(--text-xs);
    font-weight: 400;
  }
  .assert-pass-all { color: var(--color-success); }
  .assert-fail-count { color: var(--color-danger); font-weight: 600; }
  .spec-assertions-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .spec-assertion-item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    font-size: var(--text-xs);
    padding: 3px 6px;
    border-radius: 3px;
  }
  .spec-assertion-item.passed { background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .spec-assertion-item.failed { background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .assert-icon { font-size: 12px; flex-shrink: 0; }
  .spec-assertion-item.passed .assert-icon { color: var(--color-success); }
  .spec-assertion-item.failed .assert-icon { color: var(--color-danger); }
  .assert-text {
    font-family: var(--font-mono);
    color: var(--color-text);
    flex: 1;
  }
  .assert-explain {
    color: var(--color-text-muted);
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spec-editor-actions {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
  }

  .spec-editor-cancel-btn {
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .spec-editor-cancel-btn:hover {
    background: var(--color-surface);
    border-color: var(--color-text-muted);
  }

  .spec-editor-cancel-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .spec-editor-preview-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast), opacity var(--transition-fast);
  }

  .spec-editor-preview-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-primary) 85%, black);
  }

  .spec-editor-preview-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .spec-editor-preview-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .spec-editor-thorough-btn {
    display: flex; align-items: center; gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: #7c3aed; border: 1px solid #7c3aed;
    border-radius: var(--radius); color: #fff;
    font-family: var(--font-body); font-size: var(--text-sm); font-weight: 500;
    cursor: pointer; transition: background var(--transition-fast), opacity var(--transition-fast);
  }
  .spec-editor-thorough-btn:hover:not(:disabled) { background: #6d28d9; }
  .spec-editor-thorough-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .spec-editor-publish-btn {
    padding: 6px 16px;
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    border: none;
    background: #22c55e;
    color: #fff;
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .spec-editor-publish-btn:hover:not(:disabled) {
    background: #16a34a;
  }
  .spec-editor-publish-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .spec-editor-publish-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Manual View Query Editor panel ─────────────────────────────── */
  .query-editor-panel {
    width: 340px;
    min-width: 280px;
    max-width: 420px;
    border-left: 1px solid var(--color-border);
    background: var(--color-surface);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .query-editor-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    background: var(--color-surface-elevated);
  }

  .query-editor-title {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .query-editor-close {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: var(--space-1);
    border-radius: var(--radius-sm);
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .query-editor-close:hover {
    color: var(--color-text);
    background: var(--color-surface);
  }

  .query-editor-close:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .query-editor-presets {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .query-editor-presets-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
    white-space: nowrap;
  }

  .query-preset-btn {
    padding: 2px var(--space-2);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 25%, transparent);
    border-radius: var(--radius-sm);
    color: var(--color-primary);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
    white-space: nowrap;
  }

  .query-preset-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 20%, transparent);
    border-color: var(--color-primary);
  }

  .query-preset-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .query-editor-body {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .query-editor-textarea {
    flex: 1;
    width: 100%;
    resize: none;
    border: none;
    outline: none;
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    tab-size: 2;
    min-height: 120px;
  }

  .query-editor-textarea::placeholder {
    color: var(--color-text-muted);
    font-style: italic;
  }

  .query-editor-textarea:focus {
    background: color-mix(in srgb, var(--color-surface-elevated) 50%, var(--color-surface));
  }

  .query-editor-error {
    padding: var(--space-2) var(--space-4);
    font-size: var(--text-xs);
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
    border-top: 1px solid color-mix(in srgb, var(--color-danger) 20%, transparent);
    flex-shrink: 0;
  }

  .query-editor-actions {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
    background: var(--color-surface-elevated);
  }

  .query-editor-clear-btn {
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .query-editor-clear-btn:hover {
    background: var(--color-surface);
    border-color: var(--color-text-muted);
  }

  .query-editor-clear-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .query-editor-run-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast), opacity var(--transition-fast);
  }

  .query-editor-run-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-primary) 85%, black);
  }

  .query-editor-run-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .query-editor-run-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Publish confirmation dialog ──────────────────────────────────── */
  .publish-confirm-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 20;
  }

  .publish-confirm-dialog {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-5);
    max-width: 360px;
    width: 90%;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
  }

  .publish-confirm-title {
    margin: 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
  }

  .publish-confirm-desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
  }

  .publish-confirm-impact {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: var(--space-2);
    background: color-mix(in srgb, #60a5fa 8%, transparent);
    border-radius: var(--radius-sm);
    border: 1px solid color-mix(in srgb, #60a5fa 20%, transparent);
  }

  .publish-impact-label {
    font-weight: 600;
    color: #60a5fa;
  }

  .publish-confirm-actions {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
  }

  .publish-confirm-cancel {
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .publish-confirm-cancel:hover {
    background: var(--color-surface-elevated);
  }

  .publish-confirm-ok {
    padding: var(--space-2) var(--space-4);
    background: #22c55e;
    border: none;
    border-radius: var(--radius);
    color: #fff;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
  }

  .publish-confirm-ok:hover {
    background: #16a34a;
  }

  .publish-confirm-ok:focus-visible,
  .publish-confirm-cancel:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Chat panel collapse/expand controls ─────────────────────────── */
  .chat-collapse-bar {
    display: none;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  .chat-collapse-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast);
  }
  .chat-collapse-btn:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }
  .chat-collapse-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .chat-expand-btn {
    position: fixed;
    bottom: 20px;
    right: 20px;
    display: none;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    background: var(--color-primary);
    color: var(--color-text-inverse);
    border: none;
    border-radius: 24px;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 600;
    cursor: pointer;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    z-index: 100;
    transition: background var(--transition-fast), transform var(--transition-fast);
  }
  .chat-expand-btn:hover {
    background: var(--color-link-hover);
    transform: scale(1.05);
  }
  .chat-expand-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }
  .chat-expand-label {
    display: inline;
  }

  /* ── Medium viewports (768px–1024px): collapsible chat ────────────── */
  @media (max-width: 1024px) and (min-width: 769px) {
    .chat-collapse-bar {
      display: flex;
    }
    .chat-expand-btn {
      display: flex;
    }
    .explorer-chat-area {
      width: 300px;
      min-width: 240px;
      max-width: 360px;
    }
  }

  /* ── Narrow viewports (<768px): stacked layout with chat overlay ─── */
  @media (max-width: 768px) {
    .explorer-header {
      padding: var(--space-3) var(--space-4);
    }
    .header-right {
      flex-wrap: wrap;
      gap: var(--space-2);
    }
    .explorer-split {
      flex-direction: column;
    }
    .explorer-canvas-area {
      flex: 1;
      min-height: 200px;
    }
    .explorer-chat-area {
      position: fixed;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
      width: 100%;
      max-width: 100%;
      min-width: 0;
      border-left: none;
      z-index: 200;
      background: var(--color-bg);
    }
    .chat-collapse-bar {
      display: flex;
      border-bottom: 1px solid var(--color-border);
      padding: var(--space-2) var(--space-3);
      justify-content: flex-start;
    }
    .chat-expand-btn {
      display: flex;
    }
    .explorer-detail-area {
      width: 100%;
      max-width: 100%;
      min-width: 0;
      border-left: none;
      border-top: 1px solid var(--color-border);
      max-height: 40%;
    }
    .spec-editor-panel {
      width: 100%;
      max-width: 100%;
      min-width: 0;
      border-left: none;
      border-top: 1px solid var(--color-border);
      max-height: 50%;
    }
    .query-editor-panel {
      width: 100%;
      max-width: 100%;
      min-width: 0;
      border-left: none;
      border-top: 1px solid var(--color-border);
      max-height: 40%;
    }
  }

  /* ── Wider-than-1024px: hide collapse controls, always show chat ── */
  @media (min-width: 1025px) {
    .chat-collapse-bar {
      display: none;
    }
    .chat-expand-btn {
      display: none !important;
    }
  }
</style>
