<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Modal from '../lib/Modal.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let tenants = $state([]);
  let loading = $state(true);
  let createOpen = $state(false);
  let form = $state({ name: '', oidc_issuer: '' });
  let saving = $state(false);
  let deleting = $state(null);

  $effect(() => { load(); });

  async function load() {
    loading = true;
    try {
      tenants = (await api.tenants()) ?? [];
    } catch (e) {
      showToast('Failed to load tenants: ' + e.message, { type: 'error' });
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
      showToast('Tenant created', { type: 'success' });
      createOpen = false;
      form = { name: '', oidc_issuer: '' };
      await load();
    } catch (e) {
      showToast('Failed to create tenant: ' + e.message, { type: 'error' });
    } finally {
      saving = false;
    }
  }

  async function deleteTenant(id) {
    if (!confirm('Delete this tenant? This cannot be undone.')) return;
    deleting = id;
    try {
      await api.deleteTenant(id);
      showToast('Tenant deleted', { type: 'success' });
      await load();
    } catch (e) {
      showToast('Failed to delete tenant: ' + e.message, { type: 'error' });
    } finally {
      deleting = null;
    }
  }
</script>

<div class="tenant-list">
  <div class="list-header">
    <div>
      <h2>Tenants</h2>
      <p class="subtitle">Enterprise/org boundaries. Each tenant has its own users, workspaces, and budgets.</p>
    </div>
    <button class="btn-primary" onclick={() => (createOpen = true)}>+ New Tenant</button>
  </div>

  {#if loading}
    <div class="content">
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
              onclick={() => deleteTenant(t.id)}
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

<Modal bind:open={createOpen} title="New Tenant">
  <div class="form-group">
    <label for="tenant-name">Name</label>
    <input id="tenant-name" class="input" bind:value={form.name} placeholder="Acme Corp" />
  </div>
  <div class="form-group">
    <label for="tenant-oidc">OIDC Issuer (optional)</label>
    <input id="tenant-oidc" class="input" bind:value={form.oidc_issuer} placeholder="https://keycloak.example.com/realms/acme" />
  </div>
  {#snippet footer()}
    <button class="btn-secondary" onclick={() => (createOpen = false)}>Cancel</button>
    <button class="btn-primary" onclick={create} disabled={saving || !form.name.trim()}>
      {saving ? 'Creating…' : 'Create Tenant'}
    </button>
  {/snippet}
</Modal>

<style>
  .tenant-list { padding: 1.5rem; }

  .list-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 1.5rem;
    gap: 1rem;
  }
  .list-header h2 { margin: 0 0 0.25rem; font-size: 1.25rem; }
  .subtitle { color: var(--color-text-secondary, #666); margin: 0; font-size: 0.875rem; }

  .content { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 1rem; }

  .tenant-card {
    background: var(--color-surface, #fff);
    border: 1px solid var(--color-border, #e2e8f0);
    border-radius: 8px;
    padding: 1rem;
  }
  .skeleton-card { min-height: 80px; }

  .tenant-card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.5rem;
    gap: 0.5rem;
  }
  .tenant-name { font-weight: 600; font-size: 0.95rem; }
  .tenant-slug { color: var(--color-text-secondary, #666); font-size: 0.8rem; margin-left: 0.25rem; }

  .tenant-meta { margin: 0.2rem 0 0; font-size: 0.8rem; color: var(--color-text-secondary, #666); }
  .tenant-id { opacity: 0.5; font-family: monospace; font-size: 0.7rem; }

  .form-group { margin-bottom: 1rem; }
  .form-group label { display: block; font-size: 0.875rem; font-weight: 500; margin-bottom: 0.25rem; }
  .input {
    width: 100%;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--color-border, #e2e8f0);
    border-radius: 6px;
    font-size: 0.875rem;
    background: var(--color-bg, #fff);
    box-sizing: border-box;
  }

  .btn-primary {
    padding: 0.5rem 1rem;
    background: var(--color-primary, #e00);
    color: white;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
    font-weight: 500;
  }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

  .btn-secondary {
    padding: 0.5rem 1rem;
    background: transparent;
    border: 1px solid var(--color-border, #e2e8f0);
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
  }

  .btn-danger-sm {
    padding: 0.2rem 0.6rem;
    background: transparent;
    border: 1px solid var(--color-error, #dc2626);
    color: var(--color-error, #dc2626);
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.75rem;
  }
  .btn-danger-sm:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
