<script>
  import { getContext, onDestroy, untrack } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { entityName } from '../lib/entityNames.svelte.js';
  import { computeTimelineDeltaStats, computeTimelineGhostOverlays } from '../lib/timeline-utils.js';
  import { lensFromUrl, lensToUrl } from '../lib/lens-rendering.js';
  import { RECENT_CHANGE_WINDOW_MS } from '../lib/liveness.svelte.js';
  import { traceStepTargetNode } from '../lib/trace-navigation.js';
  import { onViewEvent, dispatchViewEvent, viewEventFromDom } from '../lib/viewEvents.js';
  import { applyDensityPolicy } from '../lib/canvas-density.js';
  // ui-layout.md §10 Layout Engines: the layout registry (task-170) maps
  // ViewSpec layout names to renderer components; the generated view's
  // `layout` field selects the renderer (see applyGeneratedViewSpec).
  import { getLayout } from '../lib/layoutRegistry.js';
  import { resolveScopeNodes, edgeSrc, edgeTgt } from '../lib/view-query-scope.js';
  import VirtualNodeList from '../lib/VirtualNodeList.svelte';
  import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
  import NodeDetailPanel from '../lib/NodeDetailPanel.svelte';
  import ExplorerChat from '../lib/ExplorerChat.svelte';
  // HSI §3 Generated Views (task-089): LLM-powered on-demand views via the
  // workspace generate endpoint (read-only query translator).
  import GeneratedViewChat from '../lib/GeneratedViewChat.svelte';
  import { viewSpecToViewQuery } from '../lib/generated-view.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import Briefing from './Briefing.svelte';
  import WorkspaceCards from './WorkspaceCards.svelte';
  import DependencyGraph from './DependencyGraph.svelte';
  // HSI §3 Default Views (task-088): Spec Realization, Change, System Trace.
  // Boundary is the default explorer view (canvas drill-down), so it has no
  // dedicated component — the ExplorerCanvas IS the Boundary View.
  import SpecRealizationView from './explorer/SpecRealizationView.svelte';
  import ChangeView from './explorer/ChangeView.svelte';
  import SystemTraceView from './explorer/SystemTraceView.svelte';
  // In-view filter panel (ui-navigation.md §3, task-178): category filters +
  // discovered concepts, toggled by the filter icon in the control bar.
  import ExplorerFilterPanel from './ExplorerFilterPanel.svelte';
  import { hiddenNodeIds as computeHiddenNodeIds, conceptNodeIds as computeConceptNodeIds, CATEGORY_IDS } from '../lib/filter-categories.js';

  const navigate = getContext('navigate');
  const goToWorkspaceSettings = getContext('goToWorkspaceSettings');
  const goToWorkspaceHomeCtx = getContext('goToWorkspaceHome');
  // Repo scope change from the workspace graph (ui-layout.md §5: double-click
  // a repo node → scope changes to repo). Wired by App.svelte; absent in
  // standalone/test renders where the in-place drill fallback applies.
  const goToRepoCtx = getContext('goToRepo');

  // scope: { type: 'tenant' | 'workspace' | 'repo', workspaceId?, repoId? }
  // Defaults to tenant scope for backwards compatibility with old App.svelte.
  let {
    scope = { type: 'tenant' },
    onSelectWorkspace = null,
    workspaceName = null,
    embedded = false,
    onEnterRepo = null,
    archSubTab = undefined,
    onArchSubTabChange = undefined,
  } = $props();

  let scopeType = $derived(scope?.type ?? 'tenant');

  // ── Architecture sub-tabs (ui-navigation.md §2, repo scope) ─────────────
  // Control-bar sub-tabs at repo scope: Graph (default) | Briefing (the
  // repo-scoped narrative via ?repo_id= — HSI §1.5 repo-scope Briefing row).
  // Controlled from App when the `archSubTab` prop is provided (drawer
  // Briefing clicks must switch the sub-tab even when the Architecture tab
  // is already active); `localArchSubTab` is the standalone fallback.
  let localArchSubTab = $state('graph');
  let activeArchSubTab = $derived(archSubTab !== undefined ? archSubTab : localArchSubTab);

  function setArchSubTab(sub) {
    localArchSubTab = sub;
    onArchSubTabChange?.(sub);
  }

  // ── Workspace-scope repo list ──────────────────────────────────────────
  let wsRepos = $state([]);
  let wsReposLoading = $state(true);
  let wsReposError = $state(null);
  // HSI §3 Boundary View Level 1: per-repo type/endpoint counts from the
  // workspace knowledge graph, rendered as a sub-label on each repo node.
  let wsRepoStats = $state({});   // repoId → { types, endpoints }

  // ── Workspace-scope cross-repo dependency graph (ui-layout.md §5) ──────
  // C4 "Container" level: repos as nodes, cross-repo dependencies as edges.
  let wsDepEdges = $state([]);
  let wsDepExternal = $state([]); // repos referenced by edges but not in this workspace
  let wsDepLoading = $state(true);
  let wsDepError = $state(null);
  // Repo metadata detail panel (single-click on a repo node)
  let repoDetail = $state(null);

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

  // ── HSI §3 Default Views: view selector ───────────────────────────────
  // Boundary (default) | Spec Realization | Change | System Trace.
  // Boundary is the ExplorerCanvas C4 drill-down (workspace repo cards →
  // packages → modules → code items); the other three are dedicated panels.
  const EXPLORER_VIEWS = [
    { id: 'boundary', label: 'Boundary' },
    { id: 'spec-realization', label: 'Spec Realization' },
    { id: 'change', label: 'Change' },
    { id: 'system-trace', label: 'System Trace' },
  ];
  let explorerViewMode = $state('boundary');

  // Graph data quality warnings (e.g., missing LSP toolchains)
  let graphWarnings = $state([]);
  let graphWarningsDismissed = $state(false);

  // New explorer state
  let explorerCanvasState = $state({ selectedNode: null, zoom: 1, visibleGroups: [], breadcrumb: [], recent_interactions: [] });
  let activeViewQuery = $state(null);
  let explorerFilter = $state('all');
  // Lens selection persists in the URL query param (?lens=evaluative) so a
  // view with an active lens is deep-linkable (HSI §3 Three Lenses).
  let explorerLens = $state(initialLensFromUrl());
  // Evaluative lens data (HSI §3): gate outcomes per spec path (from the most
  // recent gate results of MRs carrying that spec_ref) and the spec approval
  // ledger. Both are best-effort overlays — the structural base renders fine
  // without them.
  let gateStatusBySpec = $state({});
  let specApprovals = $state([]);
  let explorerSavedViews = $state([]);

  // In-view filter panel (ui-navigation.md §3, task-178): filter icon in
  // the control bar opens the collapsible 200px left panel inside the
  // Architecture content area (not the app sidebar). Category checkboxes
  // toggle node visibility; concept selection fades the canvas to the
  // concept's elements (task-182, system-explorer.md §4).
  let filterPanelOpen = $state(false);
  let activeFilterCategories = $state({ boundaries: true, interfaces: true, data: true, specs: true });
  let activeFilterConcepts = $state([]);

  // ── Concept views (system-explorer.md §4, task-182) ─────────────
  // Predefined concepts from the repo's spec manifest (served by
  // GET /repos/:id/graph/concepts); selecting one fetches its projection
  // (GET /repos/:id/graph/concept/:name) and fades the canvas to the
  // concept's nodes. Discovered concepts (activeFilterConcepts above)
  // use the substring mechanism — both are supported per the spec's data
  // source distinction.
  let manifestConcepts = $state([]);
  let manifestConceptsLoadedFor = $state('');
  let activeManifestConcept = $state(null); // { name, nodeIds: Set|null }

  // Fetch the manifest concept listing when the filter panel opens at
  // repo scope (once per repo — the panel is the only consumer).
  $effect(() => {
    if (!filterPanelOpen || !selectedRepoId || explorerViewMode !== 'boundary') return;
    if (manifestConceptsLoadedFor === selectedRepoId) return;
    const repoId = selectedRepoId;
    manifestConceptsLoadedFor = repoId;
    api.repoGraphConcepts(repoId)
      .then((res) => {
        if (selectedRepoId !== repoId) return; // stale
        manifestConcepts = Array.isArray(res?.concepts) ? res.concepts : [];
      })
      .catch(() => {
        // No manifest / endpoint unavailable — no predefined concepts to
        // list; allow a retry on the next panel open.
        if (selectedRepoId === repoId) manifestConceptsLoadedFor = '';
      });
  });

  /** Toggle a manifest concept view: fetching the projection resolves the
   *  concept's node set (manifest-based — distinct from the substring
   *  mechanism); toggling the active concept clears it. */
  async function toggleManifestConcept(name) {
    if (activeManifestConcept?.name === name) {
      activeManifestConcept = null;
      return;
    }
    activeManifestConcept = { name, nodeIds: null }; // projection loading
    pushInteraction({ action: 'concept', detail: `manifest:${name}` });
    try {
      const result = await api.getGraphConcept(selectedRepoId, name);
      if (activeManifestConcept?.name !== name) return; // stale toggle
      const ids = new Set((result?.nodes ?? []).map((n) => n?.id).filter(Boolean));
      activeManifestConcept = { name, nodeIds: ids };
    } catch (e) {
      if (activeManifestConcept?.name === name) activeManifestConcept = null;
      showToast(`Concept "${name}" could not be loaded: ${e.message}`, { type: 'error' });
    }
  }

  // ── Ad-hoc concept creation (system-explorer.md §4) ─────────────
  // Multi-select on the canvas (Shift+Click) + "Create concept" (context
  // menu or the selection bar) opens a naming dialog; saving persists the
  // grouping as an explorer view (POST /workspaces/:id/explorer-views)
  // whose ViewQuery concept scope seeds exactly the selected nodes.
  let conceptDialog = $state(null); // { name, error, saving, nodes: [...] }

  function openConceptDialog(nodeIds) {
    const ids = Array.isArray(nodeIds) ? nodeIds : [...(nodeIds ?? [])];
    const nodes = (effectiveGraph?.nodes ?? []).filter((n) => ids.includes(n.id));
    if (nodes.length < 2) return;
    conceptDialog = { name: '', error: '', saving: false, nodes };
  }

  function closeConceptDialog() {
    conceptDialog = null;
  }

  async function saveConceptDialog() {
    if (!conceptDialog) return;
    const name = (conceptDialog.name ?? '').trim();
    if (!name) {
      conceptDialog = { ...conceptDialog, error: 'Name the concept before saving.' };
      return;
    }
    const workspaceId = scope?.workspaceId;
    if (!workspaceId) {
      conceptDialog = { ...conceptDialog, error: 'No workspace scope — cannot save a view here.' };
      return;
    }
    // ViewQuery grammar (view-query-grammar.md): a concept scope seeded
    // with the selected nodes' qualified names. No edge expansion — the
    // ad-hoc concept is exactly the grouped nodes.
    const seedNodes = conceptDialog.nodes.map((n) => n.qualified_name ?? n.name);
    const spec = {
      scope: {
        type: 'concept',
        seed_nodes: seedNodes,
        expand_edges: [],
        expand_depth: 0,
        expand_direction: 'both',
      },
      emphasis: { highlight: { matched: { color: '#a78bfa' } }, dim_unmatched: 0.15 },
      zoom: 'fit',
      annotation: {
        title: `Concept: ${name}`,
        description: `${seedNodes.length} grouped nodes`,
      },
    };
    conceptDialog = { ...conceptDialog, saving: true, error: '' };
    try {
      await api.saveExplorerView(workspaceId, {
        name,
        description: `Ad-hoc concept grouping ${seedNodes.length} nodes`,
        spec,
      });
      pushInteraction({ action: 'save_concept', detail: name });
      showToast(`Concept "${name}" saved as an explorer view`, { type: 'success' });
      conceptDialog = null;
    } catch (e) {
      conceptDialog = { ...conceptDialog, saving: false, error: e.message };
    }
  }

  // ── View spec editor (ui-navigation.md §3, task-178) ────────────────
  // "Edit view spec" opens a collapsible JSON editor for the view spec
  // grammar (data/layout/encoding/highlight); Apply translates via
  // viewSpecToViewQuery and re-renders the canvas.
  let viewSpecEditorOpen = $state(false);
  let viewSpecText = $state('');
  let viewSpecError = $state('');
  let activeViewSpec = $state(null); // generated/edited ViewSpec currently applied
  let detailNode = $state(null);
  let highlightedSpanId = $state(null);
  // Causal trace step focus: clicking a step in the detail panel pans/zooms
  // the canvas to that node (explorer-canvas.md §6 step 6). The nonce lets
  // repeated clicks on the same step re-trigger the camera move.
  let traceFocusRequest = $state(null);
  let traceFocusNonce = 0;

  // ── HSI §3 Generated Views (task-089) ──────────────────────────────────
  // Chat column mode: 'ask' is the repo-scoped Explorer agent (WebSocket);
  // 'generate' is the LLM view generator (workspace generate endpoint) that
  // translates natural-language questions into ViewSpec documents.
  let chatMode = $state('ask');
  // Trace playback request for generated "flow" views: the nonce lets a new
  // flow view re-trigger the canvas particle animation.
  let tracePlaybackRequest = $state(null);
  let tracePlaybackNonce = 0;

  /**
   * Apply a generated ViewSpec (HSI §3) to the Explorer.
   *
   * The ViewSpec's `layout` field selects the renderer through the layout
   * registry (ui-layout.md §10 Layout Engines): 'hierarchical'/'layered'
   * render on the SVG canvas positioned by ELK, 'side-by-side' on the
   * CSS-Grid composite, 'diff' on the structural-diff component, 'list' on
   * the shell's virtualized node list; 'graph'/'timeline' stay on the
   * unified ExplorerCanvas (Canvas 2D + TimelineScrubber), and 'flow'
   * stays on ExplorerCanvas too, which delegates its rendering to
   * FlowCanvas (SVG nodes/edges + Canvas/WebGL particle overlay, §4).
   *
   * For the canvas-rendered layouts the ViewSpec is translated to a
   * ViewQuery the canvas renders (concept / node-type filters, callouts,
   * and the LLM's explanation as the query annotation — the sidebar
   * annotation alongside the graph). A "flow" layout additionally fetches
   * the trace_source MR's captured trace and starts particle playback on
   * the evaluative lens; the server already guarantees flow views are
   * trace-backed, so a fetch failure here falls back to the structural
   * lens (client mirror of the server fallback).
   */
  function applyGeneratedViewSpec(spec) {
    if (!spec || typeof spec !== 'object') return;
    activeViewSpec = spec; // view spec editor prefills the active spec
    const layoutName = typeof spec.layout === 'string' ? spec.layout.trim() : '';
    activeLayout = getLayout(layoutName) ? layoutName : 'graph';
    const query = viewSpecToViewQuery(spec);
    if (query) {
      activeViewQuery = query;
      pushInteraction({ action: 'query', detail: `generated:${spec.layout ?? 'view'}` });
    }
    if (spec.layout === 'flow' && spec.data?.trace_source?.mr_id) {
      const mrId = spec.data.trace_source.mr_id;
      api.mrTrace(mrId)
        .then((trace) => {
          if (trace?.spans?.length > 0) {
            traceData = trace;
            tracePlaybackRequest = { nonce: ++tracePlaybackNonce };
          } else {
            explorerLens = 'structural';
          }
        })
        .catch((err) => {
          console.warn('[ExplorerView] flow view trace fetch failed:', err);
          explorerLens = 'structural';
        });
    } else {
      explorerLens = 'structural';
    }
  }

  // ── Active generated-view layout (ui-layout.md §10 Layout Engines) ─────
  // The generated ViewSpec's `layout` field (set in applyGeneratedViewSpec)
  // selects the renderer through the layout registry. Layouts rendered by
  // the unified canvas (graph/flow/timeline) and the shell-owned list
  // layout have no dedicated registry component — the boundary branch
  // below renders them; the rest dispatch to their registered renderer.
  const CANVAS_LAYOUTS = new Set(['graph', 'flow', 'timeline']);
  let activeLayout = $state('graph');

  /** Dismiss the generated layout — return to the default graph canvas.
   *  The active view query is kept (it stays visible via the canvas's own
   *  annotation UI and can be cleared from the manual query editor). */
  function clearGeneratedLayout() {
    activeLayout = 'graph';
  }

  // ── Recent interactions (fed to LLM via canvas_state) ──────────────────
  // ExplorerCanvas tracks canvas-level interactions (click, drill) in
  // canvasState.recent_interactions.  ExplorerView adds higher-level
  // interactions (search, load_view) so the LLM has full context.
  function pushInteraction(record) {
    const existing = explorerCanvasState?.recent_interactions ?? [];
    const updated = [...existing, record].slice(-10);
    explorerCanvasState = { ...explorerCanvasState, recent_interactions: updated };
  }

  // ── Uniform ViewEvent handling (ui-layout.md §10 Interaction Events) ──────────
  // The Explorer shell handles all ViewEvents uniformly:
  //   click        → open the detail panel
  //   dblclick     → drill down (scope into the entity's repo)
  //   hover        → show a tooltip
  //   context-menu → open an actions menu
  // Events marked `handled` were already serviced by an integrated emitter
  // (ExplorerCanvas routes its own click/drill/tooltip/menu interactions
  // through its direct callbacks) — they are observed here for interaction
  // tracking only, never re-performed.
  let viewTooltip = $state(null); // { x, y, name, type }
  let viewMenu = $state(null);    // { x, y, entity, data }

  /** Open the node detail panel (shared by canvas callback + ViewEvents). */
  function openNodeDetail(n) {
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
    }
    // 'view_code' and plain nodes render in the detail panel (location shown
    // by NodeDetailPanel) — setting detailNode above covers them.
  }

  function handleViewEvent(ev) {
    if (!ev) return;
    pushInteraction(`viewevent:${ev.type}:${ev.entity_type}:${ev.entity_id}`);
    if (ev.handled) return;
    const data = ev.data;
    switch (ev.type) {
      case 'click':
        viewMenu = null;
        if (data) openNodeDetail(data);
        break;
      case 'dblclick':
        // Drill-down: scope the Explorer into the entity's repo when it
        // lives elsewhere; otherwise open its detail panel.
        if (data?.repo_id && data.repo_id !== selectedRepoId) {
          selectRepo({ id: data.repo_id });
        } else if (data) {
          openNodeDetail(data);
        }
        break;
      case 'hover':
        viewTooltip = data
          ? {
              x: ev.position?.x ?? 0,
              y: ev.position?.y ?? 0,
              name: data.name ?? data.qualified_name ?? data.id,
              type: data.node_type ?? ev.entity_type,
            }
          : null;
        break;
      case 'context-menu':
        viewTooltip = null;
        viewMenu = {
          x: ev.position?.x ?? 0,
          y: ev.position?.y ?? 0,
          entity: ev.entity_type,
          data,
        };
        break;
    }
  }

  $effect(() => onViewEvent(handleViewEvent));

  function onWindowClick(e) {
    // Clicking anywhere dismisses the hover tooltip; clicking outside the
    // ViewEvent actions menu dismisses it.
    if (viewTooltip) viewTooltip = null;
    if (viewMenu && !e.target.closest?.('.viewevent-menu')) viewMenu = null;
    // Clicking outside the view selector dropdown dismisses it.
    if (viewSelectOpen && !e.target.closest?.('.explorer-view-select')) viewSelectOpen = false;
  }

  function viewMenuAction(action) {
    const data = viewMenu?.data;
    viewMenu = null;
    if (!data) return;
    if (action === 'details') {
      openNodeDetail(data);
    } else if (action === 'code' && data.file_path) {
      openNodeDetail({ ...data, _action: 'view_code' });
    } else if (action === 'spec' && data.spec_path) {
      openNodeDetail({ ...data, _action: 'view_spec' });
    } else if (action === 'copy_id') {
      navigator.clipboard?.writeText?.(String(data.id ?? ''));
    }
  }

  /** Cards (Architecture Insights entries) emit ViewEvents too — they are
   *  interactive elements under ui-layout.md §10 ("nodes, edges, rows, cards"). */
  function emitCardViewEvent(domEvent, node) {
    dispatchViewEvent(
      domEvent.currentTarget,
      viewEventFromDom(domEvent, 'node', node?.id ?? node?.qualified_name ?? node?.name ?? 'unknown', node)
    );
  }

  function cardClick(domEvent, node) {
    emitCardViewEvent(domEvent, node);
  }

  function cardKeydown(e, node) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      emitCardViewEvent(e, node);
    }
  }
  // §10 Interaction Context: the active view query is part of the canvas
  // state sent with each message, so the LLM knows what view is currently
  // applied ("what about this one?" can build on the active query).
  $effect(() => {
    const q = activeViewQuery;
    const cs = untrack(() => explorerCanvasState);
    explorerCanvasState = { ...cs, active_query: q };
  });

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
  // HSI §3 Layer 1: MR gate evaluation records for the current repo —
  // [{ mrId, spec_ref, gates }] — drive the Evaluative-lens green/red edges.
  let gateRecords = $state([]);
  // HSI §3 Layer 1: recent ArchitecturalDelta records (last hour only) for
  // the fading highlight rings. Loaded on canvas load so the rings do not
  // depend on the collapsed-by-default Architecture-Insights panel; the
  // insights panel's full timeline (a superset) takes precedence once live.
  let livenessTimeline = $state(null);
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
          .catch(() => {
            // Live check unavailable — fall back to the results persisted by
            // the last push's knowledge-graph check (§9).
            api.getSpecAssertionResults(selectedRepoId, specPath)
              .then(result => { specAssertionResults = result?.assertions ?? []; })
              .catch(() => { specAssertionResults = []; });
          })
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
      // §10: user-composed JSON queries are query interactions
      pushInteraction({ action: 'query', detail: parsed.scope.type });
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
      repoDetail = null;
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

  // Read initial Architecture sub-tab from URL params (set by goToRepoTab
  // context — same convention as the Code tab's subTab param).
  // The param is intentionally NOT deleted after reading: App's
  // setRepoArchSubTab keeps the ?subTab= URL param mirroring the rendered
  // sub-tab (docs/ui.md "deep-linkable via ?subTab=briefing"), so
  // reload/copy of the current URL must keep opening the same sub-tab.
  $effect(() => {
    if (scopeType !== 'repo') return;
    const params = new URLSearchParams(window.location.search);
    const initialSubTab = params.get('subTab');
    if (initialSubTab === 'briefing' || initialSubTab === 'graph') {
      setArchSubTab(initialSubTab);
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
    loadWsDepGraph();
    loadWsBoundaryStats();
  }

  /** Load cross-repo dependency edges for the workspace graph canvas
   *  (ui-layout.md §5 Workspace Scope — Realized Architecture). */
  async function loadWsDepGraph() {
    if (!scope.workspaceId) {
      wsDepLoading = false;
      return;
    }
    wsDepLoading = true;
    wsDepError = null;
    try {
      const data = await api.workspaceDependencyGraph(scope.workspaceId);
      wsDepEdges = data?.edges ?? [];
      wsDepExternal = data?.nodes ?? [];
    } catch (e) {
      // The dependency graph is additive context — repos-as-nodes still render
      // from wsRepos; surface the failure without blocking the canvas.
      wsDepError = e.message ?? 'Failed to load cross-repo dependencies';
      wsDepEdges = [];
      wsDepExternal = [];
    } finally {
      wsDepLoading = false;
    }
  }

  // Workspace graph nodes: every repo in the workspace (C4 containers), plus
  // external repos referenced by dependency edges so cross-workspace edges
  // have visible endpoints. Isolated repos (no edges) still appear as nodes.
  // Each node carries its HSI §3 Level 1 type/endpoint counts (when loaded)
  // for the counts sub-label inside the node box.
  let wsGraphNodes = $derived.by(() => {
    const nodes = wsRepos.map((r) => ({
      repo_id: r.id,
      name: r.name,
      repo: r,
      types: wsRepoStats[r.id]?.types ?? null,
      endpoints: wsRepoStats[r.id]?.endpoints ?? null,
    }));
    const seen = new Set(wsRepos.map((r) => r.id));
    for (const n of wsDepExternal) {
      if (n?.repo_id && !seen.has(n.repo_id)) {
        nodes.push({ repo_id: n.repo_id, name: n.name ?? n.repo_id, external: true, types: null, endpoints: null });
        seen.add(n.repo_id);
      }
    }
    return nodes;
  });

  let wsGraphEdges = $derived(wsDepEdges);

  /** Single-click a repo node → detail panel with repo metadata (§5). */
  function showRepoDetail(node) {
    const repo = wsRepos.find((r) => r.id === node.repo_id);
    repoDetail = repo
      ? { ...repo, external: false }
      : { id: node.repo_id, name: node.name, external: true };
  }

  /** Double-click a repo node → drill to crate level: scope changes to repo
   *  (URL update + breadcrumb change in the host shell). Falls back to the
   *  in-place crate-level graph when no host navigation is available. */
  function enterRepo(nodeOrRepo) {
    const repo = nodeOrRepo.repo ?? wsRepos.find((r) => r.id === nodeOrRepo.repo_id) ?? nodeOrRepo;
    if (!repo?.id && !repo?.repo_id) return;
    repoDetail = null;
    if (onEnterRepo) {
      onEnterRepo(repo.id ? repo : { id: repo.repo_id, name: repo.name });
      return;
    }
    if (goToRepoCtx) {
      goToRepoCtx(repo.id ? repo : { id: repo.repo_id, name: repo.name }, 'architecture');
      return;
    }
    // No host shell — drill in place (canvas switches to the repo's crate-level graph)
    selectRepo(repo.id ? repo : { id: repo.repo_id, name: repo.name, description: repo.description });
  }

  /** Empty-state action: go where repos can be created (workspace home). */
  function goToCreateRepo() {
    goToWorkspaceHomeCtx?.();
  }

  /** HSI §3 Boundary View Level 1 — enrich the workspace graph nodes with
   *  type/endpoint counts (from the aggregated workspace knowledge graph).
   *  Best-effort: failures leave the nodes without counts rather than
   *  breaking the canvas (the counts render as a muted sub-label on each
   *  repo node, per the HSI §3 Level 1 node boxes). */
  async function loadWsBoundaryStats() {
    if (!scope.workspaceId) return;
    try {
      const g = await api.workspaceGraph(scope.workspaceId);
      const stats = {};
      for (const n of g?.nodes ?? []) {
        const rid = n.repo_id;
        if (!rid) continue;
        if (!stats[rid]) stats[rid] = { types: 0, endpoints: 0 };
        const t = String(n.node_type ?? '').toLowerCase();
        if (['type', 'interface', 'trait', 'class', 'enum'].includes(t)) stats[rid].types += 1;
        if (t === 'endpoint') stats[rid].endpoints += 1;
      }
      wsRepoStats = stats;
    } catch {
      wsRepoStats = {};
    }
  }

  /** Switch the Explorer view (HSI §3 view selector). Resetting to Boundary
   *  restores the canvas drill-down exactly as it was (canvas state is kept
   *  in explorerCanvasState, so nothing is lost while other views are shown). */
  function setExplorerView(mode) {
    explorerViewMode = mode;
    activeSavedViewName = null;
    pushInteraction({ action: 'view_switch', detail: mode });
  }

  // ── View selector dropdown (ui-navigation.md §3, task-178) ───────────
  // "The view selector is a dropdown listing built-in views, then user's
  // saved views, then a 'Generate view...' option."
  let viewSelectOpen = $state(false);
  let activeSavedViewName = $state(null);

  // Saved views via REST (task-178 repair): the dropdown must list the
  // workspace's saved views even when the explorer chat socket fails or
  // the chat column is collapsed before its `views` message arrives — the
  // socket is one delivery path, not the only one. Fetched when the
  // dropdown opens; best-effort (a failed fetch keeps the current list).
  // Both scopes are merged by id, mirroring the socket's ListViews: the
  // repo-scoped records carry the stored ViewQuery directly, the
  // workspace-scoped records carry a ViewSpec (translated via
  // viewSpecToViewQuery).
  let savedViewsRefreshing = false;
  async function refreshSavedViews() {
    if (savedViewsRefreshing) return;
    savedViewsRefreshing = true;
    try {
      const wsId = scope?.workspaceId;
      const repoId = selectedRepoId;
      const [wsViews, repoViews] = await Promise.all([
        wsId ? api.explorerViews(wsId).catch(() => null) : null,
        repoId ? api.savedViews(repoId).catch(() => null) : null,
      ]);
      if (!Array.isArray(wsViews) && !Array.isArray(repoViews)) return;
      const byId = new Map();
      for (const r of Array.isArray(repoViews) ? repoViews : []) {
        if (r?.id && !byId.has(r.id)) byId.set(r.id, { ...r, query: r.query ?? null });
      }
      for (const r of Array.isArray(wsViews) ? wsViews : []) {
        if (!r?.id || byId.has(r.id)) continue;
        byId.set(r.id, {
          ...r,
          is_system: r.is_system ?? r.is_builtin ?? false,
          query: r.query ?? (r.spec ? viewSpecToViewQuery(r.spec) : null),
        });
      }
      explorerSavedViews = [...byId.values()];
    } finally {
      savedViewsRefreshing = false;
    }
  }

  /** Load a saved view: apply its query to the canvas (works with or
   *  without the chat WebSocket — the query is part of the view record). */
  function selectSavedView(view) {
    viewSelectOpen = false;
    explorerViewMode = 'boundary';
    activeSavedViewName = view?.name ?? null;
    if (view?.query && typeof view.query === 'object') {
      activeViewQuery = JSON.parse(JSON.stringify(view.query));
      pushInteraction({ action: 'query', preset: view.name ?? null });
    }
  }

  /** "Generate view..." — focus the Ask input (natural-language view
   *  generation via POST /workspaces/:id/explorer-views/generate). */
  function openGenerateView() {
    viewSelectOpen = false;
    chatCollapsed = false;
    chatMode = 'generate';
  }

  let activeViewLabel = $derived(
    activeSavedViewName
    ?? EXPLORER_VIEWS.find((v) => v.id === explorerViewMode)?.label
    ?? 'Boundary'
  );

  // ── View spec editor (ui-navigation.md §3, task-178) ──────────────
  /** The ViewSpec to prefill the JSON editor with: the active generated
   *  spec when one is applied, else a spec synthesized from the current
   *  view state (data layer derived from the active view query's scope). */
  function currentViewSpecForEditor() {
    if (activeViewSpec) return activeViewSpec;
    const q = activeViewQuery;
    const spec = {
      name: activeViewLabel,
      layout: activeLayout ?? 'graph',
      data: {},
      encoding: { color: 'spec_coverage', size: 'complexity' },
      highlight: {},
    };
    if (q?.scope?.type === 'concept' && Array.isArray(q.scope.seed_nodes) && q.scope.seed_nodes.length > 0) {
      spec.data.concept = q.scope.seed_nodes[0];
      spec.data.depth = q.scope.expand_depth ?? 2;
    } else if (q?.scope?.type === 'filter' && Array.isArray(q.scope.node_types) && q.scope.node_types.length > 0) {
      spec.data.node_types = [...q.scope.node_types];
    }
    return spec;
  }

  function toggleViewSpecEditor() {
    viewSpecEditorOpen = !viewSpecEditorOpen;
    if (viewSpecEditorOpen && !viewSpecText.trim()) {
      viewSpecText = JSON.stringify(currentViewSpecForEditor(), null, 2);
      viewSpecError = '';
    }
  }

  /** Apply the edited ViewSpec: validate JSON, translate to a ViewQuery
   *  (viewSpecToViewQuery), and re-render the canvas. Errors are shown
   *  inline in the editor panel. */
  function applyViewSpecEditor() {
    const text = viewSpecText.trim();
    if (!text) {
      viewSpecError = 'View spec cannot be empty';
      return;
    }
    let parsed;
    try {
      parsed = JSON.parse(text);
    } catch (e) {
      viewSpecError = `Invalid JSON: ${e.message}`;
      return;
    }
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      viewSpecError = 'View spec must be a JSON object';
      return;
    }
    const query = viewSpecToViewQuery(parsed);
    if (!query) {
      viewSpecError = 'View spec must be an object (layers: data, layout, encoding, highlight)';
      return;
    }
    viewSpecError = '';
    activeViewSpec = parsed;
    applyGeneratedViewSpec(parsed);
  }

  /** Navigate from another view (Change / Spec Realization) to a node's
   *  implementation details in the Boundary View. */
  function navigateToBoundaryNode(node) {
    explorerViewMode = 'boundary';
    explorerCanvasState = { ...explorerCanvasState, selectedNode: node };
    detailNode = node;
  }

  /** Read the drilled repo name from the current URL (?repo=name), or null.
   *  Scope drill per ui-layout.md §3: selectRepo pushes this param; reading
   *  it back makes the drilled state deep-linkable on reload/refresh and
   *  lets popstate (browser Back/Forward) keep the view in sync with the
   *  URL instead of drifting. */
  function drilledRepoFromUrl() {
    try {
      const param = new URL(window.location.href).searchParams.get('repo');
      return param || null;
    } catch {
      return null;
    }
  }

  /** Read the lens from the current URL (?lens=evaluative). Called once at
   *  component init (from the explorerLens $state initializer) so a lens
   * deep link restores on reload/refresh. Structural is the default;
   *  observable is future work and normalizes back to structural. */
  function initialLensFromUrl() {
    try {
      return lensFromUrl(window.location.href);
    } catch {
      return 'structural';
    }
  }

  // Lens selection persists in the URL query param (?lens=evaluative) so
  // an overlaid view is deep-linkable. replaceState (no history entry):
  // switching lenses is a view concern, not a navigation — browser Back must
  // not step through lens toggles. The structural default drops the param
  // to keep URLs clean. Other params (e.g. ?repo=) and the #drill= hash are
  // preserved.
  $effect(() => {
    const lensValue = explorerLens;
    try {
      const url = lensToUrl(lensValue, window.location.href);
      if (url !== window.location.href) {
        history.replaceState(null, '', url);
      }
    } catch { /* URL unavailable (SSR) — skip persistence */ }
  });

  function selectRepo(repo) {
    // In workspace-scope mode, selecting a repo loads its graph in this view.
    // Scope drill per ui-layout.md §3: URL updates via pushState (no reload),
    // so the drilled state is deep-linkable and Back returns to the repo list.
    selectedRepoId = repo.id;
    showingRepoGraph = true;
    clearConceptSearch();
    const url = new URL(window.location.href);
    url.searchParams.set('repo', repo.name ?? repo.id);
    window.history.pushState(window.history.state, '', url.toString());
    loadGraph(repo.id);
  }

  function backToRepoList() {
    showingRepoGraph = false;
    selectedRepoId = '';
    graph = null;
    graphError = null;
    // Pop the drill URL entry pushed by selectRepo (Back from drilled state)
    const url = new URL(window.location.href);
    if (url.searchParams.has('repo')) {
      url.searchParams.delete('repo');
      window.history.pushState(window.history.state, '', url.toString());
    }
  }

  // Restore the drilled repo from ?repo= once the workspace repo list is
  // available (repo name -> id resolution needs the loaded list). Runs only
  // at workspace scope with a drill param present; the mount-time restore
  // deliberately does NOT push another history entry - the URL is already
  // the drilled one.
  let drillRestored = false;
  $effect(() => {
    if (drillRestored || scopeType !== 'workspace' || wsReposLoading || !wsRepos.length) return;
    const name = drilledRepoFromUrl();
    if (!name) return;
    const repo = wsRepos.find(r => (r.name ?? r.id) === name);
    if (!repo) return;
    drillRestored = true;
    selectedRepoId = repo.id;
    showingRepoGraph = true;
    clearConceptSearch();
    loadGraph(repo.id);
  });

  // Browser Back/Forward (popstate): keep the view in sync with the URL.
  // App-level popstate handles mode changes; this handles the drill param
  // within the explorer view. Back from a drilled state (param gone) returns
  // to the repo list; Forward re-enters the drilled graph.
  $effect(() => {
    const onPopstate = () => {
      if (scopeType !== 'workspace') return;
      const name = drilledRepoFromUrl();
      if (!name) {
        if (showingRepoGraph) {
          showingRepoGraph = false;
          selectedRepoId = '';
          graph = null;
          graphError = null;
        }
        return;
      }
      const repo = wsRepos.find(r => (r.name ?? r.id) === name);
      if (repo && (!showingRepoGraph || selectedRepoId !== repo.id)) {
        selectedRepoId = repo.id;
        showingRepoGraph = true;
        clearConceptSearch();
        loadGraph(repo.id);
      }
    };
    window.addEventListener('popstate', onPopstate);
    return () => window.removeEventListener('popstate', onPopstate);
  });

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
    if (!repoId) { graph = null; traceData = null; gateRecords = []; livenessTimeline = null; return; }
    loading = true;
    graph = null;
    graphError = null;
    selectedNode = null;
    traceData = null;
    gateStatusBySpec = {};
    specApprovals = [];
    gateRecords = [];
    livenessTimeline = null;
    graphWarnings = [];
    graphWarningsDismissed = false;
    perfWarningDismissed = false;
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
      // Load evaluative lens overlays: gate outcomes + spec approvals
      // (best-effort, non-blocking)
      loadEvaluativeData(repoId);
      // Load MR gate records for evaluative-lens edge colors (HSI §3 Layer 1)
      loadGateData(repoId);
      // Load recent ArchitecturalDelta records for the fading change rings
      // (HSI §3 Layer 1) — the canvas-load query the insights panel's lazy
      // load must not gate.
      loadLivenessTimeline(repoId);
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
      const mrs = await api.mergeRequests({ repository_id: repoId, per_page: 5 });
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

  /** Load Evaluative lens data (HSI §3): gate outcomes and spec approvals.
   *
   *  Gate status: gate results are recorded per MR, and each MR carries a
   *  spec_ref ("path@sha"), so the outcome is joined to graph nodes via the
   *  node's governing spec paths. For each spec path, the most recent MR
   *  with concluded gate results decides: pass = every gate passed,
   *  fail = at least one failed.
   *
   *  Spec approvals: the approval ledger (GET /api/v1/specs/approvals)
   *  provides ✓ approved / ⏳ pending / ✗ rejected status per spec path.
   *
   *  Both loads are best-effort and non-blocking — the structural base
   *  renders fine without them. */
  async function loadEvaluativeData(repoId) {
    // Spec approval ledger — one fetch, filtered client-side by the paths
    // actually present on graph nodes.
    try {
      api.specsApprovals()
        .then((list) => { specApprovals = Array.isArray(list) ? list : []; })
        .catch(() => { /* ledger unavailable — approval icons omitted */ });
    } catch { /* ledger endpoint unavailable — approval icons omitted */ }

    // Gate outcomes per spec path from recent MRs.
    try {
      // GET /api/v1/merge-requests scopes by `repository_id` and caps via
      // `per_page` (newest first — ListMrsQuery), so the gate-status window
      // is requested repo-scoped with a bounded per-MR gate fetch fan-out
      // (the same ≤10 bound the liveness loaders uphold); a repo_id param
      // is silently ignored and would attribute other repos' gate outcomes
      // to this repo's evaluative overlay. A client-side sort keeps the
      // recency order explicit even if a backend ignores the cap.
      const mrs = await api.mergeRequests({ repository_id: repoId, per_page: 10 });
      const mrList = mrs?.merge_requests ?? mrs ?? [];
      // Most recent first: the latest MR with concluded gates decides a
      // spec path's status; older MRs for the same path are ignored.
      const sorted = [...mrList].sort((a, b) => {
        const at = Number(a?.created_at ?? 0) || 0;
        const bt = Number(b?.created_at ?? 0) || 0;
        return bt - at;
      }).slice(0, 10);
      const byPath = {};
      for (const mr of sorted) {
        const specRef = mr?.spec_ref;
        if (!specRef) continue;
        const path = specRef.split('@')[0];
        if (!path || byPath[path]) continue;
        try {
          const results = await api.mrGates(mr.id);
          // Keep only the most recent result per gate (re-runs append new
          // records; the latest run decides the outcome).
          const latestByGate = new Map();
          for (const r of results ?? []) {
            if (r?.status !== 'passed' && r?.status !== 'failed') continue;
            const gid = r.gate_id ?? r.id;
            const prev = latestByGate.get(gid);
            if (!prev || (r.started_at ?? 0) >= (prev.started_at ?? 0)) latestByGate.set(gid, r);
          }
          const concluded = [...latestByGate.values()];
          if (concluded.length === 0) continue;
          byPath[path] = concluded.some(r => r.status === 'failed') ? 'fail' : 'pass';
        } catch { /* no gate results for this MR — try older MRs for the path */ }
      }
      gateStatusBySpec = byPath;
    } catch {
      gateStatusBySpec = {};
    }
  }

  /**
   * HSI §3 Layer 1: load MR gate evaluation records for the current repo.
   * Each MR's spec_ref links its gate results to the graph nodes the spec
   * governs; the Evaluative lens colors edges green (all passed) / red (any
   * failed). Best-effort and non-blocking, like trace loading.
   */
  async function loadGateData(repoId) {
    try {
      const mrs = await api.mergeRequests({ repository_id: repoId, per_page: 10 });
      const mrList = mrs?.merge_requests ?? mrs ?? [];
      const records = [];
      for (const mr of mrList) {
        if (!mr?.spec_ref) continue;
        try {
          const gates = await api.mrGates(mr.id);
          if (Array.isArray(gates) && gates.length > 0) {
            records.push({ mrId: mr.id, spec_ref: mr.spec_ref, gates });
          }
        } catch { /* no gates for this MR, skip */ }
      }
      gateRecords = records;
    } catch { /* gate loading is best-effort */ }
  }

  /**
   * HSI §3 Layer 1: load the repo's recent ArchitecturalDelta records on
   * canvas load so the fading highlight rings have a data source in the
   * default view (the Architecture-Insights panel that owns the full
   * timeline is collapsed by default). The query is bounded to the recent-
   * change window (since = now − 1h, no until) so the payload stays small;
   * computeRecentChanges re-applies the same window, and the insights
   * panel's full timeline supersedes this slice once loaded. Best-effort
   * and non-blocking, like trace/gate loading.
   */
  async function loadLivenessTimeline(repoId) {
    try {
      const since = Math.floor((Date.now() - RECENT_CHANGE_WINDOW_MS) / 1000);
      const timeline = await api.repoGraphTimeline(repoId, since);
      if (selectedRepoId !== repoId) return; // stale — user switched repos
      livenessTimeline = Array.isArray(timeline) ? timeline : [];
    } catch { /* recent-change rings are best-effort */ }
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
      if (viewSelectOpen) { viewSelectOpen = false; return; }
      if (conceptDialog) { conceptDialog = null; return; }
      if (viewMenu) { viewMenu = null; return; }
      if (viewTooltip) { viewTooltip = null; return; }
      if (specEditorOpen) { closeSpecEditorWithGuard(); return; }
      if (viewSpecEditorOpen) { viewSpecEditorOpen = false; return; }
      if (queryEditorOpen) { queryEditorOpen = false; return; }
      if (detailNode) { detailNode = null; return; }
      if (repoDetail) { repoDetail = null; return; }
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

  // ── Large graph performance warning (explorer-implementation.md §28 Phase 4) ──
  // Graphs above 10k nodes are the spec's "large graph" threshold: rendering
  // stays correct (viewport culling + zoom-level LOD) but interaction can
  // degrade, so the user is told why instead of guessing.
  const LARGE_GRAPH_THRESHOLD = 10000;
  let perfWarningDismissed = $state(false);
  let largeGraphWarning = $derived.by(() => {
    if (perfWarningDismissed) return null;
    const count = effectiveGraph?.nodes?.length ?? 0;
    if (count <= LARGE_GRAPH_THRESHOLD) return null;
    return $t('explorer_view.large_graph_warning', { values: { count: count.toLocaleString() } });
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

  // ── In-view filter panel → canvas node visibility (task-178) ──────────
  // filter-categories.js owns the classification; the panel's category
  // checkboxes reduce to the set of node ids the canvas hides (concept
  // selection fades instead — see explorerConceptNodeIds below). null when
  // no category is unchecked (no filtering cost on the draw loop).
  let explorerHiddenNodeIds = $derived.by(() => {
    const hiddenCategories = CATEGORY_IDS.filter((id) => !activeFilterCategories[id]);
    if (hiddenCategories.length === 0) return null;
    return computeHiddenNodeIds(effectiveGraph.nodes ?? [], {
      hiddenCategories,
    });
  });

  // ── Concept view emphasis set (task-182, system-explorer.md §4) ──────
  // Union of the manifest concept's projection node ids and the
  // discovered-concept substring matches. The canvas fades every node
  // outside this set to background (0.15) and highlights edges within it.
  // null when no concept is active.
  let explorerConceptNodeIds = $derived.by(() => {
    const sets = [];
    if (activeManifestConcept?.nodeIds && activeManifestConcept.nodeIds.size > 0) {
      sets.push(activeManifestConcept.nodeIds);
    }
    if (activeFilterConcepts.length > 0) {
      const discovered = computeConceptNodeIds(effectiveGraph.nodes ?? [], activeFilterConcepts);
      if (discovered.size > 0) sets.push(discovered);
    }
    if (sets.length === 0) return null;
    const union = new Set();
    for (const s of sets) for (const id of s) union.add(id);
    return union.size > 0 ? union : null;
  });

  // Unchecked category ids + selected concepts, BEFORE reduction to node
  // ids: the canvas classifies tree-group containers (synthetic boundary
  // boxes) through their own node, so it needs the panel state itself, not
  // just the hidden leaf-id set.
  let explorerHiddenCategories = $derived(CATEGORY_IDS.filter((id) => !activeFilterCategories[id]));

  // ── Evaluative lens risk overlay (task-178) ───────────────────
  // GET /repos/:id/graph/risks — fetched when the Evaluative lens is active
  // (best-effort, non-blocking; the lens renders fine without it). The score
  // mirrors the canvas's client-side risk definition (churn × complexity)
  // but sourced from the server's stored metrics.
  let riskByNode = $state({});
  let riskLoadedFor = $state('');
  $effect(() => {
    if (explorerLens !== 'evaluative' || explorerViewMode !== 'boundary' || !selectedRepoId) return;
    if (riskLoadedFor === selectedRepoId) return;
    const currentRepoId = selectedRepoId;
    riskLoadedFor = currentRepoId;
    api.repoGraphRisks(currentRepoId)
      .then((risks) => {
        if (selectedRepoId !== currentRepoId) return; // stale
        const m = {};
        for (const r of Array.isArray(risks) ? risks : []) {
          if (!r?.node_id) continue;
          const churn = Number(r.churn_rate ?? 0) || 0;
          const complexity = Number(r.complexity ?? 0) || 0;
          m[r.node_id] = churn * complexity;
        }
        riskByNode = m;
      })
      .catch(() => {
        // Risk endpoint unavailable — allow a retry on the next lens toggle.
        if (selectedRepoId === currentRepoId) riskLoadedFor = '';
      });
  });

  // Rendering density policy for the canvas area (ui-layout.md §10):
  // the shell owns the >1000-node list fallback; the public-only filter
  // (>500) is applied inside ExplorerCanvas with its own banner.
  let graphDensity = $derived(applyDensityPolicy(effectiveGraph?.nodes ?? []));

  // ── Generated-view layout dispatch (ui-layout.md §10 Layout Engines) ──
  /** Renderer component for the active layout (capitalized for Svelte's
   *  dynamic component syntax; null → unified canvas / shell list). */
  let LayoutRenderer = $derived.by(() => {
    if (CANVAS_LAYOUTS.has(activeLayout) || activeLayout === 'list') return null;
    return getLayout(activeLayout)?.component ?? null;
  });

  /** The active layout's registry entry (label key, engine selector). */
  let activeLayoutReg = $derived(getLayout(activeLayout));

  /** Nodes/edges the generated layout renderer receives: the active view
   *  query's scope applied to the effective graph, so a generated
   *  hierarchical/layered/side-by-side/diff/list view shows the same node
   *  set the unified canvas would show for the same spec (null scope or an
   *  unmatched scope → full graph, mirroring the canvas). */
  let generatedLayoutGraph = $derived.by(() => {
    if (CANVAS_LAYOUTS.has(activeLayout)) return null;
    const allNodes = effectiveGraph?.nodes ?? [];
    const allEdges = effectiveGraph?.edges ?? [];
    const scoped = resolveScopeNodes(activeViewQuery?.scope, allNodes, allEdges);
    const nodes = scoped ?? allNodes;
    const ids = new Set(nodes.map((n) => n.id));
    const edges = allEdges.filter((e) => ids.has(edgeSrc(e)) && ids.has(edgeTgt(e)));
    return { nodes, edges };
  });

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

<svelte:window onkeydown={onWindowKeydown} onbeforeunload={onBeforeUnload} onclick={onWindowClick} />

{#if scopeType === 'tenant'}
  <!-- Tenant scope: workspace cards grid (S4.4a) -->
  <WorkspaceCards {onSelectWorkspace} />

{:else if scopeType === 'workspace' && !showingRepoGraph}
  <!-- Workspace scope: realized architecture — repos as nodes, cross-repo
       dependencies as edges (ui-layout.md §5, C4 "Container" level) — S4.4b -->
  <div class="ws-arch-view" aria-busy={wsReposLoading || wsDepLoading}>
    {#if !embedded}
      <div class="ws-repo-header">
        <h1 class="page-title">{$t('explorer_view.workspace_title')}</h1>
        <p class="ws-repo-desc">{$t('explorer_view.workspace_desc')}</p>
      </div>
    {/if}
    {#if wsReposLoading}
      <div class="ws-arch-canvas-loading">
        <Skeleton lines={6} />
      </div>
    {:else if wsReposError}
      <div class="error-banner" role="alert">
        <span>{wsReposError}</span>
        <button onclick={() => { wsReposError = null; loadWsRepos(); }} class="retry-btn">{$t('common.retry')}</button>
      </div>
    {:else if wsRepos.length === 0}
      <!-- Empty state (§5): zero repos → centered message with a create link -->
      <div class="ws-arch-empty">
        <EmptyState
          title={$t('explorer_view.no_repos')}
          description={$t('explorer_view.no_repos_desc')}
        >
          {#snippet action()}
            <button class="go-admin-btn" onclick={goToCreateRepo} data-testid="ws-arch-create-repo">
              {$t('explorer_view.create_repo')}
            </button>
          {/snippet}
        </EmptyState>
      </div>
    {:else}
      <div class="ws-arch-body">
        <div class="ws-arch-canvas" data-testid="ws-arch-canvas">
          {#if wsDepError}
            <div class="ws-dep-warning" role="status">
              <span>{$t('explorer_view.dep_graph_unavailable', { values: { error: wsDepError } })}</span>
              <button class="chip-clear" onclick={loadWsDepGraph} aria-label={$t('common.retry')}>↻</button>
            </div>
          {/if}
          <DependencyGraph
            nodes={wsGraphNodes}
            edges={wsGraphEdges}
            scope="workspace"
            showScopeToggle={false}
            onNodeClick={showRepoDetail}
            onNodeDblClick={enterRepo}
          />
        </div>
        {#if repoDetail}
          <!-- Single-click detail panel: repo metadata (§5) -->
          <aside class="ws-repo-detail" data-testid="ws-repo-detail" aria-label={repoDetail.name}>
            <div class="ws-repo-detail-header">
              <h2 class="ws-repo-detail-title" title={repoDetail.name}>{repoDetail.name}</h2>
              <button
                class="ws-repo-detail-close"
                onclick={() => { repoDetail = null; }}
                aria-label={$t('common.close')}
                type="button"
              >✕</button>
            </div>
            {#if repoDetail.description}
              <p class="ws-repo-detail-desc">{repoDetail.description}</p>
            {/if}
            {#if repoDetail.external}
              <p class="ws-repo-detail-external">{$t('explorer_view.repo_external')}</p>
            {/if}
            <dl class="ws-repo-detail-meta">
              <div class="meta-row">
                <dt>{$t('explorer_view.repo_detail_id')}</dt>
                <dd class="mono">{repoDetail.id ?? repoDetail.repo_id}</dd>
              </div>
              {#if repoDetail.default_branch}
                <div class="meta-row">
                  <dt>{$t('explorer_view.repo_detail_branch')}</dt>
                  <dd class="mono">{repoDetail.default_branch}</dd>
                </div>
              {/if}
              {#if repoDetail.created_at}
                <div class="meta-row">
                  <dt>{$t('explorer_view.repo_detail_created')}</dt>
                  <dd>{new Date(repoDetail.created_at * 1000).toLocaleDateString()}</dd>
                </div>
              {/if}
              {#if repoDetail.updated_at}
                <div class="meta-row">
                  <dt>{$t('explorer_view.repo_detail_updated')}</dt>
                  <dd>{new Date(repoDetail.updated_at * 1000).toLocaleDateString()}</dd>
                </div>
              {/if}
            </dl>
            <div class="ws-repo-detail-actions">
              <!-- Keyboard-reachable equivalent of the double-click drill -->
              <button
                class="enter-btn"
                onclick={() => enterRepo(repoDetail)}
                data-testid="ws-repo-detail-explore"
              >
                {$t('explorer_view.explore_architecture')}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
                  <path d="M9 18l6-6-6-6"/>
                </svg>
              </button>
            </div>
          </aside>
        {/if}
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

        {#if scopeType === 'repo'}
          <!-- Control-bar sub-tabs (ui-navigation.md §2): Graph (default) |
               Briefing (repo-scoped narrative, HSI §1.5). -->
          <div class="arch-subtabs" role="tablist" aria-label={$t('explorer_view.arch_subtabs_label')}>
            <button
              class="arch-subtab-btn"
              class:active={activeArchSubTab === 'graph'}
              role="tab"
              aria-selected={activeArchSubTab === 'graph'}
              onclick={() => setArchSubTab('graph')}
              data-testid="arch-subtab-graph"
            >{$t('explorer_view.sub_tabs.graph')}</button>
            <button
              class="arch-subtab-btn"
              class:active={activeArchSubTab === 'briefing'}
              role="tab"
              aria-selected={activeArchSubTab === 'briefing'}
              onclick={() => setArchSubTab('briefing')}
              data-testid="arch-subtab-briefing"
            >{$t('explorer_view.sub_tabs.briefing')}</button>
          </div>
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


    <!-- Control bar — view selector + concept search + filter toggle, always shown when repo is selected -->
    {#if selectedRepoId}
      <div class="concept-search-bar">
        <!-- HSI §3 / ui-navigation.md §3 view selector: a dropdown listing
             built-in views, then the user's saved views, then a "Generate
             view..." option (task-178). -->
        <div class="explorer-view-select" data-testid="explorer-view-selector">
          <button
            class="view-select-btn"
            onclick={() => {
              viewSelectOpen = !viewSelectOpen;
              // REST refresh on open: the saved-views list must not depend
              // solely on the chat socket's `views` message.
              if (viewSelectOpen) void refreshSavedViews();
            }}
            aria-haspopup="listbox"
            aria-expanded={viewSelectOpen}
            aria-label="View selector"
            type="button"
          >
            <span class="view-select-label">{activeViewLabel}</span>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true"><polyline points="6 9 12 15 18 9"/></svg>
          </button>
          {#if viewSelectOpen}
            <div class="view-select-menu" role="listbox" aria-label="Explorer views">
              <div class="view-select-section">Built-in views</div>
              {#each EXPLORER_VIEWS as v (v.id)}
                <button
                  class="view-select-option"
                  class:active={explorerViewMode === v.id && !activeSavedViewName}
                  role="option"
                  aria-selected={explorerViewMode === v.id && !activeSavedViewName}
                  onclick={() => { viewSelectOpen = false; setExplorerView(v.id); }}
                  data-testid={`view-option-${v.id}`}
                  type="button"
                >{v.label}</button>
              {/each}
              {#if explorerSavedViews.length > 0}
                <div class="view-select-section">Saved views</div>
                {#each explorerSavedViews as view (view.id)}
                  <button
                    class="view-select-option"
                    class:active={activeSavedViewName === view.name}
                    role="option"
                    aria-selected={activeSavedViewName === view.name}
                    onclick={() => selectSavedView(view)}
                    data-testid="view-option-saved"
                    title={view.description ?? view.name}
                    type="button"
                  >{view.name}</button>
                {/each}
              {/if}
              <div class="view-select-divider" role="presentation"></div>
              <button
                class="view-select-option view-select-generate"
                role="option"
                aria-selected="false"
                onclick={openGenerateView}
                data-testid="view-option-generate"
                title="Describe a view in natural language — the LLM generates it"
                type="button"
              >Generate view…</button>
            </div>
          {/if}
        </div>

        {#if explorerViewMode === 'boundary'}
        <!-- In-view filter panel toggle (ui-navigation.md §3): filter icon in
             the control bar opens the collapsible 200px left panel inside the
             Architecture content area (not the app sidebar). -->
        <button
          class="ctrl-btn icon-btn"
          class:active={filterPanelOpen}
          onclick={() => { filterPanelOpen = !filterPanelOpen; }}
          title="Toggle filter panel"
          aria-label="Toggle filter panel"
          aria-pressed={filterPanelOpen}
          data-testid="filter-panel-toggle"
          type="button"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>
          </svg>
        </button>

        <!-- View spec editor toggle (ui-navigation.md §3): "Edit view spec"
             opens a collapsible JSON editor for the view spec grammar. -->
        <button
          class="ctrl-btn icon-btn"
          class:active={viewSpecEditorOpen}
          onclick={toggleViewSpecEditor}
          title="Edit view spec"
          aria-label="Edit view spec"
          aria-pressed={viewSpecEditorOpen}
          data-testid="view-spec-editor-toggle"
          type="button"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>
          </svg>
        </button>

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
        {/if}

        <!-- Active concept filter chips (system-explorer.md §4, task-182):
             the active concept view (manifest or discovered) shows as a chip
             in the control bar — click ✕ to clear, restoring the full graph. -->
        {#if explorerViewMode === 'boundary'}
          {#if activeManifestConcept}
            <span class="concept-chip concept-filter-chip" data-testid="concept-filter-chip">
              Concept: {activeManifestConcept.name}
              <button
                class="chip-clear"
                onclick={() => { activeManifestConcept = null; }}
                aria-label="Clear concept filter"
                data-testid="concept-filter-chip-clear"
                type="button"
              >✕</button>
            </span>
          {/if}
          {#each activeFilterConcepts as c (c)}
            <span class="concept-chip concept-filter-chip" data-testid="concept-filter-chip">
              Concept: {c}
              <button
                class="chip-clear"
                onclick={() => { activeFilterConcepts = activeFilterConcepts.filter((x) => x !== c); }}
                aria-label="Clear concept filter"
                data-testid="concept-filter-chip-clear"
                type="button"
              >✕</button>
            </span>
          {/each}
        {/if}
      </div>
    {/if}

    <!-- Main content -->
    <div class="explorer-body">
      <div class="explorer-body-main">
        {#if scopeType === 'repo' && activeArchSubTab === 'briefing' && scope.repoId}
          <!-- Briefing sub-tab (ui-navigation.md §2): repo-scoped narrative —
               HSI §1.5 repo-scope Briefing row (?repo_id= filter). -->
          <div class="arch-briefing-wrap" data-testid="arch-briefing">
            <Briefing workspaceId={scope.workspaceId} repoId={scope.repoId} scope="repo" />
          </div>
        {:else if !selectedRepoId}
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
              {#if missingCallEdgesWarning || largeGraphWarning}
                <div class="canvas-warning-stack">
                  {#if missingCallEdgesWarning}
                    <div class="toolchain-warning" role="alert">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" style="flex-shrink:0"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
                      <span>{missingCallEdgesWarning}</span>
                      <button class="toolchain-warning-dismiss" onclick={() => { graphWarningsDismissed = true; }} title="Dismiss" type="button" aria-label="Dismiss warning">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                      </button>
                    </div>
                  {/if}
                  {#if largeGraphWarning}
                    <div class="toolchain-warning perf-warning" role="status" data-testid="large-graph-warning">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" style="flex-shrink:0" aria-hidden="true"><path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z"/></svg>
                      <span>{largeGraphWarning}</span>
                      <button class="toolchain-warning-dismiss" onclick={() => { perfWarningDismissed = true; }} title="Dismiss" type="button" aria-label="Dismiss performance warning">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                      </button>
                    </div>
                  {/if}
                </div>
              {/if}
              {#if explorerViewMode === 'boundary'}
              <!-- In-view filter panel row (ui-navigation.md §3): collapsible
                   200px left panel INSIDE the Architecture content area —
                   category filters + concepts, not the app sidebar. -->
              <div class="explorer-boundary-row">
              {#if filterPanelOpen}
                <ExplorerFilterPanel
                  nodes={effectiveGraph.nodes ?? []}
                  bind:activeCategories={activeFilterCategories}
                  bind:activeConcepts={activeFilterConcepts}
                  manifestConcepts={manifestConcepts}
                  activeManifestConcept={activeManifestConcept?.name ?? null}
                  onToggleManifestConcept={toggleManifestConcept}
                  onClose={() => { filterPanelOpen = false; }}
                />
              {/if}
              <div class="explorer-boundary-main">
              {#if LayoutRenderer}
                <!-- Generated-view layout dispatch (ui-layout.md §10 Layout
                     Engines): the ViewSpec `layout` field selects the renderer
                     through the layout registry — hierarchical/layered render on
                     the SVG canvas positioned by ELK, side-by-side on the CSS
                     Grid composite, diff on the structural-diff component. The
                     scope of the spec's ViewQuery filters the node set. -->
                <div class="gen-layout-wrap" data-layout={activeLayout} data-testid="generated-layout">
                  <div class="gen-layout-chip" role="status">
                    <span class="gen-layout-chip-label">{$t(activeLayoutReg?.labelKey ?? 'explorer_view.layout_graph')}</span>
                    <button
                      class="gen-layout-chip-clear"
                      onclick={clearGeneratedLayout}
                      aria-label={$t('explorer_view.layout_dismiss')}
                      title={$t('explorer_view.layout_dismiss')}
                      type="button"
                    >✕</button>
                  </div>
                  <div class="gen-layout-body">
                    <LayoutRenderer
                      nodes={generatedLayoutGraph.nodes}
                      edges={generatedLayoutGraph.edges}
                      repoId={selectedRepoId}
                      engine={activeLayoutReg?.engine ?? 'graph'}
                      onSelectNode={openNodeDetail}
                    />
                  </div>
                </div>
              {:else if activeLayout === 'list'}
                <!-- ui-layout.md §10 `list` engine: native Svelte {#each} rows
                     with virtual scrolling (registry component is null — the
                     shell owns the list renderer). -->
                <div class="gen-layout-wrap" data-layout="list" data-testid="generated-layout">
                  <div class="gen-layout-chip" role="status">
                    <span class="gen-layout-chip-label">{$t('explorer_view.layout_list')}</span>
                    <button
                      class="gen-layout-chip-clear"
                      onclick={clearGeneratedLayout}
                      aria-label={$t('explorer_view.layout_dismiss')}
                      title={$t('explorer_view.layout_dismiss')}
                      type="button"
                    >✕</button>
                  </div>
                  <div class="gen-layout-body">
                    <VirtualNodeList
                      nodes={generatedLayoutGraph.nodes}
                      viewportHeight={520}
                    />
                  </div>
                </div>
              {:else if graphDensity.mode === 'list'}
                <!-- ui-layout.md §10 Canvas Rendering: >1000 nodes even after
                     the public filter → list layout with virtual scrolling +
                     a warning (the shell owns the fallback swap). -->
                <div class="density-list-fallback" role="region" aria-label="Graph as virtualized list">
                  <div class="density-list-banner" role="status">
                    Graph too large for canvas rendering ({graphDensity.total} nodes) —
                    switched to list layout with virtual scrolling.
                  </div>
                  <VirtualNodeList
                    nodes={effectiveGraph.nodes ?? []}
                    viewportHeight={520}
                  />
                </div>
              {:else}
              <!-- windowEscape={false}: ExplorerView owns the window-level Escape
                   cascade (onWindowKeydown: spec editor → query editor → detail panel →
                   active view query) and already exits trace mode through it; a second
                   window Escape listener in the canvas would clear the trace on the same
                   keypress that closes an overlay above it. -->
              <ExplorerCanvas
                repoId={selectedRepoId}
                rootLabel={repos.find(r => r.id === selectedRepoId)?.name ?? ''}
                layout={activeLayout}
                nodes={effectiveGraph.nodes ?? []}
                edges={effectiveGraph.edges ?? []}
                activeQuery={activeViewQuery}
                bind:filter={explorerFilter}
                bind:lens={explorerLens}
                gateStatusBySpec={gateStatusBySpec}
                specApprovals={specApprovals}
                bind:canvasState={explorerCanvasState}
                onNodeDetail={openNodeDetail}
                onInteractiveQuery={(q) => { activeViewQuery = q; pushInteraction({ action: 'query', detail: 'interactive' }); }}
                ghostOverlays={mergedGhostOverlays}
                {traceData}
                playbackRequest={tracePlaybackRequest}
                queryResult={viewQueryResult}
                assertionResults={specAssertionResults}
                assertionSpecPath={specEditorOpen ? specEditorPath : null}
                {highlightedSpanId}
                focusRequest={traceFocusRequest}
                windowEscape={false}
                timeline={graphTimeline ?? livenessTimeline ?? []}
                timelineActive={timelineScrubActive}
                timelineScrubIndex={timelineScrubIndex}
                {gateRecords}
                riskByNode={explorerLens === 'evaluative' ? riskByNode : {}}
                hiddenNodeIds={explorerHiddenNodeIds}
                hiddenCategories={explorerHiddenCategories}
                filterConcepts={activeFilterConcepts}
                conceptNodeIds={explorerConceptNodeIds}
                onCreateConcept={openConceptDialog}
                onTimelineToggle={() => {
                  timelineScrubActive = !timelineScrubActive;
                  if (!timelineScrubActive) timelineScrubIndex = -1;
                  else if (graphTimeline?.length) timelineScrubIndex = graphTimeline.length - 1;
                }}
                onTimelineScrub={(idx) => { timelineScrubIndex = idx; }}
                onTimelineMarkerClick={(_delta) => {}}
                {timelineDeltaStats}
              />
              {/if}
              </div><!-- /explorer-boundary-main -->
              </div><!-- /explorer-boundary-row -->
              {:else if explorerViewMode === 'spec-realization'}
                <!-- HSI §3 Spec Realization View: specs ↔ linked graph nodes -->
                <SpecRealizationView
                  repoId={selectedRepoId}
                  {graph}
                  onOpenSpec={(path) => openSpecEditor(path)}
                  onSelectNode={navigateToBoundaryNode}
                />
              {:else if explorerViewMode === 'change'}
                <!-- HSI §3 Change View: architectural deltas since last visit -->
                <ChangeView
                  repoId={selectedRepoId}
                  {graph}
                  onNavigateToNode={navigateToBoundaryNode}
                />
              {:else if explorerViewMode === 'system-trace'}
                <!-- HSI §3 System Trace View: SDLC timeline for an MR -->
                <SystemTraceView repoId={selectedRepoId} />
              {/if}
              <!-- Architecture Insights — collapsible panel inside canvas area (Boundary view only) -->
              {#if explorerViewMode === 'boundary' && selectedRepoId && !loading && (repoDeps || repoRisks?.length || graphTypes?.length || graphModules?.length || graphTimeline?.length)}
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
                            <div
                              class="arch-type-card"
                              title={node.qualified_name ?? node.name}
                              tabindex="0"
                              role="button"
                              aria-label="{node.name ?? node.qualified_name} ({node.node_type ?? 'type'})"
                              onclick={(e) => cardClick(e, node)}
                              onmouseenter={(e) => emitCardViewEvent(e, node)}
                              oncontextmenu={(e) => { e.preventDefault(); emitCardViewEvent(e, node); }}
                              onkeydown={(e) => cardKeydown(e, node)}
                            >
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
                            <li
                              class="arch-risk-item"
                              tabindex="0"
                              role="button"
                              aria-label="{node.qualified_name ?? node.name} risk hotspot"
                              onclick={(e) => cardClick(e, node)}
                              onmouseenter={(e) => emitCardViewEvent(e, node)}
                              oncontextmenu={(e) => { e.preventDefault(); emitCardViewEvent(e, node); }}
                              onkeydown={(e) => cardKeydown(e, node)}
                            >
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
                  onTraceStepNavigate={(step) => {
                    // Show the step's node in the detail panel and pan/zoom
                    // the canvas to it (explorer-canvas.md §6 step 6). The
                    // step list is carried onto the target so the panel
                    // keeps showing the full clickable trace across
                    // navigation clicks.
                    const target = traceStepTargetNode(detailNode, step, graph.nodes ?? []);
                    if (target) detailNode = target;
                    traceFocusNonce += 1;
                    traceFocusRequest = { nodeId: step.nodeId, nonce: traceFocusNonce };
                  }}
                  onInteractiveQuery={(q) => { activeViewQuery = q; pushInteraction({ action: 'query', detail: 'interactive' }); }}
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
            {#if viewSpecEditorOpen}
              <!-- View spec editor (ui-navigation.md §3): collapsible JSON editor
                   for the view spec grammar — data / layout / encoding / highlight
                   layers. Apply translates the spec to a ViewQuery and re-renders
                   the canvas live. -->
              <div class="query-editor-panel" role="complementary" aria-label="View spec editor" data-testid="view-spec-editor">
                <div class="query-editor-header">
                  <h3 class="query-editor-title">View Spec Editor</h3>
                  <button class="query-editor-close" onclick={() => { viewSpecEditorOpen = false; }} aria-label="Close view spec editor" type="button">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
                      <path d="M18 6L6 18M6 6l12 12"/>
                    </svg>
                  </button>
                </div>
                <div class="query-editor-body">
                  <textarea
                    class="query-editor-textarea"
                    bind:value={viewSpecText}
                    placeholder={'{\n  "name": "My view",\n  "layout": "graph",\n  "data": { "node_types": ["endpoint"] },\n  "encoding": { "color": "spec_coverage", "size": "complexity" },\n  "highlight": {}\n}'}
                    spellcheck="false"
                    aria-label="View spec JSON"
                    data-testid="view-spec-textarea"
                  ></textarea>
                </div>
                {#if viewSpecError}
                  <div class="query-editor-error" role="alert">
                    {viewSpecError}
                  </div>
                {/if}
                <div class="query-editor-actions">
                  <button
                    class="query-editor-clear-btn"
                    onclick={() => { viewSpecText = JSON.stringify(currentViewSpecForEditor(), null, 2); viewSpecError = ''; }}
                    type="button"
                  >Reset</button>
                  <button
                    class="query-editor-run-btn"
                    onclick={applyViewSpecEditor}
                    disabled={!viewSpecText.trim()}
                    type="button"
                  >Apply</button>
                </div>
              </div>
            {/if}
            {#if !chatCollapsed}
              <div class="explorer-chat-area">
                <div class="chat-collapse-bar">
                  <!-- HSI §3: chat column mode — Explorer agent (ask) vs LLM view
                       generator (generate). -->
                  <div class="chat-mode-tabs" role="tablist" aria-label="Chat mode">
                    <button
                      class="chat-mode-tab"
                      class:active={chatMode === 'ask'}
                      role="tab"
                      aria-selected={chatMode === 'ask'}
                      onclick={() => { chatMode = 'ask'; }}
                      type="button"
                    >{$t('explorer_chat.title')}</button>
                    <button
                      class="chat-mode-tab"
                      class:active={chatMode === 'generate'}
                      role="tab"
                      aria-selected={chatMode === 'generate'}
                      onclick={() => { chatMode = 'generate'; }}
                      type="button"
                    >{$t('generated_view.title')}</button>
                  </div>
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
                {#if chatMode === 'ask'}
                <ExplorerChat
                  repoId={selectedRepoId}
                  canvasState={explorerCanvasState}
                  onViewQuery={(q) => { activeViewQuery = q; pushInteraction({ action: 'query', detail: q?.scope?.type ?? 'query' }); }}
                  onOpenSpec={(path) => openSpecEditor(path)}
                  savedViews={explorerSavedViews}
                  onSavedViewsUpdate={(views) => {
                    // The socket's SavedViewSummary carries no query — keep
                    // the query/spec fields already fetched via REST for ids
                    // that survive the replace.
                    const prev = new Map(explorerSavedViews.map((v) => [v.id, v]));
                    explorerSavedViews = (views ?? []).map((v) =>
                      prev.has(v.id) ? { ...prev.get(v.id), ...v } : v
                    );
                  }}
                  onLoadView={(view) => pushInteraction({ action: 'query', preset: view?.name ?? null })}
                  {graphHints}
                  graphNodes={graph?.nodes ?? []}
                  graphEdges={graph?.edges ?? []}
                />
                {:else}
                <GeneratedViewChat
                  workspaceId={scope?.workspaceId ?? ''}
                  repoId={selectedRepoId}
                  onApplyView={applyGeneratedViewSpec}
                />
                {/if}
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
      </div>
        <!-- Architecture Insights moved inside explorer-canvas-area (see above) -->
    </div>
  </div>
{/if}

<!-- ── Ad-hoc concept creation dialog (system-explorer.md §4, task-182) ──── -->
{#if conceptDialog}
  <div
    class="concept-dialog-backdrop"
    onclick={closeConceptDialog}
    oncontextmenu={(e) => { e.preventDefault(); closeConceptDialog(); }}
    role="presentation"
  ></div>
  <div
    class="concept-dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Create concept"
    data-testid="concept-dialog"
  >
    <h3 class="concept-dialog-title">Create concept</h3>
    <p class="concept-dialog-desc">
      Group {conceptDialog.nodes.length} selected nodes into a named concept. Saving stores it
      as an explorer view you can reload from the view selector.
    </p>
    <label class="concept-dialog-label" for="concept-dialog-name">Concept name</label>
    <input
      id="concept-dialog-name"
      class="concept-dialog-input"
      type="text"
      bind:value={conceptDialog.name}
      placeholder="e.g. Authentication"
      data-testid="concept-dialog-name"
      disabled={conceptDialog.saving}
    />
    {#if conceptDialog.error}
      <p class="concept-dialog-error" role="alert">{conceptDialog.error}</p>
    {/if}
    <div class="concept-dialog-actions">
      <button
        class="concept-dialog-save"
        onclick={saveConceptDialog}
        disabled={conceptDialog.saving}
        data-testid="concept-dialog-save"
        type="button"
      >{conceptDialog.saving ? 'Saving…' : 'Save concept'}</button>
      <button
        class="concept-dialog-cancel"
        onclick={closeConceptDialog}
        disabled={conceptDialog.saving}
        type="button"
      >Cancel</button>
    </div>
  </div>
{/if}

<!-- ── Uniform ViewEvent surfaces (ui-layout.md §10 Interaction Events) ────── -->
<!-- Hover tooltip fed by ViewEvents from non-integrated emitters (SVG canvas,
     list rows, cards). ExplorerCanvas renders its own tooltip internally. -->
{#if viewTooltip}
  <div
    class="viewevent-tooltip"
    style="left: {viewTooltip.x + 14}px; top: {viewTooltip.y + 14}px;"
    role="tooltip"
    aria-hidden="true"
  >
    <span class="viewevent-tooltip-type">{viewTooltip.type}</span>
    <span class="viewevent-tooltip-name">{viewTooltip.name}</span>
  </div>
{/if}

<!-- Context-menu actions fed by ViewEvents -->
{#if viewMenu}
  <div class="viewevent-menu-backdrop" onclick={() => (viewMenu = null)} oncontextmenu={(e) => { e.preventDefault(); viewMenu = null; }} role="presentation"></div>
  <div
    class="viewevent-menu"
    style="left: {viewMenu.x}px; top: {viewMenu.y}px;"
    role="menu"
    aria-label="Entity actions"
  >
    <div class="viewevent-menu-title">{viewMenu.data?.name ?? viewMenu.data?.id ?? viewMenu.entity}</div>
    <button class="viewevent-menu-item" role="menuitem" onclick={() => viewMenuAction('details')}>View details</button>
    {#if viewMenu.data?.file_path}
      <button class="viewevent-menu-item" role="menuitem" onclick={() => viewMenuAction('code')}>Open in code</button>
    {/if}
    {#if viewMenu.data?.spec_path}
      <button class="viewevent-menu-item" role="menuitem" onclick={() => viewMenuAction('spec')}>View governing spec</button>
    {/if}
    <button class="viewevent-menu-item" role="menuitem" onclick={() => viewMenuAction('copy_id')}>Copy ID</button>
  </div>
{/if}

<style>
  /* ── Workspace scope: realized architecture graph (§5) ───────────── */
  .ws-arch-view {
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

  .ws-arch-canvas-loading {
    padding: var(--space-4);
  }

  .ws-arch-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .ws-arch-body {
    display: flex;
    gap: var(--space-4);
    align-items: stretch;
    min-height: 420px;
    flex: 1;
  }

  .ws-arch-canvas {
    flex: 1;
    min-width: 0;
    position: relative;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .ws-dep-warning {
    position: absolute;
    top: var(--space-2);
    right: var(--space-2);
    z-index: 2;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-warning, #eab308);
    border-radius: var(--radius);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .ws-dep-warning .chip-clear {
    background: none;
    border: none;
    color: var(--color-link);
    cursor: pointer;
    padding: 0;
    font-size: var(--text-sm);
  }

  /* Repo metadata detail panel (single-click, §5) */
  .ws-repo-detail {
    width: 300px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow-y: auto;
  }

  .ws-repo-detail-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .ws-repo-detail-title {
    margin: 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    overflow-wrap: anywhere;
  }

  .ws-repo-detail-close {
    background: none;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-1);
    line-height: 1;
  }

  .ws-repo-detail-close:hover { color: var(--color-text); }

  .ws-repo-detail-close:focus-visible,
  .enter-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .ws-repo-detail-desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
  }

  .ws-repo-detail-external {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .ws-repo-detail-meta {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .meta-row {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .meta-row dt {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    font-weight: 600;
  }

  .meta-row dd {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .ws-repo-detail .mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }

  .ws-repo-detail-actions {
    margin-top: auto;
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }

  .enter-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    color: var(--color-link);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    padding: 0;
  }

  .enter-btn:hover { color: var(--color-link-hover); }

  @media (max-width: 900px) {
    .ws-arch-body { flex-direction: column; }
    .ws-repo-detail { width: 100%; }
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

  /* Control-bar sub-tabs at repo scope (ui-navigation.md §2): Graph | Briefing */
  .arch-subtabs {
    display: flex;
    gap: 0;
    border-bottom: none;
    margin-top: var(--space-2);
  }

  .arch-subtab-btn {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-muted, var(--color-text));
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: color 0.15s ease, border-color 0.15s ease;
  }

  .arch-subtab-btn.active {
    color: var(--color-primary);
    border-bottom-color: var(--color-primary);
  }

  .arch-subtab-btn:not(.active):hover {
    color: var(--color-text);
  }

  .arch-subtab-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .arch-briefing-wrap {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
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

  /* HSI §3 / ui-navigation.md §3 view selector dropdown (task-178):
     built-in views → saved views → "Generate view..." option. */
  .explorer-view-select {
    position: relative;
    flex-shrink: 0;
  }

  .view-select-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md, 8px);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    white-space: nowrap;
  }

  .view-select-btn:hover { border-color: var(--color-border-strong); }
  .view-select-btn[aria-expanded='true'] { border-color: var(--color-focus); }

  .view-select-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 60;
    min-width: 220px;
    max-height: 320px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md, 8px);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    padding: var(--space-1);
  }

  .view-select-section {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    padding: var(--space-2) var(--space-3) var(--space-1);
  }

  .view-select-option {
    border: none;
    background: none;
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    text-align: left;
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-sm, 6px);
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .view-select-option:hover { background: var(--color-surface); }
  .view-select-option.active { font-weight: 600; box-shadow: inset 2px 0 0 var(--color-focus); }
  .view-select-generate { color: var(--color-focus, #3b82f6); font-style: italic; }

  .view-select-divider {
    height: 1px;
    background: var(--color-border);
    margin: var(--space-1) 0;
  }

  /* In-view filter panel row (task-178): filter panel left, canvas right. */
  .explorer-boundary-row {
    display: flex;
    flex: 1;
    min-height: 0;
    align-items: stretch;
  }

  .explorer-boundary-main {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    min-height: 0;
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

  /* Active concept filter chip (system-explorer.md §4) — distinct accent
     from the search result chip: the concept view is a filter, not a
     search result. */
  .concept-filter-chip {
    background: color-mix(in srgb, #a78bfa 12%, transparent);
    border-color: color-mix(in srgb, #a78bfa 40%, transparent);
    color: #c4b5fd;
  }

  /* ── Ad-hoc concept creation dialog (system-explorer.md §4) ─────── */
  .concept-dialog-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(2, 6, 23, 0.6);
    z-index: 300;
  }

  .concept-dialog {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 301;
    width: min(400px, calc(100vw - 48px));
    padding: var(--space-5);
    background: var(--color-bg-secondary, #0f172a);
    border: 1px solid var(--color-border-strong, #334155);
    border-radius: var(--radius-lg, 10px);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    font-family: system-ui, -apple-system, sans-serif;
  }

  .concept-dialog-title {
    margin: 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text-primary, #e2e8f0);
  }

  .concept-dialog-desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-muted, #94a3b8);
    line-height: 1.5;
  }

  .concept-dialog-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted, #94a3b8);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .concept-dialog-input {
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg-primary, #0f0f1a);
    border: 1px solid var(--color-border-strong, #334155);
    border-radius: var(--radius-md, 6px);
    color: var(--color-text-primary, #e2e8f0);
    font-size: var(--text-sm);
  }
  .concept-dialog-input:focus-visible {
    outline: 2px solid var(--color-focus, #3b82f6);
    outline-offset: 1px;
  }

  .concept-dialog-error {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-danger, #ef4444);
  }

  .concept-dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .concept-dialog-save,
  .concept-dialog-cancel {
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-md, 6px);
    font-size: var(--text-sm);
    cursor: pointer;
    border: 1px solid transparent;
  }
  .concept-dialog-save {
    background: var(--color-focus, #3b82f6);
    color: #fff;
    border-color: var(--color-focus, #3b82f6);
  }
  .concept-dialog-save:disabled { opacity: 0.6; cursor: default; }
  .concept-dialog-cancel {
    background: transparent;
    color: var(--color-text-muted, #94a3b8);
    border-color: var(--color-border-strong, #334155);
  }

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
    display: flex;
    flex-direction: column;
  }

  .canvas-warning-stack {
    position: absolute; top: 8px; left: 50%; transform: translateX(-50%); z-index: 45;
    display: flex; flex-direction: column; align-items: center; gap: 6px;
    max-width: 600px;
  }
  .toolchain-warning {
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

  /* Slide-in per ui-layout.md §3 Drill-Down: 200ms ease-out from the right.
     Width 0→target animates the slide; min-width animates in lockstep. */
  .explorer-detail-area {
    width: 320px;
    min-width: 280px;
    max-width: 380px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--color-border);
    transition: width 200ms ease-out, min-width 200ms ease-out;
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
  /* Slide-in per ui-layout.md §3 Drill-Down: 200ms ease-out from the right. */
  .spec-editor-panel {
    width: 420px;
    min-width: 320px;
    max-width: 520px;
    border-left: 1px solid var(--color-border);
    background: var(--color-surface);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    transition: width 200ms ease-out, min-width 200ms ease-out;
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
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  /* HSI §3: chat mode tabs (Explorer agent | Generated Views) */
  .chat-mode-tabs {
    display: flex;
    gap: var(--space-1);
  }

  .chat-mode-tab {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    cursor: pointer;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .chat-mode-tab:hover { color: var(--color-text); }

  .chat-mode-tab.active {
    color: var(--color-text);
    background: var(--color-surface-elevated);
    border-color: var(--color-border-strong);
    font-weight: 600;
  }

  .chat-mode-tab:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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
  .arch-risk-item {
    cursor: pointer;
  }

  .arch-type-card {
    cursor: pointer;
  }
  .arch-type-card:focus-visible,
  .arch-risk-item:focus-visible {
    outline: 2px solid var(--color-focus, #3b82f6);
    outline-offset: 1px;
  }

  /* ── Density list fallback (ui-layout.md §10: >1000 nodes → list) ───── */
  .density-list-fallback {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
    min-height: 320px;
    padding: var(--space-3, 12px);
    box-sizing: border-box;
  }
  .density-list-banner {
    padding: 8px 12px;
    border: 1px solid #f87171;
    border-radius: 8px;
    background: rgba(30, 41, 59, 0.9);
    color: #f87171;
    font-size: 13px;
  }

  /* ── Generated-view layout dispatch (ui-layout.md §10 Layout Engines) ── */
  .gen-layout-wrap {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
    min-height: 320px;
  }
  .gen-layout-chip {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    align-self: flex-start;
    padding: 3px 6px 3px 10px;
    border: 1px solid #334155;
    border-radius: 999px;
    background: rgba(30, 41, 59, 0.9);
    color: #cbd5e1;
    font-size: 12px;
  }
  .gen-layout-chip-clear {
    background: transparent;
    border: none;
    color: #94a3b8;
    cursor: pointer;
    font-size: 12px;
    line-height: 1;
    padding: 2px 4px;
    border-radius: 4px;
  }
  .gen-layout-chip-clear:hover {
    color: #e2e8f0;
    background: #1e293b;
  }
  .gen-layout-body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  /* ── ViewEvent surfaces (ui-layout.md §10 Interaction Events) ───────── */
  .viewevent-tooltip {
    position: fixed;
    z-index: 300;
    background: rgba(15, 15, 26, 0.95);
    border: 1px solid #334155;
    border-radius: 8px;
    padding: 6px 10px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    pointer-events: none;
    max-width: 320px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
  }
  .viewevent-tooltip-type {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #94a3b8;
  }
  .viewevent-tooltip-name {
    font-size: 13px;
    color: #e2e8f0;
    word-break: break-all;
  }

  .viewevent-menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 290;
  }
  .viewevent-menu {
    position: fixed;
    z-index: 300;
    min-width: 200px;
    background: var(--color-surface, #111827);
    border: 1px solid var(--color-border, #334155);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.6);
    display: flex;
    flex-direction: column;
  }
  .viewevent-menu-title {
    font-size: 12px;
    color: #94a3b8;
    padding: 6px 10px 8px;
    border-bottom: 1px solid #1e293b;
    margin-bottom: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 280px;
  }
  .viewevent-menu-item {
    background: transparent;
    border: none;
    color: #e2e8f0;
    text-align: left;
    padding: 7px 10px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }
  .viewevent-menu-item:hover {
    background: #1e293b;
  }
</style>
