<script>
  import { api } from '../lib/api.js';
  import Tabs from '../lib/Tabs.svelte';
  import Table from '../lib/Table.svelte';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let { repo, onBack, onSelectMr } = $props();

  let activeTab = $state('branches');
  let branches = $state([]);
  let commits = $state([]);
  let mrs = $state([]);
  let selectedBranch = $state(repo.default_branch || 'main');
  let loading = $state(false);
  let error = $state(null);
  let cloneCopied = $state(false);

  let jjChanges = $state([]);
  let jjLoading = $state(false);
  let jjError = $state(null);
  let jjInitLoading = $state(false);
  let jjInitMsg = $state(null);

  let aibom = $state(null);
  let aibomLoading = $state(false);
  let aibomError = $state(null);

  // Speculative merge results — map of branch name -> result object
  let speculative = $state({});
  // Agent commits — map of sha -> agent_id
  let agentCommitMap = $state({});
  // Commit signature map — map of sha -> true (signed) | false (unsigned)
  let sigMap = $state({});

  // Activity tab (hot files + blame)
  let hotFiles = $state([]);
  let hotFilesLoading = $state(false);
  let activityError = $state(null);
  let blameFile = $state(null);
  let blameLines = $state([]);
  let blameLoading = $state(false);

  // Policy tab (ABAC + spec policy)
  let abacPolicies = $state([]);
  let specPolicy = $state({ require_spec_ref: false, require_approved_spec: false, warn_stale_spec: false, require_current_spec: false });
  let policyLoading = $state(false);
  let policyError = $state(null);
  let policySaving = $state(false);
  let policySaveMsg = $state(null);
  // New ABAC policy editing state
  let newPolicyName = $state('');
  let newPolicyClaim = $state('');
  let newPolicyValue = $state('');

  // Gates tab
  let gates = $state([]);
  let pushGates = $state(null);
  let gatesLoading = $state(false);
  let gatesError = $state(null);
  let showGateForm = $state(false);
  let newGateName = $state('');
  let newGateType = $state('TestCommand');
  let newGateCmd = $state('');
  let creatingGate = $state(false);
  let deletingGateId = $state(null);
  const GATE_TYPES = ['TestCommand', 'LintCommand', 'AgentReview', 'AgentValidation', 'RequiredApprovals'];
  const BUILTIN_PUSH_GATES = ['conventional-commit', 'task-ref', 'no-em-dash'];

  const cloneUrl = `${window.location.origin}/git/${repo.id}/${repo.name}.git`;

  const tabs = $derived([
    { id: 'branches', label: 'Branches', count: branches.length || undefined },
    { id: 'commits',  label: 'Commits' },
    { id: 'mrs',      label: 'Merge Requests', count: mrs.length || undefined },
    { id: 'activity', label: 'Activity' },
    { id: 'policy',   label: 'Policy' },
    { id: 'gates',    label: 'Gates', count: gates.length || undefined },
    { id: 'jj',       label: 'jj' },
    { id: 'aibom',    label: 'AIBOM' },
  ]);

  async function copyCloneUrl() {
    try {
      await navigator.clipboard.writeText(cloneUrl);
      cloneCopied = true;
      setTimeout(() => { cloneCopied = false; }, 2000);
    } catch { /* clipboard not available */ }
  }

  $effect(() => {
    loadBranches();
    loadMrs();
    loadSpeculative();
  });

  $effect(() => {
    if (activeTab === 'commits') loadCommits(selectedBranch);
  });

  $effect(() => {
    if (activeTab === 'jj') loadJjLog();
  });

  $effect(() => {
    if (activeTab === 'aibom') loadAibom();
  });

  $effect(() => {
    if (activeTab === 'activity') loadActivity();
  });

  $effect(() => {
    if (activeTab === 'policy') loadPolicy();
  });

  $effect(() => {
    if (activeTab === 'gates') loadGates();
  });

  async function loadSpeculative() {
    try {
      const results = await api.repoSpeculative(repo.id);
      const map = {};
      if (Array.isArray(results)) {
        for (const r of results) {
          if (r.branch) map[r.branch] = r;
        }
      }
      speculative = map;
    } catch { /* silently ignore */ }
  }

  async function loadAgentCommits() {
    try {
      const records = await api.repoAgentCommits(repo.id);
      const map = {};
      if (Array.isArray(records)) {
        for (const r of records) {
          if (r.sha) map[r.sha] = r.agent_id ?? null;
        }
      }
      agentCommitMap = map;
    } catch { /* silently ignore */ }
  }

  async function loadSignatures(shas) {
    const entries = await Promise.all(
      shas.map(async (sha) => {
        try {
          await api.commitSignature(repo.id, sha);
          return [sha, true];
        } catch {
          return [sha, false];
        }
      })
    );
    const map = { ...sigMap };
    for (const [sha, signed] of entries) {
      map[sha] = signed;
    }
    sigMap = map;
  }

  async function loadJjLog() {
    jjLoading = true; jjError = null;
    try {
      jjChanges = await api.jjLog(repo.id);
    } catch (e) {
      jjError = e.message;
    } finally {
      jjLoading = false;
    }
  }

  async function initJj() {
    jjInitLoading = true; jjInitMsg = null; jjError = null;
    try {
      await api.jjInit(repo.id);
      jjInitMsg = 'jj initialized successfully.';
      await loadJjLog();
    } catch (e) {
      jjError = e.message;
    } finally {
      jjInitLoading = false;
    }
  }

  async function loadBranches() {
    loading = true; error = null;
    try {
      branches = await api.repoBranches(repo.id);
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function loadCommits(branch) {
    loading = true; error = null;
    try {
      commits = await api.repoCommits(repo.id, branch);
      // Load agent commits + signatures in parallel after commits are fetched
      const shas = commits.map(c => c.sha).filter(Boolean);
      loadAgentCommits();
      if (shas.length > 0) loadSignatures(shas);
    } catch (e) {
      error = e.message.includes('500')
        ? `Could not load commits for branch '${branch}' — the branch may not exist or the repository may be empty.`
        : e.message;
    } finally {
      loading = false;
    }
  }

  async function loadMrs() {
    try {
      mrs = await api.mergeRequests({ repository_id: repo.id });
    } catch { mrs = []; }
  }

  async function loadActivity() {
    hotFilesLoading = true; activityError = null;
    try {
      hotFiles = await api.repoHotFiles(repo.id);
    } catch (e) {
      activityError = e.message;
    } finally {
      hotFilesLoading = false;
    }
  }

  async function loadBlame(path) {
    blameFile = path; blameLines = []; blameLoading = true;
    try {
      blameLines = await api.repoBlame(repo.id, path);
    } catch { blameLines = []; }
    finally { blameLoading = false; }
  }

  async function loadPolicy() {
    policyLoading = true; policyError = null;
    try {
      const [abac, spec] = await Promise.all([
        api.repoAbacPolicy(repo.id),
        api.repoSpecPolicy(repo.id),
      ]);
      abacPolicies = Array.isArray(abac) ? abac : [];
      specPolicy = spec ?? { require_spec_ref: false, require_approved_spec: false, warn_stale_spec: false, require_current_spec: false };
    } catch (e) {
      policyError = e.message;
    } finally {
      policyLoading = false;
    }
  }

  async function saveSpecPolicy() {
    policySaving = true; policySaveMsg = null;
    try {
      await api.setRepoSpecPolicy(repo.id, specPolicy);
      policySaveMsg = 'Spec policy saved.';
      setTimeout(() => { policySaveMsg = null; }, 3000);
    } catch (e) {
      policyError = e.message;
    } finally {
      policySaving = false;
    }
  }

  async function addAbacPolicy() {
    if (!newPolicyName.trim() || !newPolicyClaim.trim() || !newPolicyValue.trim()) return;
    // Server AbacPolicy format: {resource_type, required_claims: {claim: value}}
    const newPolicy = {
      resource_type: newPolicyName.trim(),
      required_claims: { [newPolicyClaim.trim()]: newPolicyValue.trim() },
    };
    const updated = [...abacPolicies, newPolicy];
    policySaving = true; policySaveMsg = null;
    try {
      await api.setRepoAbacPolicy(repo.id, updated);
      abacPolicies = updated;
      newPolicyName = ''; newPolicyClaim = ''; newPolicyValue = '';
      policySaveMsg = 'ABAC policy added.';
      setTimeout(() => { policySaveMsg = null; }, 3000);
    } catch (e) {
      policyError = e.message;
    } finally {
      policySaving = false;
    }
  }

  async function removeAbacPolicy(idx) {
    const updated = abacPolicies.filter((_, i) => i !== idx);
    policySaving = true;
    try {
      await api.setRepoAbacPolicy(repo.id, updated);
      abacPolicies = updated;
    } catch (e) {
      policyError = e.message;
    } finally {
      policySaving = false;
    }
  }

  async function loadGates() {
    gatesLoading = true; gatesError = null;
    try {
      [gates, pushGates] = await Promise.all([
        api.repoGates(repo.id),
        api.repoPushGates(repo.id).catch(() => null),
      ]);
    } catch (e) {
      gatesError = e.message;
    } finally {
      gatesLoading = false;
    }
  }

  async function createGate() {
    creatingGate = true;
    try {
      const body = { name: newGateName, gate_type: newGateType };
      if (newGateCmd) body.command = newGateCmd;
      const gate = await api.createRepoGate(repo.id, body);
      gates = [...gates, gate];
      newGateName = '';
      newGateType = 'TestCommand';
      newGateCmd = '';
      showGateForm = false;
      toastSuccess('Gate created.');
    } catch (e) {
      toastError(e.message);
    } finally {
      creatingGate = false;
    }
  }

  async function deleteGate(gateId) {
    deletingGateId = gateId;
    try {
      await api.deleteRepoGate(repo.id, gateId);
      gates = gates.filter(g => g.id !== gateId);
      toastSuccess('Gate deleted.');
    } catch (e) {
      toastError(e.message);
    } finally {
      deletingGateId = null;
    }
  }

  async function togglePushGate(gateName) {
    if (!pushGates) return;
    const active = pushGates.gates ?? pushGates.active_gates ?? [];
    const next = active.includes(gateName)
      ? active.filter(g => g !== gateName)
      : [...active, gateName];
    try {
      pushGates = await api.setRepoPushGates(repo.id, { gates: next });
      toastSuccess('Push gates updated.');
    } catch (e) {
      toastError(e.message);
    }
  }

  function relativeTime(ts) {
    if (!ts) return '—';
    const diff = Date.now() - ts * 1000;
    const secs = Math.floor(diff / 1000);
    if (secs < 60) return `${secs}s ago`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}m ago`;
    const hrs = Math.floor(mins / 60);
    if (hrs < 24) return `${hrs}h ago`;
    return `${Math.floor(hrs / 24)}d ago`;
  }

  function shortSha(sha) {
    return sha ? sha.slice(0, 8) : '—';
  }

  function speculativeVariant(branch) {
    const r = speculative[branch];
    if (!r) return null;
    if (r.has_conflict) return 'danger';
    return 'success';
  }

  function speculativeLabel(branch) {
    const r = speculative[branch];
    if (!r) return null;
    return r.has_conflict ? 'conflict' : 'clean';
  }

  async function loadAibom() {
    aibomLoading = true; aibomError = null;
    try {
      aibom = await api.repoAibom(repo.id);
    } catch (e) {
      aibomError = e.message;
    } finally {
      aibomLoading = false;
    }
  }

  function attestationVariant(level) {
    if (level === 'server-verified') return 'success';
    if (level === 'self-reported') return 'warning';
    return 'default';
  }
</script>

<div class="page">
  <div class="page-hdr">
    <div class="breadcrumb">
      <button class="back-btn" onclick={onBack} aria-label="Back to projects list">← Projects</button>
      <span class="sep">/</span>
      <h1 class="repo-name">{repo.name}</h1>
    </div>
    <span class="default-badge">default: {repo.default_branch}</span>
  </div>

  <div class="clone-bar">
    <span class="clone-label">Clone</span>
    <code class="clone-url-text">{cloneUrl}</code>
    <button class="copy-btn" onclick={copyCloneUrl} aria-label="Copy clone URL">{cloneCopied ? 'Copied!' : 'Copy'}</button>
  </div>

  <div class="tabs-wrap">
    <Tabs {tabs} bind:active={activeTab} />
  </div>

  <div class="tab-content" role="tabpanel" id="tabpanel-{activeTab}" aria-labelledby="tab-{activeTab}" tabindex="0">
    {#if error}
      <div class="error-msg" role="alert">Error: {error}</div>
    {:else if loading && (activeTab === 'branches' || activeTab === 'commits')}
      <Skeleton lines={8} height="2.5rem" />
    {:else if activeTab === 'branches'}
      {#if branches.length === 0}
        <EmptyState title="No branches" description="No branches found in this repository." />
      {:else}
        <Table
          columns={[
            { key: 'name', label: 'Branch', sortable: true },
            { key: 'sha', label: 'Head SHA' },
            { key: 'default', label: '' },
            { key: 'speculative', label: 'Merge Status' },
          ]}
        >
          {#snippet children()}
            {#each branches as b (b.name)}
              <tr>
                <td class="branch-name-cell">{b.name}</td>
                <td><code class="sha">{shortSha(b.sha)}</code></td>
                <td>
                  {#if b.name === repo.default_branch}
                    <Badge value="default" variant="info" />
                  {/if}
                </td>
                <td>
                  {#if speculativeLabel(b.name)}
                    <Badge value={speculativeLabel(b.name)} variant={speculativeVariant(b.name)} />
                  {:else}
                    <span class="secondary-cell">—</span>
                  {/if}
                </td>
              </tr>
            {/each}
          {/snippet}
        </Table>
      {/if}
    {:else if activeTab === 'commits'}
      <div class="commits-toolbar">
        <label class="branch-label">
          Branch:
          <select class="branch-select" bind:value={selectedBranch} onchange={() => loadCommits(selectedBranch)}>
            {#each branches as b (b.name)}
              <option value={b.name}>{b.name}</option>
            {/each}
            {#if branches.length === 0}
              <option value={selectedBranch}>{selectedBranch}</option>
            {/if}
          </select>
        </label>
      </div>
      {#if commits.length === 0}
        <EmptyState title="No commits" description="No commits found on branch {selectedBranch}." />
      {:else}
        <Table
          columns={[
            { key: 'sha', label: 'SHA' },
            { key: 'message', label: 'Message' },
            { key: 'author', label: 'Author' },
            { key: 'agent', label: 'Agent' },
            { key: 'sig', label: 'Sig' },
            { key: 'time', label: 'Time' },
          ]}
        >
          {#snippet children()}
            {#each commits as c (c.sha)}
              <tr>
                <td><code class="sha">{shortSha(c.sha)}</code></td>
                <td class="commit-msg-cell">{c.message}</td>
                <td class="secondary-cell">{c.author}</td>
                <td class="secondary-cell mono">
                  {#if agentCommitMap[c.sha]}
                    <span class="agent-ref">{agentCommitMap[c.sha].slice(0, 8)}</span>
                  {:else}
                    <span class="secondary-cell">—</span>
                  {/if}
                </td>
                <td>
                  {#if sigMap[c.sha] === true}
                    <Badge value="signed" variant="success" />
                  {:else if sigMap[c.sha] === false}
                    <Badge value="unsigned" variant="default" />
                  {:else}
                    <span class="secondary-cell">…</span>
                  {/if}
                </td>
                <td class="secondary-cell">{relativeTime(c.timestamp)}</td>
              </tr>
            {/each}
          {/snippet}
        </Table>
      {/if}
    {:else if activeTab === 'mrs'}
      {#if mrs.length === 0}
        <EmptyState title="No merge requests" description="No merge requests for this repository." />
      {:else}
        <Table
          columns={[
            { key: 'status', label: 'Status' },
            { key: 'title', label: 'Title' },
            { key: 'author', label: 'Author' },
            { key: 'branches', label: 'Branches' },
          ]}
        >
          {#snippet children()}
            {#each mrs as mr (mr.id)}
              <tr class="clickable" onclick={() => onSelectMr(mr)} tabindex="0" aria-label="View merge request: {mr.title}" onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectMr(mr); } }}>
                <td><Badge value={mr.status} /></td>
                <td class="mr-title-cell">{mr.title}</td>
                <td class="secondary-cell">{mr.author ?? '—'}</td>
                <td class="secondary-cell mono">{mr.source_branch} → {mr.target_branch}</td>
              </tr>
            {/each}
          {/snippet}
        </Table>
      {/if}

    {:else if activeTab === 'activity'}
      {#if activityError}
        <div class="error-msg" role="alert">Error: {activityError}</div>
      {:else if hotFilesLoading}
        <Skeleton lines={6} height="2.5rem" />
      {:else}
        <div class="activity-layout">
          <div class="hot-files-panel">
            <h3 class="section-title">Hot Files</h3>
            {#if hotFiles.length === 0}
              <EmptyState title="No hot files" description="No files with concurrent agent activity in the last 24h." />
            {:else}
              <div class="hot-files-list">
                {#each hotFiles as f (f.path)}
                  <button
                    class="hot-file-row"
                    class:selected={blameFile === f.path}
                    onclick={() => loadBlame(f.path)}
                    aria-label="View blame for {f.path}"
                    aria-pressed={blameFile === f.path}
                  >
                    <span class="hot-file-path">{f.path}</span>
                    <Badge value={`${f.agent_count} agent${f.agent_count === 1 ? '' : 's'}`} variant="info" />
                  </button>
                {/each}
              </div>
            {/if}
          </div>

          {#if blameFile}
            <div class="blame-panel">
              <h3 class="section-title">Blame: <code class="inline-code">{blameFile}</code></h3>
              {#if blameLoading}
                <Skeleton lines={10} height="1.8rem" />
              {:else if blameLines.length === 0}
                <EmptyState title="No blame data" description="No per-line attribution available for this file." />
              {:else}
                <div class="blame-table">
                  {#each blameLines as line, i (i)}
                    <div class="blame-row">
                      <span class="blame-lineno">{i + 1}</span>
                      <span class="blame-agent secondary-cell">{line.agent_id ? line.agent_id.slice(0, 8) : '—'}</span>
                      <code class="blame-content">{line.content ?? ''}</code>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {:else}
            <div class="blame-panel blame-empty">
              <EmptyState title="Select a file" description="Click a hot file on the left to view per-line agent attribution." />
            </div>
          {/if}
        </div>
      {/if}

    {:else if activeTab === 'policy'}
      {#if policyLoading}
        <Skeleton lines={8} height="2.5rem" />
      {:else if policyError}
        <div class="error-msg" role="alert">Error: {policyError}</div>
      {:else}
        <div class="policy-section">
          <h3 class="section-title">Spec Policy</h3>
          <p class="section-desc">Control how spec references are enforced on merge requests for this repository.</p>
          <div class="policy-toggles">
            <label class="toggle-row">
              <input type="checkbox" bind:checked={specPolicy.require_spec_ref} />
              <span class="toggle-label">Require spec_ref on all MRs</span>
            </label>
            <label class="toggle-row">
              <input type="checkbox" bind:checked={specPolicy.require_approved_spec} />
              <span class="toggle-label">Require fully approved spec before merge</span>
            </label>
            <label class="toggle-row">
              <input type="checkbox" bind:checked={specPolicy.warn_stale_spec} />
              <span class="toggle-label">Warn on stale spec (emits StaleSpecWarning event)</span>
            </label>
            <label class="toggle-row">
              <input type="checkbox" bind:checked={specPolicy.require_current_spec} />
              <span class="toggle-label">Require current spec (blocks queue when stale)</span>
            </label>
          </div>
          <div class="policy-actions">
            <button class="policy-btn primary" onclick={saveSpecPolicy} disabled={policySaving}>
              {policySaving ? 'Saving…' : 'Save Spec Policy'}
            </button>
            {#if policySaveMsg}
              <span class="save-msg">{policySaveMsg}</span>
            {/if}
          </div>
        </div>

        <div class="policy-section">
          <h3 class="section-title">ABAC Policies</h3>
          <p class="section-desc">Attribute-based access control policies evaluated on JWT claims for push and spawn. Policies are OR'd; rules within a policy are AND'd.</p>

          {#if abacPolicies.length === 0}
            <EmptyState title="No ABAC policies" description="All JWT bearers have access. Add a policy to restrict by JWT claims." />
          {:else}
            <div class="abac-list">
              {#each abacPolicies as policy, idx (idx)}
                <div class="abac-card">
                  <div class="abac-card-hdr">
                    <span class="abac-name">{policy.resource_type || `Policy ${idx + 1}`}</span>
                    <button class="abac-remove-btn" onclick={() => removeAbacPolicy(idx)} disabled={policySaving} aria-label="Remove ABAC policy {idx + 1}">Remove</button>
                  </div>
                  <div class="abac-rules">
                    {#each Object.entries(policy.required_claims ?? {}) as [claim, value], ri (ri)}
                      <div class="abac-rule">
                        <code class="rule-claim">{claim}</code>
                        <span class="rule-op">=</span>
                        <code class="rule-value">"{value}"</code>
                      </div>
                    {/each}
                  </div>
                </div>
              {/each}
            </div>
          {/if}

          <div class="abac-add-form">
            <h4 class="form-title">Add Policy</h4>
            <div class="abac-form-row">
              <input class="policy-input" type="text" placeholder="Resource type (e.g. repo, task)" bind:value={newPolicyName} />
            </div>
            <div class="abac-form-row">
              <input class="policy-input" type="text" placeholder="JWT claim (e.g. sub, role)" bind:value={newPolicyClaim} />
              <span class="policy-op-label">=</span>
              <input class="policy-input" type="text" placeholder="Value" bind:value={newPolicyValue} />
              <button class="policy-btn primary" onclick={addAbacPolicy} disabled={policySaving}>Add</button>
            </div>
          </div>
        </div>
      {/if}

    {:else if activeTab === 'gates'}
      {#if gatesError}
        <div class="error-msg" role="alert">Error: {gatesError}</div>
      {:else if gatesLoading}
        <Skeleton lines={6} height="2.5rem" />
      {:else}
        <!-- Quality gates -->
        <div class="gates-section-hdr">
          <h3 class="gates-section-title">Quality Gates</h3>
          <button class="jj-btn primary" onclick={() => showGateForm = !showGateForm} aria-expanded={showGateForm} aria-controls="gate-form">
            {showGateForm ? 'Cancel' : '+ New Gate'}
          </button>
        </div>
        {#if showGateForm}
          <div class="gate-form" id="gate-form">
            <input class="branch-select" placeholder="Gate name" bind:value={newGateName} />
            <select class="branch-select" bind:value={newGateType} aria-label="Gate type">
              {#each GATE_TYPES as t (t)}
                <option value={t}>{t}</option>
              {/each}
            </select>
            <input class="branch-select" placeholder="Command (optional)" bind:value={newGateCmd} />
            <button class="jj-btn primary" onclick={createGate} disabled={creatingGate || !newGateName.trim()}>
              {creatingGate ? 'Creating…' : 'Create'}
            </button>
          </div>
        {/if}
        {#if gates.length === 0}
          <EmptyState title="No quality gates" description="Add a quality gate to enforce checks before merge." />
        {:else}
          <Table
            columns={[
              { key: 'name', label: 'Name' },
              { key: 'type', label: 'Type' },
              { key: 'command', label: 'Command' },
              { key: 'actions', label: '' },
            ]}
          >
            {#snippet children()}
              {#each gates as gate (gate.id)}
                <tr>
                  <td class="branch-name-cell">{gate.name}</td>
                  <td><Badge value={gate.gate_type ?? gate.kind ?? '—'} /></td>
                  <td class="secondary-cell mono">{gate.command ?? gate.config?.command ?? '—'}</td>
                  <td>
                    <button
                      class="gate-del-btn"
                      onclick={() => deleteGate(gate.id)}
                      disabled={deletingGateId === gate.id}
                      aria-label="Delete gate {gate.name}"
                    >
                      {deletingGateId === gate.id ? '…' : 'Delete'}
                    </button>
                  </td>
                </tr>
              {/each}
            {/snippet}
          </Table>
        {/if}

        <!-- Push gates -->
        {#if pushGates !== null}
          <div class="gates-section-hdr" style="margin-top: var(--space-6);">
            <h3 class="gates-section-title">Pre-accept Push Gates</h3>
          </div>
          <div class="push-gates-list">
            {#each BUILTIN_PUSH_GATES as gateName (gateName)}
              {@const active = (pushGates.gates ?? pushGates.active_gates ?? []).includes(gateName)}
              <label class="push-gate-toggle">
                <input
                  type="checkbox"
                  checked={active}
                  onchange={() => togglePushGate(gateName)}
                />
                <span class="push-gate-name">{gateName}</span>
                <Badge value={active ? 'active' : 'inactive'} variant={active ? 'success' : 'default'} />
              </label>
            {/each}
          </div>
        {/if}
      {/if}

    {:else if activeTab === 'aibom'}
      {#if aibomError}
        <div class="error-msg" role="alert">Error: {aibomError}</div>
      {:else if aibomLoading}
        <Skeleton lines={6} height="2.5rem" />
      {:else if !aibom}
        <EmptyState title="AIBOM not loaded" description="Loading AI Bill of Materials…" />
      {:else}
        <div class="aibom-header">
          <div class="aibom-stat">
            <span class="aibom-stat-value">{aibom.total_commits}</span>
            <span class="aibom-stat-label">AI Commits</span>
          </div>
          <div class="aibom-stat">
            <span class="aibom-stat-value">{aibom.agents.length}</span>
            <span class="aibom-stat-label">Agents</span>
          </div>
          <div class="aibom-stat">
            <span class="aibom-stat-value">{aibom.attested_percentage.toFixed(1)}%</span>
            <span class="aibom-stat-label">Attested</span>
          </div>
          <div class="aibom-stat aibom-version">
            <span class="aibom-stat-label">AIBOM {aibom.aibom_version}</span>
          </div>
        </div>

        {#if aibom.agents.length === 0}
          <EmptyState title="No AI commits" description="No agent-authored commits recorded for this repository." />
        {:else}
          <h3 class="aibom-section-title">Agent Contributions</h3>
          <Table
            columns={[
              { key: 'name', label: 'Agent' },
              { key: 'commits', label: 'Commits' },
              { key: 'model', label: 'Model' },
              { key: 'level', label: 'Attestation' },
            ]}
          >
            {#snippet children()}
              {#each aibom.agents as agent (agent.id)}
                {@const barPct = aibom.total_commits > 0 ? (agent.commit_count / aibom.total_commits * 100) : 0}
                <tr>
                  <td class="agent-name-cell">
                    <div class="agent-name">{agent.name}</div>
                    <div class="agent-id secondary-cell">{agent.id}</div>
                  </td>
                  <td>
                    <div class="commit-bar-wrap">
                      <div class="commit-bar" style="width: {barPct}%"></div>
                      <span class="commit-count">{agent.commit_count}</span>
                    </div>
                  </td>
                  <td class="secondary-cell">{agent.model ?? '—'}</td>
                  <td><Badge value={agent.attestation_level} variant={attestationVariant(agent.attestation_level)} /></td>
                </tr>
              {/each}
            {/snippet}
          </Table>

          {#if aibom.commits.length > 0}
            <h3 class="aibom-section-title">Commit Attribution</h3>
            <Table
              columns={[
                { key: 'sha', label: 'SHA' },
                { key: 'agent', label: 'Agent' },
                { key: 'task', label: 'Task' },
                { key: 'step', label: 'Ralph Step' },
                { key: 'level', label: 'Attestation' },
                { key: 'time', label: 'Time' },
              ]}
            >
              {#snippet children()}
                {#each aibom.commits as c (c.sha)}
                  <tr>
                    <td><code class="sha">{shortSha(c.sha)}</code></td>
                    <td class="secondary-cell">{c.agent_id}</td>
                    <td class="secondary-cell">{c.task_id ?? '—'}</td>
                    <td class="secondary-cell">{c.ralph_step ?? '—'}</td>
                    <td><Badge value={c.attestation_level} variant={attestationVariant(c.attestation_level)} /></td>
                    <td class="secondary-cell">{relativeTime(c.timestamp)}</td>
                  </tr>
                {/each}
              {/snippet}
            </Table>
          {/if}
        {/if}
      {/if}
    {:else if activeTab === 'jj'}
      <div class="jj-toolbar">
        <button class="jj-btn primary" onclick={initJj} disabled={jjInitLoading}>
          {jjInitLoading ? 'Initializing…' : 'Init jj'}
        </button>
        <button class="jj-btn" onclick={loadJjLog} disabled={jjLoading}>Refresh</button>
        {#if jjInitMsg}
          <span class="jj-success">{jjInitMsg}</span>
        {/if}
      </div>
      {#if jjError}
        <div class="error-msg" role="alert">{jjError}</div>
      {:else if jjLoading}
        <Skeleton lines={6} height="2.5rem" />
      {:else if jjChanges.length === 0}
        <EmptyState title="No jj changes" description="No jj changes found. Initialize jj first." />
      {:else}
        <Table
          columns={[
            { key: 'change_id', label: 'Change ID' },
            { key: 'description', label: 'Description' },
            { key: 'author', label: 'Author' },
            { key: 'bookmarks', label: 'Bookmarks' },
          ]}
        >
          {#snippet children()}
            {#each jjChanges as c (c.change_id)}
              <tr>
                <td><code class="sha">{c.change_id.slice(0, 8)}</code></td>
                <td class="commit-msg-cell">{c.description || '(no description)'}</td>
                <td class="secondary-cell">{c.author}</td>
                <td class="secondary-cell">{c.bookmarks.join(', ') || '—'}</td>
              </tr>
            {/each}
          {/snippet}
        </Table>
      {/if}
    {/if}
  </div>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .page-hdr {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .breadcrumb { display: flex; align-items: center; gap: var(--space-2); }

  .back-btn {
    background: none;
    border: none;
    color: var(--color-link);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: 0;
    transition: color var(--transition-fast);
  }

  .back-btn:hover { color: var(--color-link-hover); }

  .sep { color: var(--color-text-muted); }

  .repo-name {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .default-badge {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 0.15rem 0.5rem;
  }

  .clone-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-6);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .clone-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .clone-url-text {
    flex: 1;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 0.2rem var(--space-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .copy-btn {
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-link);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    padding: 0.2rem var(--space-3);
    cursor: pointer;
    white-space: nowrap;
    transition: all var(--transition-fast);
  }

  .copy-btn:hover { background: var(--color-surface-elevated); }

  .tabs-wrap { flex-shrink: 0; }

  .tab-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .commits-toolbar { flex-shrink: 0; }

  .branch-label {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .branch-select {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
  }

  .sha {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-surface-elevated);
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm);
  }

  .branch-name-cell {
    font-weight: 500;
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .commit-msg-cell {
    color: var(--color-text);
    max-width: 400px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .mr-title-cell { color: var(--color-text); font-weight: 500; }

  .secondary-cell { color: var(--color-text-secondary); font-size: var(--text-xs); }

  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }

  .clickable { cursor: pointer; }

  .agent-ref {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    background: var(--color-surface-elevated);
    padding: 0.1rem 0.3rem;
    border-radius: var(--radius-sm);
  }

  /* Activity tab */
  .activity-layout {
    display: flex;
    gap: var(--space-6);
    flex: 1;
    min-height: 0;
  }

  .hot-files-panel {
    width: 260px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .hot-files-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .hot-file-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    text-align: left;
    transition: all var(--transition-fast);
  }

  .hot-file-row:hover { background: var(--color-surface-elevated); }
  .hot-file-row.selected { border-color: var(--color-primary); background: var(--color-surface-elevated); }

  .hot-file-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .blame-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
    overflow: hidden;
  }

  .blame-empty { justify-content: center; }

  .blame-table {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    overflow-y: auto;
    flex: 1;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .blame-row {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    padding: 0.15rem var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  .blame-row:last-child { border-bottom: none; }
  .blame-row:hover { background: var(--color-surface-elevated); }

  .blame-lineno {
    width: 2.5rem;
    text-align: right;
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .blame-agent {
    width: 5rem;
    flex-shrink: 0;
    color: var(--color-primary);
  }

  .blame-content {
    color: var(--color-text);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Policy tab */
  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    margin: 0 0 var(--space-2);
  }

  .section-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0 0 var(--space-4);
  }

  .policy-section {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .policy-toggles { display: flex; flex-direction: column; gap: var(--space-2); }

  .toggle-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .toggle-label { flex: 1; }

  .policy-actions { display: flex; align-items: center; gap: var(--space-3); }

  .policy-btn {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .policy-btn:hover { background: var(--color-surface-elevated); }
  .policy-btn.primary { background: var(--color-primary); color: #fff; border-color: var(--color-primary); }
  .policy-btn.primary:hover { background: var(--color-primary-hover); }
  .policy-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .save-msg { font-size: var(--text-sm); color: var(--color-success); }

  .abac-list { display: flex; flex-direction: column; gap: var(--space-2); }

  .abac-card {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
  }

  .abac-card-hdr {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-2);
  }

  .abac-name { font-weight: 600; font-size: var(--text-sm); color: var(--color-text); }

  .abac-remove-btn {
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-danger);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    padding: 0.1rem var(--space-2);
    cursor: pointer;
  }

  .abac-rules { display: flex; flex-direction: column; gap: var(--space-1); }

  .abac-rule {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .rule-claim, .rule-value {
    font-family: var(--font-mono);
    background: var(--color-surface-elevated);
    padding: 0.1rem 0.4rem;
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
  }

  .rule-op { color: var(--color-text-muted); font-size: var(--text-xs); }

  .abac-add-form {
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .form-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    margin: 0;
  }

  .abac-form-row { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }

  .policy-input {
    flex: 1;
    min-width: 120px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
  }

  .policy-input:focus:not(:focus-visible) { outline: none; }
  .policy-input:focus-visible { outline: 2px solid var(--color-focus, #4db0ff); outline-offset: 2px; border-color: var(--color-focus, #4db0ff); }

  .policy-op-label {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    flex-shrink: 0;
    padding: 0 var(--space-1);
  }

  .inline-code {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  /* Gates tab */
  .gates-section-hdr {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
  }
  .gates-section-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }
  .gate-form {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }
  .gate-del-btn {
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius-sm);
    color: var(--color-danger);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    font-family: var(--font-body);
  }
  .gate-del-btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .push-gates-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }
  .push-gate-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .push-gate-name {
    flex: 1;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  /* jj */
  .jj-toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .jj-btn {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .jj-btn:hover { background: var(--color-surface-elevated); }
  .jj-btn.primary { background: var(--color-primary); color: #fff; border-color: var(--color-primary); }
  .jj-btn.primary:hover { background: var(--color-primary-hover); }
  .jj-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .jj-success { font-size: var(--text-sm); color: var(--color-success); }

  .error-msg {
    padding: var(--space-8);
    color: var(--color-danger);
    text-align: center;
    font-size: var(--text-sm);
  }

  /* AIBOM tab */
  .aibom-header {
    display: flex;
    gap: var(--space-6);
    padding: var(--space-4) 0;
    border-bottom: 1px solid var(--color-border);
    margin-bottom: var(--space-4);
    flex-shrink: 0;
  }

  .aibom-stat {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .aibom-stat-value {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
  }

  .aibom-stat-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .aibom-version { justify-content: flex-end; margin-left: auto; }

  .aibom-section-title {
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    margin: var(--space-4) 0 var(--space-2);
  }

  .agent-name-cell { min-width: 160px; }
  .agent-name { font-weight: 500; color: var(--color-text); }
  .agent-id { font-family: var(--font-mono); font-size: var(--text-xs); }

  .commit-bar-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 120px;
  }

  .commit-bar {
    height: 6px;
    background: var(--color-primary);
    border-radius: 3px;
    min-width: 2px;
    flex-shrink: 0;
  }

  .commit-count {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .back-btn:focus-visible,
  .toggle-row:focus-visible,
  .gate-del-btn:focus-visible,
  .push-gate-toggle:focus-visible,
  .jj-btn:focus-visible,
  .copy-btn:focus-visible,
  .abac-remove-btn:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }

  .branch-select:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }
</style>
