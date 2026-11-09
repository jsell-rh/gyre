<script>
  /**
   * SecretsPanel — scoped secrets management (platform-model.md §7 UI).
   *
   * Reusable panel mounted in the Workspace / Repo / Tenant admin settings
   * tabs. Shows secret names, scopes, lifecycle type, created/expiry/rotation
   * metadata, and create/delete/rotate actions.
   *
   * Values are write-only: the create/rotate forms send them to the server,
   * and the list NEVER displays or receives a value — the server returns
   * metadata only, and this component renders `••••••••` in its place to
   * make the write-only contract visible.
   *
   * Props:
   *   scope   — 'workspace' | 'repo' | 'tenant' (the API scope value)
   *   scopeId — the scoped entity's id (null/undefined for tenant scope)
   */
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { toastError } from '../lib/toast.svelte.js';

  let {
    scope = 'workspace',
    scopeId = null,
  } = $props();

  let secrets = $state([]);
  let loading = $state(false);
  let error = $state(null);

  // ── Create form ───────────────────────────────────────────────────────
  let showForm = $state(false);
  let newName = $state('');
  let newType = $state('static');
  let newValue = $state('');
  let newExpiresAt = $state('');
  let saving = $state(false);

  // ── Rotate / delete ───────────────────────────────────────────────────
  let rotatingId = $state(null);
  let rotateValue = $state('');
  let rotateExpiresAt = $state('');
  let deletingId = $state(null);

  const SECRET_TYPES = ['static', 'ephemeral', 'rotated', 'derived'];

  function fmtDate(ts) {
    if (!ts) return '—';
    return new Date(typeof ts === 'number' ? ts * 1000 : ts).toLocaleString();
  }

  function expiryStatus(s) {
    if (!s.expires_at) return 'none';
    const now = Math.floor(Date.now() / 1000);
    if (s.expires_at <= now) return 'expired';
    if (s.expires_at <= now + 3600) return 'expiring';
    return 'active';
  }

  async function load() {
    loading = true;
    error = null;
    try {
      secrets = (await api.secretsList(scope, scopeId)) ?? [];
    } catch (e) {
      error = e?.message ?? String(e);
      secrets = [];
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    // Reload whenever the scope target changes (tab switches between
    // entities reuse the same panel instance).
    void scope;
    void scopeId;
    load();
  });;

  async function save() {
    if (!newName.trim() || !newValue) return;
    saving = true;
    try {
      await api.secretCreate({
        name: newName.trim(),
        scope,
        scope_id: scopeId ?? undefined,
        secret_type: newType,
        value: newValue,
        expires_at: newExpiresAt ? Number(newExpiresAt) : undefined,
      });
      showForm = false;
      newName = '';
      newValue = '';
      newExpiresAt = '';
      await load();
    } catch (e) {
      toastError($t('secrets_panel.save_failed', { values: { error: e?.message ?? 'unknown' } }));
    } finally {
      saving = false;
    }
  }

  function startRotate(s) {
    rotatingId = s.id;
    rotateValue = '';
    rotateExpiresAt = '';
  }

  async function confirmRotate() {
    if (!rotatingId || !rotateValue) return;
    saving = true;
    try {
      await api.secretRotate(rotatingId, {
        new_value: rotateValue,
        expires_at: rotateExpiresAt ? Number(rotateExpiresAt) : undefined,
      });
      rotatingId = null;
      await load();
    } catch (e) {
      toastError($t('secrets_panel.rotate_failed', { values: { error: e?.message ?? 'unknown' } }));
    } finally {
      saving = false;
    }
  }

  async function confirmDelete() {
    if (!deletingId) return;
    saving = true;
    try {
      await api.secretDelete(deletingId);
      deletingId = null;
      await load();
    } catch (e) {
      toastError($t('secrets_panel.delete_failed', { values: { error: e?.message ?? 'unknown' } }));
    } finally {
      saving = false;
    }
  }
</script>

<div class="secrets-panel" data-testid="secrets-panel" data-scope={scope}>
  <div class="secrets-toolbar">
    <p class="secrets-hint">{$t('secrets_panel.hint')}</p>
    <button
      class="btn-primary secrets-add-btn"
      onclick={() => (showForm = !showForm)}
      data-testid="secrets-add-btn"
    >
      {showForm ? $t('secrets_panel.cancel') : $t('secrets_panel.add')}
    </button>
  </div>

  {#if showForm}
    <form
      class="secrets-form"
      data-testid="secrets-create-form"
      onsubmit={(e) => { e.preventDefault(); save(); }}
    >
      <label>
        <span>{$t('secrets_panel.col_name')}</span>
        <input
          type="text"
          bind:value={newName}
          required
          maxlength="255"
          placeholder="DATABASE_URL"
          data-testid="secrets-new-name"
        />
      </label>
      <label>
        <span>{$t('secrets_panel.col_type')}</span>
        <select bind:value={newType} data-testid="secrets-new-type">
          {#each SECRET_TYPES as ty}
            <option value={ty}>{ty}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>{$t('secrets_panel.value_label')}</span>
        <input
          type="password"
          bind:value={newValue}
          required
          autocomplete="new-password"
          data-testid="secrets-new-value"
        />
      </label>
      <label>
        <span>{$t('secrets_panel.expires_label')}</span>
        <input
          type="number"
          bind:value={newExpiresAt}
          min="0"
          placeholder="unix epoch (optional)"
          data-testid="secrets-new-expires"
        />
      </label>
      <button type="submit" class="btn-primary" disabled={saving} data-testid="secrets-save-btn">
        {saving ? $t('secrets_panel.saving') : $t('secrets_panel.save')}
      </button>
    </form>
  {/if}

  {#if loading}
    <p class="loading-text" data-testid="secrets-loading">{$t('secrets_panel.loading')}</p>
  {:else if error}
    <p class="error-text" role="alert" data-testid="secrets-error">{error}</p>
  {:else if secrets.length === 0}
    <p class="empty-text" data-testid="secrets-empty">{$t('secrets_panel.empty')}</p>
  {:else}
    <table class="secrets-table" data-testid="secrets-table">
      <thead>
        <tr>
          <th scope="col">{$t('secrets_panel.col_name')}</th>
          <th scope="col">{$t('secrets_panel.value_label')}</th>
          <th scope="col">{$t('secrets_panel.col_type')}</th>
          <th scope="col">{$t('secrets_panel.col_created')}</th>
          <th scope="col">{$t('secrets_panel.col_expires')}</th>
          <th scope="col">{$t('secrets_panel.col_last_rotated')}</th>
          <th scope="col">{$t('secrets_panel.col_status')}</th>
          <th scope="col"><span class="sr-only">{$t('secrets_panel.col_actions')}</span></th>
        </tr>
      </thead>
      <tbody>
        {#each secrets as s (s.id)}
          <tr data-testid="secrets-row">
            <td class="secret-name">{s.name}</td>
            <td class="secret-value" aria-label={$t('secrets_panel.value_hidden_label')}>••••••••</td>
            <td>{s.secret_type}</td>
            <td>{fmtDate(s.created_at)}</td>
            <td>{fmtDate(s.expires_at)}</td>
            <td>{fmtDate(s.last_rotated_at)}</td>
            <td>
              <span class={`status-badge status-${expiryStatus(s)}`} data-testid="secret-status">
                {expiryStatus(s)}
              </span>
            </td>
            <td class="secret-actions">
              {#if rotatingId === s.id}
                <form
                  class="rotate-form"
                  data-testid="secrets-rotate-form"
                  onsubmit={(e) => { e.preventDefault(); confirmRotate(); }}
                >
                  <input
                    type="password"
                    bind:value={rotateValue}
                    required
                    autocomplete="new-password"
                    placeholder={$t('secrets_panel.new_value_label')}
                    aria-label={$t('secrets_panel.new_value_label')}
                    data-testid="secrets-rotate-value"
                  />
                  <input
                    type="number"
                    bind:value={rotateExpiresAt}
                    min="0"
                    placeholder="unix epoch (optional)"
                    aria-label={$t('secrets_panel.expires_label')}
                    data-testid="secrets-rotate-expires"
                  />
                  <button type="submit" class="btn-primary" disabled={saving}>
                    {$t('secrets_panel.rotate_confirm')}
                  </button>
                  <button type="button" class="btn-secondary" onclick={() => (rotatingId = null)}>
                    {$t('secrets_panel.cancel')}
                  </button>
                </form>
              {:else if deletingId === s.id}
                <span class="confirm-row" data-testid="secrets-delete-confirm">
                  <span>{$t('secrets_panel.delete_confirm')}</span>
                  <button class="btn-danger" onclick={confirmDelete} disabled={saving}>
                    {$t('secrets_panel.delete_yes')}
                  </button>
                  <button class="btn-secondary" onclick={() => (deletingId = null)}>
                    {$t('secrets_panel.cancel')}
                  </button>
                </span>
              {:else}
                <button
                  class="btn-secondary"
                  onclick={() => startRotate(s)}
                  data-testid="secrets-rotate-btn"
                >
                  {$t('secrets_panel.rotate')}
                </button>
                <button
                  class="btn-danger"
                  onclick={() => (deletingId = s.id)}
                  data-testid="secrets-delete-btn"
                >
                  {$t('secrets_panel.delete')}
                </button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .secrets-panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .secrets-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .secrets-hint {
    margin: 0;
    color: var(--color-text-muted, #888);
    font-size: 0.9em;
  }

  .secrets-form {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: flex-end;
    padding: 12px;
    border: 1px solid var(--color-border, #333);
    border-radius: 8px;
  }

  .secrets-form label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 0.85em;
    color: var(--color-text-muted, #888);
  }

  .secrets-form input,
  .secrets-form select {
    min-width: 160px;
  }

  .secrets-table {
    width: 100%;
    border-collapse: collapse;
  }

  .secrets-table th,
  .secrets-table td {
    text-align: left;
    padding: 8px 10px;
    border-bottom: 1px solid var(--color-border, #333);
    font-size: 0.9em;
  }

  .secret-name {
    font-weight: 600;
  }

  .secret-value {
    color: var(--color-text-muted, #888);
    letter-spacing: 2px;
  }

  .secret-actions {
    white-space: nowrap;
  }

  .secret-actions button {
    margin-right: 6px;
  }

  .rotate-form {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-wrap: wrap;
  }

  .rotate-form input[type='password'] {
    width: 140px;
  }

  .rotate-form input[type='number'] {
    width: 130px;
  }

  .confirm-row {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    font-size: 0.85em;
  }

  .status-badge {
    display: inline-block;
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 0.8em;
  }

  .status-active {
    background: rgba(46, 160, 67, 0.2);
    color: #2ea043;
  }

  .status-expiring {
    background: rgba(187, 128, 9, 0.2);
    color: #bb8009;
  }

  .status-expired {
    background: rgba(248, 81, 73, 0.2);
    color: #f85149;
  }

  .status-none {
    background: var(--color-bg-secondary, #222);
    color: var(--color-text-muted, #888);
  }

  .loading-text,
  .empty-text {
    color: var(--color-text-muted, #888);
  }

  .error-text {
    color: #f85149;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  .btn-primary {
    background: var(--color-accent, #4493f8);
    color: #fff;
    border: none;
    border-radius: 6px;
    padding: 6px 12px;
    cursor: pointer;
  }

  .btn-primary:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .btn-secondary {
    background: transparent;
    color: var(--color-text, #eee);
    border: 1px solid var(--color-border, #444);
    border-radius: 6px;
    padding: 5px 10px;
    cursor: pointer;
  }

  .btn-danger {
    background: transparent;
    color: #f85149;
    border: 1px solid #f85149;
    border-radius: 6px;
    padding: 5px 10px;
    cursor: pointer;
  }
</style>
