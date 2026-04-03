<script>
  import { getContext } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { entityName, shortId } from '../lib/entityNames.svelte.js';
  import { detectLang, highlightLine } from '../lib/syntaxHighlight.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let { repoId = null, repo = null } = $props();

  const openDetailPanel = getContext('openDetailPanel');
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;
  const getScope = getContext('getScope') ?? (() => ({}));

  let subTab = $state('files');
  const SUB_TABS = [
    { id: 'files', label: 'Files' },
    { id: 'commits', labelKey: 'code_tab.commits' },
    { id: 'branches', labelKey: 'code_tab.branches' },
    { id: 'hot-files', label: 'Hot Files' },
    { id: 'provenance', label: 'Provenance' },
  ];

  // Clone URL copy state
  let cloneCopied = $state(false);
  let cloneUrl = $derived(repo?.clone_url ?? '');
  let copyTimer = null;

  async function copyCloneUrl() {
    if (!cloneUrl) return;
    try {
      await navigator.clipboard.writeText(cloneUrl);
      cloneCopied = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => { cloneCopied = false; copyTimer = null; }, 2000);
    } catch {
      // clipboard not available — silently fail
    }
  }

  $effect(() => {
    return () => { if (copyTimer) clearTimeout(copyTimer); };
  });

  // Per-tab data
  let branches = $state([]);
  let commits = $state([]);
  let mrs = $state([]);
  let queue = $state([]);
  let tasks = $state([]);
  let agents = $state([]);
  let agentCommits = $state({});
  let hotFiles = $state([]);
  let aibomEntries = $state([]);
  let agentCommitRecords = $state([]);
  // Files sub-tab state
  let fileTree = $state([]);
  let selectedFile = $state(null);
  let blameData = $state(null);
  let blameLoading = $state(false);
  let reviewRouting = $state([]);
  let fileViewMode = $state('code'); // 'code' | 'blame'
  let loading = $state(true);
  let error = $state(null);
  let filterQuery = $state('');
  let investigateLoading = $state(null); // commit SHA being investigated
  let fileLang = $derived(selectedFile ? detectLang(selectedFile) : 'text');

  // Inline blame popover state (code view click-to-reveal)
  let popoverLineIdx = $state(null);  // index into blame lines array
  let popoverEl = $state(null);       // DOM ref for positioning

  // Agent color assignment for attribution markers
  // Uses a plain Map (not $state) to avoid state_unsafe_mutation when called from templates.
  const AGENT_COLORS = ['#c678dd','#61afef','#e5c07b','#56b6c2','#e06c75','#98c379','#d19a66','#be5046'];
  const agentColorMap = new Map();
  function agentColor(agentId) {
    if (!agentId) return 'transparent';
    if (agentColorMap.has(agentId)) return agentColorMap.get(agentId);
    const color = AGENT_COLORS[agentColorMap.size % AGENT_COLORS.length];
    agentColorMap.set(agentId, color);
    return color;
  }

  /**
   * Group consecutive blame lines by same agent+sha into visual blocks (GitHub blame style).
   * Returns array of { agentId, sha, specRef, startIdx, lines[] }.
   */
  function computeBlameGroups(lines) {
    const groups = [];
    let current = null;
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const agentId = line.agent_id ?? line.agent ?? null;
      const sha = line.sha ?? line.commit_sha ?? null;
      const specRef = line.spec_ref ?? line.spec_path ?? null;
      const key = `${agentId}:${sha}`;
      if (current && current._key === key) {
        current.lines.push(line);
      } else {
        current = { agentId, sha, specRef, startIdx: i, lines: [line], _key: key };
        groups.push(current);
      }
    }
    return groups;
  }

  // Sort state
  let sortField = $state('name');
  let sortDir = $state('asc');

  // Read initial sub-tab and file from URL params (set by goToRepoTab context)
  let initialFileToSelect = null;
  $effect(() => {
    const params = new URLSearchParams(window.location.search);
    const initialSubTab = params.get('subTab');
    const initialFile = params.get('file');
    if (initialSubTab && SUB_TABS.some(st => st.id === initialSubTab)) {
      subTab = initialSubTab;
      if (initialFile && initialSubTab === 'files') {
        initialFileToSelect = initialFile;
      }
      // Clean up the URL params after reading
      const url = new URL(window.location.href);
      url.searchParams.delete('subTab');
      url.searchParams.delete('file');
      window.history.replaceState(window.history.state, '', url.toString());
    }
    if (repoId) loadTab(subTab);
  });

  // After files tab loads, auto-select the requested file
  $effect(() => {
    if (initialFileToSelect && !loading && subTab === 'files' && fileTree.length > 0) {
      const file = initialFileToSelect;
      initialFileToSelect = null;
      selectFile(file);
    }
  });

  async function loadTab(tab) {
    if (!repoId) return;
    loading = true;
    error = null;
    filterQuery = '';
    try {
      if (tab === 'branches') {
        branches = await api.repoBranches(repoId);
      } else if (tab === 'commits') {
        const branch = repo?.default_branch ?? 'main';
        const [commitList, agentCommitList] = await Promise.all([
          api.repoCommits(repoId, branch, 50),
          api.repoAgentCommits(repoId).catch(() => []),
        ]);
        commits = commitList;
        const acMap = {};
        for (const ac of (Array.isArray(agentCommitList) ? agentCommitList : [])) {
          if (ac.sha) acMap[ac.sha] = ac.agent_id;
          if (ac.commit_sha) acMap[ac.commit_sha] = ac.agent_id;
        }
        agentCommits = acMap;
      } else if (tab === 'merge-requests') {
        const mrList = await api.mergeRequests({ repository_id: repoId });
        // Enrich MRs with gate results summary
        mrs = Array.isArray(mrList) ? mrList : [];
        // Load gate results for each MR in parallel (best-effort, API enriches names)
        const gatePromises = mrs.map(mr =>
          api.mrGates(mr.id).then(gates => {
            const arr = Array.isArray(gates) ? gates : (gates?.gates ?? []);
            const passed = arr.filter(g => g.status === 'Passed' || g.status === 'passed').length;
            const failed = arr.filter(g => g.status === 'Failed' || g.status === 'failed').length;
            const total = arr.length;
            const details = arr.map(g => {
              const gateType = (g.gate_type ?? '').replace(/_/g, ' ');
              return {
                name: g.gate_name ?? g.name ?? (gateType || 'Quality gate'),
                status: (g.status === 'Passed' || g.status === 'passed') ? 'passed' : (g.status === 'Failed' || g.status === 'failed') ? 'failed' : 'pending',
                gate_type: g.gate_type,
                required: g.required,
              };
            });
            return { id: mr.id, passed, failed, total, details };
          }).catch(() => ({ id: mr.id, passed: 0, failed: 0, total: 0, details: [] }))
        );
        const gateResults = await Promise.all(gatePromises);
        const gateMap = Object.fromEntries(gateResults.map(g => [g.id, g]));
        mrs = mrs.map(mr => ({ ...mr, _gates: gateMap[mr.id] }));
      } else if (tab === 'merge-queue') {
        const [all, mrList, specMerges] = await Promise.all([
          api.mergeQueue(),
          api.mergeRequests({ repository_id: repoId }),
          api.repoSpeculative(repoId).catch(() => []),
        ]);
        const mrMap = Object.fromEntries((Array.isArray(mrList) ? mrList : []).map(m => [m.id, m]));
        const specMap = new Map();
        for (const sm of (Array.isArray(specMerges) ? specMerges : [])) {
          if (sm.branch) specMap.set(sm.branch, sm);
        }
        queue = (Array.isArray(all) ? all : [])
          .filter(e => e.repository_id === repoId || e.repo_id === repoId)
          .map(e => {
            const mrId = e.merge_request_id ?? e.mr_id;
            const mr = mrMap[mrId];
            const spec = mr?.source_branch ? specMap.get(mr.source_branch) : null;
            return { ...e, _mr_title: mr?.title, _mr_status: mr?.status, _mr_branch: mr?.source_branch, _speculative: spec };
          });
      } else if (tab === 'tasks') {
        const all = await api.tasks({ repoId });
        // Client-side filter: only show tasks explicitly linked to this repo
        tasks = (Array.isArray(all) ? all : []).filter(t => t.repo_id === repoId);
      } else if (tab === 'files') {
        // Build file tree from hot-files + agent-commits + MR diffs + blame data
        const [hf, acList, mrList] = await Promise.all([
          api.repoHotFiles(repoId, 100).catch(() => []),
          api.repoAgentCommits(repoId).catch(() => []),
          api.mergeRequests({ repository_id: repoId }).catch(() => []),
        ]);
        const hotMap = new Map();
        for (const f of (Array.isArray(hf) ? hf : [])) {
          const p = f.path ?? f.file;
          if (p) hotMap.set(p, f);
        }
        // Collect unique file paths from agent commits
        const pathSet = new Set(hotMap.keys());
        for (const ac of (Array.isArray(acList) ? acList : [])) {
          if (ac.files) ac.files.forEach(f => pathSet.add(f));
          if (ac.path) pathSet.add(ac.path);
        }
        // Always collect file paths from MR diffs — most reliable source
        const mrArr = Array.isArray(mrList) ? mrList : [];
        if (mrArr.length > 0) {
          const diffPromises = mrArr.slice(0, 10).map(mr =>
            api.mrDiff(mr.id).then(d => d?.files ?? []).catch(() => [])
          );
          const diffResults = await Promise.all(diffPromises);
          for (const files of diffResults) {
            for (const f of files) {
              const p = f.path ?? f.file;
              if (p) pathSet.add(p);
            }
          }
        }
        // Build flat file list with hot-file metadata
        fileTree = [...pathSet].sort().map(p => {
          const hot = hotMap.get(p);
          return {
            path: p,
            change_count: hot?.change_count ?? hot?.commits ?? 0,
            author_count: hot?.author_count ?? hot?.authors ?? 0,
            last_modified: hot?.last_modified ?? hot?.updated_at,
          };
        });
        selectedFile = null;
        blameData = null;
      } else if (tab === 'hot-files') {
        hotFiles = await api.repoHotFiles(repoId, 30).catch(() => []);
        if (!Array.isArray(hotFiles)) hotFiles = [];
      } else if (tab === 'provenance') {
        const [aibom, acList] = await Promise.all([
          api.repoAibom(repoId).catch(() => []),
          api.repoAgentCommits(repoId).catch(() => []),
        ]);
        aibomEntries = Array.isArray(aibom) ? aibom : [];
        agentCommitRecords = Array.isArray(acList) ? acList : [];
      } else if (tab === 'agents') {
        agents = await api.agents({ repoId });
        if (!Array.isArray(agents)) agents = [];
      }
    } catch (e) {
      error = $t('code_tab.load_failed', { values: { tab, error: e.message } });
    } finally {
      loading = false;
    }
  }

  function switchSubTab(id) {
    subTab = id;
    loadTab(id);
  }

  function onRowClick(row, type) {
    if (goToEntityDetail) {
      goToEntityDetail(type, row.id, row);
    } else {
      openDetailPanel?.({ type, id: row.id, data: row });
    }
  }

  // Use shared entity name resolution (global singleton cache)
  function resolveEntityName(type, id) {
    return entityName(type, id);
  }

  async function investigateLine(line) {
    const sha = line.sha ?? line.commit_sha;
    const agentId = line.agent_id ?? line.agent;
    if (!sha || !repoId) return;
    investigateLoading = sha;
    try {
      // Look up the task from the original agent to get context
      let taskId = null;
      if (agentId) {
        try {
          const ag = await api.agent(agentId);
          taskId = ag?.task_id ?? ag?.current_task_id;
        } catch { /* best effort */ }
      }
      if (!taskId) {
        // Create a lightweight investigation task
        const scope = getScope();
        const task = await api.createTask({
          title: `Investigate ${selectedFile}:${line.line_number ?? '?'}`,
          description: `Investigation of code at ${selectedFile} line ${line.line_number ?? '?'}, commit ${sha.slice(0, 7)}`,
          task_type: 'investigation',
          repo_id: repoId,
          workspace_id: scope.workspaceId,
        });
        taskId = task.id;
      }
      const result = await api.spawnAgent({
        name: `investigate-${sha.slice(0, 8)}`,
        repo_id: repoId,
        task_id: taskId,
        branch: `investigate/${sha.slice(0, 8)}`,
        agent_type: 'interrogation',
        conversation_sha: line.conversation_sha ?? sha,
      });
      const newAgentId = result?.agent?.id;
      if (newAgentId) {
        showToast('Investigation agent spawned', { type: 'success' });
        if (goToEntityDetail) goToEntityDetail('agent', newAgentId, result.agent);
        else openDetailPanel?.({ type: 'agent', id: newAgentId, data: result.agent });
      }
    } catch (e) {
      showToast(`Failed to spawn: ${e?.message ?? 'Unknown error'}`, { type: 'error' });
    } finally {
      investigateLoading = null;
    }
  }

  function toggleSort(field) {
    if (sortField === field) {
      sortDir = sortDir === 'asc' ? 'desc' : 'asc';
    } else {
      sortField = field;
      sortDir = 'asc';
    }
  }

  function sortIcon(field) {
    if (sortField !== field) return '↕';
    return sortDir === 'asc' ? '↑' : '↓';
  }

  function matchesFilter(row) {
    if (!filterQuery.trim()) return true;
    const q = filterQuery.toLowerCase();
    const str = Object.values(row).filter(v => typeof v === 'string').join(' ').toLowerCase();
    return str.includes(q);
  }

  let filteredBranches = $derived.by(() => {
    let rows = branches.filter(matchesFilter);
    rows.sort((a, b) => {
      const av = a[sortField] ?? '';
      const bv = b[sortField] ?? '';
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  let filteredMrs = $derived.by(() => {
    let rows = mrs.filter(matchesFilter);
    rows.sort((a, b) => {
      const av = a[sortField] ?? '';
      const bv = b[sortField] ?? '';
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  let filteredCommits = $derived.by(() => {
    let rows = commits.filter(matchesFilter);
    rows.sort((a, b) => {
      let av, bv;
      if (sortField === 'sha') { av = a.sha ?? a.id ?? ''; bv = b.sha ?? b.id ?? ''; }
      else if (sortField === 'message') { av = a.message ?? a.summary ?? ''; bv = b.message ?? b.summary ?? ''; }
      else if (sortField === 'author') { av = a.author ?? a.author_name ?? ''; bv = b.author ?? b.author_name ?? ''; }
      else if (sortField === 'date') { av = a.timestamp ?? a.authored_at ?? a.date ?? ''; bv = b.timestamp ?? b.authored_at ?? b.date ?? ''; }
      else { av = a[sortField] ?? ''; bv = b[sortField] ?? ''; }
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  let filteredTasks = $derived.by(() => {
    let rows = tasks.filter(matchesFilter);
    rows.sort((a, b) => {
      let av, bv;
      if (sortField === 'status') { av = a.status ?? ''; bv = b.status ?? ''; }
      else if (sortField === 'priority') {
        const pOrder = { critical: 0, high: 1, medium: 2, low: 3 };
        av = pOrder[a.priority] ?? 2; bv = pOrder[b.priority] ?? 2;
        return sortDir === 'asc' ? av - bv : bv - av;
      }
      else { av = a[sortField] ?? ''; bv = b[sortField] ?? ''; }
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  let filteredAgents = $derived.by(() => {
    let rows = agents.filter(matchesFilter);
    rows.sort((a, b) => {
      const av = a[sortField] ?? '';
      const bv = b[sortField] ?? '';
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  let filteredQueue = $derived.by(() => {
    let rows = queue.filter(matchesFilter);
    rows.sort((a, b) => {
      let av, bv;
      if (sortField === 'mr') { av = a.merge_request_id ?? a.mr_id ?? ''; bv = b.merge_request_id ?? b.mr_id ?? ''; }
      else if (sortField === 'priority') { av = a.priority ?? 0; bv = b.priority ?? 0; return sortDir === 'asc' ? av - bv : bv - av; }
      else { av = a[sortField] ?? ''; bv = b[sortField] ?? ''; }
      return sortDir === 'asc' ? String(av).localeCompare(String(bv)) : String(bv).localeCompare(String(av));
    });
    return rows;
  });

  function closePopover() {
    popoverLineIdx = null;
  }

  function togglePopover(idx, line) {
    const agentId = line.agent_id ?? line.agent;
    const sha = line.sha ?? line.commit_sha;
    if (!agentId && !sha) return; // nothing to show
    if (popoverLineIdx === idx) {
      closePopover();
    } else {
      popoverLineIdx = idx;
    }
  }

  async function selectFile(path) {
    selectedFile = path;
    blameData = null;
    blameLoading = true;
    reviewRouting = [];
    popoverLineIdx = null;
    agentColorMap.clear();
    try {
      const [blame, routing] = await Promise.all([
        api.repoBlame(repoId, path).catch(() => null),
        api.repoReviewRouting(repoId, path).catch(() => []),
      ]);
      blameData = blame;
      reviewRouting = Array.isArray(routing) ? routing : [];
      // Auto-switch to blame view when agent attribution is present —
      // this is Gyre's unique value proposition for code exploration
      const blameLines = Array.isArray(blame) ? blame : (blame?.lines ?? blame?.blame ?? []);
      const hasAgentAttribution = blameLines.some(l => l.agent_id ?? l.agent);
      if (hasAgentAttribution) {
        fileViewMode = 'blame';
      }
    } catch {
      blameData = null;
    } finally {
      blameLoading = false;
    }
  }

  function relativeTime(ts) {
    if (!ts) return '';
    const d = new Date(typeof ts === 'number' ? ts * 1000 : ts);
    const diff = (Date.now() - d.getTime()) / 1000;
    if (diff < 60) return $t('code_tab.time_just_now');
    if (diff < 3600) return $t('code_tab.time_minutes_ago', { values: { count: Math.floor(diff / 60) } });
    if (diff < 86400) return $t('code_tab.time_hours_ago', { values: { count: Math.floor(diff / 3600) } });
    return $t('code_tab.time_days_ago', { values: { count: Math.floor(diff / 86400) } });
  }
</script>

<div class="code-tab">
  <span class="sr-only" aria-live="polite">{loading ? "" : $t('code_tab.loaded')}</span>

  <!-- Clone URL header -->
  {#if cloneUrl}
    <div class="clone-url-bar">
      <span class="clone-label">{$t('code_tab.clone')}</span>
      <code class="clone-url-text" title={cloneUrl}>{cloneUrl}</code>
      <button class="clone-copy-btn" onclick={copyCloneUrl} aria-label={$t('code_tab.copy_clone_url')} title={$t('code_tab.copy_clone_url')}>
        {#if cloneCopied}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><polyline points="20 6 9 17 4 12"/></svg>
          {$t('code_tab.copied')}
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
          {$t('code_tab.copy')}
        {/if}
      </button>
    </div>
  {/if}

  <!-- Sub-tab bar -->
  <div class="subtab-bar" role="tablist" aria-label={$t('code_tab.sub_tabs_label')}>
    {#each SUB_TABS as st}
      <button
        class="subtab-btn {subTab === st.id ? 'active' : ''}"
        role="tab"
        aria-selected={subTab === st.id}
        onclick={() => switchSubTab(st.id)}
        type="button"
      >{st.labelKey ? $t(st.labelKey) : st.label}</button>
    {/each}
  </div>

  <!-- Filter input -->
  <div class="filter-bar">
    <input
      type="search"
      class="filter-input"
      placeholder={$t('code_tab.filter_placeholder', { values: { tab: $t(SUB_TABS.find(st => st.id === subTab)?.labelKey ?? '') } })}
      bind:value={filterQuery}
      aria-label={$t('code_tab.filter_list')}
    />
  </div>

  <!-- Content -->
  <div class="table-wrap" role="tabpanel" aria-busy={loading}>
    {#if error}
      <div class="error-banner" role="alert">
        <span>{error}</span>
        <button class="retry-btn" onclick={() => { error = null; loadTab(subTab); }}>{$t('common.retry')}</button>
      </div>
    {:else if loading}
      <Skeleton lines={6} />
    {:else if subTab === 'branches'}
      {#if filteredBranches.length === 0}
        <EmptyState title={$t('code_tab.no_branches')} message={filterQuery ? $t('code_tab.no_branches_filter') : $t('code_tab.no_branches_empty')} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('name')}>{$t('code_tab.col_name')} {sortIcon('name')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('last_commit')}>{$t('code_tab.col_last_commit')} {sortIcon('last_commit')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('author')}>{$t('code_tab.col_author')} {sortIcon('author')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('status')}>{$t('code_tab.col_status')} {sortIcon('status')}</button></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredBranches as branch}
              <tr class="table-row" onclick={() => onRowClick(branch, 'branch')} tabindex="0" role="button" aria-label="View branch {branch.name}" onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onRowClick(branch, 'branch'); } }}>
                <td class="mono">{branch.name}</td>
                <td class="secondary">{branch.last_commit ? branch.last_commit.slice(0, 7) : '—'}</td>
                <td class="secondary">{branch.author ?? '—'}</td>
                <td><span class="status-badge">{branch.status ?? 'active'}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if subTab === 'commits'}
      {#if filteredCommits.length === 0}
        <EmptyState title={$t('code_tab.no_commits')} message={filterQuery ? $t('code_tab.no_commits_filter') : $t('code_tab.no_commits_empty')} />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('sha')}>{$t('code_tab.col_sha')} {sortIcon('sha')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('message')}>{$t('code_tab.col_message')} {sortIcon('message')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('author')}>{$t('code_tab.col_author')} {sortIcon('author')}</button></th>
              <th scope="col"><button class="sort-btn" onclick={() => toggleSort('date')}>{$t('code_tab.col_date')} {sortIcon('date')}</button></th>
            </tr>
          </thead>
          <tbody>
            {#each filteredCommits as commit}
              {@const commitSha = commit.sha ?? commit.id ?? ''}
              {@const commitAgent = agentCommits[commitSha]}
              <tr class="table-row" onclick={() => onRowClick(commitAgent ? { ...commit, agent_id: commitAgent } : commit, 'commit')} tabindex="0" role="button" aria-label="Commit {commitSha}" onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onRowClick(commitAgent ? { ...commit, agent_id: commitAgent } : commit, 'commit'); } }}>
                <td class="mono">{commitSha.slice(0, 7)}</td>
                <td class="commit-msg" title={commit.message ?? commit.summary ?? ''}>{commit.message ?? commit.summary ?? '—'}</td>
                <td class="secondary">
                  {#if commitAgent}
                    <button class="agent-link" title={commitAgent} onclick={(e) => { e.stopPropagation(); onRowClick({ id: commitAgent }, 'agent'); }}>
                      <span class="agent-icon" aria-hidden="true">&#x2699;</span>
                      {resolveEntityName('agent', commitAgent)}
                    </button>
                  {:else}
                    {commit.author ?? commit.author_name ?? '—'}
                  {/if}
                </td>
                <td class="secondary">{relativeTime(commit.timestamp ?? commit.authored_at ?? commit.date)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if subTab === 'files'}
      {#if fileTree.length === 0}
        <EmptyState title="No files tracked" message="File data appears after agents commit code. Try viewing Hot Files or Provenance for available data." />
      {:else if selectedFile}
        <!-- File view (code or blame) -->
        <div class="file-blame-view">
          <div class="file-blame-header">
            <nav class="blame-breadcrumb" aria-label="File navigation">
              <button class="breadcrumb-link" onclick={() => { selectedFile = null; blameData = null; fileViewMode = 'code'; }}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><polyline points="15 18 9 12 15 6"/></svg>
                Files
              </button>
              <span class="breadcrumb-sep" aria-hidden="true">/</span>
              <span class="breadcrumb-current mono">{selectedFile}</span>
            </nav>
            <div class="file-view-toggle">
              <button class="view-toggle-btn" class:active={fileViewMode === 'code'} onclick={() => { fileViewMode = 'code'; }} title="View source code">Code</button>
              <button class="view-toggle-btn" class:active={fileViewMode === 'blame'} onclick={() => { fileViewMode = 'blame'; }} title="View with agent attribution">Blame</button>
            </div>
          </div>

          {#if reviewRouting.length > 0}
            <div class="review-routing-bar">
              <span class="routing-label">Suggested reviewers:</span>
              {#each reviewRouting.slice(0, 3) as reviewer}
                <button class="routing-agent" onclick={() => { if (reviewer.agent_id) onRowClick({ id: reviewer.agent_id }, 'agent'); }} title={reviewer.agent_id ?? reviewer.name}>
                  <span class="agent-icon" aria-hidden="true">&#x2699;</span>
                  {reviewer.name ?? resolveEntityName('agent', reviewer.agent_id) ?? shortName(reviewer.agent_id)}
                  {#if reviewer.commit_count}<span class="routing-count">({reviewer.commit_count} commits)</span>{/if}
                </button>
              {/each}
            </div>
          {/if}

          <!-- Agent color legend (blame view) -->
          {#if fileViewMode === 'blame' && blameData}
            {@const legendLines = Array.isArray(blameData) ? blameData : (blameData.lines ?? blameData.blame ?? [])}
            {@const uniqueAgents = [...new Set(legendLines.map(l => l.agent_id ?? l.agent).filter(Boolean))]}
            {#if uniqueAgents.length > 0}
              <div class="agent-legend">
                <span class="agent-legend-label">Contributors:</span>
                {#each uniqueAgents as aid}
                  <button class="agent-legend-item" onclick={() => onRowClick({ id: aid }, 'agent')} title="View agent details">
                    <span class="agent-legend-color" style="background: {agentColor(aid)}"></span>
                    <span class="agent-legend-name">{resolveEntityName('agent', aid)}</span>
                  </button>
                {/each}
              </div>
            {/if}
          {/if}

          {#if blameLoading}
            <Skeleton lines={10} />
          {:else if blameData}
            {@const lines = Array.isArray(blameData) ? blameData : (blameData.lines ?? blameData.blame ?? [])}
            {#if lines.length > 0}
              {#if fileViewMode === 'code'}
                <!-- Clean code view with line numbers, agent color gutter, and click-to-reveal blame popover -->
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div class="code-viewer" onclick={(e) => { if (!e.target.closest('.blame-popover')) closePopover(); }}>
                  <table class="code-table-viewer">
                    <tbody>
                      {#each lines as line, i}
                        {@const agentId = line.agent_id ?? line.agent}
                        {@const sha = line.sha ?? line.commit_sha}
                        {@const specRef = line.spec_ref ?? line.spec_path}
                        {@const lineContent = line.content ?? line.text ?? line.line ?? ''}
                        {@const lineNum = line.line_number ?? (i + 1)}
                        {@const hasAttribution = !!(agentId || sha)}
                        <tr
                          class="code-line"
                          class:code-line-attributed={!!agentId}
                          onclick={(e) => { if (!e.target.closest('.blame-popover')) togglePopover(i, line); }}
                          title={agentId ? `Written by ${resolveEntityName('agent', agentId)} — click for details` : sha ? `Commit ${sha.slice(0, 7)} — click for details` : ''}
                        >
                          <td class="code-gutter-agent" style="border-left: 3px solid {agentColor(agentId)}"></td>
                          <td class="code-line-num">{lineNum}</td>
                          <td class="code-line-content mono">
                            <pre class="blame-line-pre">{@html highlightLine(lineContent, fileLang) || '&nbsp;'}</pre>
                            {#if popoverLineIdx === i && hasAttribution}
                              <!-- Inline blame popover -->
                              <div class="blame-popover" onclick={(e) => e.stopPropagation()}>
                                <div class="popover-header">
                                  {#if agentId}
                                    <button class="agent-link popover-agent" onclick={() => onRowClick({ id: agentId }, 'agent')}>
                                      <span class="agent-icon" aria-hidden="true">&#x2699;</span>
                                      {resolveEntityName('agent', agentId)}
                                    </button>
                                  {:else}
                                    <span class="popover-author">{line.author ?? 'Unknown'}</span>
                                  {/if}
                                  <button class="popover-close" onclick={closePopover} aria-label="Close popover">&times;</button>
                                </div>
                                <div class="popover-details">
                                  {#if sha}
                                    <div class="popover-row">
                                      <span class="popover-label">Commit</span>
                                      <button class="entity-link-sm" onclick={() => onRowClick({ sha, id: sha, agent_id: agentId, spec_ref: specRef }, 'commit')}>{sha.slice(0, 7)}</button>
                                      {#if line.timestamp ?? line.authored_at ?? line.date}
                                        <span class="popover-time">{relativeTime(line.timestamp ?? line.authored_at ?? line.date)}</span>
                                      {/if}
                                    </div>
                                  {/if}
                                  {#if specRef}
                                    {@const specName = specRef.split('@')[0]?.split('/').pop()}
                                    <div class="popover-row">
                                      <span class="popover-label">Spec</span>
                                      <button class="entity-link-sm" onclick={() => { if (goToEntityDetail) goToEntityDetail('spec', specRef.split('@')[0], { path: specRef.split('@')[0], repo_id: repoId }); else openDetailPanel?.({ type: 'spec', id: specRef.split('@')[0], data: { path: specRef.split('@')[0], repo_id: repoId } }); }}>{specName}</button>
                                    </div>
                                  {/if}
                                  {#if line.message ?? line.commit_message}
                                    <div class="popover-row popover-message">
                                      {(line.message ?? line.commit_message).slice(0, 120)}
                                    </div>
                                  {/if}
                                </div>
                                {#if agentId && sha}
                                  <div class="popover-actions">
                                    <button
                                      class="investigate-btn-prominent popover-investigate"
                                      onclick={() => investigateLine(line)}
                                      disabled={investigateLoading === sha}
                                    >
                                      {#if investigateLoading === sha}
                                        <svg class="spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13"><path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/></svg>
                                        Spawning...
                                      {:else}
                                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>
                                        Ask why this was written
                                      {/if}
                                    </button>
                                  </div>
                                {/if}
                              </div>
                            {/if}
                          </td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              {:else}
                <!-- Blame view with agent attribution (GitHub-style grouping) -->
                {@const blameGroups = computeBlameGroups(lines)}
                <div class="blame-code-viewer">
                  <table class="blame-table">
                    <thead>
                      <tr>
                        <th scope="col" class="blame-col-marker"></th>
                        <th scope="col" class="blame-col-line">#</th>
                        <th scope="col" class="blame-col-agent">Agent</th>
                        <th scope="col" class="blame-col-sha">Commit</th>
                        <th scope="col" class="blame-col-time">When</th>
                        <th scope="col" class="blame-col-spec">Spec</th>
                        <th scope="col" class="blame-col-action"></th>
                        <th scope="col" class="blame-col-content">Content</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each blameGroups as group}
                        {#each group.lines as line, lineIdx}
                          {@const agentId = line.agent_id ?? line.agent}
                          {@const specRef = line.spec_ref ?? line.spec_path}
                          {@const lineContent = line.content ?? line.text ?? line.line ?? ''}
                          {@const isFirst = lineIdx === 0}
                          <tr class="blame-row" class:blame-agent-row={!!agentId} class:blame-group-first={isFirst} class:blame-group-cont={!isFirst}>
                            <td class="blame-marker" style="border-left: 3px solid {agentColor(group.agentId)}" title={group.agentId ? `Agent: ${resolveEntityName('agent', group.agentId)}` : ''}></td>
                            <td class="blame-line-num">{line.line_number ?? (group.startIdx + lineIdx + 1)}</td>
                            {#if isFirst}
                              <td class="blame-agent" rowspan={group.lines.length}>
                                {#if group.agentId}
                                  <button class="agent-link" onclick={(e) => { e.stopPropagation(); onRowClick({ id: group.agentId }, 'agent'); }} title="View agent: {group.agentId}">
                                    <span class="agent-icon" aria-hidden="true">&#x2699;</span>
                                    {resolveEntityName('agent', group.agentId)}
                                  </button>
                                {:else}
                                  <span class="secondary">{line.author ?? '—'}</span>
                                {/if}
                              </td>
                              <td class="blame-sha mono" rowspan={group.lines.length}>
                                {#if group.sha}
                                  <button class="entity-link-sm" onclick={() => onRowClick({ sha: group.sha, id: group.sha, agent_id: group.agentId, spec_ref: specRef, conversation_sha: line.conversation_sha }, 'commit')} title="View commit: {group.sha.slice(0, 7)}">
                                    {group.sha.slice(0, 7)}
                                  </button>
                                {:else}
                                  —
                                {/if}
                              </td>
                              <td class="blame-time secondary" rowspan={group.lines.length}>
                                {relativeTime(line.timestamp ?? line.authored_at ?? line.date)}
                              </td>
                              <td class="blame-spec" rowspan={group.lines.length}>
                                {#if group.specRef}
                                  {@const specName = group.specRef.split('@')[0]?.split('/').pop()}
                                  <button class="entity-link-sm" onclick={(e) => { e.stopPropagation(); if (goToEntityDetail) goToEntityDetail('spec', group.specRef.split('@')[0], { path: group.specRef.split('@')[0], repo_id: repoId }); else openDetailPanel?.({ type: 'spec', id: group.specRef.split('@')[0], data: { path: group.specRef.split('@')[0], repo_id: repoId } }); }} title={group.specRef}>
                                    {specName}
                                  </button>
                                {:else}
                                  <span class="secondary">—</span>
                                {/if}
                              </td>
                              <td class="blame-action" rowspan={group.lines.length}>
                                {#if group.agentId && group.sha}
                                  <button
                                    class="investigate-btn-prominent"
                                    onclick={(e) => { e.stopPropagation(); investigateLine(line); }}
                                    disabled={investigateLoading === group.sha}
                                    title="Spawn an interrogation agent to discuss why this code was written this way"
                                  >
                                    {#if investigateLoading === group.sha}
                                      <svg class="spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13"><path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/></svg>
                                      <span>Spawning...</span>
                                    {:else}
                                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>
                                      <span>Ask why</span>
                                    {/if}
                                  </button>
                                {/if}
                              </td>
                            {/if}
                            <td class="blame-content mono"><pre class="blame-line-pre">{@html highlightLine(lineContent, fileLang) || '&nbsp;'}</pre></td>
                          </tr>
                        {/each}
                      {/each}
                    </tbody>
                  </table>
                </div>
              {/if}
            {:else}
              <p class="no-data">Blame data has no lines. The file may be empty or binary.</p>
            {/if}
          {:else}
            <p class="no-data">No blame data available for this file. The blame API requires agent commit attribution to display file content. Try viewing the file through a merge request diff instead.</p>
          {/if}
        </div>
      {:else}
        <!-- File tree list -->
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col">File</th>
              <th scope="col">Changes</th>
              <th scope="col">Contributors</th>
              <th scope="col">Last Modified</th>
            </tr>
          </thead>
          <tbody>
            {#each fileTree.filter(matchesFilter) as file}
              {@const pathParts = file.path.split('/')}
              <tr class="table-row" onclick={() => selectFile(file.path)} tabindex="0" role="button" aria-label="View blame for {file.path}" onkeydown={(e) => { if (e.key === 'Enter') selectFile(file.path); }}>
                <td class="mono file-path-cell">
                  {#if pathParts.length > 1}
                    <span class="file-path-dir">{pathParts.slice(0, -1).join('/')}/</span>
                  {/if}
                  <span class="file-path-name">{pathParts[pathParts.length - 1]}</span>
                </td>
                <td>{file.change_count || '—'}</td>
                <td class="secondary">{file.author_count || '—'}</td>
                <td class="secondary">{relativeTime(file.last_modified)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if subTab === 'hot-files'}
      {#if hotFiles.length === 0}
        <EmptyState title="No hot files" message="No files have been frequently modified yet. Hot files appear after multiple commits touch the same paths." />
      {:else}
        <table class="code-table">
          <thead>
            <tr>
              <th scope="col">File</th>
              <th scope="col">Changes</th>
              <th scope="col">Authors</th>
              <th scope="col">Last Modified</th>
            </tr>
          </thead>
          <tbody>
            {#each hotFiles as file}
              {@const filePath = file.path ?? file.file ?? null}
              {@const hotParts = filePath ? filePath.split('/') : []}
              <tr class="table-row" onclick={() => { if (filePath) { subTab = 'files'; selectFile(filePath); } }} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter' && filePath) { subTab = 'files'; selectFile(filePath); } }} title={filePath ? `View blame for ${filePath}` : ''}>
                <td class="mono file-path-cell">
                  {#if hotParts.length > 1}
                    <span class="file-path-dir">{hotParts.slice(0, -1).join('/')}/</span>
                  {/if}
                  <span class="file-path-name">{hotParts.length > 0 ? hotParts[hotParts.length - 1] : '—'}</span>
                </td>
                <td>{file.change_count ?? file.commits ?? file.count ?? 0}</td>
                <td class="secondary">{file.author_count ?? file.authors ?? '—'}</td>
                <td class="secondary">{relativeTime(file.last_modified ?? file.updated_at)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

    {:else if subTab === 'provenance'}
      <div class="provenance-tab">
        {#if agentCommitRecords.length > 0}
          <div class="provenance-section">
            <h3 class="provenance-heading">Agent Commit Attribution</h3>
            <p class="provenance-desc">Every commit is tracked back to the agent that authored it, forming an auditable chain from spec to code.</p>
            <table class="code-table">
              <thead>
                <tr>
                  <th scope="col">Commit</th>
                  <th scope="col">Agent</th>
                  <th scope="col">Branch</th>
                  <th scope="col">Time</th>
                </tr>
              </thead>
              <tbody>
                {#each agentCommitRecords as ac}
                  <tr class="table-row" onclick={() => { if (ac.agent_id) onRowClick({ id: ac.agent_id }, 'agent'); }} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter' && ac.agent_id) onRowClick({ id: ac.agent_id }, 'agent'); }}>
                    <td class="mono">{(ac.sha ?? ac.commit_sha ?? '').slice(0, 7)}</td>
                    <td>
                      {#if ac.agent_id}
                        <button class="agent-link" onclick={(e) => { e.stopPropagation(); onRowClick({ id: ac.agent_id }, 'agent'); }} title={ac.agent_id}>
                          <span class="agent-icon" aria-hidden="true">&#x2699;</span>
                          {ac.agent_name ?? resolveEntityName('agent', ac.agent_id)}
                        </button>
                      {:else}
                        <span class="secondary">—</span>
                      {/if}
                    </td>
                    <td class="mono secondary">{ac.branch ?? '—'}</td>
                    <td class="secondary">{relativeTime(ac.timestamp ?? ac.created_at)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}

        {#if aibomEntries.length > 0}
          <div class="provenance-section">
            <h3 class="provenance-heading">AI Bill of Materials (AIBOM)</h3>
            <p class="provenance-desc">Records of AI model usage in code generation, for compliance and auditability.</p>
            <table class="code-table">
              <thead>
                <tr>
                  <th scope="col">Model</th>
                  <th scope="col">Agent</th>
                  <th scope="col">Tokens</th>
                  <th scope="col">Time</th>
                </tr>
              </thead>
              <tbody>
                {#each aibomEntries as entry}
                  <tr class="table-row">
                    <td>{entry.model ?? entry.model_name ?? '—'}</td>
                    <td>
                      {#if entry.agent_id}
                        <button class="agent-link" onclick={(e) => { e.stopPropagation(); onRowClick({ id: entry.agent_id }, 'agent'); }} title={entry.agent_id}>
                          <span class="agent-icon" aria-hidden="true">&#x2699;</span>
                          {resolveEntityName('agent', entry.agent_id)}
                        </button>
                      {:else}
                        <span class="secondary">—</span>
                      {/if}
                    </td>
                    <td>{entry.total_tokens ?? entry.tokens ?? '—'}</td>
                    <td class="secondary">{relativeTime(entry.timestamp ?? entry.created_at)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}

        {#if agentCommitRecords.length === 0 && aibomEntries.length === 0}
          <EmptyState title="No provenance data" message="Provenance records appear after agents commit code. Each commit is attributed to its authoring agent, and AI model usage is tracked." />
        {/if}
      </div>

    {/if}
  </div>
</div>

<style>
  .code-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  /* Clone URL bar */
  .clone-url-bar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    overflow: hidden;
  }

  .clone-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    flex-shrink: 0;
  }

  .clone-url-text {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .clone-copy-btn {
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
    flex-shrink: 0;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .clone-copy-btn:hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  .clone-copy-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .commit-msg {
    max-width: 400px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .subtab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .subtab-btn {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .subtab-btn.active {
    color: var(--color-primary);
    border-bottom-color: var(--color-primary);
  }

  .subtab-btn:not(.active):hover {
    color: var(--color-text);
  }

  .filter-bar {
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .filter-input {
    width: 100%;
    max-width: 320px;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
    outline: none;
  }

  .filter-input:focus:not(:focus-visible) { outline: none; border-color: var(--color-focus); }
  .filter-input:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }
  .filter-input::-webkit-search-cancel-button { display: none; }

  .table-wrap {
    flex: 1;
    overflow-y: auto;
    padding: 0;
  }

  .code-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .code-table thead {
    position: sticky;
    top: 0;
    background: var(--color-surface-elevated);
    z-index: 1;
  }

  .code-table th {
    padding: var(--space-2) var(--space-4);
    text-align: left;
    font-weight: 600;
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    border-bottom: 1px solid var(--color-border);
  }

  .sort-btn {
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    font: inherit;
    padding: 0;
    white-space: nowrap;
    transition: color var(--transition-fast);
  }

  .sort-btn:hover { color: var(--color-text); }

  .table-row {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .table-row:hover {
    background: var(--color-surface-hover);
  }

  .table-row td {
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
  }

  .mono { font-family: var(--font-mono); }
  .secondary { color: var(--color-text-secondary); }

  .status-badge {
    display: inline-block;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    color: var(--color-text-secondary);
  }

  .status-badge.status-open { background: color-mix(in srgb, var(--color-success) 10%, transparent); border-color: color-mix(in srgb, var(--color-success) 40%, transparent); color: var(--color-success); }
  .status-badge.status-merged { background: color-mix(in srgb, var(--color-info) 10%, transparent); border-color: color-mix(in srgb, var(--color-info) 40%, transparent); color: var(--color-info); }
  .status-badge.status-closed { background: color-mix(in srgb, var(--color-danger) 10%, transparent); border-color: color-mix(in srgb, var(--color-danger) 40%, transparent); color: var(--color-danger); }

  .table-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .sort-btn:focus-visible,
  .subtab-btn:focus-visible,
  .filter-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .error-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid var(--color-danger);
    border-radius: var(--radius);
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  .retry-btn {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) var(--space-3);
    font-family: var(--font-body);
    white-space: nowrap;
  }
  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
    border-color: var(--color-primary);
  }
  .retry-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* MR title cell with spec ref */
  .mr-title-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .mr-spec-ref {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    padding: 1px var(--space-1);
    background: color-mix(in srgb, var(--color-info) 8%, transparent);
    border-radius: var(--radius-sm);
    width: fit-content;
  }

  /* Queue MR cell */
  .queue-mr-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .queue-branch {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* Priority pill */
  .priority-pill {
    display: inline-block;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .priority-high {
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    color: var(--color-danger);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
  }

  .priority-normal {
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
    color: var(--color-warning);
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent);
  }

  .priority-low {
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }

  /* MR updated cell with diff stats */
  .mr-updated-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .mr-diff-stats {
    display: flex;
    gap: var(--space-2);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .mr-diff-ins { color: var(--color-success); }
  .mr-diff-del { color: var(--color-danger); }

  /* Task title cell with spec ref */
  .task-title-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* Agent name cell */
  .agent-name-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .agent-type-tag {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    padding: 1px var(--space-1);
    background: color-mix(in srgb, var(--color-info) 8%, transparent);
    border-radius: var(--radius-sm);
    width: fit-content;
  }

  /* Agent status colors */
  .status-badge.status-active { background: color-mix(in srgb, var(--color-success) 10%, transparent); border-color: color-mix(in srgb, var(--color-success) 40%, transparent); color: var(--color-success); }
  .status-badge.status-completed, .status-badge.status-idle { background: color-mix(in srgb, var(--color-info) 10%, transparent); border-color: color-mix(in srgb, var(--color-info) 40%, transparent); color: var(--color-info); }
  .status-badge.status-failed, .status-badge.status-dead { background: color-mix(in srgb, var(--color-danger) 10%, transparent); border-color: color-mix(in srgb, var(--color-danger) 40%, transparent); color: var(--color-danger); }

  /* Task status colors */
  .status-badge.status-done { background: color-mix(in srgb, var(--color-success) 10%, transparent); border-color: color-mix(in srgb, var(--color-success) 40%, transparent); color: var(--color-success); }
  .status-badge.status-in_progress { background: color-mix(in srgb, var(--color-warning) 10%, transparent); border-color: color-mix(in srgb, var(--color-warning) 40%, transparent); color: var(--color-warning); }
  .status-badge.status-blocked { background: color-mix(in srgb, var(--color-danger) 10%, transparent); border-color: color-mix(in srgb, var(--color-danger) 40%, transparent); color: var(--color-danger); }
  .status-badge.status-backlog { background: var(--color-surface-elevated); border-color: var(--color-border); color: var(--color-text-muted); }

  /* MR meta line (agent + spec) */
  .mr-meta-line {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    flex-wrap: wrap;
  }

  /* Gate summary badge */
  .gate-summary {
    display: inline-block;
    padding: 1px var(--space-2);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-mono);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
  }

  .gate-all-pass {
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 40%, transparent);
    color: var(--color-success);
  }

  .gate-has-fail {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 40%, transparent);
    color: var(--color-danger);
  }

  .gate-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .gate-names {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
  }

  .gate-name-tag {
    font-size: 10px;
    padding: 0 4px;
    border-radius: var(--radius);
    white-space: nowrap;
    line-height: 1.4;
  }

  .gate-name-passed {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
  }

  .gate-name-failed {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
  }

  .gate-name-pending {
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
  }

  /* Agent link in commit table */
  .agent-link {
    background: none;
    border: none;
    color: var(--color-primary);
    cursor: pointer;
    font: inherit;
    padding: 0;
    text-decoration: underline;
    text-underline-offset: 2px;
    text-decoration-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }
  .agent-link:hover { text-decoration-color: var(--color-primary); }
  .agent-icon { margin-right: 2px; font-size: var(--text-xs); }

  /* Provenance tab */
  .provenance-tab {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    padding: var(--space-2) 0;
  }

  .provenance-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .provenance-heading {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
    padding: 0 var(--space-4);
  }

  .provenance-desc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    padding: 0 var(--space-4);
  }

  /* Speculative merge badge */
  .dep-indicator {
    font-size: var(--text-xs);
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
  }

  .dep-blocked {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
    font-weight: 600;
  }

  .dep-group {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    color: var(--color-primary);
    font-family: var(--font-mono);
  }

  .spec-merge-badge {
    display: inline-block;
    padding: 1px var(--space-2);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 600;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
  }

  .spec-merge-clean {
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 40%, transparent);
    color: var(--color-success);
  }

  .spec-merge-conflict {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-danger) 40%, transparent);
    color: var(--color-danger);
  }

  /* Files / Blame view */
  .file-blame-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .file-blame-header {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .blame-breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .breadcrumb-link {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: none;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-link);
    cursor: pointer;
    font: inherit;
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    transition: background var(--transition-fast), color var(--transition-fast);
    font-weight: 600;
  }

  .breadcrumb-link:hover {
    background: var(--color-surface-hover);
    color: var(--color-primary);
  }

  .breadcrumb-sep {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }

  .breadcrumb-current {
    font-size: var(--text-sm);
    color: var(--color-text);
    font-weight: 600;
  }

  .file-view-toggle {
    display: flex;
    gap: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .view-toggle-btn {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: none;
    border-right: 1px solid var(--color-border);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .view-toggle-btn:last-child { border-right: none; }

  .view-toggle-btn.active {
    background: var(--color-primary);
    color: var(--color-text-inverse);
  }

  .view-toggle-btn:hover:not(.active) {
    background: var(--color-surface-elevated);
  }

  /* Clean code viewer */
  .code-viewer {
    overflow: auto;
    max-height: calc(100vh - 200px);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .code-table-viewer {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .code-line {
    cursor: default;
    transition: background var(--transition-fast);
  }

  .code-line:hover {
    background: var(--color-surface-elevated);
  }

  .code-line[title]:not([title=""]) {
    cursor: pointer;
  }

  .code-gutter-agent {
    width: 4px;
    padding: 0;
  }

  .code-line-num {
    padding: 0 var(--space-2) 0 var(--space-3);
    text-align: right;
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    user-select: none;
    white-space: nowrap;
    min-width: 40px;
    border-right: 1px solid var(--color-border);
  }

  .code-line-content {
    padding: 0 var(--space-3);
    white-space: pre;
    tab-size: 4;
  }

  .review-routing-bar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: color-mix(in srgb, var(--color-info) 5%, transparent);
    border-bottom: 1px solid var(--color-border);
    flex-wrap: wrap;
    flex-shrink: 0;
  }

  .routing-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 600;
  }

  .routing-agent {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font: inherit;
    font-size: var(--text-xs);
    padding: 2px var(--space-2);
  }

  .routing-agent:hover { background: var(--color-surface-hover); }

  .routing-count {
    color: var(--color-text-muted);
    font-size: 10px;
  }

  /* ── Agent legend ───────────────────────────────────────────────────── */
  .agent-legend {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
    flex-wrap: wrap;
    flex-shrink: 0;
  }

  .agent-legend-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 600;
  }

  .agent-legend-item {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    cursor: pointer;
    font: inherit;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    transition: border-color var(--transition-fast);
  }

  .agent-legend-item:hover {
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .agent-legend-color {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .blame-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-xs);
    overflow: auto;
    flex: 1;
  }

  .blame-table thead {
    position: sticky;
    top: 0;
    background: var(--color-surface-elevated);
    z-index: 1;
  }

  .blame-table th {
    padding: var(--space-1) var(--space-2);
    text-align: left;
    font-weight: 600;
    color: var(--color-text-muted);
    border-bottom: 1px solid var(--color-border);
  }

  .blame-col-marker { width: 4px; padding: 0 !important; }
  .blame-col-line { width: 40px; text-align: right; }
  .blame-col-agent { width: 120px; }
  .blame-col-sha { width: 70px; }
  .blame-col-spec { width: 100px; }
  .blame-col-time { width: 80px; }

  .blame-spec { white-space: nowrap; font-size: var(--text-xs); }

  .blame-row td {
    padding: 0 var(--space-2);
    border-bottom: 1px solid color-mix(in srgb, var(--color-border) 30%, transparent);
    vertical-align: top;
    line-height: 1.6;
  }

  .blame-row.blame-agent-row {
    background: color-mix(in srgb, var(--color-info) 3%, transparent);
  }

  /* GitHub-style blame grouping: visual separator between groups */
  .blame-row.blame-group-first td {
    border-top: 2px solid var(--color-border);
  }

  .blame-row.blame-group-cont .blame-agent,
  .blame-row.blame-group-cont .blame-sha,
  .blame-row.blame-group-cont .blame-time,
  .blame-row.blame-group-cont .blame-spec,
  .blame-row.blame-group-cont .blame-action {
    /* rowspan handles hiding; border cleanup */
  }

  .blame-row.blame-group-first .blame-agent,
  .blame-row.blame-group-first .blame-sha,
  .blame-row.blame-group-first .blame-time,
  .blame-row.blame-group-first .blame-spec,
  .blame-row.blame-group-first .blame-action {
    vertical-align: top;
    padding-top: var(--space-1);
  }

  .blame-line-num {
    text-align: right;
    color: var(--color-text-muted);
    user-select: none;
    font-family: var(--font-mono);
  }

  .blame-agent { white-space: nowrap; }

  .blame-sha { white-space: nowrap; }

  .blame-content {
    overflow-x: auto;
    max-width: 600px;
  }

  .blame-line-pre {
    margin: 0;
    white-space: pre;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .blame-marker {
    width: 4px;
    padding: 0 !important;
  }

  .blame-action {
    width: 80px;
    padding: 0 2px !important;
    white-space: nowrap;
  }

  .blame-col-action { width: 80px; }

  .blame-code-viewer {
    flex: 1;
    overflow: auto;
  }

  .investigate-btn-prominent {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px var(--space-2);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-body);
    transition: color var(--transition-fast), border-color var(--transition-fast), background var(--transition-fast);
    white-space: nowrap;
    opacity: 0;
  }

  .blame-row:hover .investigate-btn-prominent {
    opacity: 1;
  }

  .investigate-btn-prominent:hover:not(:disabled) {
    color: var(--color-text);
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 18%, transparent);
  }

  .investigate-btn-prominent:disabled {
    opacity: 0.5 !important;
    cursor: not-allowed;
  }

  .entity-link-sm {
    background: none;
    border: none;
    color: var(--color-primary);
    cursor: pointer;
    font: inherit;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: 0;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .entity-link-sm:hover { text-decoration-color: var(--color-primary); }

  .blame-time {
    white-space: nowrap;
    font-size: var(--text-xs);
  }

  /* Code line with agent attribution — subtle indicator */
  .code-line-attributed {
    cursor: pointer;
  }

  .code-line-attributed:hover {
    background: color-mix(in srgb, var(--color-info) 6%, transparent);
  }

  /* ── Inline blame popover (code view click-to-reveal) ────────────── */
  .code-line-content {
    position: relative;
  }

  .blame-popover {
    position: absolute;
    top: 100%;
    left: 40px;
    z-index: 10;
    min-width: 280px;
    max-width: 400px;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
    padding: 0;
    font-size: var(--text-xs);
  }

  .popover-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    background: color-mix(in srgb, var(--color-primary) 5%, transparent);
  }

  .popover-agent {
    font-size: var(--text-xs);
    font-weight: 600;
  }

  .popover-author {
    font-weight: 600;
    color: var(--color-text);
  }

  .popover-close {
    background: none;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0 var(--space-1);
    line-height: 1;
  }

  .popover-close:hover {
    color: var(--color-text);
  }

  .popover-details {
    padding: var(--space-2) var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .popover-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .popover-label {
    font-weight: 600;
    color: var(--color-text-muted);
    min-width: 48px;
  }

  .popover-time {
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .popover-message {
    color: var(--color-text-secondary);
    font-style: italic;
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-1);
    margin-top: var(--space-1);
    line-height: 1.4;
  }

  .popover-actions {
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--color-border);
  }

  .popover-investigate {
    opacity: 1 !important;
    width: 100%;
    justify-content: center;
  }

  .no-data {
    padding: var(--space-4);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    text-align: center;
  }

  .file-path-cell {
    white-space: nowrap;
  }

  .file-path-dir {
    color: var(--color-text-muted);
  }

  .file-path-name {
    color: var(--color-text);
    font-weight: 600;
  }

  .blame-line-pre :global(.hl-kw) { color: #c678dd; }
  .blame-line-pre :global(.hl-str) { color: #98c379; }
  .blame-line-pre :global(.hl-cmt) { color: #5c6370; font-style: italic; }
  .blame-line-pre :global(.hl-num) { color: #d19a66; }

  @media (prefers-reduced-motion: reduce) {
    .subtab-btn, .sort-btn, .table-row { transition: none; }
  }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
