<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Card from '../lib/Card.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import Modal from '../lib/Modal.svelte';
  import Button from '../lib/Button.svelte';

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

  let specs     = $state([]);
  let loading   = $state(true);
  let error     = $state(null);
  let kindFilter = $state('all');

  // Blast radius modal
  let blastOpen       = $state(false);
  let blastPath       = $state('');
  let blastLoading    = $state(false);
  let blastResult     = $state(null);

  async function load() {
    loading = true;
    error = null;
    try {
      // Fetch all specs and filter client-side by meta kinds.
      const all = await api.getSpecs();
      specs = Array.isArray(all)
        ? all.filter(s => s.kind && s.kind.startsWith('meta:'))
        : [];
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  $effect(() => { load(); });

  const filtered = $derived(() => {
    if (kindFilter === 'all') return specs;
    return specs.filter(s => s.kind === kindFilter);
  });

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

  function kindBadgeVariant(kind) {
    return KIND_COLORS[kind] || 'gray';
  }

  function kindLabel(kind) {
    return KIND_LABELS[kind] || kind;
  }
</script>

<div class="meta-specs-view">
  <div class="view-header">
    <h2>Meta-Specs</h2>
    <p class="subtitle">Versioned specs that govern agent behavior — personas, principles, standards, and process norms.</p>
  </div>

  <!-- Kind filter pills -->
  <div class="filter-pills">
    <button
      class="pill"
      class:active={kindFilter === 'all'}
      onclick={() => kindFilter = 'all'}
    >
      All
    </button>
    {#each META_KINDS as k}
      <button
        class="pill"
        class:active={kindFilter === k}
        onclick={() => kindFilter = k}
      >
        {KIND_LABELS[k]}
      </button>
    {/each}
  </div>

  {#if loading}
    <div class="grid">
      {#each Array(6) as _}
        <div class="card-skeleton"><Skeleton /></div>
      {/each}
    </div>
  {:else if error}
    <EmptyState title="Failed to load meta-specs" description={error} />
  {:else if filtered().length === 0}
    <EmptyState
      title="No meta-specs found"
      description="Add meta-spec entries to specs/manifest.yaml with kind: meta:persona (or principle/standard/process)."
    />
  {:else}
    <div class="grid">
      {#each filtered() as spec}
        <Card>
          <div class="card-inner">
            <div class="card-top">
              <Badge value={kindLabel(spec.kind)} variant={kindBadgeVariant(spec.kind)} />
              <Badge
                value={spec.approval_status}
                variant={spec.approval_status === 'approved' ? 'green' : spec.approval_status === 'pending' ? 'yellow' : 'gray'}
              />
            </div>
            <div class="card-title" title={spec.path}>{spec.title || spec.path}</div>
            <div class="card-path mono">{spec.path}</div>
            {#if spec.owner}
              <div class="card-owner">{spec.owner}</div>
            {/if}
            <div class="card-sha mono">SHA: {spec.current_sha?.slice(0, 8) || '—'}</div>
            <div class="card-actions">
              <Button
                variant="secondary"
                size="sm"
                onclick={() => openBlastRadius(spec.path)}
              >
                Blast Radius
              </Button>
            </div>
          </div>
        </Card>
      {/each}
    </div>
  {/if}
</div>

<!-- Blast Radius Modal -->
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
              <li>
                <span class="mono">{repo.id}</span>
                <Badge value={repo.reason} variant="gray" />
              </li>
            {/each}
          </ul>
        {:else}
          <p class="empty">No repos affected.</p>
        {/if}
      </div>
    {/if}
  </Modal>
{/if}

<style>
  .meta-specs-view {
    padding: var(--space-6, 1.5rem);
    max-width: 1200px;
  }

  .view-header { margin-bottom: 1.5rem; }
  .view-header h2 { margin: 0 0 0.25rem; font-size: 1.5rem; }
  .subtitle { margin: 0; color: var(--color-text-muted, #888); font-size: 0.9rem; }

  .filter-pills {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
    margin-bottom: 1.5rem;
  }

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
  .pill.active {
    background: var(--color-primary, #ee0000);
    border-color: var(--color-primary, #ee0000);
    color: #fff;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 1rem;
  }

  .card-skeleton { height: 160px; }

  .card-inner {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.25rem;
  }

  .card-top { display: flex; gap: 0.4rem; flex-wrap: wrap; }
  .card-title { font-weight: 600; font-size: 0.95rem; }
  .card-path { font-size: 0.78rem; color: var(--color-text-muted, #888); }
  .card-owner { font-size: 0.8rem; color: var(--color-text-muted, #888); }
  .card-sha { font-size: 0.78rem; color: var(--color-text-muted, #888); }
  .card-actions { margin-top: 0.5rem; }
  .mono { font-family: monospace; }

  .blast-section { margin-bottom: 1.25rem; }
  .blast-section h4 { margin: 0 0 0.5rem; font-size: 0.95rem; }
  .blast-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .blast-list li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.88rem;
    padding: 0.3rem 0.5rem;
    background: var(--color-surface-2, #1a1a1a);
    border-radius: 4px;
  }
  .empty { color: var(--color-text-muted, #888); font-size: 0.88rem; margin: 0; }
  .error { color: var(--color-error, #f55); font-size: 0.88rem; }
</style>
