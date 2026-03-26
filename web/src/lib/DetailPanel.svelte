<script>
  import Tabs from './Tabs.svelte';
  import Button from './Button.svelte';
  import Badge from './Badge.svelte';
  import Skeleton from './Skeleton.svelte';
  import EmptyState from './EmptyState.svelte';
  import { api } from './api.js';
  import { toastSuccess, toastError } from './toast.svelte.js';

  /**
   * DetailPanel — slide-in panel from the right.
   *
   * Spec ref: ui-layout.md §2 (Split layout), §3 (Drill-Down pattern)
   *           ui-layout.md §6 (Spec entity tabs: Content/Edit/Progress/Links/History)
   *
   * Props:
   *   entity   — { type, id, data } | null
   *   expanded — bool, true when popped out to full-width
   *   onclose  — () => void
   *   onpopout — () => void
   */
  let {
    entity = null,
    expanded = $bindable(false),
    onclose = undefined,
    onpopout = undefined,
  } = $props();

  let activeTab = $state('info');
  let panelEl = $state(null);
  let interrogationLoading = $state(false);

  // Compute which tabs to show based on entity type.
  // Spec: ui-layout.md §2 "Detail panel tabs (contextual)"
  let tabs = $derived(computeTabs(entity));

  function computeTabs(ent) {
    if (!ent) return [];
    const type = ent.type;
    const data = ent.data ?? {};

    if (type === 'spec') {
      // Spec entities from the Specs view: richer tab set
      return [
        { id: 'content',     label: 'Content' },
        { id: 'edit',        label: 'Edit' },
        { id: 'progress',    label: 'Progress' },
        { id: 'links',       label: 'Links' },
        { id: 'history',     label: 'History' },
      ];
    }

    const result = [{ id: 'info', label: 'Info' }];

    if (type === 'mr') {
      result.push(
        { id: 'diff',        label: 'Diff' },
        { id: 'gates',       label: 'Gates' },
      );
      if (data.status === 'merged') {
        result.push({ id: 'attestation', label: 'Attestation' });
      }
      result.push({
        id: 'ask-why',
        label: 'Ask Why',
        disabled: !data.conversation_sha,
        title: data.conversation_sha ? undefined : 'Conversation unavailable',
      });
      return result;
    }

    if (type === 'agent') {
      result.push(
        { id: 'chat',    label: 'Chat' },
        { id: 'history', label: 'History' },
        { id: 'trace',   label: 'Trace' },
      );
      if (data.conversation_sha !== undefined) {
        result.push({
          id: 'ask-why',
          label: 'Ask Why',
          disabled: !data.conversation_sha,
          title: data.conversation_sha ? undefined : 'Conversation unavailable',
        });
      }
      return result;
    }

    if (type === 'node') {
      if (data.spec_path) result.push({ id: 'spec', label: 'Spec' });
      if (data.author_agent_id) result.push({ id: 'chat', label: 'Chat' });
      result.push({ id: 'history', label: 'History' });
      return result;
    }

    // Generic: info + optional extras
    if (data.spec_path) result.push({ id: 'spec', label: 'Spec' });
    if (data.author_agent_id) result.push({ id: 'chat', label: 'Chat' });
    if (data.has_history) result.push({ id: 'history', label: 'History' });
    return result;
  }

  async function startInterrogation() {
    if (!entity) return;
    const data = entity.data ?? {};
    const repoId = data.repo_id ?? data.repository_id ?? null;
    const taskId = data.task_id ?? data.current_task_id ?? null;
    const conversationSha = data.conversation_sha ?? null;
    if (!repoId || !taskId) {
      toastError('Cannot start interrogation: entity is missing repo/task context.');
      return;
    }
    interrogationLoading = true;
    try {
      await api.spawnAgent({
        name: `interrogation-${entity.type}-${entity.id}`,
        repo_id: repoId,
        task_id: taskId,
        branch: `interrogation/${entity.type}/${entity.id}`,
        agent_type: 'interrogation',
        conversation_sha: conversationSha,
      });
      toastSuccess('Interrogation agent spawned.');
    } catch (e) {
      toastError('Failed to spawn interrogation agent: ' + (e?.message ?? String(e)));
    } finally {
      interrogationLoading = false;
    }
  }

  // Reset active tab when entity changes, defaulting to the first tab.
  $effect(() => {
    if (entity) {
      const first = tabs[0];
      if (first) activeTab = first.id;
    }
  });

  // Keyboard: Esc closes the panel.
  function onkeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }

  function close() {
    expanded = false;
    onclose?.();
  }

  function popout() {
    expanded = !expanded;
    onpopout?.();
    // Update URL to reflect expanded state (deep-linkable).
    if (entity) {
      const url = new URL(window.location.href);
      if (expanded) {
        url.searchParams.set('detail', `${entity.type}:${entity.id}`);
        url.searchParams.set('expanded', 'true');
      } else {
        url.searchParams.delete('expanded');
      }
      window.history.replaceState({}, '', url.toString());
    }
  }

  // Focus management: when panel opens, move focus into it.
  $effect(() => {
    if (entity && panelEl) {
      const focusable = panelEl.querySelector(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      focusable?.focus();
    }
  });

  // Guard: if entity is cleared through any code path, ensure expanded resets.
  $effect(() => {
    if (!entity) expanded = false;
  });

  // ── Spec entity tab state (S4.5) ────────────────────────────────────────────
  // Lazy-loaded data for each tab when entity.type === 'spec'
  let specDetail = $state(null);
  let specDetailLoading = $state(false);
  let specProgress = $state(null);
  let specProgressLoading = $state(false);
  let specLinks = $state(null);
  let specLinksLoading = $state(false);
  let specHistory = $state(null);
  let specHistoryLoading = $state(false);

  // Edit tab
  let editContent = $state('');
  let llmInstruction = $state('');
  let llmStreaming = $state(false);
  let llmExplanation = $state('');
  let llmSuggestion = $state(null); // { diff: [...], explanation: string } | null
  let saving = $state(false);

  // Reset spec data when entity changes
  $effect(() => {
    if (entity?.type === 'spec') {
      specDetail = null;
      specProgress = null;
      specLinks = null;
      specHistory = null;
      editContent = '';
      llmSuggestion = null;
      llmExplanation = '';
    }
  });

  // Load data for the active spec tab
  $effect(() => {
    if (entity?.type !== 'spec') return;
    const path = entity.id;
    const repoId = entity.data?.repo_id ?? null;

    if (activeTab === 'content' && !specDetail && !specDetailLoading) {
      specDetailLoading = true;
      api.specContent(path, repoId)
        .then((d) => { specDetail = d; editContent = d?.content ?? ''; })
        .catch(() => { specDetail = null; })
        .finally(() => { specDetailLoading = false; });
    }
    if (activeTab === 'edit' && !specDetail && !specDetailLoading) {
      specDetailLoading = true;
      api.specContent(path, repoId)
        .then((d) => { specDetail = d; editContent = d?.content ?? ''; })
        .catch(() => { specDetail = null; })
        .finally(() => { specDetailLoading = false; });
    }
    if (activeTab === 'progress' && !specProgress && !specProgressLoading) {
      specProgressLoading = true;
      api.specProgress(path, repoId)
        .then((p) => { specProgress = p; })
        .catch(() => { specProgress = null; })
        .finally(() => { specProgressLoading = false; });
    }
    if (activeTab === 'links' && !specLinks && !specLinksLoading) {
      specLinksLoading = true;
      api.specLinks(path, repoId)
        .then((l) => { specLinks = l; })
        .catch(() => { specLinks = null; })
        .finally(() => { specLinksLoading = false; });
    }
    if (activeTab === 'history' && !specHistory && !specHistoryLoading) {
      specHistoryLoading = true;
      api.specHistoryRepo(path, repoId)
        .then((h) => { specHistory = Array.isArray(h) ? h : []; })
        .catch(() => { specHistory = []; })
        .finally(() => { specHistoryLoading = false; });
    }
  });

  // LLM-assisted spec editing
  async function sendLlmInstruction() {
    if (!llmInstruction.trim() || llmStreaming) return;
    const repoId = entity?.data?.repo_id;
    if (!repoId) return;
    const instruction = llmInstruction.trim();
    llmInstruction = '';
    llmStreaming = true;
    llmExplanation = '';
    llmSuggestion = null;

    try {
      const resp = await api.specsAssist(repoId, {
        spec_path: entity.id,
        instruction,
        draft_content: editContent || undefined,
      });
      if (!resp.ok) throw new Error(`LLM request failed: ${resp.status}`);

      const reader = resp.body?.getReader();
      if (!reader) throw new Error('No response body');
      const decoder = new TextDecoder();
      let buf = '';
      let done = false;

      while (!done) {
        const { value, done: streamDone } = await reader.read();
        done = streamDone;
        if (value) {
          buf += decoder.decode(value, { stream: true });
          const lines = buf.split('\n');
          buf = lines.pop() ?? '';
          for (const line of lines) {
            if (!line.startsWith('data: ')) continue;
            const raw = line.slice(6);
            if (raw === '[DONE]') { done = true; break; }
            try {
              const parsed = JSON.parse(raw);
              if (parsed.event === 'partial' || parsed.type === 'partial') {
                llmExplanation += parsed.text ?? parsed.explanation ?? '';
              } else if (parsed.event === 'complete' || parsed.type === 'complete') {
                llmSuggestion = {
                  diff: parsed.diff ?? [],
                  explanation: parsed.explanation ?? llmExplanation,
                };
                done = true; break;
              } else if (parsed.event === 'error' || parsed.type === 'error') {
                throw new Error(parsed.message ?? 'LLM error');
              }
            } catch (pe) {
              if (pe.message && !pe.message.startsWith('Unexpected token')) throw pe;
            }
          }
        }
      }
    } catch (e) {
      toastError(`LLM assist failed: ${e.message}`);
    } finally {
      llmStreaming = false;
    }
  }

  function acceptSuggestion() {
    if (!llmSuggestion) return;
    let content = editContent;
    for (const op of llmSuggestion.diff) {
      if (op.op === 'add') {
        const idx = content.indexOf(op.path);
        if (idx !== -1) {
          const lineEnd = content.indexOf('\n', idx + op.path.length);
          const insertAt = lineEnd !== -1 ? lineEnd + 1 : content.length;
          content = content.slice(0, insertAt) + op.content + '\n' + content.slice(insertAt);
        } else {
          content += '\n' + op.content;
        }
      } else if (op.op === 'replace') {
        const idx = content.indexOf(op.path);
        if (idx !== -1) {
          const end = findSectionEnd(content, idx + op.path.length);
          content = content.slice(0, idx) + op.path + '\n' + op.content + content.slice(end);
        }
      } else if (op.op === 'remove') {
        const idx = content.indexOf(op.path);
        if (idx !== -1) {
          const end = findSectionEnd(content, idx + op.path.length);
          content = content.slice(0, idx) + content.slice(end);
        }
      }
    }
    editContent = content;
    llmSuggestion = null;
  }

  function editSuggestion() {
    if (llmSuggestion?.diff?.[0]?.content) {
      editContent += '\n\n' + llmSuggestion.diff.map((d) => d.content).join('\n\n');
    }
    llmSuggestion = null;
  }

  function dismissSuggestion() { llmSuggestion = null; }

  function findSectionEnd(content, from) {
    const rest = content.slice(from);
    const match = rest.match(/\n(#{1,6} )/);
    if (match?.index !== undefined) return from + match.index + 1;
    return content.length;
  }

  async function saveSpec() {
    if (!entity || saving) return;
    const repoId = entity.data?.repo_id;
    if (!repoId) return;
    saving = true;
    try {
      const result = await api.specsSave(repoId, {
        spec_path: entity.id,
        content: editContent,
        message: `Update ${entity.id} via UI editor`,
      });
      toastSuccess(`Spec saved — MR #${result.mr_id} created`);
    } catch (e) {
      toastError(`Save failed: ${e.message}`);
    } finally {
      saving = false;
    }
  }

  // Helpers for spec tabs
  function specStatusColor(s) {
    if (s === 'approved')   return 'success';
    if (s === 'pending')    return 'warning';
    if (s === 'deprecated') return 'neutral';
    return 'neutral';
  }

  function taskStatusColor(s) {
    if (s === 'done')        return 'success';
    if (s === 'in_progress') return 'warning';
    return 'neutral';
  }

  function fmtDate(ts) {
    if (!ts) return '—';
    return new Date(ts * 1000).toLocaleString([], {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
    });
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside
  class="detail-panel"
  class:expanded
  class:open={!!entity}
  aria-label="Detail panel"
  tabindex="-1"
  onkeydown={onkeydown}
  bind:this={panelEl}
>
  {#if entity}
    <div class="panel-header">
      <div class="panel-entity">
        <span class="entity-type">{entity.type}</span>
        <span class="entity-id">{entity.data?.name ?? entity.id}</span>
      </div>
      <div class="panel-actions">
        <button
          class="panel-btn"
          onclick={popout}
          aria-label={expanded ? 'Collapse panel' : 'Pop out to full width'}
          title={expanded ? 'Collapse' : 'Pop Out'}
        >
          {#if expanded}
            <!-- Collapse icon -->
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
              <path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"/>
            </svg>
          {:else}
            <!-- Pop out icon -->
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
              <polyline points="15 3 21 3 21 9"/><polyline points="9 21 3 21 3 15"/>
              <line x1="21" y1="3" x2="14" y2="10"/><line x1="3" y1="21" x2="10" y2="14"/>
            </svg>
          {/if}
          <span class="sr-only">{expanded ? 'Collapse' : 'Pop Out'}</span>
        </button>
        <button
          class="panel-btn panel-close"
          onclick={close}
          aria-label="Close detail panel"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <path d="M18 6L6 18M6 6l12 12"/>
          </svg>
        </button>
      </div>
    </div>

    <Tabs {tabs} bind:active={activeTab} panelId="detail-panel-content" />

    <div class="panel-content" id="detail-panel-content" role="tabpanel" aria-labelledby="tab-{activeTab}">
      {#if activeTab === 'info'}
        <div class="tab-pane">
          <dl class="entity-meta">
            <dt>Type</dt><dd>{entity.type}</dd>
            <dt>ID</dt><dd class="mono">{entity.id}</dd>
            {#if entity.data?.status}
              <dt>Status</dt><dd>{entity.data.status}</dd>
            {/if}
            {#if entity.data?.created_at}
              <dt>Created</dt><dd>{new Date(entity.data.created_at * 1000).toLocaleString()}</dd>
            {/if}
            {#if entity.data?.spec_path}
              <dt>Spec</dt><dd class="mono">{entity.data.spec_path}</dd>
            {/if}
          </dl>
        </div>

      {:else if activeTab === 'content'}
        <div class="tab-pane spec-content-tab">
          {#if specDetailLoading}
            <div class="spec-skeleton">
              {#each Array(5) as _}<Skeleton width="100%" height="1.2rem" />{/each}
            </div>
          {:else if specDetail?.content}
            <dl class="spec-meta-list">
              {#if entity.data?.approval_status}
                <dt>Status</dt>
                <dd>
                  <Badge value={entity.data.approval_status} color={specStatusColor(entity.data.approval_status)} />
                </dd>
              {/if}
              {#if entity.data?.owner}
                <dt>Owner</dt><dd class="mono">{entity.data.owner}</dd>
              {/if}
              {#if entity.data?.updated_at}
                <dt>Updated</dt><dd>{fmtDate(entity.data.updated_at)}</dd>
              {/if}
            </dl>
            <div class="spec-content-box">
              <pre class="spec-content-pre">{specDetail.content}</pre>
            </div>
          {:else}
            <!-- Metadata fallback (no content field from server yet) -->
            <dl class="spec-meta-list">
              <dt>Path</dt><dd class="mono">{entity.id}</dd>
              {#if entity.data?.title}
                <dt>Title</dt><dd>{entity.data.title}</dd>
              {/if}
              {#if entity.data?.owner}
                <dt>Owner</dt><dd class="mono">{entity.data.owner}</dd>
              {/if}
              {#if entity.data?.kind}
                <dt>Kind</dt><dd>{entity.data.kind}</dd>
              {/if}
              {#if entity.data?.approval_status}
                <dt>Status</dt>
                <dd><Badge value={entity.data.approval_status} color={specStatusColor(entity.data.approval_status)} /></dd>
              {/if}
              {#if entity.data?.current_sha}
                <dt>SHA</dt><dd class="mono">{entity.data.current_sha.slice(0, 7)}</dd>
              {/if}
              {#if entity.data?.updated_at}
                <dt>Updated</dt><dd>{fmtDate(entity.data.updated_at)}</dd>
              {/if}
            </dl>
            {#if !entity.data?.repo_id}
              <p class="spec-hint">Full content requires repo context.</p>
            {/if}
          {/if}
        </div>

      {:else if activeTab === 'edit'}
        <div class="tab-pane spec-edit-tab">
          {#if specDetailLoading}
            <Skeleton width="100%" height="200px" />
          {:else}
            <textarea
              class="spec-editor-textarea"
              bind:value={editContent}
              placeholder="Spec content…"
              aria-label="Spec editor"
              spellcheck="false"
            ></textarea>

            {#if llmSuggestion}
              <div class="suggestion-block" role="region" aria-label="LLM suggestion">
                <div class="suggestion-hdr">
                  <span class="suggestion-lbl">Suggested Change</span>
                </div>
                {#if llmSuggestion.explanation}
                  <p class="suggestion-expl">{llmSuggestion.explanation}</p>
                {/if}
                {#if llmSuggestion.diff?.length > 0}
                  <div class="suggestion-diff">
                    {#each llmSuggestion.diff as op}
                      <div class="diff-op diff-op-{op.op}">
                        <span class="diff-badge">{op.op}</span>
                        <span class="diff-path">{op.path}</span>
                        {#if op.content}
                          <pre class="diff-content">{op.content}</pre>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {/if}
                <div class="suggestion-btns">
                  <Button variant="primary" onclick={acceptSuggestion}>Accept</Button>
                  <Button variant="secondary" onclick={editSuggestion}>Edit</Button>
                  <Button variant="secondary" onclick={dismissSuggestion}>Dismiss</Button>
                </div>
              </div>
            {/if}

            {#if llmStreaming && llmExplanation}
              <div class="llm-streaming" aria-live="polite">
                <span class="streaming-lbl">Thinking…</span>
                <p class="streaming-txt">{llmExplanation}<span class="blink-cursor" aria-hidden="true"></span></p>
              </div>
            {/if}

            <div class="llm-input-area">
              <div class="recipient-line">Edit spec: "{entity.data?.title || entity.id}" ▸</div>
              <div class="llm-row">
                <textarea
                  class="llm-textarea"
                  bind:value={llmInstruction}
                  placeholder="Describe a change… e.g. 'Add error handling section'"
                  rows="2"
                  disabled={llmStreaming}
                  onkeydown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); sendLlmInstruction(); } }}
                  aria-label="LLM instruction"
                ></textarea>
                <button
                  class="llm-send"
                  onclick={sendLlmInstruction}
                  disabled={!llmInstruction.trim() || llmStreaming || !entity.data?.repo_id}
                  aria-label="Send to LLM"
                >
                  {#if llmStreaming}
                    <svg class="spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
                      <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/>
                    </svg>
                  {:else}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
                      <line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/>
                    </svg>
                  {/if}
                  <span class="sr-only">Send</span>
                </button>
              </div>
              {#if !entity.data?.repo_id}
                <p class="llm-hint warn">LLM editing requires repo context.</p>
              {:else}
                <p class="llm-hint">Ctrl+Enter · Produces draft suggestions — accept to apply</p>
              {/if}
            </div>

            {#if entity.data?.repo_id}
              <div class="save-bar">
                <Button variant="primary" onclick={saveSpec} disabled={saving || !editContent.trim()}>
                  {saving ? 'Saving…' : 'Save & Create MR'}
                </Button>
              </div>
            {/if}
          {/if}
        </div>

      {:else if activeTab === 'progress'}
        <div class="tab-pane">
          {#if specProgressLoading}
            <div class="spec-skeleton">
              {#each Array(4) as _}<Skeleton width="100%" height="1.5rem" />{/each}
            </div>
          {:else if specProgress}
            {@const total = specProgress.total_tasks ?? 0}
            {@const done = specProgress.completed_tasks ?? 0}
            {@const pct = total > 0 ? Math.round((done / total) * 100) : 0}
            <div class="progress-summary">
              <span class="progress-big">{done}/{total}</span>
              <span class="progress-lbl">tasks complete</span>
            </div>
            <div
              class="progress-bar-track"
              role="progressbar"
              aria-valuenow={pct}
              aria-valuemin="0"
              aria-valuemax="100"
            >
              <div class="progress-bar-fill" style="width: {pct}%"></div>
            </div>
            {#if specProgress.tasks?.length > 0}
              <ul class="task-list">
                {#each specProgress.tasks as task}
                  <li class="task-item">
                    <Badge value={task.status} color={taskStatusColor(task.status)} />
                    <span class="task-title">{task.title}</span>
                    {#if task.agent_id}
                      <span class="task-agent mono">{task.agent_id.slice(0, 8)}</span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="no-data">No tasks linked to this spec.</p>
            {/if}
          {:else}
            <p class="no-data">Progress data requires repo context.</p>
          {/if}
        </div>

      {:else if activeTab === 'links'}
        <div class="tab-pane">
          {#if specLinksLoading}
            <div class="spec-skeleton">
              {#each Array(4) as _}<Skeleton width="100%" height="1.5rem" />{/each}
            </div>
          {:else if Array.isArray(specLinks) && specLinks.length > 0}
            <ul class="links-list">
              {#each specLinks as link}
                {@const target = typeof link === 'string' ? link : (link.target_path ?? link.target ?? JSON.stringify(link))}
                {@const kind = typeof link === 'object' ? link.kind : null}
                <li class="link-item">
                  {#if kind}<span class="link-kind">{kind}</span>{/if}
                  <span class="link-target mono">{target}</span>
                </li>
              {/each}
            </ul>
          {:else if specLinks?.links?.length > 0}
            <ul class="links-list">
              {#each specLinks.links as link}
                <li class="link-item">
                  {#if link.kind}<span class="link-kind">{link.kind}</span>{/if}
                  <span class="link-target mono">{link.target_path ?? link.target}</span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="no-data">No spec links found.</p>
          {/if}
        </div>

      {:else if activeTab === 'spec'}
        <div class="tab-pane">
          <EmptyState title="Spec not loaded" description="Spec content for {entity.data?.spec_path ?? 'this entity'} is not available in this context." />
        </div>

      {:else if activeTab === 'chat'}
        <div class="tab-pane">
          <EmptyState title="No conversation yet" description="Start a conversation by typing below." />
        </div>

      {:else if activeTab === 'history'}
        <div class="tab-pane">
          {#if entity.type === 'spec'}
            {#if specHistoryLoading}
              <div class="spec-skeleton">
                {#each Array(4) as _}<Skeleton width="100%" height="2rem" />{/each}
              </div>
            {:else if specHistory?.length > 0}
              <div class="history-list">
                {#each specHistory as ev}
                  <div class="history-item">
                    <div class="history-row">
                      <Badge
                        value={ev.event}
                        color={ev.event === 'approved' ? 'success' : ev.event === 'invalidated' ? 'danger' : 'neutral'}
                      />
                      <span class="history-user mono">{ev.user_id || ev.approver_id || '—'}</span>
                      <span class="history-time">{fmtDate(ev.timestamp || ev.approved_at)}</span>
                    </div>
                    {#if ev.sha || ev.spec_sha}
                      <span class="history-sha mono">{(ev.sha || ev.spec_sha).slice(0, 7)}</span>
                    {/if}
                  </div>
                {/each}
              </div>
            {:else}
              <p class="no-data">No approval events recorded.</p>
            {/if}
          {:else}
            <EmptyState title="No history available" description="Modification history will appear when changes are recorded." />
          {/if}
        </div>

      {:else if activeTab === 'diff'}
        <div class="tab-pane">
          <EmptyState title="Code diff not available" description="Diff view requires a merge request context." />
        </div>

      {:else if activeTab === 'gates'}
        <div class="tab-pane">
          <EmptyState title="No gate results" description="Gate checks will appear here when a merge request is active." />
        </div>

      {:else if activeTab === 'attestation'}
        <div class="tab-pane">
          <EmptyState title="No attestation data" description="Merge attestation and conversation provenance will appear after the MR is merged." />
        </div>

      {:else if activeTab === 'trace'}
        <div class="tab-pane">
          <EmptyState title="No trace data" description="System trace timeline will appear when agent activity is recorded." />
        </div>

      {:else if activeTab === 'ask-why'}
        <div class="tab-pane ask-why">
          {#if entity.data?.conversation_sha}
            <button
              class="start-interrogation"
              onclick={startInterrogation}
              disabled={interrogationLoading}
            >
              {interrogationLoading ? 'Starting…' : 'Start interrogation'}
            </button>
            <p class="ask-why-hint">Spawns an interrogation agent to answer questions about this entity's decision history.</p>
          {:else}
            <p class="ask-why-unavailable">Conversation unavailable — no conversation SHA recorded for this entity.</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</aside>

<style>
  .detail-panel {
    width: 0;
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
    transition: width 200ms ease-out, min-width 200ms ease-out;
    flex-shrink: 0;
  }

  .detail-panel.open {
    width: 40%;
    min-width: 320px;
    max-width: 480px;
  }

  .detail-panel.expanded {
    width: 100%;
    min-width: 0;
    border-left: none;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-2);
    min-height: 48px;
  }

  .panel-entity {
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow: hidden;
    min-width: 0;
  }

  .entity-type {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 600;
  }

  .entity-id {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .panel-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .panel-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    transition: color var(--transition-fast), background var(--transition-fast);
    padding: 0;
  }

  .panel-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .panel-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .panel-close:hover {
    color: var(--color-danger);
  }

  .panel-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-4);
  }

  .tab-pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  /* Entity metadata list */
  .entity-meta {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-2) var(--space-4);
    margin: 0;
    font-size: var(--text-sm);
  }

  .entity-meta dt {
    color: var(--color-text-muted);
    font-weight: 500;
    white-space: nowrap;
    padding: var(--space-1) 0;
  }

  .entity-meta dd {
    color: var(--color-text);
    margin: 0;
    padding: var(--space-1) 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .entity-meta dd.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  /* Placeholder text for tabs implemented by other slices */
  .placeholder-text {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
    padding: var(--space-4) 0;
    text-align: center;
  }

  /* Ask Why tab */
  .ask-why {
    align-items: center;
    padding: var(--space-6) var(--space-4);
    text-align: center;
  }

  .start-interrogation {
    padding: var(--space-3) var(--space-6);
    background: var(--color-primary);
    color: var(--color-surface, #fff);
    border: none;
    border-radius: var(--radius);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .start-interrogation:hover {
    background: var(--color-primary-hover);
  }

  .start-interrogation:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .ask-why-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: var(--space-3) 0 0;
  }

  .ask-why-unavailable {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  .mono {
    font-family: var(--font-mono);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border-width: 0;
  }

  /* ── Spec entity tab styles (S4.5) ────────────────────────────────────────── */
  .spec-skeleton {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  /* Content tab */
  .spec-content-tab {
    gap: var(--space-4);
  }

  .spec-meta-list {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-1) var(--space-3);
    margin: 0;
    font-size: var(--text-sm);
  }

  .spec-meta-list dt {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    padding-top: 2px;
  }

  .spec-meta-list dd {
    margin: 0;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .spec-content-box {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface-elevated);
    overflow: auto;
    max-height: 380px;
  }

  .spec-content-pre {
    margin: 0;
    padding: var(--space-3);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--color-text);
  }

  .spec-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
  }

  /* Edit tab */
  .spec-edit-tab {
    padding: 0;
    gap: 0;
  }

  .spec-editor-textarea {
    width: 100%;
    min-height: 180px;
    max-height: 300px;
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: none;
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.6;
    resize: vertical;
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .spec-editor-textarea:focus:not(:focus-visible) {
    outline: none;
  }

  .spec-editor-textarea:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: -2px;
  }

  .spec-editor-textarea:focus {
    border-color: var(--color-primary);
  }

  /* LLM suggestion block */
  .suggestion-block {
    margin: var(--space-3) var(--space-4);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--color-primary) 5%, transparent);
  }

  .suggestion-hdr {
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--color-primary) 20%, transparent);
  }

  .suggestion-lbl {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-primary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .suggestion-expl {
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.5;
  }

  .suggestion-diff {
    padding: 0 var(--space-3) var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .diff-op {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .diff-badge {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    padding: 1px 4px;
    border-radius: 2px;
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
    display: inline-block;
    width: fit-content;
  }

  .diff-op-add .diff-badge    { color: var(--color-success); }
  .diff-op-remove .diff-badge { color: var(--color-danger); }
  .diff-op-replace .diff-badge { color: var(--color-warning); }

  .diff-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .diff-content {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.5;
    color: var(--color-text);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2);
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 100px;
    overflow-y: auto;
  }

  .suggestion-btns {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid color-mix(in srgb, var(--color-primary) 15%, transparent);
  }

  /* LLM streaming */
  .llm-streaming {
    margin: var(--space-2) var(--space-4);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .streaming-lbl {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
    display: block;
    margin-bottom: 2px;
  }

  .streaming-txt {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .blink-cursor {
    display: inline-block;
    width: 2px;
    height: 1em;
    background: var(--color-primary);
    margin-left: 2px;
    vertical-align: text-bottom;
    animation: blink 1s step-end infinite;
  }

  @keyframes blink {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0; }
  }

  /* LLM input area */
  .llm-input-area {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
  }

  .recipient-line {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
  }

  .llm-row {
    display: flex;
    gap: var(--space-2);
    align-items: flex-end;
  }

  .llm-textarea {
    flex: 1;
    min-height: 44px;
    max-height: 90px;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    resize: vertical;
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .llm-textarea:focus:not(:focus-visible) {
    outline: none;
  }

  .llm-textarea:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: -2px;
  }

  .llm-textarea:focus {
    border-color: var(--color-primary);
  }

  .llm-textarea:disabled { opacity: 0.6; cursor: not-allowed; }

  .llm-send {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    padding: 0;
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-surface, #fff);
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .llm-send:hover:not(:disabled) { background: var(--color-primary-hover); }
  .llm-send:disabled { opacity: 0.4; cursor: not-allowed; }

  .llm-send:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .llm-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .llm-hint.warn { color: var(--color-warning); }

  .spin { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  @media (prefers-reduced-motion: reduce) {
    .spin { animation: none; }
    .blink-cursor { animation: none; }
  }

  /* Save bar */
  .save-bar {
    display: flex;
    justify-content: flex-end;
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
  }

  /* Progress tab */
  .progress-summary {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }

  .progress-big {
    font-family: var(--font-display);
    font-size: 1.5rem;
    font-weight: 700;
    color: var(--color-text);
  }

  .progress-lbl {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
  }

  .progress-bar-track {
    height: 8px;
    background: var(--color-border);
    border-radius: 4px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--color-success);
    border-radius: 4px;
    transition: width var(--transition-slow, 0.3s);
  }

  .task-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .task-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }

  .task-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-text);
  }

  .task-agent {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  /* Links tab */
  .links-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .link-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
  }

  .link-kind {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .link-target {
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* History list (spec type) */
  .history-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .history-item {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .history-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .history-user {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .history-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .history-sha {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* Shared */
  .no-data {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
    text-align: center;
    padding: var(--space-4) 0;
  }
</style>
