<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Modal from '../lib/Modal.svelte';
  import Button from '../lib/Button.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let tenants = $state([]);
  let loading = $state(true);
  let createOpen = $state(false);
  let form = $state({ name: '', oidc_issuer: '' });
  let saving = $state(false);
  let deleting = $state(null);
  let deleteConfirmId = $state(null);

  $effect(() => { load(); });

  async function load() {
    loading = true;
    try {
      tenants = (await api.tenants()) ?? [];
    } catch (e) {
      toastError('Failed to load tenants: ' + e.message);
    } finally {
      loading = false;
    }
  }

  async function create() {
    if (!form.name.trim()) return;
    saving = true;
    try {
      const slug = form.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
      await api.createTenant({ name: form.name.trim(), slug, oidc_issuer: form.oidc_issuer || undefined });
      toastSuccess('Tenant created');
      createOpen = false;
      form = { name: '', oidc_issuer: '' };
      await load();
    } catch (e) {
      toastError('Failed to create tenant: ' + e.message);
    } finally {
      saving = false;
    }
  }

  async function deleteTenant(id) {
    deleteConfirmId = null;
    deleting = id;
    try {
      await api.deleteTenant(id);
      toastSuccess('Tenant deleted');
      await load();
    } catch (e) {
      toastError('Failed to delete tenant: ' + e.message);
    } finally {
      deleting = null;
    }
  }
</script>

<div class="tenant-list">
  <span class="sr-only" aria-live="polite">{loading ? "" : "tenants loaded"}</span>
  <div class="list-header">
    <div>
      <h2>Tenants</h2>
      <p class="subtitle">Enterprise/org boundaries. Each tenant has its own users, workspaces, and budgets.</p>
    </div>
    <Button variant="primary" onclick={() => (createOpen = true)}>+ New Tenant</Button>
  </div>

  {#if loading}
    <div class="content" aria-busy="true" aria-label="Loading tenants">
      {#each Array(3) as _}
        <div class="tenant-card skeleton-card"><Skeleton lines={2} /></div>
      {/each}
    </div>
  {:else if tenants.length === 0}
    <EmptyState icon="🏢" title="No tenants yet" description="Create a tenant to define your enterprise/org boundary." />
  {:else}
    <div class="content">
      {#each tenants as t}
        <div class="tenant-card">
          <div class="tenant-card-header">
            <div>
              <span class="tenant-name">{t.name}</span>
              <span class="tenant-slug">/{t.slug}</span>
            </div>
            <button
              class="btn-danger-sm"
              onclick={() => (deleteConfirmId = t.id)}
              disabled={deleting === t.id}
              aria-label="Delete tenant {t.name}"
            >
              {deleting === t.id ? '…' : 'Delete'}
            </button>
          </div>
          {#if t.oidc_issuer}
            <p class="tenant-meta">OIDC: <code>{t.oidc_issuer}</code></p>
          {/if}
          {#if t.max_workspaces}
            <p class="tenant-meta">Max workspaces: {t.max_workspaces}</p>
          {/if}
          <p class="tenant-meta tenant-id">ID: {t.id}</p>
        </div>
      {/each}
    </div>
  {/if}
</div>

<Modal bind:open={createOpen} title="New Tenant" onsubmit={create}>
  <div class="form-group">
    <label for="tenant-name">Name</label>
    <input id="tenant-name" class="input" bind:value={form.name} placeholder="Acme Corp" />
  </div>
  <div class="form-group">
    <label for="tenant-oidc">OIDC Issuer (optional)</label>
    <input id="tenant-oidc" class="input" bind:value={form.oidc_issuer} placeholder="https://keycloak.example.com/realms/acme" />
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (createOpen = false)}>Cancel</Button>
    <Button variant="primary" onclick={create} disabled={saving || !form.name.trim()}>
      {saving ? 'Creating…' : 'Create Tenant'}
    </Button>
  {/snippet}
</Modal>

{#if deleteConfirmId}
  <Modal open={true} title="Delete Tenant" onclose={() => (deleteConfirmId = null)}>
    <p>Delete this tenant? This cannot be undone.</p>
    {#snippet footer()}
      <button class="btn-secondary" onclick={() => (deleteConfirmId = null)}>Cancel</button>
      <button class="btn-danger-sm" onclick={() => deleteTenant(deleteConfirmId)} disabled={deleting}>
        {deleting ? 'Deleting…' : 'Delete'}
      </button>
    {/snippet}
  </Modal>
{/if}

<style>
  .tenant-list { padding: var(--space-6); }

  .list-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: var(--space-6);
    gap: var(--space-4);
  }
  .list-header h2 { margin: 0 0 var(--space-1); font-size: var(--text-xl); font-weight: 600; color: var(--color-text); }
  .subtitle { color: var(--color-text-secondary); margin: 0; font-size: var(--text-sm); }

  .content { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: var(--space-4); }

  .tenant-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }
  .skeleton-card { min-height: 80px; }

  .tenant-card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-2);
    gap: var(--space-2);
  }
  .tenant-name { font-weight: 600; font-size: var(--text-base); color: var(--color-text); }
  .tenant-slug { color: var(--color-text-secondary); font-size: var(--text-xs); margin-left: var(--space-1); }

  .tenant-meta { margin: var(--space-1) 0 0; font-size: var(--text-xs); color: var(--color-text-secondary); }
  .tenant-id { color: var(--color-text-muted); font-family: var(--font-mono); font-size: var(--text-xs); }

  .form-group { margin-bottom: var(--space-4); }
  .form-group label { display: block; font-size: var(--text-sm); font-weight: 500; margin-bottom: var(--space-1); color: var(--color-text); }
  .input {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    background: var(--color-bg);
    color: var(--color-text);
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }
  .input:focus:not(:focus-visible) { outline: none; }
  .input:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }

  .btn-secondary {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    transition: background var(--transition-fast);
  }
  .btn-secondary:hover { background: var(--color-surface-elevated); }
  .btn-secondary:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .btn-danger-sm {
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-danger);
    color: var(--color-danger);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    transition: background var(--transition-fast);
  }
  .btn-danger-sm:hover:not(:disabled) { background: color-mix(in srgb, var(--color-danger) 10%, transparent); }
  .btn-danger-sm:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-danger-sm:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  @media (prefers-reduced-motion: reduce) {
    .btn-secondary, .btn-danger-sm, .input { transition: none; }
  }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
