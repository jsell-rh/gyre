<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import Modal from '../lib/Modal.svelte';
  import Button from '../lib/Button.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let approvals = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let filterPath = $state('');

  // Approve modal
  let showApprove = $state(false);
  let approvePath = $state('');
  let approveSha = $state('');
  let approveWorking = $state(false);

  // Revoke modal
  let showRevoke = $state(false);
  let revokeId = $state('');
  let revokeReason = $state('');
  let revokeWorking = $state(false);

  async function load() {
    loading = true;
    error = null;
    try {
      const raw = await api.specsApprovals(filterPath || undefined);
      approvals = Array.isArray(raw) ? raw : (raw?.approvals ?? []);
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  $effect(() => { load(); });

  async function doApprove() {
    if (!approvePath.trim() || approveSha.trim().length !== 40) {
      toastError('Path and 40-char SHA are required');
      return;
    }
    approveWorking = true;
    try {
      await api.specsApprove({ path: approvePath.trim(), sha: approveSha.trim() });
      toastSuccess('Spec approved');
      showApprove = false;
      approvePath = '';
      approveSha = '';
      await load();
    } catch (e) {
      toastError(e.message);
    }
    approveWorking = false;
  }

  function openRevoke(approval) {
    revokeId = approval.id;
    revokeReason = '';
    showRevoke = true;
  }

  async function doRevoke() {
    if (!revokeReason.trim()) {
      toastError('Revocation reason is required');
      return;
    }
    revokeWorking = true;
    try {
      await api.specsRevoke({ approval_id: revokeId, reason: revokeReason.trim() });
      toastSuccess('Approval revoked');
      showRevoke = false;
      await load();
    } catch (e) {
      toastError(e.message);
    }
    revokeWorking = false;
  }

  function fmtDate(ts) {
    if (!ts) return '—';
    return new Date(ts).toLocaleString();
  }

  function shortSha(sha) {
    return sha ? sha.substring(0, 8) : '—';
  }
</script>

<div class="spec-approvals">
  <div class="view-header">
    <div class="header-left">
      <h2>Spec Approvals</h2>
      <p class="header-desc">
        Cryptographic spec approval ledger (M12.3) — records spec SHA bindings for merge requests.
        For the M21 spec lifecycle dashboard (Pending→Approved state per spec, project-scoped), use
        <a class="inline-link" href="/specs">Specs</a>
        in the sidebar.
      </p>
    </div>
    <Button variant="primary" onclick={() => (showApprove = true)}>Approve Spec</Button>
  </div>

  <div class="filter-bar">
    <input
      class="filter-input"
      type="text"
      placeholder="Filter by spec path (e.g. specs/system/agent-gates.md)"
      bind:value={filterPath}
      onkeydown={(e) => e.key === 'Enter' && load()}
      aria-label="Filter spec approvals"
    />
    <Button variant="secondary" onclick={load}>Search</Button>
  </div>

  <div class="table-wrap">
    {#if loading}
      <div class="skeleton-rows">
        {#each Array(5) as _}
          <Skeleton width="100%" height="2.5rem" />
        {/each}
      </div>
    {:else if error}
      <EmptyState title="Failed to load approvals" description={error} />
    {:else if approvals.length === 0}
      <EmptyState
        title="No spec approvals"
        description="Approve a spec file to cryptographically bind it to merge requests."
      />
    {:else}
      <table class="approvals-table">
        <thead>
          <tr>
            <th>Spec Path</th>
            <th>SHA</th>
            <th>Approver</th>
            <th>Approved At</th>
            <th>Status</th>
            <th scope="col"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each approvals as a (a.id)}
            <tr class:revoked={!!a.revoked_at}>
              <td class="path-cell">
                <span class="spec-path">{a.path}</span>
              </td>
              <td><code class="sha">{shortSha(a.sha)}</code></td>
              <td class="agent-cell">{a.approver_id ?? '—'}</td>
              <td>{fmtDate(a.approved_at ?? a.created_at)}</td>
              <td>
                {#if a.revoked_at}
                  <Badge value="Revoked" color="danger" />
                {:else}
                  <Badge value="Active" color="success" />
                {/if}
              </td>
              <td>
                {#if !a.revoked_at}
                  <button class="revoke-btn" onclick={() => openRevoke(a)} aria-label="Revoke approval for {a.path}">Revoke</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>

<!-- Approve Modal -->
<Modal bind:open={showApprove} title="Approve Spec" size="sm" onsubmit={doApprove}>
  <div class="modal-form">
    <label class="field-label" for="approve-path">Spec Path</label>
    <input
      id="approve-path"
      class="field-input"
      type="text"
      bind:value={approvePath}
      placeholder="specs/system/agent-gates.md"
      onkeydown={(e) => e.key === 'Enter' && document.getElementById('approve-sha')?.focus()}
    />
    <label class="field-label" for="approve-sha">SHA (40 hex chars)</label>
    <input
      id="approve-sha"
      class="field-input mono"
      type="text"
      bind:value={approveSha}
      placeholder="abc123...40 chars"
      maxlength="40"
      onkeydown={(e) => e.key === 'Enter' && doApprove()}
    />
    <div class="modal-actions">
      <Button variant="secondary" onclick={() => (showApprove = false)}>Cancel</Button>
      <Button variant="primary" onclick={doApprove} disabled={approveWorking}>
        {approveWorking ? 'Approving…' : 'Approve'}
      </Button>
    </div>
  </div>
</Modal>

<!-- Revoke Modal -->
<Modal bind:open={showRevoke} title="Revoke Approval" size="sm" onsubmit={doRevoke}>
  <div class="modal-form">
    <label class="field-label" for="revoke-reason">Reason</label>
    <textarea
      id="revoke-reason"
      class="field-textarea"
      bind:value={revokeReason}
      placeholder="Why are you revoking this approval?"
      rows="3"
    ></textarea>
    <div class="modal-actions">
      <Button variant="secondary" onclick={() => (showRevoke = false)}>Cancel</Button>
      <Button variant="danger" onclick={doRevoke} disabled={revokeWorking}>
        {revokeWorking ? 'Revoking…' : 'Revoke'}
      </Button>
    </div>
  </div>
</Modal>

<style>
  .spec-approvals {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    padding: var(--space-6);
    gap: var(--space-5);
  }

  .view-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .header-left { display: flex; flex-direction: column; gap: var(--space-1); }

  h2 {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .header-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  .inline-link {
    color: var(--color-primary);
    text-decoration: underline;
    font-size: inherit;
  }

  .filter-bar {
    display: flex;
    gap: var(--space-2);
  }

  .filter-input {
    flex: 1;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
  }

  .filter-input:focus:not(:focus-visible) { outline: none; }
  .filter-input:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }

  .table-wrap {
    flex: 1;
    overflow-y: auto;
  }

  .skeleton-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .approvals-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .approvals-table th {
    text-align: left;
    padding: var(--space-4) var(--space-4);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-muted);
    border-bottom: 1px solid var(--color-border);
  }

  .approvals-table td {
    padding: var(--space-4) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
    vertical-align: middle;
  }

  .approvals-table tr.revoked td {
    opacity: 0.5;
  }

  .spec-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    word-break: break-all;
  }

  .sha {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-surface-elevated);
    padding: 2px var(--space-1);
    border-radius: var(--radius-sm);
  }

  .agent-cell {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .revoke-btn {
    background: transparent;
    border: 1px solid var(--color-danger);
    border-radius: var(--radius-sm);
    color: var(--color-danger);
    font-size: var(--text-xs);
    padding: 2px var(--space-2);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .revoke-btn:hover {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    border-color: var(--color-danger);
  }
  .revoke-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* Modal form */
  .modal-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .field-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .field-input,
  .field-textarea {
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    box-sizing: border-box;
    resize: vertical;
  }

  .field-input.mono { font-family: var(--font-mono); }

  .field-input:focus:not(:focus-visible),
  .field-textarea:focus:not(:focus-visible) { outline: none; }
  .field-input:focus-visible,
  .field-textarea:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
