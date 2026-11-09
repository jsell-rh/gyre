<script>
  /**
   * System Trace View (HSI §3 Default Views) — "what actually happened?"
   *
   * SDLC timeline for an MR, assembled from existing data via
   *   GET /api/v1/merge-requests/:id/timeline
   * Event types: SpecLifecycleTrigger, AgentSpawned, AgentCompleted,
   * ConversationTurn, GitPush, GateResult, GraphExtraction,
   * MergeQueueEnqueued, Merged.
   *
   * Gate results are expandable to show the individual gate pass/fail.
   * Links: [View Conversation] [View Diff] [View Attestation] — each opens
   * an inline section backed by the corresponding API.
   */
  import { api } from '../../lib/api.js';
  import Skeleton from '../../lib/Skeleton.svelte';
  import EmptyState from '../../lib/EmptyState.svelte';

  let { repoId = '' } = $props();

  let mrs = $state([]);
  let mrsLoading = $state(false);
  let mrsError = $state(null);
  let selectedMrId = $state('');

  let timeline = $state([]);
  let timelineLoading = $state(false);
  let timelineError = $state(null);

  let expandedGate = $state(null);      // GateResult event index expanded
  let openSection = $state(null);       // 'conversation' | 'diff' | 'attestation'
  let sectionLoading = $state(false);
  let sectionError = $state(null);
  let conversationText = $state(null);
  let diffText = $state(null);
  let attestation = $state(null);

  const EVENT_META = {
    SpecLifecycleTrigger: { label: 'Spec lifecycle', cls: 'ev-spec' },
    AgentSpawned: { label: 'Agent spawned', cls: 'ev-agent' },
    AgentCompleted: { label: 'Agent completed', cls: 'ev-agent' },
    ConversationTurn: { label: 'Conversation turn', cls: 'ev-turn' },
    GitPush: { label: 'Git push', cls: 'ev-push' },
    GateResult: { label: 'Gate result', cls: 'ev-gate' },
    GraphExtraction: { label: 'Graph extraction', cls: 'ev-graph' },
    MergeQueueEnqueued: { label: 'Merge queue', cls: 'ev-queue' },
    Merged: { label: 'Merged', cls: 'ev-merged' },
  };

  function eventMeta(type) {
    return EVENT_META[type] ?? { label: type ?? 'Event', cls: 'ev-other' };
  }

  function formatTime(ts) {
    if (!ts) return '';
    return new Date(ts * 1000).toLocaleTimeString(undefined, {
      hour: '2-digit', minute: '2-digit', second: '2-digit',
    });
  }
  function formatDate(ts) {
    if (!ts) return '';
    return new Date(ts * 1000).toLocaleString(undefined, {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
    });
  }

  /** Human-readable one-liner for an event's detail payload. */
  function eventDetailText(evt) {
    const d = evt.detail ?? {};
    switch (evt.type) {
      case 'SpecLifecycleTrigger':
        return [d.spec_path ? `spec ${d.spec_path}` : null, d.task_id ? `task ${d.task_id}` : null]
          .filter(Boolean).join(' · ');
      case 'AgentSpawned':
        return [d.agent_id ? `agent ${d.agent_id}` : null, d.persona ? `persona: ${d.persona}` : null]
          .filter(Boolean).join(' · ');
      case 'AgentCompleted':
        return [d.agent_id ? `agent ${d.agent_id}` : null, d.status ? `status: ${d.status}` : null]
          .filter(Boolean).join(' · ');
      case 'ConversationTurn':
        return [d.turn != null ? `turn ${d.turn}` : null, d.summary].filter(Boolean).join(' — ');
      case 'GitPush':
        return [d.commit_sha ? `${String(d.commit_sha).slice(0, 7)}` : null,
          d.files_changed != null ? `${d.files_changed} file${d.files_changed !== 1 ? 's' : ''} changed` : null]
          .filter(Boolean).join(' · ');
      case 'GateResult':
        return `${d.gate ?? 'gate'}: ${d.status ?? '?'}`;
      case 'GraphExtraction':
        return [
          d.nodes_added ? `+${d.nodes_added} node${d.nodes_added !== 1 ? 's' : ''}` : null,
          d.nodes_modified ? `~${d.nodes_modified} modified` : null,
          d.commit_sha ? `${String(d.commit_sha).slice(0, 7)}` : null,
        ].filter(Boolean).join(' · ');
      case 'MergeQueueEnqueued':
        return d.position != null ? `queue position #${d.position}` : '';
      case 'Merged':
        return 'merged to target branch';
      default:
        return '';
    }
  }

  function gateStatusClass(evt) {
    const s = evt?.detail?.status;
    if (s === 'pass' || s === 'passed') return 'gate-pass';
    if (s === 'fail' || s === 'failed') return 'gate-fail';
    return 'gate-other';
  }

  // ── MR list ─────────────────────────────────────────────────────────────
  $effect(() => {
    const id = repoId;
    if (!id) {
      mrs = [];
      return;
    }
    mrsLoading = true;
    mrsError = null;
    api
      .mergeRequests({ repository_id: id })
      .then((d) => {
        mrs = Array.isArray(d) ? d : (d?.merge_requests ?? []);
      })
      .catch((e) => {
        mrsError = e?.message ?? String(e);
        mrs = [];
      })
      .finally(() => { mrsLoading = false; });
  });

  // Auto-select the most recent MR.
  $effect(() => {
    if (selectedMrId || mrsLoading || mrs.length === 0) return;
    selectedMrId = mrs[0].id;
  });

  let selectedMr = $derived(mrs.find((m) => m.id === selectedMrId) ?? null);

  // ── Timeline ────────────────────────────────────────────────────────────
  $effect(() => {
    const mrId = selectedMrId;
    expandedGate = null;
    openSection = null;
    conversationText = null;
    diffText = null;
    attestation = null;
    if (!mrId) {
      timeline = [];
      return;
    }
    timelineLoading = true;
    timelineError = null;
    api
      .mrTimeline(mrId)
      .then((d) => {
        timeline = Array.isArray(d?.events) ? d.events : [];
      })
      .catch((e) => {
        timelineError = e?.message ?? String(e);
        timeline = [];
      })
      .finally(() => { timelineLoading = false; });
  });

  // ── Inline sections: conversation / diff / attestation ─────────────────
  function openSectionToggle(name) {
    if (openSection === name) {
      openSection = null;
      return;
    }
    openSection = name;
    sectionError = null;
    conversationText = null;
    diffText = null;
    attestation = null;
    if (!selectedMrId) return;
    sectionLoading = true;
    let promise;
    if (name === 'conversation') {
      // Conversation SHA lives on the MR attestation bundle.
      promise = api
        .mrAttestation(selectedMrId)
        .then((att) => {
          const sha = att?.attestation?.conversation_sha ?? att?.conversation_sha;
          if (!sha) throw new Error('no conversation recorded for this MR');
          return api.conversationProvenance(sha);
        })
        .then((conv) => {
          conversationText = summarizeConversation(conv);
        });
    } else if (name === 'diff') {
      promise = api.mrDiff(selectedMrId).then((d) => {
        diffText = formatDiff(d);
      });
    } else {
      promise = api.mrAttestation(selectedMrId).then((d) => {
        attestation = d && !d.error ? (d.attestation ?? d) : null;
        if (!attestation) throw new Error('no attestation recorded for this MR');
      });
    }
    promise
      .catch((e) => { sectionError = e?.message ?? String(e); })
      .finally(() => { sectionLoading = false; });
  }

  function summarizeConversation(conv) {
    if (conv == null) return '(empty conversation)';
    if (typeof conv === 'string') {
      try {
        return summarizeConversation(JSON.parse(conv));
      } catch {
        return conv.slice(0, 4000);
      }
    }
    if (Array.isArray(conv?.turns)) {
      return conv.turns
        .map((t, i) => {
          const role = t.role ?? t.sender ?? 'turn';
          const text = t.content ?? t.text ?? t.message ?? JSON.stringify(t);
          return `${i + 1}. [${role}] ${String(text).slice(0, 500)}`;
        })
        .join('\n');
    }
    if (Array.isArray(conv?.messages)) return summarizeConversation({ turns: conv.messages });
    try {
      return JSON.stringify(conv, null, 2).slice(0, 4000);
    } catch {
      return String(conv).slice(0, 4000);
    }
  }

  function formatDiff(d) {
    if (d == null) return '(no diff)';
    if (typeof d === 'string') return d.slice(0, 8000);
    if (typeof d.diff === 'string') return d.diff.slice(0, 8000);
    if (Array.isArray(d.files)) {
      return d.files
        .map((f) => {
          const path = f.path ?? f.filename ?? f.new_path ?? '';
          const patch = f.patch ?? f.diff ?? '';
          return `--- ${path}\n${String(patch).slice(0, 2000)}`;
        })
        .join('\n\n');
    }
    try {
      return JSON.stringify(d, null, 2).slice(0, 8000);
    } catch {
      return String(d).slice(0, 8000);
    }
  }
</script>

<div class="system-trace" data-testid="system-trace-view">
  {#if !repoId}
    <EmptyState title="No repository selected" description="Select a repository to see its SDLC traces." />
  {:else}
    <div class="st-header">
      <h2 class="st-title">System Trace</h2>
      <div class="st-controls">
        {#if mrsLoading}
          <Skeleton width="220px" height="2rem" />
        {:else if mrsError}
          <span class="st-error" role="alert">Failed to load MRs: {mrsError}</span>
        {:else if mrs.length === 0}
          <span class="st-no-mrs">No merge requests for this repository</span>
        {:else}
          <label class="st-mr-label" for="st-mr-select">Merge request</label>
          <select
            id="st-mr-select"
            class="st-mr-select"
            bind:value={selectedMrId}
            aria-label="Select merge request"
          >
            {#each mrs as mr (mr.id)}
              <option value={mr.id}>{mr.title ?? mr.id} ({String(mr.status ?? '')})</option>
            {/each}
          </select>
        {/if}
      </div>
    </div>

    {#if selectedMr}
      <div class="st-mr-header">
        <span class="st-mr-title">{selectedMr.title ?? selectedMr.id}</span>
        <span class="st-mr-meta">
          {#if selectedMr.author_agent_id}<span>Author: <span class="mono">{selectedMr.author_agent_id}</span></span>{/if}
          {#if selectedMr.spec_ref}<span>Spec: <span class="mono">{selectedMr.spec_ref}</span></span>{/if}
          <span>Branch: <span class="mono">{selectedMr.source_branch ?? ''} → {selectedMr.target_branch ?? ''}</span></span>
        </span>
      </div>
    {/if}

    {#if timelineLoading}
      <div class="st-loading" aria-live="polite">
        {#each Array(6) as _}<Skeleton width="100%" height="2.5rem" />{/each}
      </div>
    {:else if timelineError}
      <div class="st-error-banner" role="alert">
        <p>Failed to load timeline: {timelineError}</p>
      </div>
    {:else if timeline.length === 0}
      <EmptyState title="No timeline events" description="No SDLC activity recorded for this merge request yet." />
    {:else}
      {@const first = timeline[0]?.timestamp}
      {@const last = timeline[timeline.length - 1]?.timestamp}
      <div class="st-summary">
        <span>{timeline.length} events</span>
        {#if first && last && last > first}
          <span>duration {Math.max(1, Math.round((last - first) / 60))}m</span>
          <span>{formatDate(first)} — {formatDate(last)}</span>
          <span class="st-gate-summary">
            {timeline.filter((e) => e.type === 'GateResult' && (e.detail?.status === 'pass' || e.detail?.status === 'passed')).length} gates passed,
            {timeline.filter((e) => e.type === 'GateResult' && (e.detail?.status === 'fail' || e.detail?.status === 'failed')).length} failed
          </span>
        {/if}
      </div>
      <ol class="st-timeline" data-count={timeline.length}>
        {#each timeline as evt, i (i)}
          {@const meta = eventMeta(evt.type)}
          <li class="st-event {meta.cls}" data-event-type={evt.type}>
            <div class="st-rail">
              <span class="st-dot" aria-hidden="true"></span>
              {#if i < timeline.length - 1}<span class="st-line" aria-hidden="true"></span>{/if}
            </div>
            <div class="st-body">
              {#if evt.type === 'GateResult'}
                <button
                  type="button"
                  class="st-event-row st-gate-row {gateStatusClass(evt)}"
                  onclick={() => { expandedGate = expandedGate === i ? null : i; }}
                  aria-expanded={expandedGate === i}
                >
                  <span class="st-time mono">{formatTime(evt.timestamp)}</span>
                  <span class="st-type">{meta.label}</span>
                  <span class="st-detail">{eventDetailText(evt)}</span>
                  <span class="st-expand" aria-hidden="true">{expandedGate === i ? '▾' : '▸'}</span>
                </button>
                {#if expandedGate === i}
                  <div class="st-gate-detail" data-testid="gate-detail">
                    <dl>
                      <dt>Gate</dt><dd class="mono">{evt.detail?.gate ?? '—'}</dd>
                      <dt>Status</dt><dd class="{gateStatusClass(evt)}">{evt.detail?.status ?? '—'}</dd>
                      {#if evt.detail?.duration_ms != null}<dt>Duration</dt><dd>{evt.detail.duration_ms} ms</dd>{/if}
                      {#if evt.detail?.gate_type}<dt>Type</dt><dd>{evt.detail.gate_type}</dd>{/if}
                    </dl>
                  </div>
                {/if}
              {:else}
                <div class="st-event-row">
                  <span class="st-time mono">{formatTime(evt.timestamp)}</span>
                  <span class="st-type">{meta.label}</span>
                  <span class="st-detail">{eventDetailText(evt)}</span>
                </div>
              {/if}
            </div>
          </li>
        {/each}
      </ol>

      {#if selectedMrId}
        <div class="st-links" role="group" aria-label="Trace links">
          <button type="button" class="st-link-btn" class:active={openSection === 'conversation'} onclick={() => openSectionToggle('conversation')}>View Conversation</button>
          <button type="button" class="st-link-btn" class:active={openSection === 'diff'} onclick={() => openSectionToggle('diff')}>View Diff</button>
          <button type="button" class="st-link-btn" class:active={openSection === 'attestation'} onclick={() => openSectionToggle('attestation')}>View Attestation</button>
        </div>
        {#if openSection}
          <div class="st-section" data-testid={`st-section-${openSection}`}>
            {#if sectionLoading}
              <Skeleton width="100%" height="6rem" />
            {:else if sectionError}
              <p class="st-error" role="alert">{sectionError}</p>
            {:else if openSection === 'conversation'}
              <pre class="st-pre">{conversationText ?? '(empty)'}</pre>
            {:else if openSection === 'diff'}
              <pre class="st-pre">{diffText ?? '(empty)'}</pre>
            {:else if attestation}
              <dl class="st-attestation">
                {#if attestation.merge_commit_sha}<dt>Merge commit</dt><dd class="mono">{attestation.merge_commit_sha}</dd>{/if}
                {#if attestation.merged_at}<dt>Merged at</dt><dd>{formatDate(attestation.merged_at)}</dd>{/if}
                {#if attestation.conversation_sha}<dt>Conversation</dt><dd class="mono">{attestation.conversation_sha}</dd>{/if}
                {#if Array.isArray(attestation.gate_results)}
                  <dt>Gate results</dt>
                  <dd>
                    <ul class="st-att-gates">
                      {#each attestation.gate_results as g (g.gate_id ?? g.gate)}
                        <li class={g.status === 'pass' || g.status === 'passed' ? 'gate-pass' : (g.status === 'fail' || g.status === 'failed' ? 'gate-fail' : 'gate-other')}>
                          {g.gate_name ?? g.gate ?? g.gate_id}: {g.status}
                        </li>
                      {/each}
                    </ul>
                  </dd>
                {/if}
                {#if attestation.completion_summary}
                  <dt>Completion summary</dt><dd>{typeof attestation.completion_summary === 'string' ? attestation.completion_summary : JSON.stringify(attestation.completion_summary)}</dd>
                {/if}
              </dl>
            {/if}
          </div>
        {/if}
      {/if}
    {/if}
  {/if}
</div>

<style>
  .system-trace {
    display: flex; flex-direction: column; gap: 0.75rem; height: 100%;
    overflow-y: auto; padding: 1rem 1.25rem; box-sizing: border-box;
  }
  .st-header { display: flex; align-items: center; justify-content: space-between; gap: 1rem; flex-wrap: wrap; }
  .st-title { margin: 0; font-size: 1.1rem; font-weight: 600; }
  .st-controls { display: flex; align-items: center; gap: 0.5rem; }
  .st-mr-label { font-size: 0.8rem; color: var(--text-secondary, #8b949e); }
  .st-mr-select {
    background: var(--bg, #0d1117); border: 1px solid var(--border, #30363d);
    border-radius: 6px; color: inherit; padding: 0.35rem 0.6rem; font-size: 0.85rem; max-width: 320px;
  }
  .st-mr-header { display: flex; flex-direction: column; gap: 0.2rem; }
  .st-mr-title { font-weight: 600; font-size: 0.95rem; }
  .st-mr-meta { display: flex; gap: 1rem; flex-wrap: wrap; font-size: 0.75rem; color: var(--text-secondary, #8b949e); }
  .st-loading { display: flex; flex-direction: column; gap: 0.5rem; }
  .st-error { color: #f85149; font-size: 0.8rem; }
  .st-error-banner { background: rgba(248, 81, 73, 0.08); border: 1px solid rgba(248, 81, 73, 0.4); border-radius: 8px; padding: 0.75rem 1rem; }
  .st-error-banner p { margin: 0; font-size: 0.85rem; color: #f85149; }
  .st-no-mrs { font-size: 0.85rem; color: var(--text-secondary, #8b949e); }
  .st-summary { display: flex; gap: 1rem; flex-wrap: wrap; font-size: 0.75rem; color: var(--text-secondary, #8b949e); }
  .st-gate-summary .gate-pass, .gate-pass { color: #3fb950; }
  .gate-fail { color: #f85149; }
  .gate-other { color: #d29922; }
  .st-timeline { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
  .st-event { display: flex; gap: 0.75rem; }
  .st-rail { display: flex; flex-direction: column; align-items: center; width: 14px; flex-shrink: 0; }
  .st-dot { width: 10px; height: 10px; border-radius: 50%; background: var(--border, #30363d); border: 2px solid var(--text-secondary, #8b949e); margin-top: 6px; box-sizing: border-box; }
  .ev-merged .st-dot { background: #3fb950; border-color: #3fb950; }
  .ev-gate .st-dot { border-color: #58a6ff; }
  .ev-push .st-dot { border-color: #d29922; }
  .st-line { flex: 1; width: 2px; background: var(--border, #30363d); min-height: 12px; }
  .st-body { flex: 1; min-width: 0; padding-bottom: 0.6rem; }
  .st-event-row {
    display: flex; align-items: baseline; gap: 0.75rem; width: 100%;
    padding: 0.3rem 0.4rem; border-radius: 6px; font-size: 0.82rem;
    background: none; border: none; color: inherit; text-align: left; font: inherit;
  }
  button.st-event-row { cursor: pointer; }
  button.st-event-row:hover { background: rgba(88, 166, 255, 0.06); }
  .st-time { font-size: 0.72rem; color: var(--text-secondary, #8b949e); flex-shrink: 0; font-family: ui-monospace, monospace; }
  .st-type { font-weight: 600; flex-shrink: 0; }
  .ev-spec .st-type { color: #d2a8ff; }
  .ev-agent .st-type { color: #79c0ff; }
  .ev-turn .st-type { color: #8b949e; }
  .ev-push .st-type { color: #d29922; }
  .ev-gate .st-type { color: #58a6ff; }
  .ev-graph .st-type { color: #7ee787; }
  .ev-queue .st-type { color: #ffa657; }
  .ev-merged .st-type { color: #3fb950; }
  .st-detail { color: var(--text-secondary, #8b949e); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st-gate-detail { margin: 0.25rem 0 0.5rem 3.4rem; padding: 0.5rem 0.75rem; border: 1px solid var(--border, #30363d); border-radius: 8px; background: var(--bg-elevated, #161b22); }
  .st-gate-detail dl { display: grid; grid-template-columns: auto 1fr; gap: 0.25rem 1rem; margin: 0; font-size: 0.78rem; }
  .st-gate-detail dt { color: var(--text-secondary, #8b949e); }
  .st-gate-detail dd { margin: 0; }
  .st-expand { margin-left: auto; color: var(--text-secondary, #8b949e); }
  .st-links { display: flex; gap: 0.5rem; padding-top: 0.5rem; border-top: 1px solid var(--border, #30363d); }
  .st-link-btn {
    background: rgba(88, 166, 255, 0.1); border: 1px solid rgba(88, 166, 255, 0.4);
    color: #58a6ff; border-radius: 6px; padding: 0.3rem 0.8rem; font-size: 0.78rem; cursor: pointer;
  }
  .st-link-btn:hover, .st-link-btn.active { background: rgba(88, 166, 255, 0.2); }
  .st-section { border: 1px solid var(--border, #30363d); border-radius: 8px; padding: 0.75rem; background: var(--bg-elevated, #161b22); }
  .st-pre {
    margin: 0; max-height: 320px; overflow: auto; font-size: 0.72rem;
    font-family: ui-monospace, monospace; white-space: pre-wrap; word-break: break-word;
  }
  .st-attestation { display: grid; grid-template-columns: auto 1fr; gap: 0.3rem 1rem; margin: 0; font-size: 0.8rem; }
  .st-attestation dt { color: var(--text-secondary, #8b949e); }
  .st-attestation dd { margin: 0; }
  .st-att-gates { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 0.15rem; }
  .mono { font-family: ui-monospace, monospace; }
</style>
