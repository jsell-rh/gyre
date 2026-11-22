<script>
  import { t } from 'svelte-i18n';
  import { getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Button from '../lib/Button.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  // Notification preferences page (user-management.md §In-App Notification
  // UI: "Notification preferences page in user settings"; task plan route
  // /settings/notifications). Edits the caller's delivery-channel
  // configuration via GET/PUT /api/v1/notifications/preferences:
  // email on/off, digest frequency, minimum email priority, per-type email
  // unsubscribe (the landing target of every email's unsubscribe link),
  // webhook, and Slack configuration.

  const goToWorkspaceHome = getContext('goToWorkspaceHome');

  const PRIORITIES = ['Low', 'Medium', 'High', 'Urgent'];
  const DIGEST_OPTIONS = ['Immediate', 'Hourly', 'Daily', 'Off'];
  // Types offered for per-type unsubscribe: the server taxonomy (matches
  // EmailConfig::type_overrides keys and the `?unsubscribe=` link target).
  const TYPES = [
    'AbandonedBranch', 'AgentBudgetExhausted', 'AgentBudgetWarning', 'AgentCompleted',
    'AgentEscalation', 'AgentFailed', 'AgentNeedsClarification', 'AtomicGroupFailure',
    'BreakingChangeDetected', 'CascadeTestFailed', 'CascadeTestTriggered', 'ComputeTargetUnavailable',
    'ConflictingInterpretations', 'ConstraintViolation', 'CrossWorkspaceSpecChange', 'DependencyChainTooDeep',
    'DependencyStale', 'GateFailure', 'GatePassed', 'ImpersonationAlert', 'ImpersonationRequest',
    'InvitationReceived', 'MembershipChanged', 'MergeQueueEscalation', 'MergeQueuePaused',
    'MetaSpecDrift', 'MrMerged', 'MrReverted', 'PersonaApprovalRequested', 'PersonaChangePendingApproval',
    'SecretExpiring', 'SecurityFinding', 'SpecApprovalRequested', 'SpecAssertionFailure',
    'SpecConflict', 'SpecDriftDetected', 'SpecRejected', 'SuggestedSpecLink', 'SystemAlert',
    'TrustSuggestion', 'WorkspaceOwnerless',
  ];

  let loading = $state(true);
  let saving = $state(false);
  let loaded = $state(false);
  let unsubscribeTarget = $state(null);

  let emailEnabled = $state(false);
  let digest = $state('Off');
  let minPriority = $state('Medium');
  let typeOverrides = $state({});
  let webhookEnabled = $state(false);
  let webhookUrl = $state('');
  let webhookSecret = $state('');
  let webhookMinPriority = $state('Low');
  let slackEnabled = $state(false);
  let slackUrl = $state('');
  let slackChannel = $state('');
  let slackMinPriority = $state('Low');

  function typeLabel(typ) {
    const key = `decisions.type_labels.${typ}`;
    const val = $t(key);
    return val === key ? typ : val;
  }

  function applyChannels(ch) {
    emailEnabled = ch?.email?.enabled ?? false;
    digest = ch?.email?.digest ?? 'Off';
    minPriority = ch?.email?.min_priority ?? 'Medium';
    typeOverrides = { ...(ch?.email?.type_overrides ?? {}) };
    webhookEnabled = !!ch?.webhook;
    webhookUrl = ch?.webhook?.url ?? '';
    webhookSecret = ch?.webhook?.secret ?? '';
    webhookMinPriority = ch?.webhook?.min_priority ?? 'Low';
    slackEnabled = !!ch?.slack;
    slackUrl = ch?.slack?.webhook_url ?? '';
    slackChannel = ch?.slack?.channel ?? '';
    slackMinPriority = ch?.slack?.min_priority ?? 'Low';
  }

  async function load() {
    loading = true;
    try {
      const res = await api.notificationChannels();
      applyChannels(res?.channels ?? null);
      loaded = true;

      // Email unsubscribe link landing (§Email Notifications): apply the
      // per-type opt-out the link asked for, once, after the current
      // config is known.
      const params = new URLSearchParams(window.location.search);
      const target = params.get('unsubscribe');
      if (target && TYPES.includes(target)) {
        unsubscribeTarget = target;
        if (typeOverrides[target] !== true) {
          typeOverrides = { ...typeOverrides, [target]: true };
          await save(false);
        }
      }
    } catch (e) {
      showToast($t('notifications.settings.load_failed') + (e?.message ? `: ${e.message}` : ''), { type: 'error' });
    } finally {
      loading = false;
    }
  }

  function channelsPayload() {
    return {
      in_app: true, // Always true (can't disable) — §Delivery Channels.
      email: {
        enabled: emailEnabled,
        digest,
        min_priority: minPriority,
        type_overrides: Object.fromEntries(
          Object.entries(typeOverrides).filter(([, v]) => v === true)
        ),
      },
      webhook: webhookEnabled && webhookUrl.trim()
        ? { url: webhookUrl.trim(), secret: webhookSecret, min_priority: webhookMinPriority }
        : null,
      slack: slackEnabled && slackUrl.trim()
        ? { webhook_url: slackUrl.trim(), channel: slackChannel.trim() || null, min_priority: slackMinPriority }
        : null,
    };
  }

  async function save(showToastOnSuccess = true) {
    saving = true;
    try {
      const res = await api.updateNotificationChannels({ channels: channelsPayload() });
      applyChannels(res?.channels ?? channelsPayload());
      if (showToastOnSuccess) showToast($t('notifications.settings.saved'), { type: 'success' });
    } catch (e) {
      showToast($t('notifications.settings.save_failed') + (e?.message ? `: ${e.message}` : ''), { type: 'error' });
    } finally {
      saving = false;
    }
  }

  $effect(() => { void load(); });
</script>

<div class="notif-settings" data-testid="notification-settings">
  <header class="settings-header">
    <button
      class="back-btn"
      onclick={() => goToWorkspaceHome?.()}
      aria-label={$t('topbar.back_to_workspace')}
      title={$t('topbar.back_to_workspace')}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <path d="M19 12H5"/><path d="M12 19l-7-7 7-7"/>
      </svg>
    </button>
    <h1>{$t('notifications.settings.title')}</h1>
  </header>

  {#if unsubscribeTarget}
    <div class="unsub-banner" data-testid="unsubscribe-banner" role="status">
      {$t('notifications.settings.unsubscribed', { values: { type: typeLabel(unsubscribeTarget) } })}
    </div>
  {/if}

  {#if loading && !loaded}
    <Skeleton lines={6} />
  {:else}
    <section class="settings-section">
      <h2>{$t('notifications.settings.email_title')}</h2>
      <label class="setting-row">
        <input type="checkbox" bind:checked={emailEnabled} data-testid="email-enabled" />
        <span>{$t('notifications.settings.email_enable')}</span>
      </label>
      {#if emailEnabled}
        <label class="setting-row">
          <span class="setting-label">{$t('notifications.settings.digest_frequency')}</span>
          <select bind:value={digest} data-testid="digest-frequency">
            {#each DIGEST_OPTIONS as opt (opt)}
              <option value={opt}>{$t(`notifications.settings.digest_${opt.toLowerCase()}`)}</option>
            {/each}
          </select>
        </label>
        <label class="setting-row">
          <span class="setting-label">{$t('notifications.settings.min_priority')}</span>
          <select bind:value={minPriority} data-testid="email-min-priority">
            {#each PRIORITIES as p (p)}
              <option value={p}>{$t(`notifications.settings.priority_${p.toLowerCase()}`)}</option>
            {/each}
          </select>
        </label>
        <p class="hint">{$t('notifications.settings.min_priority_hint')}</p>

        <h3>{$t('notifications.settings.per_type_title')}</h3>
        <p class="hint">{$t('notifications.settings.per_type_hint')}</p>
        <div class="type-grid" data-testid="type-overrides">
          {#each TYPES as typ (typ)}
            <label class="type-row">
              <input
                type="checkbox"
                checked={typeOverrides[typ] !== true}
                onchange={(e) => {
                  typeOverrides = { ...typeOverrides, [typ]: !e.currentTarget.checked };
                }}
                data-testid={`type-override-${typ}`}
              />
              <span>{typeLabel(typ)}</span>
            </label>
          {/each}
        </div>
      {/if}
    </section>

    <section class="settings-section">
      <h2>{$t('notifications.settings.webhook_title')}</h2>
      <label class="setting-row">
        <input type="checkbox" bind:checked={webhookEnabled} data-testid="webhook-enabled" />
        <span>{$t('notifications.settings.webhook_enable')}</span>
      </label>
      {#if webhookEnabled}
        <label class="setting-row">
          <span class="setting-label">URL</span>
          <input type="url" bind:value={webhookUrl} placeholder="https://hooks.example.com/..." data-testid="webhook-url" />
        </label>
        <label class="setting-row">
          <span class="setting-label">{$t('notifications.settings.webhook_secret')}</span>
          <input type="password" bind:value={webhookSecret} autocomplete="off" data-testid="webhook-secret" />
        </label>
        <label class="setting-row">
          <span class="setting-label">{$t('notifications.settings.min_priority')}</span>
          <select bind:value={webhookMinPriority} data-testid="webhook-min-priority">
            {#each PRIORITIES as p (p)}
              <option value={p}>{$t(`notifications.settings.priority_${p.toLowerCase()}`)}</option>
            {/each}
          </select>
        </label>
        <p class="hint">{$t('notifications.settings.webhook_hint')}</p>
      {/if}
    </section>

    <section class="settings-section">
      <h2>{$t('notifications.settings.slack_title')}</h2>
      <label class="setting-row">
        <input type="checkbox" bind:checked={slackEnabled} data-testid="slack-enabled" />
        <span>{$t('notifications.settings.slack_enable')}</span>
      </label>
      {#if slackEnabled}
        <label class="setting-row">
          <span class="setting-label">Webhook URL</span>
          <input type="url" bind:value={slackUrl} placeholder="https://hooks.slack.com/..." data-testid="slack-url" />
        </label>
        <label class="setting-row">
          <span class="setting-label">{$t('notifications.settings.slack_channel')}</span>
          <input type="text" bind:value={slackChannel} placeholder="#alerts" data-testid="slack-channel" />
        </label>
        <label class="setting-row">
          <span class="setting-label">{$t('notifications.settings.min_priority')}</span>
          <select bind:value={slackMinPriority} data-testid="slack-min-priority">
            {#each PRIORITIES as p (p)}
              <option value={p}>{$t(`notifications.settings.priority_${p.toLowerCase()}`)}</option>
            {/each}
          </select>
        </label>
      {/if}
    </section>

    <div class="settings-actions">
      <Button variant="primary" onclick={() => save()} disabled={saving} aria-busy={saving} data-testid="save-notification-settings">
        {saving ? $t('common.saving') : $t('common.save')}
      </Button>
    </div>
  {/if}
</div>

<style>
  .notif-settings {
    max-width: 680px; margin: 0 auto; padding: var(--space-6, 24px) var(--space-4, 16px);
    display: flex; flex-direction: column; gap: var(--space-5, 20px);
  }
  .settings-header { display: flex; align-items: center; gap: 12px; }
  .settings-header h1 { margin: 0; font-size: 20px; }
  .back-btn {
    display: inline-flex; align-items: center; justify-content: center;
    width: 30px; height: 30px; border-radius: 8px; border: none; cursor: pointer;
    background: transparent; color: var(--color-text, #374151);
  }
  .back-btn:hover { background: var(--color-bg-hover, #f4f5f7); }
  .unsub-banner {
    padding: 12px 16px; border-radius: 8px; background: #ecfdf5;
    border: 1px solid #10b981; color: #065f46; font-size: 14px;
  }
  .settings-section {
    border: 1px solid var(--color-border, #e2e4e9); border-radius: 10px;
    padding: 16px; display: flex; flex-direction: column; gap: 10px;
  }
  .settings-section h2 { margin: 0 0 4px; font-size: 16px; }
  .settings-section h3 { margin: 12px 0 0; font-size: 13px; }
  .setting-row { display: flex; align-items: center; gap: 10px; font-size: 14px; }
  .setting-row .setting-label { min-width: 150px; color: var(--color-text-muted, #6b7280); }
  .setting-row select, .setting-row input[type='url'], .setting-row input[type='text'], .setting-row input[type='password'] {
    flex: 1; max-width: 380px; padding: 6px 8px; font-size: 13px;
    border-radius: 6px; border: 1px solid var(--color-border, #e2e4e9);
    background: var(--color-bg, #fff);
  }
  .hint { margin: 0; font-size: 12px; color: var(--color-text-muted, #6b7280); }
  .type-grid {
    display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 4px 12px; max-height: 240px; overflow-y: auto; padding: 4px;
  }
  .type-row { display: flex; align-items: center; gap: 6px; font-size: 12px; }
  .settings-actions { display: flex; justify-content: flex-end; }
</style>
