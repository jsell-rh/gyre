<script>
  /**
   * SpecDashboard — S4.5 Specs View (list pane)
   *
   * Spec ref: ui-layout.md §6 (Specs View Layout — Full-Width list, split on row click)
   *           human-system-interface.md §1 (nav scope: Specs)
   *
   * Props:
   *   workspaceId — string | null
   *   repoId      — string | null
   *   scope       — 'tenant' | 'workspace' | 'repo'
   *
   * Shell context (from S4.1 App Shell, PR #394):
   *   openDetailPanel({type, id, data}) — opens detail panel at 40%, compresses list to 60%
   */

  import { getContext, onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import Button from '../lib/Button.svelte';
  import Modal from '../lib/Modal.svelte';
  import SpecGraphDAG from './SpecGraphDAG.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';
  import { specStatusTooltip } from '../lib/statusTooltips.js';

  let { workspaceId = null, repoId = null, scope = 'workspace' } = $props();

  // Shell context — openDetailPanel may not exist (e.g. in tests or older shell)
  const openDetailPanel = getContext('openDetailPanel') ?? null;
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;

  // ── List state ──────────────────────────────────────────────────────────────
  let specs = $state([]);
  let loading = $state(true);
  let error = $state(null);

  // Filters
  let filterStatus = $state('all');
  let filterKind = $state('all');
  let searchQuery = $state('');
  let ownerMe = $state(false);

  // Sort (workspace/tenant scope)
  let sortCol = $state('path');
  let sortDir = $state('asc');

  // Repo-scope progress bars (preloaded for all specs in this repo)
  let progressMap = $state({});

  // Selected path (for row highlight when detail panel open)
  let selectedPath = $state(null);

  // View mode: list or graph
  let viewMode = $state('list');
  let specGraph = $state(null);
  let specGraphLoading = $state(false);
  let graphScope = $state('workspace');

  // Impact analysis state
  let impactMode = $state(false);
  let impactPath = $state(null);

  async function loadSpecGraph() {
    specGraphLoading = true;
    try {
      specGraph = await api.specsGraph();
    } catch {
      specGraph = { nodes: [], edges: [] };
    } finally {
      specGraphLoading = false;
    }
  }

  // Filter graph data to workspace scope (only specs in current workspace repos)
  const graphNodes = $derived.by(() => {
    if (!specGraph?.nodes) return [];
    if (graphScope === 'tenant') return specGraph.nodes;
    // Workspace scope: only include nodes whose path matches a loaded spec in this workspace
    const specPaths = new Set(specs.map(s => s.path));
    return specGraph.nodes.filter(n => specPaths.has(n.path));
  });

  const graphEdges = $derived.by(() => {
    if (!specGraph?.edges) return [];
    if (graphScope === 'tenant') return specGraph.edges;
    // Only include edges where both endpoints are in the workspace-filtered nodes
    const nodePaths = new Set(graphNodes.map(n => n.path));
    return specGraph.edges.filter(e => nodePaths.has(e.source) && nodePaths.has(e.target));
  });

  function handleGraphNodeClick(node) {
    const path = node.path;
    const specData = specs.find(s => s.path === path);
    const data = { path, repo_id: specData?.repo_id ?? repoId };
    if (goToEntityDetail) {
      goToEntityDetail('spec', path, data);
    } else if (openDetailPanel) {
      openDetailPanel({ type: 'spec', id: path, data });
    }
  }

  // ── Impact analysis ──────────────────────────────────────────────────────
  function handleImpactSelect(node) {
    if (impactPath === node.path) {
      // Deselect
      impactPath = null;
    } else {
      impactPath = node.path;
    }
  }

  function exitImpactMode() {
    impactMode = false;
    impactPath = null;
  }

  // Compute transitive dependents from graph edges for the summary panel
  const IMPACT_LINK_TYPES = new Set(['dependson', 'depends_on', 'implements', 'extends']);

  const impactDependentsList = $derived.by(() => {
    if (!impactPath || !specGraph) return [];

    // Use the currently-displayed edges (workspace or tenant scope)
    const currentEdges = graphEdges;

    // Build reverse adjacency
    const reverseAdj = new Map();
    for (const e of currentEdges) {
      const lt = (e.link_type ?? '').toLowerCase();
      if (!IMPACT_LINK_TYPES.has(lt)) continue;
      if (!reverseAdj.has(e.target)) reverseAdj.set(e.target, []);
      reverseAdj.get(e.target).push({ source: e.source, link_type: e.link_type });
    }

    // BFS to find transitive dependents and their link types
    const visited = new Map(); // path → { link_type, depth }
    const queue = [{ path: impactPath, depth: 0 }];
    while (queue.length > 0) {
      const { path: current, depth } = queue.shift();
      const sources = reverseAdj.get(current) ?? [];
      for (const { source, link_type } of sources) {
        if (!visited.has(source)) {
          visited.set(source, { link_type, depth: depth + 1 });
          queue.push({ path: source, depth: depth + 1 });
        }
      }
    }

    // Build list with node info
    return Array.from(visited.entries()).map(([path, { link_type, depth }]) => {
      const node = graphNodes.find(n => n.path === path);
      const specData = specs.find(s => s.path === path);
      return {
        path,
        link_type,
        depth,
        approval_status: node?.approval_status ?? 'unknown',
        repo_id: specData?.repo_id ?? null,
        repo_name: specData?.repo_name ?? specData?.repo_id ?? null,
      };
    }).sort((a, b) => a.depth - b.depth || a.path.localeCompare(b.path));
  });

  const impactRepoCount = $derived.by(() => {
    if (!impactDependentsList.length) return 0;
    return new Set(impactDependentsList.map(d => d.repo_id).filter(Boolean)).size;
  });

  const impactByRepo = $derived.by(() => {
    if (!impactDependentsList.length) return [];
    const groups = new Map();
    for (const dep of impactDependentsList) {
      const key = dep.repo_name ?? dep.repo_id ?? 'unknown';
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(dep);
    }
    return Array.from(groups.entries()).map(([repo, deps]) => ({ repo, deps }));
  });

  // ── Spec approval quick actions ──────────────────────────────────────────────
  let approvingSpec = $state(null);
  let rejectingSpec = $state(null);

  async function quickApprove(spec, e) {
    e?.stopPropagation();
    if (approvingSpec) return;
    approvingSpec = spec.path;
    try {
      await api.approveSpec(spec.path, spec.current_sha);
      toastSuccess(`Spec "${spec.path.split('/').pop()}" approved`);
      specs = specs.map(s => s.path === spec.path ? { ...s, approval_status: 'approved' } : s);
    } catch (err) {
      toastError('Approve failed: ' + (err.message ?? err));
    } finally {
      approvingSpec = null;
    }
  }

  async function quickReject(spec, e) {
    e?.stopPropagation();
    if (rejectingSpec) return;
    rejectingSpec = spec.path;
    try {
      await api.rejectSpec(spec.path, 'Rejected via dashboard');
      toastSuccess(`Spec "${spec.path.split('/').pop()}" rejected`);
      specs = specs.map(s => s.path === spec.path ? { ...s, approval_status: 'rejected' } : s);
    } catch (err) {
      toastError('Reject failed: ' + (err.message ?? err));
    } finally {
      rejectingSpec = null;
    }
  }

  // New spec modal
  let showNewSpec = $state(false);
  let newSpecPath = $state('');
  let newSpecContent = $state('# New Spec\n\n## Overview\n\n');
  let newSpecSaving = $state(false);
  let pathTouched = $state(false);

  // ── Constants ───────────────────────────────────────────────────────────────
  const STATUS_FILTERS = ['all', 'draft', 'pending', 'approved', 'rejected', 'deprecated'];
  const TABLE_COLS = [
    ['path',            'spec_dashboard.col_path'],
    ['approval_status', 'spec_dashboard.col_status'],
    ['kind',            'spec_dashboard.col_kind'],
    ['owner',           'spec_dashboard.col_owner'],
    ['updated_at',      'spec_dashboard.col_updated'],
  ];

  // ── Load specs ──────────────────────────────────────────────────────────────
  async function load() {
    loading = true;
    error = null;
    try {
      const allSpecs = await api.specsForWorkspace(workspaceId);
      if (scope === 'repo' && repoId) {
        specs = allSpecs.filter(s => s.repo_id === repoId);
        loadProgressMap().catch(e => console.error('Progress load failed:', e));
      } else {
        specs = allSpecs;
      }
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  async function loadProgressMap() {
    if (!repoId) return;
    const paths = specs.map((s) => s.path);
    const results = await Promise.allSettled(
      paths.map((p) => api.specProgress(p, repoId))
    );
    const map = {};
    results.forEach((r, i) => {
      if (r.status === 'fulfilled' && r.value) {
        map[paths[i]] = r.value;
      }
    });
    progressMap = map;
  }

  $effect(() => {
    void scope; void workspaceId; void repoId;
    // Clear stale selection from previous scope
    selectedPath = null;
    load();
  });

  // Handle URL query params on mount:
  // - ?create=true opens the "New Spec" modal (e.g. from ExplorerCanvas)
  // - ?path=<spec_path> opens the spec detail panel (e.g. from workspace home spec click)
  onMount(() => {
    const url = new URL(window.location.href);
    if (url.searchParams.get('create') === 'true') {
      showNewSpec = true;
      url.searchParams.delete('create');
      window.history.replaceState(window.history.state, '', url.toString());
    }
    const specPath = url.searchParams.get('path');
    if (specPath) {
      url.searchParams.delete('path');
      window.history.replaceState(window.history.state, '', url.toString());
      // Defer so specs have time to load before opening the panel
      load().then(() => {
        handleRowClick({ path: specPath, repo_id: repoId });
      });
    }
  });

  // ── Derived: filtered + sorted ──────────────────────────────────────────────
  const allKinds = $derived.by(() => {
    const set = new Set(specs.map((s) => s.kind || 'feature'));
    return ['all', ...Array.from(set).sort()];
  });

  const filtered = $derived.by(() => {
    let result = specs;
    if (filterStatus !== 'all') {
      result = result.filter((s) => s.approval_status === filterStatus);
    }
    if (filterKind !== 'all') {
      result = result.filter((s) => (s.kind || 'feature') === filterKind);
    }
    if (ownerMe) {
      result = result.filter((s) => s.owner === 'me' || s.is_mine);
    }
    if (searchQuery.trim()) {
      const q = searchQuery.trim().toLowerCase();
      result = result.filter((s) =>
        (s.path ?? '').toLowerCase().includes(q) ||
        (s.kind ?? '').toLowerCase().includes(q) ||
        (s.owner ?? '').toLowerCase().includes(q)
      );
    }
    result = sortList(result, sortCol, sortDir);
    return result;
  });

  function sortList(list, col, dir) {
    return [...list].sort((a, b) => {
      if (col === 'progress') {
        const pa = progressMap[a.path];
        const pb = progressMap[b.path];
        const av = pa && pa.total_tasks ? pa.completed_tasks / pa.total_tasks : -1;
        const bv = pb && pb.total_tasks ? pb.completed_tasks / pb.total_tasks : -1;
        const cmp = av - bv;
        return dir === 'asc' ? cmp : -cmp;
      }
      const av = String(a[col] ?? '');
      const bv = String(b[col] ?? '');
      const cmp = av.localeCompare(bv);
      return dir === 'asc' ? cmp : -cmp;
    });
  }

  function toggleSort(col) {
    if (sortCol === col) {
      sortDir = sortDir === 'asc' ? 'desc' : 'asc';
    } else {
      sortCol = col;
      sortDir = 'asc';
    }
  }

  function sortArrow(col) {
    if (sortCol !== col) return '↕';
    return sortDir === 'asc' ? '↑' : '↓';
  }

  // ── Row click → open full-page detail ──────────────────────────────────────
  function handleRowClick(spec) {
    selectedPath = spec.path;
    if (goToEntityDetail) {
      goToEntityDetail('spec', spec.path, { ...spec, repo_id: repoId });
    } else if (openDetailPanel) {
      openDetailPanel({
        type: 'spec',
        id: spec.path,
        data: { ...spec, repo_id: repoId },
      });
    }
  }

  // ── Progress bar helpers ────────────────────────────────────────────────────
  function progressFraction(path) {
    const p = progressMap[path];
    if (!p) return 0;
    // Handle both pre-computed {completed_tasks, total_tasks} and raw {tasks: [...]} shapes
    const total = p.total_tasks ?? (Array.isArray(p.tasks) ? p.tasks.length : 0);
    if (!total) return 0;
    const done = p.completed_tasks ?? (Array.isArray(p.tasks) ? p.tasks.filter(t => t.status === 'done' || t.status === 'completed').length : 0);
    return done / total;
  }

  function progressLabel(path) {
    const p = progressMap[path];
    if (!p) return null;
    const total = p.total_tasks ?? (Array.isArray(p.tasks) ? p.tasks.length : 0);
    const done = p.completed_tasks ?? (Array.isArray(p.tasks) ? p.tasks.filter(t => t.status === 'done' || t.status === 'completed').length : 0);
    if (!total && !done) return null;
    return $t('spec_dashboard.progress_tasks', { values: { done, total } });
  }

  // ── New spec ────────────────────────────────────────────────────────────────
  async function saveNewSpec() {
    if (!repoId || !newSpecPath.trim() || newSpecSaving) return;
    newSpecSaving = true;
    try {
      const result = await api.specsSave(repoId, {
        spec_path: newSpecPath.trim(),
        content: newSpecContent,
        message: `Create ${newSpecPath.trim()} via UI`,
      });
      toastSuccess($t('spec_dashboard.spec_created', { values: { mr_id: result.mr_id } }));
      showNewSpec = false;
      newSpecPath = '';
      newSpecContent = '# New Spec\n\n## Overview\n\n';
      await load();
    } catch (e) {
      toastError($t('spec_dashboard.create_failed', { values: { error: e.message } }));
    } finally {
      newSpecSaving = false;
    }
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────
  function statusColor(s) {
    if (s === 'approved')   return 'success';
    if (s === 'pending')    return 'warning';
    if (s === 'rejected')   return 'danger';
    if (s === 'revoked')    return 'danger';
    if (s === 'draft')      return 'info';
    if (s === 'deprecated') return 'muted';
    return 'muted';
  }

  function statusIcon(s) {
    if (s === 'approved')   return '✓';
    if (s === 'rejected')   return '✗';
    if (s === 'revoked')    return '✗';
    if (s === 'pending')    return '◐';
    if (s === 'draft')      return '✎';
    if (s === 'deprecated') return '✗';
    return '?';
  }

  function relTime(ts) {
    if (!ts) return '—';
    const diff = Date.now() - ts * 1000;
    const secs = Math.floor(diff / 1000);
    if (secs < 60) return $t('common.time_just_now');
    const mins = Math.floor(secs / 60);
    if (mins < 60) return $t('common.time_minutes_ago', { values: { count: mins } });
    const hrs = Math.floor(mins / 60);
    if (hrs < 24) return $t('common.time_hours_ago', { values: { count: hrs } });
    return $t('common.time_days_ago', { values: { count: Math.floor(hrs / 24) } });
  }
</script>

<div class="spec-view">
  <span class="sr-only" aria-live="polite">{loading ? "" : $t('spec_dashboard.loaded')}</span>
  <!-- ── Header ─────────────────────────────────────────────────────────────── -->
  <div class="view-header">
    <div>
      <h1 class="page-title">{$t('spec_dashboard.title')}</h1>
      {#if scope === 'tenant'}
        <p class="page-desc">{$t('spec_dashboard.all_workspace')}</p>
      {:else if scope === 'workspace'}
        <p class="page-desc">{$t('spec_dashboard.in_workspace')}</p>
      {/if}
    </div>
    <div class="header-actions">
      <div class="view-toggle" role="group" aria-label="View mode">
        <button class="view-toggle-btn" class:active={viewMode === 'list'} onclick={() => { viewMode = 'list'; }} title="List view">List</button>
        <button class="view-toggle-btn" class:active={viewMode === 'graph'} onclick={() => { viewMode = 'graph'; if (!specGraph && !specGraphLoading) loadSpecGraph(); }} title="Graph view — shows spec relationships">Graph</button>
      </div>
      {#if scope === 'repo' && repoId}
        <Button variant="primary" onclick={() => (showNewSpec = true)}>{$t('spec_dashboard.new_spec')}</Button>
      {/if}
      <Button variant="secondary" onclick={load}>{$t('spec_dashboard.refresh')}</Button>
    </div>
  </div>

  <!-- ── Filter bar ─────────────────────────────────────────────────────────── -->
  <div class="filter-bar">
    <div class="filter-group" role="group" aria-label={$t('spec_dashboard.filter_by_status')}>
      {#each STATUS_FILTERS as f}
        <button
          class="pill"
          class:active={filterStatus === f}
          onclick={() => (filterStatus = f)}
          aria-pressed={filterStatus === f}
        >
          {$t(`spec_dashboard.filter_${f}`)}
        </button>
      {/each}
    </div>

    {#if allKinds.length > 2}
      <div class="filter-group" role="group" aria-label={$t('spec_dashboard.filter_by_kind')}>
        <span class="filter-label">{$t('spec_dashboard.filter_kind')}</span>
        {#each allKinds as k}
          <button
            class="pill"
            class:active={filterKind === k}
            onclick={() => (filterKind = k)}
            aria-pressed={filterKind === k}
          >
            {k === 'all' ? $t('spec_dashboard.filter_all') : k.charAt(0).toUpperCase() + k.slice(1)}
          </button>
        {/each}
      </div>
    {/if}

    <label class="owner-toggle">
      <input
        type="checkbox"
        bind:checked={ownerMe}
        aria-label={$t('spec_dashboard.filter_owner_me')}
      />
      <span class="owner-toggle-label">{$t('spec_dashboard.filter_owner_me')}</span>
    </label>

    <input
      class="search-input"
      type="search"
      placeholder={$t('spec_dashboard.search_placeholder')}
      bind:value={searchQuery}
      aria-label={$t('spec_dashboard.search_placeholder')}
    />
  </div>

  <!-- ── Spec Relationship Graph (DAG) ─────────────────────────────────────── -->
  {#if viewMode === 'graph'}
    <div class="spec-graph-view" data-testid="spec-graph-view">
      {#if specGraphLoading}
        <div class="skeleton-rows">
          {#each Array(4) as _}<Skeleton width="100%" height="2.5rem" />{/each}
        </div>
      {:else if specGraph}
        <div class="graph-toolbar">
          <div class="spec-graph-info">
            <span class="graph-stat">{graphNodes.length} spec{graphNodes.length !== 1 ? 's' : ''}</span>
            <span class="graph-stat">{graphEdges.length} relationship{graphEdges.length !== 1 ? 's' : ''}</span>
          </div>
          <div class="graph-toolbar-actions">
            {#if impactMode}
              <button
                class="pill active impact-exit-btn"
                onclick={exitImpactMode}
                data-testid="exit-impact-mode"
                title="Exit impact analysis mode"
              >Exit Impact Analysis</button>
            {:else}
              <button
                class="pill impact-btn"
                onclick={() => { impactMode = true; }}
                data-testid="analyze-impact-btn"
                title="Analyze impact — click a spec to see all transitive dependents"
              >Analyze Impact</button>
            {/if}
            <div class="graph-scope-toggle" role="group" aria-label="Graph scope">
              <button
                class="pill"
                class:active={graphScope === 'workspace'}
                onclick={() => { graphScope = 'workspace'; }}
                aria-pressed={graphScope === 'workspace'}
                data-testid="scope-workspace"
              >Workspace</button>
              <button
                class="pill"
                class:active={graphScope === 'tenant'}
                onclick={() => { graphScope = 'tenant'; }}
                aria-pressed={graphScope === 'tenant'}
                data-testid="scope-tenant"
              >Tenant</button>
            </div>
          </div>
        </div>
        <!-- Impact analysis prompt -->
        {#if impactMode && !impactPath}
          <div class="impact-prompt" data-testid="impact-prompt">
            Click a spec node to analyze its impact
          </div>
        {/if}
        <!-- Impact analysis summary panel -->
        {#if impactPath}
          <div class="impact-summary" data-testid="impact-summary">
            <div class="impact-summary-header">
              <span class="impact-summary-title" data-testid="impact-summary-title">
                {impactDependentsList.length} spec{impactDependentsList.length !== 1 ? 's' : ''} across {impactRepoCount} repo{impactRepoCount !== 1 ? 's' : ''} need review
              </span>
              <button class="impact-clear-btn" onclick={() => { impactPath = null; }} title="Clear selection" data-testid="impact-clear">Clear</button>
            </div>
            {#if impactDependentsList.length > 0}
              <div class="impact-details" data-testid="impact-details">
                {#each impactByRepo as { repo, deps }}
                  <div class="impact-repo-group">
                    <span class="impact-repo-name">{repo}</span>
                    {#each deps as dep}
                      <div class="impact-dep-item" data-testid="impact-dep-{dep.path}">
                        <span class="impact-dep-path mono">{dep.path.split('/').pop()?.replace(/\.md$/, '')}</span>
                        <Badge value={dep.link_type?.replace(/_/g, ' ') ?? 'related'} variant="info" />
                        <Badge value={dep.approval_status} variant={statusColor(dep.approval_status)} />
                      </div>
                    {/each}
                  </div>
                {/each}
              </div>
            {:else}
              <p class="impact-empty">No specs depend on this spec.</p>
            {/if}
          </div>
        {/if}
        {#if graphNodes.length === 0}
          <EmptyState title="No spec relationships" description="Spec relationships appear when specs reference each other via manifest links (implements, conflicts, extends)." />
        {:else}
          <SpecGraphDAG
            nodes={graphNodes}
            edges={graphEdges}
            onNodeClick={handleGraphNodeClick}
            {impactMode}
            {impactPath}
            onImpactSelect={handleImpactSelect}
          />
        {/if}
      {:else}
        <EmptyState title="Load spec graph" description="Click the Graph button to visualize spec relationships." />
      {/if}
    </div>
  {/if}

  <!-- ── Content area ───────────────────────────────────────────────────────── -->
  <div class="content-area" class:hidden-view={viewMode === 'graph'} aria-busy={loading}>
    {#if loading}
      <div class="skeleton-rows">
        {#each Array(7) as _}
          <Skeleton width="100%" height="2.5rem" />
        {/each}
      </div>

    {:else if error}
      <div class="error-banner" role="alert">
        <span>{error}</span>
        <button onclick={load} class="retry-btn">{$t('common.retry')}</button>
      </div>

    {:else if filtered.length === 0}
      <EmptyState
        title={$t('spec_dashboard.no_specs')}
        description={filterStatus === 'all' && filterKind === 'all'
          ? $t('spec_dashboard.no_specs_registered')
          : $t('spec_dashboard.no_specs_filter')}
      />
      {#if filterStatus !== 'all' || filterKind !== 'all' || ownerMe || searchQuery.trim()}
        <div class="clear-filters-wrap">
          <button class="clear-filters-btn" onclick={() => { filterStatus = 'all'; filterKind = 'all'; ownerMe = false; searchQuery = ''; }}>{$t('spec_dashboard.clear_filters')}</button>
        </div>
      {/if}

    {:else if scope === 'repo'}
      <!-- Repo scope: sortable table with progress -->
      <table class="specs-table repo-specs-table" role="grid" aria-label={$t('spec_dashboard.title')}>
        <thead>
          <tr>
            <th scope="col" aria-sort={sortCol === 'path' ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSort('path')}>
                {$t('spec_dashboard.col_path')}
                <span class="sort-arrow" aria-hidden="true">{sortArrow('path')}</span>
              </button>
            </th>
            <th scope="col" aria-sort={sortCol === 'approval_status' ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSort('approval_status')}>
                {$t('spec_dashboard.col_status')}
                <span class="sort-arrow" aria-hidden="true">{sortArrow('approval_status')}</span>
              </button>
            </th>
            <th scope="col" aria-sort={sortCol === 'kind' ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSort('kind')}>
                {$t('spec_dashboard.col_kind')}
                <span class="sort-arrow" aria-hidden="true">{sortArrow('kind')}</span>
              </button>
            </th>
            <th scope="col" aria-sort={sortCol === 'progress' ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSort('progress')}>
                {$t('spec_dashboard.col_progress')}
                <span class="sort-arrow" aria-hidden="true">{sortArrow('progress')}</span>
              </button>
            </th>
            <th scope="col" aria-sort={sortCol === 'updated_at' ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
              <button class="sort-btn" onclick={() => toggleSort('updated_at')}>
                {$t('spec_dashboard.col_updated')}
                <span class="sort-arrow" aria-hidden="true">{sortArrow('updated_at')}</span>
              </button>
            </th>
            <th scope="col" class="col-actions"></th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as spec (spec.path)}
            {@const pct = Math.round(progressFraction(spec.path) * 100)}
            {@const label = progressLabel(spec.path)}
            <tr
              class:selected={selectedPath === spec.path}
              onclick={() => handleRowClick(spec)}
              tabindex="0"
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleRowClick(spec); }
                if (e.key === 'ArrowDown') { e.preventDefault(); const next = e.currentTarget.nextElementSibling; if (next) next.focus(); }
                if (e.key === 'ArrowUp') { e.preventDefault(); const prev = e.currentTarget.previousElementSibling; if (prev) prev.focus(); }
              }}
              aria-selected={selectedPath === spec.path}
              aria-label="{$t('spec_dashboard.title')}: {spec.path}"
            >
              <td class="col-path">
                <span class="spec-path" title={spec.path}>{#if spec.path?.includes('/')}<span class="spec-dir">{spec.path.slice(0, spec.path.lastIndexOf('/') + 1)}</span>{/if}{spec.path?.split('/').pop()?.replace(/\.md$/, '') ?? spec.path}</span>
              </td>
              <td>
                <Badge
                  value={spec.approval_status ?? 'unknown'}
                  variant={statusColor(spec.approval_status)}
                  title={specStatusTooltip(spec.approval_status)}
                />
              </td>
              <td class="col-kind">{spec.kind || '—'}</td>
              <td class="col-progress">
                {#if label}
                  <div class="progress-cell">
                    <span class="progress-label-text">{label}</span>
                    <div
                      class="progress-bar-wrap"
                      title="{pct}%"
                      role="progressbar"
                      aria-valuenow={pct}
                      aria-valuemin="0"
                      aria-valuemax="100"
                      aria-label="{$t('spec_dashboard.col_progress')}: {pct}%"
                    >
                      <div class="progress-bar">
                        <div class="progress-fill" style="width: {pct}%"></div>
                      </div>
                    </div>
                  </div>
                {:else}
                  <span class="col-time">—</span>
                {/if}
              </td>
              <td class="col-time">{relTime(spec.updated_at)}</td>
              <td class="col-actions">
                {#if spec.approval_status === 'pending' && spec.current_sha}
                  <button class="spec-action-btn spec-action-approve" onclick={(e) => quickApprove(spec, e)} disabled={approvingSpec === spec.path} title="Approve this spec — agents can begin implementation">
                    {approvingSpec === spec.path ? '...' : 'Approve'}
                  </button>
                  <button class="spec-action-btn spec-action-reject" onclick={(e) => quickReject(spec, e)} disabled={rejectingSpec === spec.path} title="Reject this spec — blocks further work">
                    {rejectingSpec === spec.path ? '...' : 'Reject'}
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>

    {:else}
      <!-- Workspace / tenant scope: sortable table -->
      <table class="specs-table" role="grid" aria-label={$t('spec_dashboard.specs_registry')}>
        <thead>
          <tr>
            {#each TABLE_COLS as [col, labelKey]}
              <th scope="col" aria-sort={sortCol === col ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none'}>
                <button class="sort-btn" onclick={() => toggleSort(col)}>
                  {$t(labelKey)}
                  <span class="sort-arrow" aria-hidden="true">{sortArrow(col)}</span>
                </button>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each filtered as spec (spec.path)}
            <tr
              class:selected={selectedPath === spec.path}
              onclick={() => handleRowClick(spec)}
              tabindex="0"
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleRowClick(spec); }
                if (e.key === 'ArrowDown') { e.preventDefault(); const next = e.currentTarget.nextElementSibling; if (next) next.focus(); }
                if (e.key === 'ArrowUp') { e.preventDefault(); const prev = e.currentTarget.previousElementSibling; if (prev) prev.focus(); }
              }}
              aria-selected={selectedPath === spec.path}
              aria-label={$t('spec_dashboard.spec_label', { values: { path: spec.path } })}
            >
              <td class="col-path">
                <span class="spec-path" title={spec.path}>{#if spec.path?.includes('/')}<span class="spec-dir">{spec.path.slice(0, spec.path.lastIndexOf('/') + 1)}</span>{/if}{spec.path?.split('/').pop()?.replace(/\.md$/, '') ?? spec.path}</span>
              </td>
              <td>
                <Badge
                  value={spec.approval_status ?? 'unknown'}
                  variant={statusColor(spec.approval_status)}
                  title={specStatusTooltip(spec.approval_status)}
                />
              </td>
              <td class="col-kind">{spec.kind || '—'}</td>
              <td class="col-owner">{spec.owner || '—'}</td>
              <td class="col-time">{relTime(spec.updated_at)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>

<!-- ── New Spec modal (Editor Split layout per ui-layout.md §2) ────────────── -->
<Modal bind:open={showNewSpec} title={$t('spec_dashboard.new_spec_title')} size="lg">
  <div class="new-spec-body">
    <!-- Left: editor -->
    <div class="editor-pane">
      <label class="field-label" for="new-spec-path">{$t('spec_dashboard.spec_path_label')}</label>
      <input
        id="new-spec-path"
        class="field-input mono"
        type="text"
        bind:value={newSpecPath}
        placeholder={$t('spec_dashboard.spec_path_placeholder')}
        aria-required="true"
        aria-invalid={pathTouched && !newSpecPath.trim() ? 'true' : 'false'}
        aria-describedby={pathTouched && !newSpecPath.trim() ? 'path-error' : undefined}
        onblur={() => { pathTouched = true; }}
      />
      {#if pathTouched && !newSpecPath.trim()}
        <span id="path-error" role="alert" style="color: var(--color-danger); font-size: var(--text-xs);">{$t('spec_dashboard.path_required')}</span>
      {/if}
      <label class="field-label" for="new-spec-content">{$t('spec_dashboard.spec_content_label')}</label>
      <textarea
        id="new-spec-content"
        class="spec-editor"
        bind:value={newSpecContent}
        spellcheck="false"
      ></textarea>
    </div>
    <!-- Right: preview -->
    <div class="preview-pane">
      <span class="preview-label">{$t('spec_dashboard.markdown_source')}</span>
      <pre class="preview-pre">{newSpecContent}</pre>
    </div>
  </div>
  <div class="modal-footer">
    <Button variant="secondary" onclick={() => { showNewSpec = false; }}>{$t('common.cancel')}</Button>
    <Button
      variant="primary"
      onclick={saveNewSpec}
      disabled={newSpecSaving || !newSpecPath.trim()}
      aria-busy={newSpecSaving}
    >
      {newSpecSaving ? $t('spec_dashboard.saving') : $t('spec_dashboard.save_create_mr')}
    </Button>
  </div>
</Modal>

<style>
  /* ── Page ────────────────────────────────────────────────────────────────── */
  .spec-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    padding: var(--space-6);
    gap: var(--space-4);
  }

  /* ── Header ──────────────────────────────────────────────────────────────── */
  .view-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    flex-shrink: 0;
  }

  .page-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .page-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
    text-transform: capitalize;
  }

  .header-actions {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  /* ── Filter bar ──────────────────────────────────────────────────────────── */
  .filter-bar {
    display: flex;
    gap: var(--space-4);
    flex-shrink: 0;
    flex-wrap: wrap;
    align-items: center;
  }

  .filter-group {
    display: flex;
    gap: var(--space-1);
    align-items: center;
  }

  .filter-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .pill {
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast),
      border-color var(--transition-fast);
  }

  .pill:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .pill:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .pill.active {
    background: color-mix(in srgb, var(--color-link) 12%, transparent);
    border-color: var(--color-link);
    color: var(--color-link);
    font-weight: 500;
  }

  .owner-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    cursor: pointer;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    white-space: nowrap;
  }

  .owner-toggle input[type="checkbox"] {
    accent-color: var(--color-link);
    cursor: pointer;
  }

  .owner-toggle-label {
    user-select: none;
  }

  .search-input {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    min-width: 160px;
    transition: border-color var(--transition-fast);
  }

  .search-input:focus:not(:focus-visible) {
    outline: none;
  }

  .search-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-color: var(--color-focus);
  }

  .search-input::placeholder {
    color: var(--color-text-muted);
  }

  /* ── Content area ────────────────────────────────────────────────────────── */
  .content-area {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }

  .skeleton-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  /* ── Sortable table (workspace/tenant scope) ─────────────────────────────── */
  .specs-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .specs-table th {
    text-align: left;
    padding: 0;
    border-bottom: 1px solid var(--color-border);
  }

  .sort-btn {
    width: 100%;
    text-align: left;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    transition: color var(--transition-fast);
  }

  .sort-btn:hover { color: var(--color-text); }

  .sort-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .sort-arrow {
    font-size: var(--text-xs);
    opacity: 0.6;
  }

  .specs-table td {
    padding: var(--space-3);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
    vertical-align: middle;
  }

  .specs-table tr {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .specs-table tbody tr:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .specs-table tr:hover td {
    background: var(--color-surface-elevated);
  }

  .specs-table tr.selected td {
    background: color-mix(in srgb, var(--color-link) 6%, transparent);
  }

  .col-path { max-width: 240px; }

  .spec-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: block;
  }

  .spec-dir {
    color: var(--color-text-muted);
    font-weight: 400;
  }

  .col-kind,
  .col-owner {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .col-actions {
    width: 1%;
    white-space: nowrap;
    text-align: right;
  }

  .spec-action-btn {
    padding: 2px var(--space-2);
    border: none;
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-weight: 600;
    cursor: pointer;
    font-family: var(--font-body);
    transition: opacity var(--transition-fast);
    white-space: nowrap;
  }

  .spec-action-btn:hover { opacity: 0.85; }
  .spec-action-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .spec-action-approve {
    background: var(--color-success);
    color: white;
    margin-right: var(--space-1);
  }

  .spec-action-reject {
    background: var(--color-danger);
    color: white;
  }

  /* ── Repo progress list ──────────────────────────────────────────────────── */
  .spec-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
  }

  .spec-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    cursor: pointer;
    transition: background var(--transition-fast);
    flex-wrap: wrap;
  }

  .spec-row:hover { background: var(--color-surface-elevated); }

  .spec-row.selected { background: color-mix(in srgb, var(--color-link) 6%, transparent); }

  .spec-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .spec-row .spec-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spec-status-inline {
    font-size: var(--text-xs);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .spec-status-inline.success { color: var(--color-success); }
  .spec-status-inline.warning { color: var(--color-warning); }
  .spec-status-inline.neutral { color: var(--color-text-muted); }

  .progress-label-text {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    flex-shrink: 0;
    min-width: 70px;
    text-align: right;
  }

  .progress-cell {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .col-progress {
    min-width: 150px;
  }

  .progress-bar-wrap {
    flex-shrink: 0;
    width: 80px;
  }

  .progress-bar {
    height: 6px;
    background: var(--color-border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--color-success);
    border-radius: var(--radius-sm);
    transition: width var(--transition-slow);
  }

  /* ── New spec modal body ─────────────────────────────────────────────────── */
  .new-spec-body {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
    min-height: 360px;
    overflow: hidden;
  }

  .editor-pane,
  .preview-pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    overflow: hidden;
  }

  .preview-pane {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
  }

  .preview-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .preview-pre {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--color-text);
    overflow-y: auto;
    flex: 1;
  }

  .spec-editor {
    flex: 1;
    min-height: 280px;
    padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.6;
    resize: none;
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .spec-editor:focus:not(:focus-visible) {
    outline: none;
  }

  .spec-editor:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    border-color: var(--color-focus);
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }

  /* ── Shared ──────────────────────────────────────────────────────────────── */
  .field-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .field-input {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .field-input.mono { font-family: var(--font-mono); }

  .field-input:focus:not(:focus-visible) {
    outline: none;
  }

  .field-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    border-color: var(--color-focus);
  }

  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }

  /* ── Clear filters ───────────────────────────────────────────────────── */
  .clear-filters-wrap {
    display: flex;
    justify-content: center;
    margin-top: var(--space-3);
  }

  .clear-filters-btn {
    padding: var(--space-1) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .clear-filters-btn:hover {
    border-color: var(--color-text-muted);
    color: var(--color-text);
  }

  .clear-filters-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Error banner ─────────────────────────────────────────────────────── */
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
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-link) 25%, transparent);
    border-color: var(--color-link);
  }

  .retry-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .pill,
    .sort-btn,
    .specs-table tr,
    .spec-row,
    .progress-fill,
    .spec-editor,
    .field-input,
    .clear-filters-btn,
    .retry-btn {
      transition: none;
      animation: none;
    }
  }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  /* ── View toggle ──────────────────────────────────────────────────── */
  .view-toggle { display: flex; border: 1px solid var(--color-border-strong); border-radius: var(--radius); overflow: hidden; }
  .view-toggle-btn {
    background: var(--color-surface);
    border: none;
    padding: var(--space-1) var(--space-3);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
    color: var(--color-text-secondary);
    transition: background var(--transition-fast), color var(--transition-fast);
  }
  .view-toggle-btn:not(:last-child) { border-right: 1px solid var(--color-border); }
  .view-toggle-btn.active { background: var(--color-primary); color: white; }
  .view-toggle-btn:hover:not(.active) { background: var(--color-surface-elevated); }
  .hidden-view { display: none !important; }

  /* ── Spec graph ───────────────────────────────────────────────────── */
  .spec-graph-view { flex: 1; overflow: hidden; display: flex; flex-direction: column; }
  .graph-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-2) 0;
    flex-shrink: 0;
  }
  .spec-graph-info { display: flex; gap: var(--space-4); }
  .graph-stat { font-size: var(--text-sm); color: var(--color-text-secondary); font-weight: 600; }
  .graph-toolbar-actions { display: flex; gap: var(--space-2); align-items: center; }
  .graph-scope-toggle { display: flex; gap: var(--space-1); }

  /* ── Impact analysis ────────────────────────────────────────────── */
  .impact-btn {
    background: color-mix(in srgb, #f59e0b 12%, transparent) !important;
    border-color: #f59e0b !important;
    color: #f59e0b !important;
  }
  .impact-btn:hover { background: color-mix(in srgb, #f59e0b 20%, transparent) !important; }
  .impact-exit-btn {
    background: color-mix(in srgb, #f59e0b 20%, transparent) !important;
    border-color: #f59e0b !important;
    color: #f59e0b !important;
  }
  .impact-prompt {
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, #f59e0b 8%, transparent);
    border: 1px solid color-mix(in srgb, #f59e0b 30%, transparent);
    border-radius: var(--radius);
    color: #f59e0b;
    font-size: var(--text-sm);
    text-align: center;
    flex-shrink: 0;
  }
  .impact-summary {
    padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    flex-shrink: 0;
    max-height: 200px;
    overflow-y: auto;
  }
  .impact-summary-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }
  .impact-summary-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }
  .impact-clear-btn {
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    cursor: pointer;
    font-family: var(--font-body);
  }
  .impact-clear-btn:hover { color: var(--color-text); border-color: var(--color-text-muted); }
  .impact-details {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .impact-repo-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .impact-repo-name {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding-bottom: var(--space-1);
    border-bottom: 1px solid var(--color-border);
  }
  .impact-dep-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) 0;
  }
  .impact-dep-path {
    font-size: var(--text-xs);
    color: var(--color-text);
    flex: 1;
  }
  .impact-empty {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }
</style>
