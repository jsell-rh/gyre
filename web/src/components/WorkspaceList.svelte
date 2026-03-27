<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Modal from '../lib/Modal.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let { onSelect } = $props();

  let workspaces = $state([]);
  let loading = $state(true);
  let createOpen = $state(false);
  let form = $state({ name: '', description: '' });
  let saving = $state(false);

  $effect(() => { load(); });

  async function load() {
    loading = true;
    try {
      workspaces = (await api.workspaces()) ?? [];
    } catch (e) {
      showToast('Failed to load workspaces: ' + e.message, { type: 'error' });
    } finally {
      loading = false;
    }
  }

  async function create() {
    if (!form.name.trim()) return;
    saving = true;
    try {
      const slug = form.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
      await api.createWorkspace({ ...form, tenant_id: 'default', slug });
      showToast('Workspace created', { type: 'success' });
      createOpen = false;
      form = { name: '', description: '' };
      await load();
    } catch (e) {
      showToast('Failed to create workspace: ' + e.message, { type: 'error' });
    } finally {
      saving = false;
    }
  }
</script>

<div class="workspace-list">
  <div class="list-header">
    <div>
      <h2>Workspaces</h2>
      <p class="subtitle">Budget-bounded environments grouping repos and agents</p>
    </div>
    <button class="btn-primary" onclick={() => (createOpen = true)} aria-haspopup="dialog">+ New Workspace</button>
  </div>

  {#if loading}
    <div class="content">
      {#each Array(4) as _}
        <div class="ws-card skeleton-card"><Skeleton lines={3} /></div>
      {/each}
    </div>
  {:else if workspaces.length === 0}
    <EmptyState
      title="No workspaces"
      description="Create a workspace to group repos under shared budget limits."
    />
  {:else}
    <div class="content">
      {#each workspaces as ws}
        <button class="ws-card" onclick={() => onSelect(ws)} aria-label="Open workspace {ws.name}">
          <div class="ws-icon" aria-hidden="true">{ws.name?.[0]?.toUpperCase() ?? 'W'}</div>
          <div class="ws-info">
            <div class="ws-name">{ws.name}</div>
            {#if ws.description}
              <div class="ws-desc">{ws.description}</div>
            {/if}
          </div>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" class="chevron" aria-hidden="true">
            <path d="M9 18l6-6-6-6"/>
          </svg>
        </button>
      {/each}
    </div>
  {/if}
</div>

<Modal bind:open={createOpen} title="New Workspace" size="sm" onsubmit={create}>
  <div class="create-form">
    <label class="field-label">Name *
      <input class="field-input" bind:value={form.name} placeholder="e.g. Backend Team" onkeydown={(e) => e.key === 'Enter' && create()} />
    </label>
    <label class="field-label">Description
      <input class="field-input" bind:value={form.description} placeholder="What is this workspace for?" />
    </label>
    <div class="form-actions">
      <button class="btn-secondary" onclick={() => (createOpen = false)}>Cancel</button>
      <button class="btn-primary" onclick={create} disabled={saving || !form.name.trim()}>
        {saving ? 'Creating…' : 'Create'}
      </button>
    </div>
  </div>
</Modal>

<style>
  .workspace-list { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .list-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .list-header h2 { margin: 0 0 var(--space-1); font-size: var(--text-xl); font-weight: 600; color: var(--color-text); }
  .subtitle { margin: 0; font-size: var(--text-sm); color: var(--color-text-secondary); }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .ws-card {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    width: 100%;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }
  .ws-card:hover { border-color: var(--color-border-strong); background: var(--color-surface-elevated); }
  .skeleton-card { min-height: 72px; cursor: default; }
  .skeleton-card:hover { border-color: var(--color-border); background: var(--color-surface); }

  .ws-icon {
    width: 40px;
    height: 40px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    color: var(--color-primary);
    font-weight: 700;
    font-size: var(--text-lg);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .ws-info { flex: 1; }
  .ws-name { font-weight: 600; color: var(--color-text); font-size: var(--text-base); }
  .ws-desc { font-size: var(--text-sm); color: var(--color-text-secondary); margin-top: var(--space-0, 2px); }

  .chevron { color: var(--color-text-muted); flex-shrink: 0; }

  .btn-primary {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast);
  }
  .btn-primary:hover:not(:disabled) { background: var(--color-primary-hover); }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-primary:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .btn-secondary {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast);
  }
  .btn-secondary:hover { background: var(--color-surface-elevated); color: var(--color-text); }
  .btn-secondary:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .create-form { display: flex; flex-direction: column; gap: var(--space-4); }
  .field-label { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--text-sm); font-weight: 500; color: var(--color-text); }
  .field-input {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
  }
  .field-input:focus:not(:focus-visible) { outline: none; }
  .field-input:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }
  .form-actions { display: flex; justify-content: flex-end; gap: var(--space-2); }
</style>
