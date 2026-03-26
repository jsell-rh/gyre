<script>
  /**
   * MetaSpecs — S4.6 Meta-specs View.
   *
   * Spec ref: ui-layout.md §9 (Meta-specs Preview Loop Layout)
   *           human-system-interface.md §1 (meta-specs nav scope table)
   *
   * Props:
   *   workspaceId — string | null
   *   repoId      — string | null
   *   scope       — 'tenant' | 'workspace' | 'repo'
   */
  import { getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Button from '../lib/Button.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import Modal from '../lib/Modal.svelte';
  import InlineChat from '../lib/InlineChat.svelte';
  import DiffSuggestion from '../lib/DiffSuggestion.svelte';
  import { toastSuccess, toastError, toastInfo } from '../lib/toast.svelte.js';

  let { workspaceId = null, repoId = null, scope = 'workspace' } = $props();

  // Shell context (may be undefined in tests/standalone)
  const navigate = getContext('navigate');

  // ─── Shared constants ────────────────────────────────────────────────────────

  const KIND_LABELS = {
    'meta:persona':   'Persona',
    'meta:principle': 'Principle',
    'meta:standard':  'Standard',
    'meta:process':   'Process',
  };
  const KIND_COLORS = {
    'meta:persona':   'purple',
    'meta:principle': 'blue',
    'meta:standard':  'orange',
    'meta:process':   'green',
  };
  const META_KINDS = Object.keys(KIND_LABELS);

  function kindBadgeVariant(kind) { return KIND_COLORS[kind] || 'gray'; }
  function kindLabel(kind) { return KIND_LABELS[kind] || kind; }

  // ─── Shared state ────────────────────────────────────────────────────────────

  let loading = $state(true);
  let error   = $state(null);

  // ─── Tenant scope — catalog ──────────────────────────────────────────────────

  let specs      = $state([]);
  let kindFilter = $state('all');
  let detailSpec = $state(null);
  let detailTab  = $state('info');

  // Blast radius modal
  let blastOpen    = $state(false);
  let blastPath    = $state('');
  let blastLoading = $state(false);
  let blastResult  = $state(null);

  const filtered = $derived.by(() => {
    if (kindFilter === 'all') return specs;
    return specs.filter(s => s.kind === kindFilter);
  });

  async function loadTenantSpecs() {
    loading = true;
    error = null;
    try {
      const all = await api.getSpecs();
      specs = Array.isArray(all) ? all.filter(s => s.kind && s.kind.startsWith('meta:')) : [];
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  async function openBlastRadius(path) {
    blastPath = path;
    blastOpen = true;
    blastLoading = true;
    blastResult = null;
    try {
      blastResult = await api.getMetaSpecBlastRadius(path);
    } catch (e) {
      blastResult = { error: e.message };
    }
    blastLoading = false;
  }

  // ─── Workspace scope — editor + preview loop ─────────────────────────────────

  /** @type {'editing' | 'running' | 'complete'} */
  let previewState = $state('editing');

  let personas          = $state([]);
  let selectedPersonaId = $state('');
  let personaContent    = $state('');

  let targetSpecs       = $state([]);
  let selectedSpecPaths = $state([]);

  let previewId       = $state(null);
  let previewProgress = $state([]);   // [{path, status: 'running'|'complete'}]
  let previewInterval = $state(null);

  let impactTab       = $state('architecture');
  let previewApiResult  = $state(null);   // full API response when preview_id is used
  let isSimulatedPreview = $state(false); // true when falling back to client-side simulation

  let suggestions      = $state([]);
  let nextSuggestionId = 0;
  let publishSaving    = $state(false);

  async function loadWorkspaceData() {
    loading = true;
    error = null;
    try {
      const [ps, sp] = await Promise.all([
        api.personas().catch(() => []),
        api.getSpecs().catch(() => []),
      ]);
      personas = Array.isArray(ps) ? ps : [];
      if (personas.length > 0 && !selectedPersonaId) {
        selectedPersonaId = personas[0].id;
        personaContent = personas[0].system_prompt || '';
      }
      targetSpecs = Array.isArray(sp)
        ? sp.filter(s => !s.kind || !s.kind.startsWith('meta:'))
        : [];
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  function onPersonaChange(id) {
    selectedPersonaId = id;
    const p = personas.find(p => p.id === id);
    personaContent = p?.system_prompt || '';
    suggestions = [];
    previewState = 'editing';
    stopPreview();
  }

  function toggleSpec(path) {
    if (selectedSpecPaths.includes(path)) {
      selectedSpecPaths = selectedSpecPaths.filter(p => p !== path);
    } else {
      selectedSpecPaths = [...selectedSpecPaths, path];
    }
  }

  function selectAll() { selectedSpecPaths = targetSpecs.map(s => s.path); }
  function clearAll()  { selectedSpecPaths = []; }

  const canPreview = $derived.by(() => selectedSpecPaths.length > 0 && previewState === 'editing');

  async function startPreview() {
    previewState = 'running';
    previewProgress = selectedSpecPaths.map(path => ({ path, status: 'running' }));
    previewApiResult = null;
    isSimulatedPreview = false;

    let usedPreviewId = null;
    try {
      const res = await api.previewPersona(workspaceId, {
        persona_id: selectedPersonaId,
        content: personaContent,
        spec_paths: selectedSpecPaths,
      });
      usedPreviewId = res?.preview_id ?? null;
      if (res && !usedPreviewId) previewApiResult = res;
    } catch { toastInfo('Preview not available from server — showing example layout'); }

    if (usedPreviewId) {
      previewId = usedPreviewId;
      pollPreview();
    } else {
      isSimulatedPreview = true;
      simulatePreview();
    }
  }

  function pollPreview() {
    let elapsed = 0;
    previewInterval = setInterval(async () => {
      elapsed += 1500;
      try {
        const status = await api.previewPersonaStatus(workspaceId, previewId);
        previewProgress = status.specs ?? previewProgress;
        if (status.state === 'complete') {
          previewApiResult = status;
          isSimulatedPreview = false;
          stopPreview();
          previewState = 'complete';
        }
      } catch {
        clearInterval(previewInterval);
        isSimulatedPreview = true;
        simulatePreview();
      }
      if (elapsed > 30000) { stopPreview(); previewState = 'complete'; }
    }, 1500);
  }

  function simulatePreview() {
    let i = 0;
    previewInterval = setInterval(() => {
      if (i < previewProgress.length) {
        previewProgress = previewProgress.map((p, idx) =>
          idx === i ? { ...p, status: 'complete' } : p
        );
        i++;
      } else {
        stopPreview();
        previewState = 'complete';
      }
    }, 1200);
  }

  function stopPreview() {
    if (previewInterval) { clearInterval(previewInterval); previewInterval = null; }
  }

  function cancelPreview() { stopPreview(); previewState = 'editing'; previewProgress = []; }
  function iterate()       { stopPreview(); previewState = 'editing'; previewProgress = []; }

  async function publish() {
    if (!selectedPersonaId || !workspaceId) return;
    publishSaving = true;
    try {
      await api.publishPersona(workspaceId, selectedPersonaId, { content: personaContent });
      toastSuccess('Persona published successfully');
    } catch (e) {
      toastError('Failed to publish: ' + (e?.message ?? 'unknown error'));
    } finally {
      publishSaving = false;
    }
  }

  async function handleChatMessage(text) {
    try {
      const res = await fetch('/api/v1/specs/assist', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${localStorage.getItem('gyre_auth_token') || 'gyre-dev-token'}`,
        },
        body: JSON.stringify({ persona_id: selectedPersonaId, message: text }),
      });
      if (res.ok) return res;
    } catch { /* fall through */ }

    // Fallback mock suggestion
    const id = `suggestion-${nextSuggestionId++}`;
    suggestions = [...suggestions, { id, content: `+ ${text}\n# Suggested addition` }];
    return 'Suggestion added below the editor.';
  }

  function acceptSuggestion(s) {
    personaContent = personaContent + '\n\n' + s.content;
    suggestions = suggestions.filter(x => x.id !== s.id);
  }
  function dismissSuggestion(id) { suggestions = suggestions.filter(s => s.id !== id); }
  function editSuggestion(s) {
    personaContent = personaContent + '\n\n' + s.content;
    suggestions = suggestions.filter(x => x.id !== s.id);
  }

  // Repo scope redirect via shell
  function handleRepoRedirect() {
    if (navigate && workspaceId) {
      navigate('meta-specs', { scope: 'workspace', workspaceId });
    }
  }

  // ─── Lifecycle ───────────────────────────────────────────────────────────────

  $effect(() => {
    if (scope === 'workspace' || scope === 'repo') {
      loadWorkspaceData();
    } else {
      loadTenantSpecs();
    }
    return () => stopPreview();
  });
</script>

<!-- ─── Repo scope redirect ──────────────────────────────────────────────────── -->
{#if scope === 'repo'}
  <div class="meta-specs-view">
    <div class="view-header"><h2>Meta-Specs</h2></div>
    <div class="repo-redirect">
      Meta-specs are workspace-scoped.
      {#if workspaceId}
        <button class="link-btn" onclick={handleRepoRedirect}>View workspace editor</button>
      {:else}
        Select a workspace to edit meta-specs.
      {/if}
    </div>
    <!-- Fall through: still render workspace editor below for convenience -->
  </div>
{/if}

<!-- ─── Workspace scope — editor + preview loop ──────────────────────────────── -->
{#if scope === 'workspace' || scope === 'repo'}
  <div class="meta-specs-view workspace-view">
    {#if scope !== 'repo'}
      <div class="view-header">
        <h2>Meta-Specs</h2>
        <p class="subtitle">Edit persona prompts and preview impact across your workspace specs.</p>
      </div>
    {/if}

    {#if loading}
      <div class="split-layout"><div class="split-left"><Skeleton /></div><div class="split-right"><Skeleton /></div></div>
    {:else if error}
      <EmptyState title="Failed to load" description={error} />
    {:else}
      <div class="split-layout" data-testid="preview-loop">
        <!-- LEFT: Persona editor / diff view -->
        <div class="split-left">
          <div class="editor-header">
            <label class="persona-label" for="persona-select">Persona</label>
            <select
              id="persona-select"
              class="persona-select"
              value={selectedPersonaId}
              onchange={(e) => onPersonaChange(e.target.value)}
              disabled={previewState === 'running'}
            >
              {#each personas as p (p.id)}
                <option value={p.id}>{p.name}</option>
              {/each}
            </select>
          </div>

          {#if previewState === 'running'}
            <!-- Locked diff view -->
            <div class="persona-diff" aria-label="Persona diff (read-only)">
              {#each personaContent.split('\n') as line}
                <div class="diff-line {line.startsWith('+') ? 'add' : line.startsWith('-') ? 'remove' : 'ctx'}">{line}</div>
              {/each}
            </div>
          {:else}
            <!-- Editable textarea (editing + complete) -->
            <textarea
              class="persona-textarea"
              bind:value={personaContent}
              placeholder="Enter system prompt for this persona…"
              aria-label="Persona system prompt"
              data-testid="persona-textarea"
            ></textarea>

            {#each suggestions as s (s.id)}
              <DiffSuggestion
                suggestion={s}
                onaccept={() => acceptSuggestion(s)}
                onedit={() => editSuggestion(s)}
                ondismiss={() => dismissSuggestion(s.id)}
              />
            {/each}

            <InlineChat
              recipient="persona editor"
              recipientType="spec-edit"
              onmessage={handleChatMessage}
            />

            <div class="editor-actions">
              {#if previewState === 'complete'}
                <Button variant="secondary" onclick={iterate}>Iterate</Button>
              {:else}
                <Button variant="secondary" onclick={startPreview} disabled={!canPreview}>Preview</Button>
              {/if}
              <Button variant="primary" onclick={publish} disabled={publishSaving}>{publishSaving ? 'Publishing\u2026' : 'Publish'}</Button>
            </div>
          {/if}
        </div>

        <!-- RIGHT: Spec selector / progress / impact -->
        <div class="split-right">
          {#if previewState === 'editing'}
            <div class="spec-selector">
              <div class="spec-selector-header">
                <span class="spec-selector-title">Select specs to preview against:</span>
                <div class="spec-selector-shortcuts">
                  <button class="link-btn" onclick={selectAll}>Select All</button>
                  <button class="link-btn" onclick={clearAll}>Clear</button>
                </div>
              </div>
              <div class="spec-checklist">
                {#each targetSpecs as spec (spec.path)}
                  <label class="spec-check-item">
                    <input
                      type="checkbox"
                      checked={selectedSpecPaths.includes(spec.path)}
                      onchange={() => toggleSpec(spec.path)}
                    />
                    <span class="spec-check-path">{spec.path}</span>
                  </label>
                {:else}
                  <p class="empty-specs">No specs available in this workspace.</p>
                {/each}
              </div>
            </div>

          {:else if previewState === 'running'}
            <div class="preview-progress" data-testid="preview-running" aria-live="polite">
              <div class="progress-header">Preview: Running</div>
              <div class="progress-list">
                {#each previewProgress as item (item.path)}
                  <div class="progress-item">
                    <span class="progress-icon" aria-hidden="true">{item.status === 'complete' ? '✓' : '◐'}</span>
                    <span class="progress-path">{item.path}</span>
                    <span class="progress-status">{item.status === 'complete' ? 'Complete' : 'Agent implementing…'}</span>
                  </div>
                {/each}
              </div>
              <div class="progress-summary">
                Progress: {previewProgress.filter(p => p.status === 'complete').length}/{previewProgress.length} specs
              </div>
              <Button variant="secondary" onclick={cancelPreview}>Cancel Preview</Button>
            </div>

          {:else}
            <!-- State 3: Impact panel -->
            <div class="impact-panel" data-testid="preview-complete">
              {#if isSimulatedPreview}
                <div class="sim-banner" role="status">
                  ⚠ Preview unavailable — showing example layout only. Results are not based on real data.
                </div>
              {/if}
              <div class="impact-tabs">
                <button class="impact-tab" class:active={impactTab === 'architecture'} onclick={() => impactTab = 'architecture'}>Architecture</button>
                <button class="impact-tab" class:active={impactTab === 'code-diff'} onclick={() => impactTab = 'code-diff'}>Code Diff</button>
              </div>
              {#if isSimulatedPreview}
                <div class="impact-content impact-unavailable">
                  <span class="impact-unavailable-label">Preview unavailable — showing example layout.</span>
                  {#if impactTab === 'architecture'}
                    <div class="arch-diff">
                      <div class="arch-line add">+ ErrorHandler module (payment-domain)</div>
                      <div class="arch-line mod">~ ChargeService: +3 error result returns</div>
                      <div class="arch-line ctx">= 45 types unchanged</div>
                    </div>
                  {:else}
                    <div class="code-diff">
                      {#each previewProgress as item (item.path)}
                        <div class="code-diff-file">
                          <div class="code-diff-path">{item.path}</div>
                          <pre class="code-diff-body">--- original
+++ modified
@@ persona system prompt applied @@</pre>
                        </div>
                      {/each}
                    </div>
                  {/if}
                </div>
              {:else if impactTab === 'architecture'}
                <div class="impact-content arch-diff">
                  {#if previewApiResult?.architecture_diff?.length}
                    {#each previewApiResult.architecture_diff as line}
                      <div class="arch-line" class:add={line.startsWith('+')} class:mod={line.startsWith('~')} class:ctx={line.startsWith('=')}>{line}</div>
                    {/each}
                  {:else}
                    <span class="impact-empty">No architecture changes detected.</span>
                  {/if}
                </div>
              {:else}
                <div class="impact-content code-diff">
                  {#if previewApiResult?.specs_diff?.length}
                    {#each previewApiResult.specs_diff as item (item.path)}
                      <div class="code-diff-file">
                        <div class="code-diff-path">{item.path}</div>
                        <pre class="code-diff-body">{item.diff}</pre>
                      </div>
                    {/each}
                  {:else}
                    {#each previewProgress as item (item.path)}
                      <div class="code-diff-file">
                        <div class="code-diff-path">{item.path}</div>
                        <pre class="code-diff-body">No diff available.</pre>
                      </div>
                    {/each}
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </div>

<!-- ─── Tenant scope — catalog table ────────────────────────────────────────── -->
{:else}
  <div class="meta-specs-view">
    <div class="view-header">
      <h2>Meta-Specs</h2>
      <p class="subtitle">Versioned specs that govern agent behavior — personas, principles, standards, and process norms.</p>
    </div>

    <div class="filter-pills" role="tablist">
      <button class="pill" class:active={kindFilter === 'all'} onclick={() => kindFilter = 'all'} role="tab" aria-selected={kindFilter === 'all'}>All</button>
      {#each META_KINDS as k}
        <button class="pill" class:active={kindFilter === k} onclick={() => kindFilter = k} role="tab" aria-selected={kindFilter === k}>{KIND_LABELS[k]}</button>
      {/each}
    </div>

    {#if loading}
      <Skeleton />
    {:else if error}
      <EmptyState title="Failed to load meta-specs" description={error} />
    {:else if filtered.length === 0}
      <EmptyState
        title="No meta-specs found"
        description="Add meta-spec entries with kind: meta:persona (or principle/standard/process)."
      />
    {:else}
      <table class="catalog-table" data-testid="catalog-table">
        <thead>
          <tr>
            <th scope="col">Path</th>
            <th scope="col">Kind</th>
            <th scope="col">Name</th>
            <th scope="col">Status</th>
            <th scope="col">SHA</th>
            <th scope="col"></th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as spec (spec.path)}
            <tr
              class="catalog-row"
              onclick={() => { detailSpec = spec; detailTab = 'info'; }}
              role="button"
              tabindex="0"
              aria-label="View {spec.title || spec.path}"
              onkeydown={(e) => { if (e.key === 'Enter') { detailSpec = spec; detailTab = 'info'; } }}
            >
              <td class="mono cell-path">{spec.path}</td>
              <td><Badge value={kindLabel(spec.kind)} variant={kindBadgeVariant(spec.kind)} /></td>
              <td>{spec.title || '—'}</td>
              <td>
                <Badge
                  value={spec.approval_status || 'unknown'}
                  variant={spec.approval_status === 'approved' ? 'green' : spec.approval_status === 'pending' ? 'yellow' : 'gray'}
                />
              </td>
              <td class="mono cell-sha">{spec.current_sha?.slice(0, 8) || '—'}</td>
              <td>
                <Button variant="secondary" size="sm" onclick={(e) => { e.stopPropagation(); openBlastRadius(spec.path); }}>
                  Blast Radius
                </Button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>

  <!-- Detail panel modal -->
  {#if detailSpec}
    <Modal title={detailSpec.title || detailSpec.path} onclose={() => detailSpec = null}>
      <div class="detail-tabs">
        <button class="detail-tab" class:active={detailTab === 'info'} onclick={() => detailTab = 'info'}>Info</button>
        <button class="detail-tab" class:active={detailTab === 'content'} onclick={() => detailTab = 'content'}>Content</button>
      </div>
      {#if detailTab === 'info'}
        <div class="detail-info">
          <div class="detail-row"><span class="detail-key">Path</span><span class="mono">{detailSpec.path}</span></div>
          <div class="detail-row"><span class="detail-key">Kind</span><Badge value={kindLabel(detailSpec.kind)} variant={kindBadgeVariant(detailSpec.kind)} /></div>
          <div class="detail-row"><span class="detail-key">Status</span><span>{detailSpec.approval_status || '—'}</span></div>
          <div class="detail-row"><span class="detail-key">Owner</span><span>{detailSpec.owner || '—'}</span></div>
          <div class="detail-row"><span class="detail-key">SHA</span><span class="mono">{detailSpec.current_sha || '—'}</span></div>
        </div>
      {:else}
        <pre class="detail-content">{detailSpec.content || 'No content available.'}</pre>
      {/if}
    </Modal>
  {/if}

  <!-- Blast radius modal -->
  {#if blastOpen}
    <Modal title="Blast Radius: {blastPath}" onclose={() => blastOpen = false}>
      {#if blastLoading}
        <Skeleton />
      {:else if blastResult?.error}
        <p class="error">{blastResult.error}</p>
      {:else if blastResult}
        <div class="blast-section">
          <h4>Affected Workspaces ({blastResult.affected_workspaces?.length ?? 0})</h4>
          {#if blastResult.affected_workspaces?.length}
            <ul class="blast-list">
              {#each blastResult.affected_workspaces as ws}
                <li class="mono">{ws.id}</li>
              {/each}
            </ul>
          {:else}
            <p class="empty">No workspaces currently bind this meta-spec.</p>
          {/if}
        </div>
        <div class="blast-section">
          <h4>Affected Repos ({blastResult.affected_repos?.length ?? 0})</h4>
          {#if blastResult.affected_repos?.length}
            <ul class="blast-list">
              {#each blastResult.affected_repos as repo}
                <li><span class="mono">{repo.id}</span><Badge value={repo.reason} variant="gray" /></li>
              {/each}
            </ul>
          {:else}
            <p class="empty">No repos affected.</p>
          {/if}
        </div>
      {/if}
    </Modal>
  {/if}
{/if}

<style>
  .meta-specs-view {
    padding: var(--space-6, 1.5rem);
    max-width: 1400px;
  }

  .view-header { margin-bottom: 1.5rem; }
  .view-header h2 { margin: 0 0 0.25rem; font-size: 1.5rem; }
  .subtitle { margin: 0; color: var(--color-text-muted, #888); font-size: 0.9rem; }

  .repo-redirect {
    padding: 1rem;
    background: var(--color-surface-2, #1a1a1a);
    border: 1px solid var(--color-border, #333);
    border-radius: var(--radius, 6px);
    color: var(--color-text-muted, #888);
    font-size: 0.9rem;
    margin-bottom: 1rem;
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  /* ── Filter pills ── */
  .filter-pills { display: flex; gap: 0.5rem; flex-wrap: wrap; margin-bottom: 1.5rem; }
  .pill {
    padding: 0.3rem 0.8rem;
    border-radius: 999px;
    border: 1px solid var(--color-border, #333);
    background: transparent;
    color: var(--color-text, #eee);
    cursor: pointer;
    font-size: 0.85rem;
    transition: background 0.15s;
  }
  .pill:hover { background: var(--color-surface-2, #222); }
  .pill.active { background: var(--color-primary, #ee0000); border-color: var(--color-primary, #ee0000); color: #fff; }

  /* ── Catalog table ── */
  .catalog-table { width: 100%; border-collapse: collapse; font-size: 0.875rem; }
  .catalog-table th {
    text-align: left;
    padding: 0.5rem 0.75rem;
    border-bottom: 2px solid var(--color-border, #333);
    color: var(--color-text-muted, #888);
    font-weight: 600;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .catalog-row { cursor: pointer; transition: background 0.12s; }
  .catalog-row:hover { background: var(--color-surface-2, #1a1a1a); }
  .catalog-row td { padding: 0.6rem 0.75rem; border-bottom: 1px solid var(--color-border, #333); vertical-align: middle; }
  .cell-path { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cell-sha { font-size: 0.78rem; }

  /* ── Split layout (workspace scope) ── */
  .workspace-view { max-width: 1400px; }
  .split-layout { display: grid; grid-template-columns: 60fr 40fr; gap: 1.5rem; align-items: start; }
  .split-left { display: flex; flex-direction: column; gap: 1rem; }
  .split-right { display: flex; flex-direction: column; gap: 1rem; position: sticky; top: 1rem; }

  /* ── Editor ── */
  .editor-header { display: flex; align-items: center; gap: 0.75rem; }
  .persona-label { font-size: 0.85rem; font-weight: 600; color: var(--color-text-muted, #888); white-space: nowrap; }
  .persona-select {
    flex: 1;
    padding: 0.4rem 0.75rem;
    background: var(--color-surface-elevated, #1a1a1a);
    border: 1px solid var(--color-border-strong, #444);
    border-radius: var(--radius, 6px);
    color: var(--color-text, #eee);
    font-size: 0.875rem;
  }

  .persona-textarea {
    width: 100%;
    min-height: 280px;
    padding: 0.75rem;
    background: var(--color-surface-elevated, #141414);
    border: 1px solid var(--color-border-strong, #444);
    border-radius: var(--radius, 6px);
    color: var(--color-text, #eee);
    font-family: var(--font-mono, monospace);
    font-size: 0.85rem;
    line-height: 1.6;
    resize: vertical;
    box-sizing: border-box;
  }
  .persona-textarea:focus:not(:focus-visible) { outline: none; border-color: var(--color-primary, #ee0000); }
  .persona-textarea:focus-visible { outline: 2px solid var(--color-primary); outline-offset: 2px; border-color: var(--color-primary, #ee0000); }

  .persona-diff {
    min-height: 280px;
    padding: 0.75rem;
    background: var(--color-surface-elevated, #141414);
    border: 1px solid var(--color-border, #333);
    border-radius: var(--radius, 6px);
    font-family: var(--font-mono, monospace);
    font-size: 0.85rem;
    overflow-x: auto;
  }
  .diff-line { padding: 0 0.25rem; line-height: 1.5; }
  .diff-line.add { background: color-mix(in srgb, var(--color-success, #3fb950) 12%, transparent); color: var(--color-success, #3fb950); }
  .diff-line.remove { background: color-mix(in srgb, var(--color-danger) 12%, transparent); color: var(--color-danger); }
  .diff-line.ctx { color: var(--color-text-muted, #888); }

  .editor-actions { display: flex; gap: 0.5rem; justify-content: flex-end; }

  /* ── Spec selector ── */
  .spec-selector { background: var(--color-surface, #111); border: 1px solid var(--color-border, #333); border-radius: var(--radius, 6px); overflow: hidden; }
  .spec-selector-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.6rem 0.75rem;
    border-bottom: 1px solid var(--color-border, #333);
    background: var(--color-surface-elevated, #1a1a1a);
  }
  .spec-selector-title { font-size: 0.85rem; font-weight: 600; color: var(--color-text, #eee); }
  .spec-selector-shortcuts { display: flex; gap: 0.5rem; }
  .link-btn { background: none; border: none; color: var(--color-primary, #ee0000); font-size: 0.8rem; cursor: pointer; padding: 0; text-decoration: underline; font-family: var(--font-body, sans-serif); }
  .spec-checklist { max-height: 360px; overflow-y: auto; padding: 0.5rem 0; }
  .spec-check-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.3rem 0.75rem;
    cursor: pointer;
    font-size: 0.85rem;
    color: var(--color-text, #eee);
    transition: background 0.1s;
  }
  .spec-check-item:hover { background: var(--color-surface-2, #1a1a1a); }
  .spec-check-path { font-family: var(--font-mono, monospace); font-size: 0.8rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty-specs { padding: 1rem 0.75rem; color: var(--color-text-muted, #888); font-size: 0.85rem; margin: 0; }

  /* ── Preview progress ── */
  .preview-progress {
    background: var(--color-surface, #111);
    border: 1px solid var(--color-border, #333);
    border-radius: var(--radius, 6px);
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .progress-header { font-weight: 600; font-size: 0.95rem; color: var(--color-text, #eee); }
  .progress-list { display: flex; flex-direction: column; gap: 0.4rem; }
  .progress-item { display: flex; align-items: center; gap: 0.5rem; font-size: 0.85rem; }
  .progress-icon { font-size: 0.9rem; width: 1.2rem; text-align: center; }
  .progress-path { font-family: var(--font-mono, monospace); flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--color-text, #eee); }
  .progress-status { color: var(--color-text-muted, #888); font-size: 0.8rem; white-space: nowrap; }
  .progress-summary { font-size: 0.85rem; color: var(--color-text-muted, #888); }

  /* ── Impact panel ── */
  .impact-panel { background: var(--color-surface, #111); border: 1px solid var(--color-border, #333); border-radius: var(--radius, 6px); overflow: hidden; }
  .impact-tabs { display: flex; border-bottom: 1px solid var(--color-border, #333); }
  .impact-tab { padding: 0.5rem 1rem; background: none; border: none; border-bottom: 2px solid transparent; color: var(--color-text-muted, #888); cursor: pointer; font-size: 0.875rem; transition: color 0.15s; font-family: var(--font-body, sans-serif); }
  .impact-tab.active { color: var(--color-text, #eee); border-bottom-color: var(--color-primary, #ee0000); }
  .impact-content { padding: 1rem; font-size: 0.875rem; }
  .arch-diff { display: flex; flex-direction: column; gap: 0.3rem; }
  .arch-line { padding: 0.2rem 0.4rem; border-radius: 3px; font-family: var(--font-mono, monospace); }
  .arch-line.add { color: var(--color-success, #3fb950); background: color-mix(in srgb, var(--color-success, #3fb950) 10%, transparent); }
  .arch-line.mod { color: var(--color-warning, #d29922); background: color-mix(in srgb, var(--color-warning, #d29922) 10%, transparent); }
  .arch-line.ctx { color: var(--color-text-muted, #888); }
  .code-diff { display: flex; flex-direction: column; gap: 0.75rem; }
  .code-diff-file { border: 1px solid var(--color-border, #333); border-radius: 4px; overflow: hidden; }
  .code-diff-path { padding: 0.3rem 0.6rem; background: var(--color-surface-elevated, #1a1a1a); font-family: var(--font-mono, monospace); font-size: 0.8rem; color: var(--color-text-muted, #888); border-bottom: 1px solid var(--color-border, #333); }
  .code-diff-body { margin: 0; padding: 0.5rem 0.75rem; font-family: var(--font-mono, monospace); font-size: 0.8rem; color: var(--color-text, #eee); line-height: 1.5; }
  .impact-unavailable { display: flex; flex-direction: column; gap: 0.75rem; }
  .sim-banner { background: color-mix(in srgb, var(--color-warning) 12%, transparent); border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent); border-radius: var(--radius); padding: var(--space-2) var(--space-3); font-size: var(--text-sm); color: var(--color-text-secondary); margin: var(--space-3) var(--space-3) 0; }
  .impact-unavailable-label { font-size: 0.78rem; color: var(--color-text-muted, #888); font-style: italic; }
  .impact-empty { font-size: 0.85rem; color: var(--color-text-muted, #888); }

  /* ── Detail panel ── */
  .detail-tabs { display: flex; margin-bottom: 1rem; border-bottom: 1px solid var(--color-border, #333); }
  .detail-tab { padding: 0.4rem 1rem; background: none; border: none; border-bottom: 2px solid transparent; color: var(--color-text-muted, #888); cursor: pointer; font-size: 0.875rem; font-family: var(--font-body, sans-serif); }
  .detail-tab.active { color: var(--color-text, #eee); border-bottom-color: var(--color-primary, #ee0000); }
  .detail-info { display: flex; flex-direction: column; gap: 0.6rem; }
  .detail-row { display: flex; align-items: center; gap: 0.75rem; }
  .detail-key { font-size: 0.8rem; font-weight: 600; color: var(--color-text-muted, #888); min-width: 60px; }
  .detail-content { font-family: var(--font-mono, monospace); font-size: 0.85rem; line-height: 1.6; color: var(--color-text, #eee); white-space: pre-wrap; word-break: break-word; margin: 0; }

  /* ── Blast radius ── */
  .blast-section { margin-bottom: 1.25rem; }
  .blast-section h4 { margin: 0 0 0.5rem; font-size: 0.95rem; }
  .blast-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.35rem; }
  .blast-list li { display: flex; align-items: center; gap: 0.5rem; font-size: 0.88rem; padding: 0.3rem 0.5rem; background: var(--color-surface-2, #1a1a1a); border-radius: 4px; }

  .mono { font-family: monospace; }
  .empty { color: var(--color-text-muted, #888); font-size: 0.88rem; margin: 0; }
  .error { color: var(--color-error, #f55); font-size: 0.88rem; }

  /* Focus-visible for interactive elements */
  .pill:focus-visible,
  .impact-tab:focus-visible,
  .detail-tab:focus-visible,
  .link-btn:focus-visible,
  .persona-select:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
</style>
