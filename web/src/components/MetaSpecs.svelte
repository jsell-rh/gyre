<script>
  /**
   * MetaSpecs — S4.6 Meta-specs View (first-class creative surface).
   *
   * Meta-specs are the human's PRIMARY ENCODING MECHANISM — personas, principles,
   * standards, and process norms that govern every agent in the platform. This view
   * treats them accordingly: a rich editor-first surface, not an admin config panel.
   *
   * Layout (tenant scope):
   *   [Sidebar: list + filter] | [Editor: Edit | Impact | History | Approval tabs]
   *
   * Spec ref: ui-layout.md §9, human-system-interface.md §1, agent-runtime spec §2
   *
   * Props:
   *   workspaceId — string | null
   *   repoId      — string | null
   *   scope       — 'tenant' | 'workspace' | 'repo'
   */
  import { getContext, onDestroy } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { entityName } from '../lib/entityNames.svelte.js';
  import Badge from '../lib/Badge.svelte';
  import Button from '../lib/Button.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import Modal from '../lib/Modal.svelte';
  import InlineChat from '../lib/InlineChat.svelte';
  import DiffSuggestion from '../lib/DiffSuggestion.svelte';
  import { toastSuccess, toastError, toastInfo } from '../lib/toast.svelte.js';

  let { workspaceId = null, repoId = null, scope = 'workspace' } = $props();

  const navigate = getContext('navigate');

  // Entity name resolution uses shared singleton cache
  function resolveEntityName(type, id) {
    return entityName(type, id);
  }

  // ─── Constants ───────────────────────────────────────────────────────────────

  const KIND_COLORS = {
    'meta:persona':   'purple',
    'meta:principle': 'info',
    'meta:standard':  'warning',
    'meta:process':   'success',
  };
  const META_KINDS = ['meta:persona', 'meta:principle', 'meta:standard', 'meta:process'];

  function kindBadgeVariant(kind) { return KIND_COLORS[kind] || 'muted'; }
  function kindLabel(kind) { return $t(`meta_specs.kind_labels.${kind}`) || kind; }

  function approvalVariant(status) {
    if (status === 'Approved') return 'success';
    if (status === 'Pending') return 'warning';
    if (status === 'Rejected') return 'danger';
    return 'muted';
  }

  function approvalIcon(status) {
    if (status === 'Approved') return '✓';
    if (status === 'Pending') return '◎';
    if (status === 'Rejected') return '✗';
    return '?';
  }

  // ─── Shared state ────────────────────────────────────────────────────────────

  let loading = $state(true);
  let error   = $state(null);

  // ─── Tenant scope — sidebar + editor ─────────────────────────────────────────

  let specs      = $state([]);
  let kindFilter = $state('all');
  let selected   = $state(null);   // selected meta-spec
  let editorTab  = $state('edit'); // 'edit' | 'impact' | 'history' | 'approval'

  // Edit tab
  let editContent  = $state('');
  let editDirty    = $state(false);
  let editSaving   = $state(false);
  let editSuggestions     = $state([]);
  let nextSuggestionId    = 0;

  // Impact tab
  let blastLoading = $state(false);
  let blastResult  = $state(null);

  // History tab
  let versions        = $state([]);
  let versionsLoading = $state(false);
  let diffVersion     = $state(null); // version to show diff for

  // Approval tab
  let approvalSaving = $state(null); // 'approve' | 'reject' | null

  // Create flow
  let createMode = $state(false); // true = showing create panel instead of list
  let createForm = $state({ kind: 'meta:persona', name: '', scope: 'Global', scope_id: '', prompt: '', required: false });
  let createSaving = $state(false);

  // Delete
  let deleteTarget = $state(null);
  let deleteSaving = $state(false);

  // Discard-changes confirmation (replaces native confirm())
  let discardTarget = $state(null); // { action: 'select', spec } | { action: 'create' }

  // Required toggle
  let requiredSaving = $state(null); // spec id

  const filtered = $derived.by(() => {
    if (kindFilter === 'all') return specs;
    return specs.filter(s => s.kind === kindFilter);
  });

  async function loadTenantSpecs() {
    loading = true;
    error = null;
    try {
      const all = await api.getMetaSpecs();
      specs = Array.isArray(all) ? all : [];
      // Auto-select first if nothing selected
      if (specs.length > 0 && !selected) {
        selectSpec(specs[0], true);
      }
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  function selectSpec(spec, force = false) {
    if (!force && editDirty && selected && selected.id !== spec.id) {
      discardTarget = { action: 'select', spec };
      return;
    }
    selected = spec;
    editContent = spec.prompt || '';
    editDirty = false;
    editorTab = 'edit';
    blastResult = null;
    versions = [];
    diffVersion = null;
    editSuggestions = [];
  }

  function onEditorTabChange(tab) {
    editorTab = tab;
    if (tab === 'impact' && selected && !blastResult) {
      loadBlastRadius();
    }
    if (tab === 'history' && selected && versions.length === 0) {
      loadVersionHistory();
    }
  }

  async function loadBlastRadius() {
    if (!selected) return;
    blastLoading = true;
    blastResult = null;
    try {
      blastResult = await api.getMetaSpecBlastRadius(selected.id);
    } catch (e) {
      blastResult = { error: e.message };
    }
    blastLoading = false;
  }

  async function loadVersionHistory() {
    if (!selected) return;
    versionsLoading = true;
    versions = [];
    try {
      versions = await api.getMetaSpecVersions(selected.id);
    } catch (e) {
      versions = [];
    }
    versionsLoading = false;
  }

  async function saveEdit() {
    if (!selected || !editDirty) return;
    editSaving = true;
    try {
      const updated = await api.updateMetaSpec(selected.id, { prompt: editContent });
      specs = specs.map(s => s.id === selected.id ? updated : s);
      selected = updated;
      editDirty = false;
      toastSuccess($t('meta_specs.toast.saved_version', { values: { version: updated.version } }));
    } catch (e) {
      toastError($t('meta_specs.toast.save_failed', { values: { error: e?.message ?? 'unknown error' } }));
    }
    editSaving = false;
  }

  async function handleApprove() {
    if (!selected) return;
    approvalSaving = 'approve';
    try {
      const updated = await api.updateMetaSpec(selected.id, { approval_status: 'Approved' });
      specs = specs.map(s => s.id === selected.id ? updated : s);
      selected = updated;
      toastSuccess($t('meta_specs.toast.approved'));
    } catch (e) {
      toastError($t('meta_specs.toast.approve_failed', { values: { error: e?.message ?? 'unknown error' } }));
    }
    approvalSaving = null;
  }

  async function handleReject() {
    if (!selected) return;
    approvalSaving = 'reject';
    try {
      const updated = await api.updateMetaSpec(selected.id, { approval_status: 'Rejected' });
      specs = specs.map(s => s.id === selected.id ? updated : s);
      selected = updated;
      toastSuccess($t('meta_specs.toast.rejected'));
    } catch (e) {
      toastError($t('meta_specs.toast.reject_failed', { values: { error: e?.message ?? 'unknown error' } }));
    }
    approvalSaving = null;
  }

  async function handleRequiredToggle(spec) {
    requiredSaving = spec.id;
    try {
      const updated = await api.updateMetaSpec(spec.id, { required: !spec.required });
      specs = specs.map(s => s.id === spec.id ? updated : s);
      if (selected?.id === spec.id) selected = updated;
      toastSuccess(updated.required ? $t('meta_specs.toast.marked_required') : $t('meta_specs.toast.marked_optional'));
    } catch (e) {
      toastError($t('meta_specs.toast.update_failed', { values: { error: e?.message ?? 'unknown error' } }));
    }
    requiredSaving = null;
  }

  async function handleCreate() {
    if (!createForm.name.trim()) { toastError($t('meta_specs.toast.name_required')); return; }
    createSaving = true;
    try {
      const payload = {
        kind: createForm.kind,
        name: createForm.name.trim(),
        scope: createForm.scope,
        prompt: createForm.prompt,
        required: createForm.required,
      };
      if (createForm.scope === 'Workspace' && createForm.scope_id.trim()) {
        payload.scope_id = createForm.scope_id.trim();
      }
      const created = await api.createMetaSpec(payload);
      specs = [created, ...specs];
      createMode = false;
      createForm = { kind: 'meta:persona', name: '', scope: 'Global', scope_id: '', prompt: '', required: false };
      selectSpec(created, true);
      toastSuccess($t('meta_specs.toast.created', { values: { name: created.name } }));
    } catch (e) {
      toastError($t('meta_specs.toast.create_failed', { values: { error: e?.message ?? 'unknown error' } }));
    }
    createSaving = false;
  }

  async function handleDelete() {
    if (!deleteTarget) return;
    deleteSaving = true;
    try {
      await api.deleteMetaSpec(deleteTarget.id);
      specs = specs.filter(s => s.id !== deleteTarget.id);
      if (selected?.id === deleteTarget.id) {
        if (specs.length > 0) {
          selectSpec(specs[0]);
        } else {
          selected = null;
          editContent = '';
          editDirty = false;
          editSuggestions = [];
          blastResult = null;
          versions = [];
          diffVersion = null;
        }
      }
      deleteTarget = null;
      toastSuccess($t('meta_specs.toast.deleted'));
    } catch (e) {
      toastError($t('meta_specs.toast.delete_failed', { values: { error: e?.message ?? 'unknown error' } }));
    }
    deleteSaving = false;
  }

  async function handleChatMessage(text) {
    // Try LLM assist via specsAssist (repo-scoped) if repoId available
    if (repoId) {
      try {
        const res = await api.specsAssist(repoId, {
          message: text,
          context: editContent,
          kind: selected?.kind,
        });
        if (res.ok) return res;
      } catch { /* fall through */ }
    }
    // Fallback: add as suggestion
    const id = `suggestion-${nextSuggestionId++}`;
    editSuggestions = [...editSuggestions, { id, content: `# Suggested addition\n${text}` }];
    return $t('meta_specs.toast.suggestion_added');
  }

  function acceptSuggestion(s) {
    editContent = editContent + '\n\n' + s.content;
    editDirty = true;
    editSuggestions = editSuggestions.filter(x => x.id !== s.id);
  }
  function dismissSuggestion(id) { editSuggestions = editSuggestions.filter(s => s.id !== id); }

  // Diff between two version contents
  function computeDiff(older, newer) {
    if (!older || !newer) return [];
    const oldLines = older.split('\n');
    const newLines = newer.split('\n');
    // Simple line-by-line diff
    const result = [];
    const maxLen = Math.max(oldLines.length, newLines.length);
    for (let i = 0; i < maxLen; i++) {
      const o = oldLines[i];
      const n = newLines[i];
      if (o === undefined) result.push({ type: 'add', text: n });
      else if (n === undefined) result.push({ type: 'remove', text: o });
      else if (o !== n) {
        result.push({ type: 'remove', text: o });
        result.push({ type: 'add', text: n });
      } else {
        result.push({ type: 'ctx', text: o });
      }
    }
    return result;
  }

  // ─── Workspace scope — preview loop ──────────────────────────────────────────

  let wsLoading   = $state(true);
  let wsError     = $state(null);

  /** @type {'editing' | 'running' | 'complete'} */
  let previewState = $state('editing');

  let wsMetaSpecs       = $state([]);   // all kinds, not just personas
  let selectedMsId      = $state('');
  let selectedMsContent = $state('');

  let targetSpecs       = $state([]);
  let selectedSpecPaths = $state([]);

  let previewId        = null;
  let previewProgress  = $state([]);
  let previewInterval  = null;

  let impactTab        = $state('architecture');
  let previewApiResult = $state(null);
  let isSimulatedPreview = $state(false);

  let wsSuggestions     = $state([]);
  let wsNextSuggId      = 0;
  let publishSaving     = $state(false);

  async function loadWorkspaceData() {
    wsLoading = true;
    wsError = null;
    try {
      const [ms, sp] = await Promise.all([
        api.getMetaSpecs().catch(() => []),
        api.getSpecs().catch(() => []),
      ]);
      wsMetaSpecs = Array.isArray(ms) ? ms : [];
      if (wsMetaSpecs.length > 0 && !selectedMsId) {
        selectedMsId = wsMetaSpecs[0].id;
        selectedMsContent = wsMetaSpecs[0].prompt || '';
      }
      targetSpecs = Array.isArray(sp)
        ? sp.filter(s => !s.kind || !s.kind.startsWith('meta:'))
        : [];
    } catch (e) {
      wsError = e.message;
    }
    wsLoading = false;
  }

  function onMsChange(id) {
    selectedMsId = id;
    const ms = wsMetaSpecs.find(m => m.id === id);
    selectedMsContent = ms?.prompt || '';
    wsSuggestions = [];
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
    // Clear any lingering interval from a prior preview run
    stopPreview();
    previewState = 'running';
    previewProgress = selectedSpecPaths.map(path => ({ path, status: 'running' }));
    previewApiResult = null;
    isSimulatedPreview = false;

    let usedPreviewId = null;
    try {
      const res = await api.previewPersona(workspaceId, {
        persona_id: selectedMsId,
        content: selectedMsContent,
        spec_paths: selectedSpecPaths,
      });
      usedPreviewId = res?.preview_id ?? null;
      if (res && !usedPreviewId) previewApiResult = res;
    } catch { toastInfo($t('meta_specs.toast.preview_unavailable')); }

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
    if (!selectedMsId || !workspaceId) return;
    publishSaving = true;
    try {
      await api.publishPersona(workspaceId, selectedMsId, { content: selectedMsContent });
      toastSuccess($t('meta_specs.toast.published'));
    } catch (e) {
      toastError($t('meta_specs.toast.publish_failed', { values: { error: e?.message ?? 'unknown error' } }));
    } finally {
      publishSaving = false;
    }
  }

  async function handleWsChatMessage(text) {
    if (repoId) {
      try {
        const res = await api.specsAssist(repoId, { message: text, context: selectedMsContent });
        if (res.ok) return res;
      } catch { /* fall through */ }
    }
    try {
      const res = await api.specsAssistGlobal({ persona_id: selectedMsId, message: text });
      if (res.ok) return res;
    } catch { /* fall through */ }

    const id = `ws-suggestion-${wsNextSuggId++}`;
    wsSuggestions = [...wsSuggestions, { id, content: `+ ${text}\n# Suggested addition` }];
    return $t('meta_specs.toast.suggestion_added');
  }

  function wsAcceptSuggestion(s) {
    selectedMsContent = selectedMsContent + '\n\n' + s.content;
    wsSuggestions = wsSuggestions.filter(x => x.id !== s.id);
  }
  function wsDismissSuggestion(id) { wsSuggestions = wsSuggestions.filter(s => s.id !== id); }
  function wsEditSuggestion(s) {
    selectedMsContent = selectedMsContent + '\n\n' + s.content;
    wsSuggestions = wsSuggestions.filter(x => x.id !== s.id);
  }

  function handleRepoRedirect() {
    if (navigate && workspaceId) navigate('meta-specs', { scope: 'workspace', workspaceId });
  }

  // ─── Lifecycle ───────────────────────────────────────────────────────────────

  // onDestroy guarantees cleanup even if the $effect cleanup is skipped
  // (e.g. parent unmounts this component while a preview interval is active).
  onDestroy(() => stopPreview());

  $effect(() => {
    if (scope === 'workspace' || scope === 'repo') {
      loadWorkspaceData();
    } else {
      loadTenantSpecs();
    }
    return () => stopPreview();
  });
</script>

<svelte:window onkeydown={(e) => {
  if (e.key === 'Escape' && previewState === 'running') cancelPreview();
  if (e.key === 'Escape' && createMode) createMode = false;
}} onbeforeunload={(e) => {
  if (editDirty) { e.preventDefault(); return ''; }
}} />

<!-- ─── Repo scope redirect ──────────────────────────────────────────────────── -->
{#if scope === 'repo'}
  <div class="meta-specs-view">
    <div class="view-header"><h1 class="page-title">{$t('meta_specs.title')}</h1></div>
    <div class="repo-redirect">
      {$t('meta_specs.repo_redirect.workspace_scoped')}
      {#if workspaceId}
        <button class="link-btn" onclick={handleRepoRedirect}>{$t('meta_specs.repo_redirect.view_workspace_editor')}</button>
      {:else}
        {$t('meta_specs.repo_redirect.select_workspace')}
      {/if}
    </div>
  </div>
{/if}

<!-- ─── Workspace scope — preview loop ───────────────────────────────────────── -->
{#if scope === 'workspace'}
  <div class="meta-specs-view workspace-view" aria-busy={wsLoading}>
    <div class="view-header">
      <h2>{$t('meta_specs.title')}</h2>
      <p class="subtitle">{$t('meta_specs.workspace.subtitle')}</p>
    </div>

    {#if wsLoading}
      <div class="split-layout"><div class="split-left"><Skeleton /></div><div class="split-right"><Skeleton /></div></div>
    {:else if wsError}
      <div role="alert"><EmptyState title={$t('meta_specs.workspace.failed_to_load')} description={wsError} /></div>
      <button class="retry-btn" onclick={loadWorkspaceData}>{$t('common.retry')}</button>
    {:else}
      <div class="split-layout" data-testid="preview-loop">
        <!-- LEFT: Meta-spec editor -->
        <div class="split-left">
          <div class="editor-header">
            <label class="persona-label" for="ms-select">{$t('meta_specs.workspace.meta_spec_label')}</label>
            <select
              id="ms-select"
              class="persona-select"
              value={selectedMsId}
              onchange={(e) => onMsChange(e.target.value)}
              disabled={previewState === 'running'}
              aria-label={$t('meta_specs.workspace.persona_aria')}
            >
              {#each wsMetaSpecs as ms (ms.id)}
                <option value={ms.id}>[{kindLabel(ms.kind)}] {ms.name}</option>
              {/each}
            </select>
          </div>

          {#if previewState === 'running'}
            <div class="persona-diff" role="region" aria-label={$t('meta_specs.workspace.diff_readonly_aria')}>
              {#each selectedMsContent.split('\n') as line}
                <div class="diff-line {line.startsWith('+') ? 'add' : line.startsWith('-') ? 'remove' : 'ctx'}">{line}</div>
              {/each}
            </div>
          {:else}
            <textarea
              class="persona-textarea"
              bind:value={selectedMsContent}
              placeholder={$t('meta_specs.workspace.textarea_placeholder')}
              aria-label={$t('meta_specs.workspace.textarea_aria')}
              data-testid="persona-textarea"
            ></textarea>

            {#each wsSuggestions as s (s.id)}
              <DiffSuggestion
                suggestion={s}
                onaccept={() => wsAcceptSuggestion(s)}
                onedit={() => wsEditSuggestion(s)}
                ondismiss={() => wsDismissSuggestion(s.id)}
              />
            {/each}

            <InlineChat
              recipient="meta-spec editor"
              recipientType="spec-edit"
              onmessage={handleWsChatMessage}
            />

            <div class="editor-actions">
              {#if previewState === 'complete'}
                <Button variant="secondary" onclick={iterate}>{$t('meta_specs.workspace.iterate')}</Button>
              {:else}
                <Button variant="secondary" onclick={startPreview} disabled={!canPreview}>{$t('meta_specs.workspace.preview')}</Button>
              {/if}
              <Button variant="primary" onclick={publish} disabled={publishSaving}>{publishSaving ? $t('meta_specs.workspace.publishing') : $t('meta_specs.workspace.publish')}</Button>
            </div>
          {/if}
        </div>

        <!-- RIGHT: Spec selector / progress / impact -->
        <div class="split-right">
          {#if previewState === 'editing'}
            <div class="spec-selector">
              <div class="spec-selector-header">
                <span class="spec-selector-title">{$t('meta_specs.workspace.preview_against')}</span>
                <div class="spec-selector-shortcuts">
                  <button class="link-btn" onclick={selectAll}>{$t('meta_specs.workspace.select_all')}</button>
                  <button class="link-btn" onclick={clearAll}>{$t('meta_specs.workspace.clear')}</button>
                </div>
              </div>
              <div class="spec-checklist">
                {#each targetSpecs as spec (spec.path)}
                  <label class="spec-check-item">
                    <input type="checkbox" checked={selectedSpecPaths.includes(spec.path)} onchange={() => toggleSpec(spec.path)} />
                    <span class="spec-check-path">{spec.path}</span>
                  </label>
                {:else}
                  <p class="empty-specs">{$t('meta_specs.workspace.no_specs_available')}</p>
                {/each}
              </div>
            </div>

          {:else if previewState === 'running'}
            <div class="preview-progress" data-testid="preview-running" aria-live="polite">
              <div class="progress-header" role="status">{$t('meta_specs.workspace.preview_running')}</div>
              <div class="progress-list">
                {#each previewProgress as item (item.path)}
                  <div class="progress-item">
                    <span class="progress-icon" aria-hidden="true">{item.status === 'complete' ? '✓' : '◐'}</span>
                    <span class="progress-path">{item.path}</span>
                    <span class="progress-status">{item.status === 'complete' ? $t('meta_specs.workspace.status_complete') : $t('meta_specs.workspace.agent_implementing')}</span>
                  </div>
                {/each}
              </div>
              <div class="progress-summary">{$t('meta_specs.workspace.progress_label', { values: { done: previewProgress.filter(p => p.status === 'complete').length, total: previewProgress.length } })}</div>
              <Button variant="secondary" onclick={cancelPreview}>{$t('meta_specs.workspace.cancel_preview')} <kbd>Esc</kbd></Button>
            </div>

          {:else}
            <div class="impact-panel" data-testid="preview-complete">
              {#if isSimulatedPreview}
                <div class="sim-banner" role="status">{$t('meta_specs.workspace.sim_banner')}</div>
              {/if}
              <div class="impact-tabs" role="tablist" aria-label={$t('meta_specs.workspace.impact_view_aria')} tabindex="0"
                onkeydown={(e) => {
                  const tabs = ['architecture', 'code-diff'];
                  const ids = ['impact-tab-arch', 'impact-tab-diff'];
                  const idx = tabs.indexOf(impactTab);
                  if (e.key === 'ArrowRight') { e.preventDefault(); const ni = (idx + 1) % 2; impactTab = tabs[ni]; document.getElementById(ids[ni])?.focus(); }
                  if (e.key === 'ArrowLeft')  { e.preventDefault(); const ni = (idx - 1 + 2) % 2; impactTab = tabs[ni]; document.getElementById(ids[ni])?.focus(); }
                }}
              >
                <button class="impact-tab" role="tab" id="impact-tab-arch" aria-controls="impact-panel-arch" aria-selected={impactTab === 'architecture'} class:active={impactTab === 'architecture'} tabindex={impactTab === 'architecture' ? 0 : -1} onclick={() => impactTab = 'architecture'}>{$t('meta_specs.workspace.architecture')}</button>
                <button class="impact-tab" role="tab" id="impact-tab-diff" aria-controls="impact-panel-diff" aria-selected={impactTab === 'code-diff'} class:active={impactTab === 'code-diff'} tabindex={impactTab === 'code-diff' ? 0 : -1} onclick={() => impactTab = 'code-diff'}>{$t('meta_specs.workspace.code_diff')}</button>
              </div>
              {#if isSimulatedPreview}
                <div class="impact-content impact-unavailable" role="tabpanel" id={impactTab === 'architecture' ? 'impact-panel-arch' : 'impact-panel-diff'} aria-labelledby={impactTab === 'architecture' ? 'impact-tab-arch' : 'impact-tab-diff'}>
                  <span class="impact-unavailable-label">{$t('meta_specs.workspace.preview_unavailable_label')}</span>
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
@@ meta-spec applied @@</pre>
                        </div>
                      {/each}
                    </div>
                  {/if}
                </div>
              {:else if impactTab === 'architecture'}
                <div class="impact-content arch-diff" role="tabpanel" id="impact-panel-arch" aria-labelledby="impact-tab-arch">
                  {#if previewApiResult?.architecture_diff?.length}
                    {#each previewApiResult.architecture_diff as line}
                      <div class="arch-line" class:add={line.startsWith('+')} class:mod={line.startsWith('~')} class:ctx={line.startsWith('=')}>{line}</div>
                    {/each}
                  {:else}
                    <span class="impact-empty">{$t('meta_specs.workspace.no_arch_changes')}</span>
                  {/if}
                </div>
              {:else}
                <div class="impact-content code-diff" role="tabpanel" id="impact-panel-diff" aria-labelledby="impact-tab-diff">
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
                        <pre class="code-diff-body">{$t('meta_specs.workspace.no_diff_available')}</pre>
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

<!-- ─── Tenant scope — split panel creative surface ───────────────────────────── -->
{:else}
  <div class="meta-specs-view tenant-view" aria-busy={loading}>
    <!-- Top bar -->
    <div class="top-bar">
      <div class="top-bar-left">
        <h1 class="page-title">{$t('meta_specs.title')}</h1>
        <p class="subtitle">{$t('meta_specs.subtitle')}</p>
      </div>
      <div class="top-bar-actions">
        <Button variant="primary" onclick={() => { if (editDirty) { discardTarget = { action: 'create' }; return; } createMode = true; selected = null; editDirty = false; }}>{$t('meta_specs.new_meta_spec')}</Button>
      </div>
    </div>

    <!-- Filter pills -->
    <div class="filter-pills" role="group" aria-label={$t('meta_specs.filter_by_kind')}>
      <button class="pill" class:active={kindFilter === 'all'} onclick={() => kindFilter = 'all'} aria-pressed={kindFilter === 'all'}>{$t('meta_specs.filter_all')}</button>
      {#each META_KINDS as k}
        <button class="pill" class:active={kindFilter === k} onclick={() => kindFilter = k} aria-pressed={kindFilter === k}>{kindLabel(k)}</button>
      {/each}
    </div>

    {#if loading}
      <div class="creative-surface">
        <div class="spec-sidebar"><Skeleton /></div>
        <div class="spec-editor"><Skeleton /></div>
      </div>
    {:else if error}
      <div role="alert"><EmptyState title={$t('meta_specs.failed_to_load')} description={error} /></div>
      <button class="retry-btn" onclick={loadTenantSpecs}>{$t('common.retry')}</button>
    {:else}
      <div class="creative-surface">
        <!-- Sidebar: list of meta-specs -->
        <nav class="spec-sidebar" aria-label={$t('meta_specs.sidebar_aria')}>
          {#if filtered.length === 0}
            <div class="sidebar-empty">
              <p>{$t('meta_specs.no_meta_specs_yet')}</p>
              <button class="link-btn" onclick={() => createMode = true}>{$t('meta_specs.create_one')}</button>
            </div>
          {:else}
            {#each filtered as spec (spec.id)}
              <button
                class="sidebar-item"
                class:active={selected?.id === spec.id && !createMode}
                onclick={() => { createMode = false; selectSpec(spec); }}
                aria-current={selected?.id === spec.id && !createMode ? 'true' : undefined}
              >
                <div class="sidebar-item-top">
                  <span class="sidebar-item-name">{spec.name}</span>
                  <span class="sidebar-status-dot status-{spec.approval_status?.toLowerCase()}" title={spec.approval_status}></span>
                </div>
                <div class="sidebar-item-meta">
                  <Badge value={kindLabel(spec.kind)} variant={kindBadgeVariant(spec.kind)} />
                  {#if spec.required}
                    <span class="required-chip">{$t('common.required')}</span>
                  {/if}
                  <span class="ver-chip">v{spec.version}</span>
                </div>
              </button>
            {/each}
          {/if}
        </nav>

        <!-- Editor area -->
        <main class="spec-editor">
          {#if createMode}
            <!-- ─── Create panel ───────────────────────────────────────── -->
            <div class="create-panel">
              <div class="create-panel-header">
                <h3>{$t('meta_specs.create.title')}</h3>
                <p class="create-subtitle">{$t('meta_specs.create.subtitle')}</p>
              </div>

              <div class="create-kind-grid" role="group" aria-label={$t('meta_specs.create.select_kind')}>
                {#each META_KINDS as k}
                  <button
                    class="kind-card"
                    class:selected={createForm.kind === k}
                    onclick={() => createForm.kind = k}
                    aria-pressed={createForm.kind === k}
                  >
                    <span class="kind-card-label">{kindLabel(k)}</span>
                    <span class="kind-card-desc">{
                      k === 'meta:persona' ? $t('meta_specs.create.kind_persona_desc') :
                      k === 'meta:principle' ? $t('meta_specs.create.kind_principle_desc') :
                      k === 'meta:standard' ? $t('meta_specs.create.kind_standard_desc') :
                      $t('meta_specs.create.kind_process_desc')
                    }</span>
                  </button>
                {/each}
              </div>

              <div class="form-row">
                <div class="form-field form-field-grow">
                  <label for="cf-name">{$t('meta_specs.create.name_label')}</label>
                  <input id="cf-name" type="text" bind:value={createForm.name} placeholder={$t('meta_specs.create.name_placeholder')} required aria-required="true" />
                </div>
                <div class="form-field">
                  <label for="cf-scope">{$t('meta_specs.create.scope_label')}</label>
                  <select id="cf-scope" bind:value={createForm.scope}>
                    <option value="Global">{$t('meta_specs.create.scope_global')}</option>
                    <option value="Workspace">{$t('meta_specs.create.scope_workspace')}</option>
                  </select>
                </div>
              </div>

              {#if createForm.scope === 'Workspace'}
                <div class="form-field">
                  <label for="cf-scope-id">{$t('meta_specs.create.workspace_id_label')}</label>
                  <input id="cf-scope-id" type="text" bind:value={createForm.scope_id} placeholder={$t('meta_specs.create.workspace_id_placeholder')} />
                </div>
              {/if}

              <div class="form-field">
                <label for="cf-prompt">{$t('meta_specs.create.content_label')}</label>
                <textarea id="cf-prompt" bind:value={createForm.prompt} placeholder={$t('meta_specs.create.content_placeholder')} rows="10"></textarea>
              </div>

              <div class="form-field form-field-inline">
                <input id="cf-required" type="checkbox" bind:checked={createForm.required} />
                <label for="cf-required">{$t('meta_specs.create.required_label')}</label>
              </div>

              <div class="create-actions">
                <Button variant="secondary" onclick={() => createMode = false}>{$t('common.cancel')}</Button>
                <Button variant="primary" onclick={handleCreate} disabled={createSaving}>
                  {createSaving ? $t('meta_specs.create.creating') : $t('meta_specs.create.create_btn')}
                </Button>
              </div>
            </div>

          {:else if !selected}
            <div class="editor-empty">
              <EmptyState title={$t('meta_specs.select_or_create')} description={$t('meta_specs.select_or_create_desc')} />
            </div>

          {:else}
            <!-- ─── Editor tabs ─────────────────────────────────────────── -->
            <div class="editor-header-bar">
              <div class="editor-title-row">
                <h3 class="editor-title">{selected.name}</h3>
                <Badge value={kindLabel(selected.kind)} variant={kindBadgeVariant(selected.kind)} />
                <Badge value={selected.approval_status} variant={approvalVariant(selected.approval_status)} />
                {#if selected.required}
                  <span class="required-chip">{$t('common.required')}</span>
                {/if}
                <span class="ver-chip">v{selected.version}</span>
              </div>
              <div class="editor-header-actions">
                <button
                  class="required-toggle"
                  class:required-on={selected.required}
                  onclick={() => handleRequiredToggle(selected)}
                  disabled={requiredSaving === selected.id}
                  aria-label={selected.required ? $t('meta_specs.required_toggle.click_optional') : $t('meta_specs.required_toggle.click_required')}
                >
                  {selected.required ? $t('common.required') : $t('common.optional')}
                </button>
                <Button variant="danger" size="sm" onclick={() => deleteTarget = selected}>{$t('common.delete')}</Button>
              </div>
            </div>

            <div
              class="editor-tabs"
              role="tablist"
              aria-label={$t('meta_specs.edit.meta_spec_editor_aria')}
              tabindex="0"
              onkeydown={(e) => {
                const tabs = ['edit', 'impact', 'history', 'approval'];
                const idx = tabs.indexOf(editorTab);
                if (e.key === 'ArrowRight') { e.preventDefault(); onEditorTabChange(tabs[(idx + 1) % tabs.length]); }
                if (e.key === 'ArrowLeft')  { e.preventDefault(); onEditorTabChange(tabs[(idx + tabs.length - 1) % tabs.length]); }
              }}
            >
              <button class="editor-tab" role="tab" id="etab-edit" aria-controls="epanel-edit" aria-selected={editorTab === 'edit'} class:active={editorTab === 'edit'} tabindex={editorTab === 'edit' ? 0 : -1} onclick={() => onEditorTabChange('edit')}>{$t('meta_specs.editor_tabs.edit')}</button>
              <button class="editor-tab" role="tab" id="etab-impact" aria-controls="epanel-impact" aria-selected={editorTab === 'impact'} class:active={editorTab === 'impact'} tabindex={editorTab === 'impact' ? 0 : -1} onclick={() => onEditorTabChange('impact')}>{$t('meta_specs.editor_tabs.impact')}</button>
              <button class="editor-tab" role="tab" id="etab-history" aria-controls="epanel-history" aria-selected={editorTab === 'history'} class:active={editorTab === 'history'} tabindex={editorTab === 'history' ? 0 : -1} onclick={() => onEditorTabChange('history')}>{$t('meta_specs.editor_tabs.history')}</button>
              <button class="editor-tab" role="tab" id="etab-approval" aria-controls="epanel-approval" aria-selected={editorTab === 'approval'} class:active={editorTab === 'approval'} tabindex={editorTab === 'approval' ? 0 : -1} onclick={() => onEditorTabChange('approval')}>{$t('meta_specs.editor_tabs.approval')}</button>
            </div>

            <!-- Edit tab -->
            {#if editorTab === 'edit'}
              <div class="editor-panel" role="tabpanel" id="epanel-edit" aria-labelledby="etab-edit">
                <textarea
                  class="spec-textarea"
                  bind:value={editContent}
                  oninput={() => editDirty = true}
                  placeholder={$t('meta_specs.edit.textarea_placeholder')}
                  aria-label={$t('meta_specs.edit.textarea_aria')}
                  data-testid="spec-textarea"
                ></textarea>

                {#each editSuggestions as s (s.id)}
                  <DiffSuggestion
                    suggestion={s}
                    onaccept={() => acceptSuggestion(s)}
                    onedit={() => acceptSuggestion(s)}
                    ondismiss={() => dismissSuggestion(s.id)}
                  />
                {/each}

                <InlineChat
                  recipient="meta-spec editor"
                  recipientType="spec-edit"
                  onmessage={handleChatMessage}
                />

                <div class="edit-actions">
                  <span class="word-count" aria-live="polite">{$t('meta_specs.edit.words', { values: { count: editContent.split(/\s+/).filter(Boolean).length } })}</span>
                  <Button variant="primary" onclick={saveEdit} disabled={!editDirty || editSaving}>
                    {editSaving ? $t('meta_specs.edit.saving') : editDirty ? $t('meta_specs.edit.save_version', { values: { version: selected.version + 1 } }) : $t('meta_specs.edit.saved')}
                  </Button>
                </div>
              </div>

            <!-- Impact tab -->
            {:else if editorTab === 'impact'}
              <div class="editor-panel" role="tabpanel" id="epanel-impact" aria-labelledby="etab-impact">
                <!-- Metric cards (usage stats — data available via future endpoint) -->
                <div class="metric-grid">
                  <div class="metric-card">
                    <div class="metric-label">{$t('meta_specs.impact.bound_workspaces')}</div>
                    <div class="metric-value">{blastResult?.affected_workspaces?.length ?? '—'}</div>
                    <div class="metric-sub">{$t('meta_specs.impact.currently_binding')}</div>
                  </div>
                  <div class="metric-card">
                    <div class="metric-label">{$t('meta_specs.impact.affected_repos')}</div>
                    <div class="metric-value">{blastResult?.affected_repos?.length ?? '—'}</div>
                    <div class="metric-sub">{$t('meta_specs.impact.transitively_impacted')}</div>
                  </div>
                  <div class="metric-card metric-card-dim">
                    <div class="metric-label">{$t('meta_specs.impact.agent_runs')}</div>
                    <div class="metric-value">—</div>
                    <div class="metric-sub">{$t('meta_specs.impact.usage_coming_soon')}</div>
                  </div>
                  <div class="metric-card metric-card-dim">
                    <div class="metric-label">{$t('meta_specs.impact.gate_failures')}</div>
                    <div class="metric-value">—</div>
                    <div class="metric-sub">{$t('meta_specs.impact.drift_coming_soon')}</div>
                  </div>
                </div>

                {#if blastLoading}
                  <Skeleton />
                {:else if blastResult?.error}
                  <p class="impact-error" role="alert">{blastResult.error}</p>
                {:else if blastResult}
                  <!-- Binding panel -->
                  <div class="binding-section">
                    <h4 class="binding-title">{$t('meta_specs.impact.bound_workspaces_title')}</h4>
                    {#if blastResult.affected_workspaces?.length}
                      <div class="binding-list">
                        {#each blastResult.affected_workspaces as ws}
                          <div class="binding-row">
                            <span class="mono">{ws.id}</span>
                            <Badge value={$t('meta_specs.impact.active')} variant="success" />
                          </div>
                        {/each}
                      </div>
                    {:else}
                      <p class="impact-empty">{$t('meta_specs.impact.no_workspaces_bind')}</p>
                    {/if}
                  </div>

                  <div class="binding-section">
                    <h4 class="binding-title">{$t('meta_specs.impact.transitively_affected_title')}</h4>
                    {#if blastResult.affected_repos?.length}
                      <div class="binding-list">
                        {#each blastResult.affected_repos as repo}
                          <div class="binding-row">
                            <span class="mono">{resolveEntityName('repo', repo.id)}</span>
                            <Badge value={repo.reason} variant="muted" />
                            <span class="mono text-muted">{resolveEntityName('workspace', repo.workspace_id)}</span>
                          </div>
                        {/each}
                      </div>
                    {:else}
                      <p class="impact-empty">{$t('meta_specs.impact.no_repos_affected')}</p>
                    {/if}
                  </div>

                  <!-- Affected repos — clickable to Architecture tab with meta-spec overlays -->
                  {#if blastResult.affected_repos?.length}
                    <div class="binding-section">
                      <h4 class="binding-title">{$t('meta_specs.impact.view_in_architecture')}</h4>
                      <p class="impact-sub">{$t('meta_specs.impact.view_in_architecture_desc')}</p>
                      <div class="binding-list">
                        {#each blastResult.affected_repos as repo}
                          {@const archUrl = `/workspaces/${repo.workspace_id}/r/${repo.id}/architecture?show_overlays=metaspec:${selected.id}`}
                          <a
                            class="arch-nav-row"
                            href={archUrl}
                            data-testid="arch-nav-link"
                            aria-label={$t('meta_specs.impact.view_repo_arch_aria', { values: { repoId: repo.id } })}
                          >
                            <span class="mono">{resolveEntityName('repo', repo.id)}</span>
                            <span class="arch-nav-hint">{$t('meta_specs.impact.architecture_link_hint')}</span>
                          </a>
                        {/each}
                      </div>
                    </div>
                  {/if}

                  <!-- Drift section -->
                  <div class="binding-section">
                    <h4 class="binding-title">{$t('meta_specs.impact.version_drift_title')}</h4>
                    <div class="drift-notice">
                      <p class="drift-text">
                        {$t('meta_specs.impact.drift_text_prefix')} <strong>v{selected.version}</strong>.
                        {$t('meta_specs.impact.drift_text_suffix')}
                      </p>
                    </div>
                  </div>
                {:else}
                  <div class="impact-cta">
                    <button class="link-btn" onclick={loadBlastRadius}>{$t('meta_specs.impact.load_blast_radius')}</button>
                  </div>
                {/if}
              </div>

            <!-- History tab -->
            {:else if editorTab === 'history'}
              <div class="editor-panel" role="tabpanel" id="epanel-history" aria-labelledby="etab-history">
                {#if versionsLoading}
                  <Skeleton />
                {:else if versions.length === 0}
                  <EmptyState title={$t('meta_specs.history.no_history_title')} description={$t('meta_specs.history.no_history_desc')} />
                {:else}
                  <div class="version-timeline">
                    {#each versions as ver, i (ver.version)}
                      {@const prev = versions[i + 1]}
                      <div class="version-entry" class:selected-ver={diffVersion?.version === ver.version}>
                        <div class="version-spine"></div>
                        <button
                          class="version-node"
                          onclick={() => diffVersion = diffVersion?.version === ver.version ? null : ver}
                          aria-expanded={diffVersion?.version === ver.version}
                        >
                          <span class="ver-badge">v{ver.version}</span>
                          <span class="ver-hash mono">{ver.content_hash?.slice(0, 10)}</span>
                          {#if i === 0}<Badge value={$t('meta_specs.history.current')} variant="success" />{/if}
                        </button>

                        {#if diffVersion?.version === ver.version}
                          <div class="version-diff-panel">
                            {#if prev}
                              {@const diffLines = computeDiff(prev.prompt, ver.prompt)}
                              {#if diffLines.length === 0}
                                <p class="impact-empty">{$t('meta_specs.history.no_changes')}</p>
                              {:else}
                                <pre class="diff-output">{#each diffLines as dl}<span class="dl-{dl.type}">{dl.type === 'add' ? '+' : dl.type === 'remove' ? '-' : ' '} {dl.text}
</span>{/each}</pre>
                              {/if}
                            {:else}
                              <pre class="diff-output">{ver.prompt || $t('meta_specs.history.empty_content')}</pre>
                            {/if}
                          </div>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>

            <!-- Approval tab -->
            {:else}
              <div class="editor-panel" role="tabpanel" id="epanel-approval" aria-labelledby="etab-approval">
                <!-- Approval flow visualization -->
                <div class="approval-flow" aria-label={$t('meta_specs.approval.workflow_aria')}>
                  <div class="flow-step" class:flow-done={selected.approval_status !== 'Pending'} aria-current={selected.approval_status === 'Pending' ? 'step' : undefined}>
                    <div class="flow-step-icon flow-icon-done">✓</div>
                    <div class="flow-step-label">{$t('meta_specs.approval.draft')}</div>
                  </div>
                  <div class="flow-connector" class:flow-done={selected.approval_status === 'Approved' || selected.approval_status === 'Rejected'}></div>
                  <div class="flow-step" class:flow-active={selected.approval_status === 'Pending'} class:flow-done={selected.approval_status === 'Approved' || selected.approval_status === 'Rejected'} aria-current={selected.approval_status === 'Pending' ? 'step' : undefined}>
                    <div class="flow-step-icon {selected.approval_status === 'Pending' ? 'flow-icon-active' : selected.approval_status !== 'Pending' ? 'flow-icon-done' : ''}">
                      {approvalIcon(selected.approval_status === 'Pending' ? 'Pending' : 'Approved')}
                    </div>
                    <div class="flow-step-label">{$t('meta_specs.approval.review')}</div>
                  </div>
                  <div class="flow-connector" class:flow-done={selected.approval_status === 'Approved'}></div>
                  <div class="flow-step" class:flow-active={selected.approval_status === 'Approved'} class:flow-rejected={selected.approval_status === 'Rejected'} aria-current={selected.approval_status === 'Approved' ? 'step' : undefined}>
                    <div class="flow-step-icon {selected.approval_status === 'Approved' ? 'flow-icon-done' : selected.approval_status === 'Rejected' ? 'flow-icon-rejected' : ''}">
                      {selected.approval_status === 'Approved' ? '✓' : selected.approval_status === 'Rejected' ? '✗' : '◎'}
                    </div>
                    <div class="flow-step-label">{selected.approval_status === 'Rejected' ? $t('meta_specs.approval.rejected') : $t('meta_specs.approval.approved')}</div>
                  </div>
                </div>

                <div class="approval-status-detail">
                  <div class="approval-current">
                    <Badge value={selected.approval_status} variant={approvalVariant(selected.approval_status)} />
                    <span class="approval-status-text">
                      {#if selected.approval_status === 'Approved'}
                        {$t('meta_specs.approval.approved_by', { values: { user: selected.approved_by || 'unknown' } })}
                      {:else if selected.approval_status === 'Pending'}
                        {$t('meta_specs.approval.pending_text')}
                      {:else}
                        {$t('meta_specs.approval.rejected_text')}
                      {/if}
                    </span>
                  </div>

                  {#if selected.approval_status === 'Pending'}
                    <div class="approval-actions">
                      <Button variant="primary" onclick={handleApprove} disabled={approvalSaving !== null}>
                        {approvalSaving === 'approve' ? $t('meta_specs.approval.approving') : $t('meta_specs.approval.approve')}
                      </Button>
                      <Button variant="danger" onclick={handleReject} disabled={approvalSaving !== null}>
                        {approvalSaving === 'reject' ? $t('meta_specs.approval.rejecting') : $t('meta_specs.approval.reject')}
                      </Button>
                    </div>
                  {:else if selected.approval_status === 'Approved'}
                    <div class="approval-actions">
                      <Button variant="danger" onclick={handleReject} disabled={approvalSaving !== null}>
                        {approvalSaving === 'reject' ? $t('meta_specs.approval.revoking') : $t('meta_specs.approval.revoke_approval')}
                      </Button>
                    </div>
                  {:else}
                    <div class="approval-actions">
                      <Button variant="primary" onclick={handleApprove} disabled={approvalSaving !== null}>
                        {approvalSaving === 'approve' ? $t('meta_specs.approval.re_approving') : $t('meta_specs.approval.re_approve')}
                      </Button>
                    </div>
                  {/if}
                </div>

                <div class="approval-meta">
                  <div class="approval-meta-row"><span>{$t('meta_specs.approval.scope_label')}</span><span>{selected.scope}{selected.scope_id ? ' / ' + selected.scope_id : ''}</span></div>
                  <div class="approval-meta-row"><span>{$t('meta_specs.approval.created_by_label')}</span><code>{selected.created_by}</code></div>
                  <div class="approval-meta-row"><span>{$t('meta_specs.approval.version_label')}</span><span>v{selected.version}</span></div>
                  {#if selected.approved_by}
                    <div class="approval-meta-row"><span>{$t('meta_specs.approval.approved_by_label')}</span><code>{selected.approved_by}</code></div>
                  {/if}
                </div>
              </div>
            {/if}
          {/if}
        </main>
      </div>
    {/if}
  </div>

  <!-- Discard unsaved changes confirmation modal -->
  {#if discardTarget}
    <Modal open={true} title={$t('meta_specs.discard_modal.title')} onclose={() => discardTarget = null}>
      <p class="delete-confirm-text">{$t('meta_specs.discard_modal.message')}</p>
      <div class="form-actions">
        <Button variant="secondary" onclick={() => discardTarget = null}>{$t('meta_specs.discard_modal.keep_editing')}</Button>
        <Button variant="danger" onclick={() => { const t = discardTarget; discardTarget = null; editDirty = false; if (t.action === 'select') { selectSpec(t.spec, true); } else if (t.action === 'create') { createMode = true; selected = null; } }}>{$t('meta_specs.discard_modal.discard')}</Button>
      </div>
    </Modal>
  {/if}

  <!-- Delete confirmation modal -->
  {#if deleteTarget}
    <Modal open={true} title={$t('meta_specs.delete_modal.title')} onclose={() => deleteTarget = null}>
      <p class="delete-confirm-text">
        {$t('meta_specs.delete_modal.confirm_prefix')} <strong>{deleteTarget.name}</strong>?
        {$t('meta_specs.delete_modal.confirm_suffix')}
      </p>
      <div class="form-actions">
        <Button variant="secondary" onclick={() => deleteTarget = null}>{$t('common.cancel')}</Button>
        <Button variant="danger" onclick={handleDelete} disabled={deleteSaving}>
          {deleteSaving ? $t('meta_specs.delete_modal.deleting') : $t('common.delete')}
        </Button>
      </div>
    </Modal>
  {/if}
{/if}

<style>
  /* ── Base ── */
  .meta-specs-view { padding: var(--space-6); max-width: 100%; }
  .repo-redirect {
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    margin-bottom: var(--space-4);
    display: flex; align-items: center; gap: var(--space-2);
  }

  /* ── Tenant top bar ── */
  .tenant-view { padding: var(--space-4) var(--space-6); height: calc(100vh - 80px); display: flex; flex-direction: column; overflow: hidden; }
  .top-bar { display: flex; align-items: flex-start; justify-content: space-between; gap: var(--space-4); margin-bottom: var(--space-3); flex-shrink: 0; }
  .top-bar .page-title { margin: 0 0 var(--space-1); font-size: var(--text-2xl); }
  .top-bar-left { flex: 1; }
  .subtitle { margin: 0; color: var(--color-text-muted); font-size: var(--text-sm); }

  /* ── Filter pills ── */
  .filter-pills { display: flex; gap: var(--space-2); flex-wrap: wrap; margin-bottom: var(--space-3); flex-shrink: 0; }
  .pill {
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-full);
    border: 1px solid var(--color-border);
    background: transparent; color: var(--color-text);
    cursor: pointer; font-size: var(--text-sm);
    transition: background var(--transition-fast);
  }
  .pill:hover { background: var(--color-surface-elevated); }
  .pill.active { background: color-mix(in srgb, var(--color-link) 15%, transparent); border-color: var(--color-link); color: var(--color-link); }

  /* ── Creative surface layout ── */
  .creative-surface { display: grid; grid-template-columns: 240px 1fr; gap: 0; flex: 1; min-height: 0; border: 1px solid var(--color-border); border-radius: var(--radius); overflow: hidden; }

  /* ── Sidebar ── */
  .spec-sidebar { border-right: 1px solid var(--color-border); overflow-y: auto; background: var(--color-surface); }
  .sidebar-item {
    display: block; width: 100%; text-align: left;
    padding: var(--space-3) var(--space-3);
    border: none; border-bottom: 1px solid var(--color-border);
    background: transparent; cursor: pointer;
    transition: background var(--transition-fast);
    color: var(--color-text);
    font-family: var(--font-body);
  }
  .sidebar-item:hover { background: var(--color-surface-elevated); }
  .sidebar-item.active { background: color-mix(in srgb, var(--color-link) 10%, transparent); border-left: 3px solid var(--color-link); }
  .sidebar-item-top { display: flex; align-items: center; justify-content: space-between; margin-bottom: var(--space-1); gap: var(--space-2); }
  .sidebar-item-name { font-size: var(--text-sm); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sidebar-item-meta { display: flex; align-items: center; gap: var(--space-1); flex-wrap: wrap; }
  .sidebar-status-dot {
    width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0;
  }
  .status-approved { background: var(--color-success); }
  .status-pending { background: var(--color-warning); }
  .status-rejected { background: var(--color-danger); }
  .sidebar-empty { padding: var(--space-4); color: var(--color-text-muted); font-size: var(--text-sm); text-align: center; }

  .required-chip {
    font-size: 10px; padding: 1px 4px;
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent);
    color: var(--color-warning); border-radius: var(--radius-sm);
    white-space: nowrap;
  }
  .ver-chip {
    font-size: 10px; padding: 1px 4px;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    color: var(--color-text-muted); border-radius: var(--radius-sm);
    font-family: var(--font-mono);
  }

  /* ── Editor area ── */
  .spec-editor { display: flex; flex-direction: column; overflow: hidden; background: var(--color-surface); }
  .editor-empty { flex: 1; display: flex; align-items: center; justify-content: center; padding: var(--space-8); }

  .editor-header-bar {
    display: flex; align-items: center; justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    gap: var(--space-3); flex-shrink: 0; flex-wrap: wrap;
  }
  .editor-title-row { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
  .editor-title { margin: 0; font-size: var(--text-base); font-weight: 600; }
  .editor-header-actions { display: flex; align-items: center; gap: var(--space-2); }

  .required-toggle {
    font-size: var(--text-xs); padding: 2px var(--space-2);
    border-radius: var(--radius-sm); border: 1px solid var(--color-border);
    background: var(--color-surface-elevated); color: var(--color-text-muted);
    cursor: pointer; font-family: var(--font-body); transition: background var(--transition-fast);
  }
  .required-toggle.required-on {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    border-color: color-mix(in srgb, var(--color-warning) 40%, transparent);
    color: var(--color-warning);
  }
  .required-toggle:disabled { opacity: 0.6; cursor: not-allowed; }

  /* ── Editor tabs ── */
  .editor-tabs { display: flex; border-bottom: 1px solid var(--color-border); background: var(--color-surface-elevated); flex-shrink: 0; }
  .editor-tab {
    padding: var(--space-2) var(--space-4);
    background: none; border: none; border-bottom: 2px solid transparent;
    color: var(--color-text-muted); cursor: pointer; font-size: var(--text-sm);
    font-family: var(--font-body); transition: color var(--transition-fast);
  }
  .editor-tab.active { color: var(--color-text); border-bottom-color: var(--color-link, var(--color-focus)); }

  .editor-panel { flex: 1; overflow-y: auto; padding: var(--space-4); display: flex; flex-direction: column; gap: var(--space-4); }

  /* ── Edit tab ── */
  .spec-textarea {
    flex: 1; min-height: 300px;
    padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text); font-family: var(--font-mono); font-size: var(--text-sm);
    line-height: 1.6; resize: vertical; box-sizing: border-box;
  }
  .spec-textarea:focus:not(:focus-visible) { outline: none; }
  .spec-textarea:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }
  .edit-actions { display: flex; align-items: center; justify-content: space-between; gap: var(--space-3); }
  .word-count { font-size: var(--text-xs); color: var(--color-text-muted); font-family: var(--font-mono); }

  /* ── Impact tab ── */
  .metric-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: var(--space-3); }
  .metric-card {
    padding: var(--space-3); border: 1px solid var(--color-border);
    border-radius: var(--radius); background: var(--color-surface-elevated);
    display: flex; flex-direction: column; gap: var(--space-1);
  }
  .metric-card-dim { opacity: 0.6; }
  .metric-label { font-size: var(--text-xs); font-weight: 600; color: var(--color-text-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .metric-value { font-size: var(--text-2xl); font-weight: 700; color: var(--color-text); font-family: var(--font-mono); }
  .metric-sub { font-size: var(--text-xs); color: var(--color-text-muted); }

  .binding-section { display: flex; flex-direction: column; gap: var(--space-2); }
  .binding-title { margin: 0; font-size: var(--text-sm); font-weight: 600; color: var(--color-text); }
  .binding-list { display: flex; flex-direction: column; gap: var(--space-1); }
  .binding-row {
    display: flex; align-items: center; gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border); border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }
  .text-muted { color: var(--color-text-muted); font-size: var(--text-xs); }
  .impact-empty { font-size: var(--text-sm); color: var(--color-text-muted); margin: 0; }
  .impact-error { color: var(--color-danger); font-size: var(--text-sm); }
  .impact-cta { padding: var(--space-4); text-align: center; }
  .impact-sub { font-size: var(--text-xs); color: var(--color-text-muted); margin: 0; }

  .arch-nav-row {
    display: flex; align-items: center; justify-content: space-between;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border); border-radius: var(--radius-sm);
    font-size: var(--text-sm); color: var(--color-text); text-decoration: none;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }
  .arch-nav-row:hover {
    background: color-mix(in srgb, var(--color-link) 8%, var(--color-surface-elevated));
    border-color: var(--color-link);
  }
  .arch-nav-row:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .arch-nav-hint { font-size: var(--text-xs); color: var(--color-link); white-space: nowrap; }

  .drift-notice {
    padding: var(--space-3);
    background: color-mix(in srgb, var(--color-warning) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 25%, transparent);
    border-radius: var(--radius);
  }
  .drift-text { margin: 0; font-size: var(--text-sm); color: var(--color-text); }

  /* ── History tab ── */
  .version-timeline { display: flex; flex-direction: column; gap: 0; }
  .version-entry { display: flex; flex-direction: column; position: relative; padding-left: var(--space-6); }
  .version-spine {
    position: absolute; left: 11px; top: 28px; bottom: 0;
    width: 2px; background: var(--color-border);
  }
  .version-entry:last-child .version-spine { display: none; }
  .version-node {
    display: flex; align-items: center; gap: var(--space-2);
    padding: var(--space-2) 0;
    background: none; border: none; cursor: pointer; text-align: left;
    font-family: var(--font-body); color: var(--color-text);
    width: 100%;
  }
  .version-node::before {
    content: '';
    position: absolute; left: 6px;
    width: 12px; height: 12px; border-radius: 50%;
    background: var(--color-link); border: 2px solid var(--color-surface);
    flex-shrink: 0;
  }
  .ver-badge {
    font-size: var(--text-sm); font-weight: 600;
    font-family: var(--font-mono); color: var(--color-text);
  }
  .ver-hash { font-size: var(--text-xs); color: var(--color-text-muted); }
  .version-diff-panel {
    margin: var(--space-2) 0 var(--space-3);
    border: 1px solid var(--color-border); border-radius: var(--radius-sm); overflow: hidden;
  }
  .diff-output {
    margin: 0; padding: var(--space-3); font-family: var(--font-mono); font-size: var(--text-xs);
    line-height: 1.5; white-space: pre-wrap; word-break: break-word; background: var(--color-surface-elevated);
  }
  :global(.dl-add) { color: var(--color-success); display: block; }
  :global(.dl-remove) { color: var(--color-danger); display: block; }
  :global(.dl-ctx) { color: var(--color-text-muted); display: block; }

  /* ── Approval tab ── */
  .approval-flow { display: flex; align-items: center; gap: 0; padding: var(--space-6) var(--space-4); }
  .flow-step { display: flex; flex-direction: column; align-items: center; gap: var(--space-2); flex-shrink: 0; }
  .flow-step-icon {
    width: 40px; height: 40px; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    font-size: var(--text-base); font-weight: 700;
    border: 2px solid var(--color-border);
    background: var(--color-surface-elevated); color: var(--color-text-muted);
  }
  .flow-icon-done { background: color-mix(in srgb, var(--color-success) 15%, transparent); border-color: var(--color-success); color: var(--color-success); }
  .flow-icon-active { background: color-mix(in srgb, var(--color-warning) 15%, transparent); border-color: var(--color-warning); color: var(--color-warning); }
  .flow-icon-rejected { background: color-mix(in srgb, var(--color-danger) 15%, transparent); border-color: var(--color-danger); color: var(--color-danger); }
  .flow-step-label { font-size: var(--text-xs); font-weight: 600; color: var(--color-text-muted); text-transform: uppercase; letter-spacing: 0.04em; }
  .flow-connector { flex: 1; height: 2px; background: var(--color-border); }
  .flow-connector.flow-done { background: var(--color-success); }

  .approval-status-detail { display: flex; flex-direction: column; gap: var(--space-3); padding: var(--space-4); background: var(--color-surface-elevated); border: 1px solid var(--color-border); border-radius: var(--radius); }
  .approval-current { display: flex; align-items: flex-start; gap: var(--space-3); }
  .approval-status-text { font-size: var(--text-sm); color: var(--color-text); }
  .approval-actions { display: flex; gap: var(--space-2); }
  .approval-meta { display: flex; flex-direction: column; gap: var(--space-2); border: 1px solid var(--color-border); border-radius: var(--radius); overflow: hidden; }
  .approval-meta-row { display: flex; gap: var(--space-4); padding: var(--space-2) var(--space-3); border-bottom: 1px solid var(--color-border); font-size: var(--text-sm); }
  .approval-meta-row:last-child { border-bottom: none; }
  .approval-meta-row > span:first-child { color: var(--color-text-muted); font-weight: 600; min-width: 100px; }
  .approval-meta-row code { font-family: var(--font-mono); font-size: var(--text-xs); }

  /* ── Create panel ── */
  .create-panel { padding: var(--space-6); overflow-y: auto; flex: 1; display: flex; flex-direction: column; gap: var(--space-5); }
  .create-panel-header h3 { margin: 0 0 var(--space-1); font-size: var(--text-xl); }
  .create-subtitle { margin: 0; color: var(--color-text-muted); font-size: var(--text-sm); }

  .create-kind-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: var(--space-3); }
  .kind-card {
    display: flex; flex-direction: column; gap: var(--space-1);
    padding: var(--space-3); border: 2px solid var(--color-border);
    border-radius: var(--radius); background: var(--color-surface-elevated);
    cursor: pointer; text-align: left; font-family: var(--font-body);
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }
  .kind-card:hover { border-color: var(--color-link); }
  .kind-card.selected { border-color: var(--color-link); background: color-mix(in srgb, var(--color-link) 8%, transparent); }
  .kind-card-label { font-size: var(--text-sm); font-weight: 600; color: var(--color-text); }
  .kind-card-desc { font-size: var(--text-xs); color: var(--color-text-muted); line-height: 1.4; }

  .form-row { display: flex; gap: var(--space-3); }
  .form-field { display: flex; flex-direction: column; gap: var(--space-1); }
  .form-field-grow { flex: 1; }
  .form-field label { font-size: var(--text-sm); font-weight: 600; color: var(--color-text); }
  .form-field input[type="text"],
  .form-field select,
  .form-field textarea {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius); color: var(--color-text);
    font-size: var(--text-sm); font-family: var(--font-body);
  }
  .form-field textarea { resize: vertical; font-family: var(--font-mono); line-height: 1.5; }
  .form-field input[type="text"]:focus-visible,
  .form-field select:focus-visible,
  .form-field textarea:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .form-field-inline { flex-direction: row; align-items: center; gap: var(--space-2); }
  .form-field-inline label { font-weight: 400; }
  .form-actions { display: flex; gap: var(--space-2); justify-content: flex-end; }
  .create-actions { display: flex; gap: var(--space-2); justify-content: flex-end; margin-top: var(--space-2); }

  /* ── Workspace scope ── */
  .workspace-view { max-width: 1400px; }
  .view-header { margin-bottom: var(--space-6); }
  .view-header .page-title { margin: 0 0 var(--space-1); font-size: var(--text-2xl); }
  .split-layout { display: grid; grid-template-columns: 60fr 40fr; gap: var(--space-6); align-items: start; }
  .split-left { display: flex; flex-direction: column; gap: var(--space-4); }
  .split-right { display: flex; flex-direction: column; gap: var(--space-4); position: sticky; top: var(--space-4); }

  .editor-header { display: flex; align-items: center; gap: var(--space-3); }
  .persona-label { font-size: var(--text-sm); font-weight: 600; color: var(--color-text-muted); white-space: nowrap; }
  .persona-select {
    flex: 1; padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius); color: var(--color-text); font-size: var(--text-sm);
  }
  .persona-textarea {
    width: 100%; min-height: 280px; padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius); color: var(--color-text);
    font-family: var(--font-mono); font-size: var(--text-sm);
    line-height: 1.6; resize: vertical; box-sizing: border-box;
  }
  .persona-textarea:focus:not(:focus-visible) { outline: none; border-color: var(--color-border-strong); }
  .persona-textarea:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }
  .persona-diff {
    min-height: 280px; padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border); border-radius: var(--radius);
    font-family: var(--font-mono); font-size: var(--text-sm); overflow-x: auto;
  }
  .diff-line { padding: 0 var(--space-1); line-height: 1.5; }
  .diff-line.add { background: color-mix(in srgb, var(--color-success) 12%, transparent); color: var(--color-success); }
  .diff-line.remove { background: color-mix(in srgb, var(--color-danger) 12%, transparent); color: var(--color-danger); }
  .diff-line.ctx { color: var(--color-text-muted); }
  .editor-actions { display: flex; gap: var(--space-2); justify-content: flex-end; }

  .spec-selector { background: var(--color-surface); border: 1px solid var(--color-border); border-radius: var(--radius); overflow: hidden; }
  .spec-selector-header { display: flex; justify-content: space-between; align-items: center; padding: var(--space-2) var(--space-3); border-bottom: 1px solid var(--color-border); background: var(--color-surface-elevated); }
  .spec-selector-title { font-size: var(--text-sm); font-weight: 600; color: var(--color-text); }
  .spec-selector-shortcuts { display: flex; gap: var(--space-2); }
  .link-btn { background: none; border: none; color: var(--color-link); font-size: var(--text-xs); cursor: pointer; padding: 0; text-decoration: underline; font-family: var(--font-body); }
  .spec-checklist { max-height: 360px; overflow-y: auto; padding: var(--space-2) 0; }
  .spec-check-item { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-1) var(--space-3); cursor: pointer; font-size: var(--text-sm); color: var(--color-text); transition: background var(--transition-fast); }
  .spec-check-item:hover { background: var(--color-surface-elevated); }
  .spec-check-path { font-family: var(--font-mono); font-size: var(--text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty-specs { padding: 1rem 0.75rem; color: var(--color-text-muted); font-size: var(--text-sm); margin: 0; }

  .preview-progress { background: var(--color-surface); border: 1px solid var(--color-border); border-radius: var(--radius); padding: var(--space-4); display: flex; flex-direction: column; gap: var(--space-3); }
  .progress-header { font-weight: 600; font-size: var(--text-base); }
  .progress-list { display: flex; flex-direction: column; gap: var(--space-1); }
  .progress-item { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-sm); }
  .progress-icon { font-size: 0.9rem; width: 1.2rem; text-align: center; }
  .progress-path { font-family: var(--font-mono); flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .progress-status { color: var(--color-text-muted); font-size: var(--text-xs); white-space: nowrap; }
  .progress-summary { font-size: var(--text-sm); color: var(--color-text-muted); }

  .impact-panel { background: var(--color-surface); border: 1px solid var(--color-border); border-radius: var(--radius); overflow: hidden; }
  .impact-tabs { display: flex; border-bottom: 1px solid var(--color-border); }
  .impact-tab { padding: 0.5rem 1rem; background: none; border: none; border-bottom: 2px solid transparent; color: var(--color-text-muted); cursor: pointer; font-size: var(--text-sm); transition: color var(--transition-fast); font-family: var(--font-body); }
  .impact-tab.active { color: var(--color-text); border-bottom-color: var(--color-link, var(--color-focus)); }
  .impact-content { padding: var(--space-4); font-size: var(--text-sm); }
  .arch-diff { display: flex; flex-direction: column; gap: var(--space-1); }
  .arch-line { padding: var(--space-1); border-radius: var(--radius-sm); font-family: var(--font-mono); }
  .arch-line.add { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 10%, transparent); }
  .arch-line.mod { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 10%, transparent); }
  .arch-line.ctx { color: var(--color-text-muted); }
  .code-diff { display: flex; flex-direction: column; gap: var(--space-3); }
  .code-diff-file { border: 1px solid var(--color-border); border-radius: var(--radius-sm); overflow: hidden; }
  .code-diff-path { padding: var(--space-1) var(--space-2); background: var(--color-surface-elevated); font-family: var(--font-mono); font-size: var(--text-xs); color: var(--color-text-muted); border-bottom: 1px solid var(--color-border); }
  .code-diff-body { margin: 0; padding: var(--space-2) var(--space-3); font-family: var(--font-mono); font-size: var(--text-xs); line-height: 1.5; }
  .impact-unavailable { display: flex; flex-direction: column; gap: var(--space-3); }
  .impact-unavailable-label { font-size: var(--text-xs); color: var(--color-text-muted); font-style: italic; }
  .impact-empty { font-size: var(--text-sm); color: var(--color-text-muted); }
  .sim-banner { background: color-mix(in srgb, var(--color-warning) 12%, transparent); border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent); border-radius: var(--radius); padding: var(--space-2) var(--space-3); font-size: var(--text-sm); margin: var(--space-3) var(--space-3) 0; }

  /* ── Delete confirm ── */
  .delete-confirm-text { font-size: var(--text-sm); color: var(--color-text); margin: 0 0 var(--space-4); }

  /* ── Retry button ── */
  .retry-btn {
    background: color-mix(in srgb, var(--color-focus) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-focus) 30%, transparent);
    border-radius: var(--radius); color: var(--color-focus); cursor: pointer;
    font-family: var(--font-body); font-size: var(--text-sm); font-weight: 500;
    padding: var(--space-2) var(--space-4); margin-top: var(--space-3);
  }
  .retry-btn:hover { background: color-mix(in srgb, var(--color-focus) 25%, transparent); border-color: var(--color-focus); }
  .retry-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* ── Focus-visible ── */
  .pill:focus-visible, .impact-tab:focus-visible, .editor-tab:focus-visible,
  .link-btn:focus-visible, .persona-select:focus-visible, .required-toggle:focus-visible,
  .sidebar-item:focus-visible, .kind-card:focus-visible, .version-node:focus-visible {
    outline: 2px solid var(--color-focus); outline-offset: 2px;
  }

  .mono { font-family: var(--font-mono); }

  @media (prefers-reduced-motion: reduce) {
    .spec-check-item, .pill, .impact-tab, .editor-tab, .link-btn, .required-toggle,
    .sidebar-item, .kind-card, .persona-textarea, .spec-textarea { transition: none; }
  }
</style>
