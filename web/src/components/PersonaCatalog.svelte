<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Modal from '../lib/Modal.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let personas = $state([]);
  let loading = $state(true);
  let createOpen = $state(false);
  let form = $state({ name: '', slug: '', description: '', scopeKind: 'Tenant', scopeId: '', capabilities: '', system_prompt: '' });
  let saving = $state(false);
  let scopeObjects = $state([]);

  $effect(() => { load(); });

  $effect(() => {
    if (form.scopeKind === 'Workspace') {
      api.workspaces().then(data => { scopeObjects = Array.isArray(data) ? data : []; }).catch(() => { scopeObjects = []; });
    } else if (form.scopeKind === 'Repo') {
      api.allRepos().then(data => { scopeObjects = Array.isArray(data) ? data : []; }).catch(() => { scopeObjects = []; });
    } else {
      scopeObjects = [];
    }
    form.scopeId = '';
  });

  async function load() {
    loading = true;
    try {
      personas = (await api.personas()) ?? [];
    } catch {
      showToast('Failed to load personas', { type: 'error' });
    } finally {
      loading = false;
    }
  }

  function autoSlug(name) {
    return name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  }

  async function create() {
    if (!form.name.trim()) return;
    if (!form.scopeId.trim()) {
      showToast('Scope ID (workspace/tenant/repo UUID) is required', { type: 'error' });
      return;
    }
    saving = true;
    try {
      const caps = form.capabilities.split(',').map(s => s.trim()).filter(Boolean);
      const slug = form.slug.trim() || autoSlug(form.name.trim());
      const scope = { kind: form.scopeKind, id: form.scopeId.trim() };
      await api.createPersona({
        name: form.name.trim(),
        slug,
        description: form.description.trim() || undefined,
        scope,
        capabilities: caps,
        system_prompt: form.system_prompt.trim(),
      });
      showToast('Persona created', { type: 'success' });
      createOpen = false;
      form = { name: '', slug: '', description: '', scopeKind: 'Tenant', scopeId: '', capabilities: '', system_prompt: '' };
      await load();
    } catch (e) {
      showToast('Failed to create persona: ' + e.message, { type: 'error' });
    } finally {
      saving = false;
    }
  }

  async function deletePersona(id) {
    try {
      await api.deletePersona(id);
      showToast('Persona deleted', { type: 'success' });
      personas = personas.filter(p => p.id !== id);
    } catch (e) {
      showToast('Failed to delete persona', { type: 'error' });
    }
  }

  // API returns scope as {kind:"Tenant"|"Workspace"|"Repo", id:"..."} — handle both object and string
  function scopeVariant(scope) {
    const s = typeof scope === 'object' ? (scope?.kind ?? '').toLowerCase() : (scope ?? '').toLowerCase();
    if (s === 'tenant') return 'danger';
    if (s === 'workspace') return 'info';
    if (s === 'repo') return 'warning';
    return 'default';
  }
  function scopeLabel(scope) {
    if (typeof scope === 'object') return scope?.kind ?? 'workspace';
    return scope ?? 'workspace';
  }
</script>

<div class="persona-catalog">
  <div class="catalog-header">
    <div>
      <h2>Persona Catalog</h2>
      <p class="subtitle">Reusable agent persona definitions with scoped capabilities</p>
    </div>
    <button class="btn-primary" onclick={() => (createOpen = true)}>+ New Persona</button>
  </div>

  {#if loading}
    <div class="grid">
      {#each Array(6) as _}
        <div class="persona-card skeleton-card"><Skeleton lines={3} /></div>
      {/each}
    </div>
  {:else if personas.length === 0}
    <EmptyState
      title="No personas yet"
      message="Create a persona to define reusable agent configurations with scoped capabilities."
    />
  {:else}
    <div class="grid">
      {#each personas as persona}
        <div class="persona-card">
          <div class="card-top">
            <div class="persona-icon" aria-hidden="true">
              {persona.name?.[0]?.toUpperCase() ?? 'P'}
            </div>
            <div class="card-meta">
              <div class="persona-name">{persona.name}</div>
              <Badge variant={scopeVariant(persona.scope)} value={scopeLabel(persona.scope)} />
            </div>
          </div>
          {#if persona.description}
            <p class="persona-desc">{persona.description}</p>
          {/if}
          {#if persona.capabilities?.length > 0}
            <div class="capabilities">
              {#each persona.capabilities as cap}
                <Badge variant="default" value={cap} />
              {/each}
            </div>
          {/if}
          <div class="card-actions">
            <button class="btn-danger-sm" onclick={() => deletePersona(persona.id)}>Delete</button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<Modal bind:open={createOpen} title="New Persona" size="md" onsubmit={create}>
  <div class="create-form">
    <label class="field-label">Name *
      <input class="field-input" bind:value={form.name} placeholder="e.g. Backend Developer" />
    </label>
    <label class="field-label">Slug (auto-generated if blank)
      <input class="field-input" bind:value={form.slug} placeholder="backend-developer" />
    </label>
    <label class="field-label">Description
      <input class="field-input" bind:value={form.description} placeholder="Short description" />
    </label>
    <label class="field-label">Scope
      <select class="field-input" bind:value={form.scopeKind}>
        <option value="Tenant">Tenant (global)</option>
        <option value="Workspace">Workspace</option>
        <option value="Repo">Repo</option>
      </select>
    </label>
    <label class="field-label">Scope ID *
      {#if scopeObjects.length > 0}
        <select class="field-input" bind:value={form.scopeId}>
          <option value="">— select {form.scopeKind.toLowerCase()} —</option>
          {#each scopeObjects as obj}
            <option value={obj.id}>{obj.name} ({obj.id.substring(0, 8)}…)</option>
          {/each}
        </select>
      {:else}
        <input class="field-input" bind:value={form.scopeId} placeholder="UUID of the {form.scopeKind.toLowerCase()}" />
      {/if}
      <span class="field-hint">
        {#if form.scopeKind === 'Tenant'}Tenant scope applies globally. Enter a tenant UUID (or use "default").
        {:else}Select or enter the UUID of the {form.scopeKind.toLowerCase()} this persona is scoped to.{/if}
      </span>
    </label>
    <label class="field-label">Capabilities (comma-separated)
      <input class="field-input" bind:value={form.capabilities} placeholder="rust, api-design, code-review" />
    </label>
    <label class="field-label">System Prompt
      <textarea class="field-input field-textarea" bind:value={form.system_prompt} placeholder="Optional system prompt for this persona..."></textarea>
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
  .persona-catalog {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .catalog-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .catalog-header h2 {
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

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--space-4);
    padding: var(--space-6);
    overflow-y: auto;
    flex: 1;
  }

  .persona-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    transition: border-color var(--transition-fast);
  }

  .persona-card:hover { border-color: var(--color-border-strong); }
  .skeleton-card { min-height: 140px; }

  .card-top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .persona-icon {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: rgba(238, 0, 0, 0.12);
    color: var(--color-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
    font-size: var(--text-lg);
    flex-shrink: 0;
  }

  .persona-name {
    font-weight: 600;
    color: var(--color-text);
    font-size: var(--text-base);
  }

  .persona-desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
  }

  .capabilities {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .card-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: auto;
  }

  .btn-primary {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: #fff;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }
  .btn-primary:hover:not(:disabled) { opacity: 0.85; }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

  .btn-secondary {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .btn-danger-sm {
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-danger);
    border-radius: var(--radius);
    color: var(--color-danger);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .btn-danger-sm:hover { background: rgba(255, 80, 80, 0.08); }

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
  .field-input:focus { outline: none; border-color: var(--color-primary); }
  .field-textarea { min-height: 80px; resize: vertical; font-family: var(--font-mono); }

  .field-hint { font-size: 0.7rem; color: var(--color-text-muted); margin-top: 2px; display: block; }
  .form-actions { display: flex; justify-content: flex-end; gap: var(--space-2); }
</style>
