<script>
  import { getContext } from 'svelte';
  import { t } from 'svelte-i18n';
  import Tabs from './Tabs.svelte';
  import Button from './Button.svelte';
  import Badge from './Badge.svelte';
  import Skeleton from './Skeleton.svelte';
  import EmptyState from './EmptyState.svelte';
  import EditorSplit from './EditorSplit.svelte';
  import ArchPreviewCanvas from './ArchPreviewCanvas.svelte';
  import { api } from './api.js';
  import { toastSuccess, toastError } from './toast.svelte.js';

  const goToRepoTab = getContext('goToRepoTab') ?? null;
  const openDetailPanel = getContext('openDetailPanel') ?? null;

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
    onback = undefined,
  } = $props();

  let activeTab = $state('info');
  let panelEl = $state(null);
  let previousFocus = null;
  let interrogationLoading = $state(false);
  let interrogationAgentId = $state(null);
  let enqueueing = $state(false);

  // Editor Split pop-out (spec entities only)
  let showEditorSplit = $state(false);

  function openEditorSplit() {
    expanded = true;
    showEditorSplit = true;
    onpopout?.();
  }

  function closeEditorSplit() {
    showEditorSplit = false;
    expanded = false;
    const url = new URL(window.location.href);
    if (url.searchParams.has('detail') || url.searchParams.has('expanded')) {
      url.searchParams.delete('detail');
      url.searchParams.delete('expanded');
      window.history.replaceState({}, '', url.toString());
    }
  }

  // Reset editor split when entity changes
  $effect(() => {
    if (entity) showEditorSplit = false;
  });

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
        { id: 'content',      label: $t('detail_panel.tabs.content') },
        { id: 'edit',         label: $t('detail_panel.tabs.edit') },
        { id: 'progress',     label: $t('detail_panel.tabs.progress') },
        { id: 'links',        label: $t('detail_panel.tabs.links') },
        { id: 'history',      label: $t('detail_panel.tabs.history') },
        { id: 'architecture', label: $t('detail_panel.tabs.architecture'), disabled: !data?.repo_id },
      ];
    }

    const result = [{ id: 'info', label: $t('detail_panel.tabs.info') }];

    if (type === 'mr') {
      result.push(
        { id: 'diff',        label: $t('detail_panel.tabs.diff') },
        { id: 'timeline',    label: 'Timeline' },
        { id: 'gates',       label: $t('detail_panel.tabs.gates') },
        { id: 'reviews',     label: 'Reviews' },
      );
      if (data.status === 'merged') {
        result.push({ id: 'attestation', label: $t('detail_panel.tabs.attestation') });
      }
      result.push({
        id: 'ask-why',
        label: $t('detail_panel.tabs.ask_why'),
        disabled: !data.conversation_sha,
        title: data.conversation_sha ? undefined : $t('detail_panel.conversation_unavailable'),
      });
      return result;
    }

    if (type === 'agent') {
      result.push(
        { id: 'chat',    label: $t('detail_panel.tabs.chat') },
        { id: 'history', label: 'Logs' },
        { id: 'trace',   label: $t('detail_panel.tabs.trace') },
      );
      if (data.conversation_sha !== undefined) {
        result.push({
          id: 'ask-why',
          label: $t('detail_panel.tabs.ask_why'),
          disabled: !data.conversation_sha,
          title: data.conversation_sha ? undefined : $t('detail_panel.conversation_unavailable'),
        });
      }
      return result;
    }

    if (type === 'node') {
      if (data.spec_path) result.push({ id: 'spec', label: $t('detail_panel.tabs.spec') });
      if (data.author_agent_id) result.push({ id: 'chat', label: $t('detail_panel.tabs.chat') });
      result.push({ id: 'history', label: $t('detail_panel.tabs.history') });
      return result;
    }

    if (type === 'task') {
      result.push({ id: 'activity', label: 'Activity' });
      return result;
    }

    // Generic: info + optional extras
    if (data.spec_path) result.push({ id: 'spec', label: $t('detail_panel.tabs.spec') });
    if (data.author_agent_id) result.push({ id: 'chat', label: $t('detail_panel.tabs.chat') });
    if (data.has_history) result.push({ id: 'history', label: $t('detail_panel.tabs.history') });
    return result;
  }

  async function startInterrogation() {
    if (!entity) return;
    const data = entity.data ?? {};
    const repoId = data.repo_id ?? data.repository_id ?? null;
    const taskId = data.task_id ?? data.current_task_id ?? null;
    const conversationSha = data.conversation_sha ?? null;
    if (!repoId || !taskId) {
      toastError($t('detail_panel.interrogation_no_context'));
      return;
    }
    interrogationLoading = true;
    interrogationAgentId = null;
    try {
      const result = await api.spawnAgent({
        name: `interrogation-${entity.type}-${entity.id}`,
        repo_id: repoId,
        task_id: taskId,
        branch: `interrogation/${entity.type}/${entity.id}`,
        agent_type: 'interrogation',
        conversation_sha: conversationSha,
      });
      interrogationAgentId = result?.agent?.id ?? null;
      toastSuccess($t('detail_panel.interrogation_spawned'));
    } catch (e) {
      toastError($t('detail_panel.interrogation_failed', { values: { error: e?.message ?? String(e) } }));
    } finally {
      interrogationLoading = false;
    }
  }

  // Reset active tab when entity changes, defaulting to a sensible tab.
  // MRs default to 'diff' (like GitHub), everything else defaults to first tab.
  $effect(() => {
    if (entity) {
      const freshTabs = computeTabs(entity);
      if (entity.type === 'mr' && freshTabs.some(t => t.id === 'diff')) {
        activeTab = 'diff';
      } else if (entity.type === 'spec' && freshTabs.some(t => t.id === 'content')) {
        activeTab = 'content';
      } else if (freshTabs.length > 0) {
        activeTab = freshTabs[0].id;
      }
    }
  });

  function handleKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      // Escape goes back if there's history, otherwise closes
      if (onback) onback();
      else close();
      return;
    }
    if (e.key === 'Tab' && panelEl) {
      const focusable = panelEl.querySelectorAll(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  function close() {
    expanded = false;
    // Clean up URL params added by popout
    const url = new URL(window.location.href);
    if (url.searchParams.has('detail') || url.searchParams.has('expanded')) {
      url.searchParams.delete('detail');
      url.searchParams.delete('expanded');
      window.history.replaceState({}, '', url.toString());
    }
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
        url.searchParams.delete('detail');
        url.searchParams.delete('expanded');
      }
      window.history.replaceState({}, '', url.toString());
    }
  }

  // Focus management: when panel opens, move focus into it; restore on close.
  $effect(() => {
    if (entity && panelEl) {
      previousFocus = document.activeElement;
      const focusable = panelEl.querySelector(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      focusable?.focus();
    } else if (!entity && previousFocus) {
      previousFocus?.focus();
      previousFocus = null;
    }
  });

  // Guard: if entity is cleared through any code path, ensure expanded resets.
  $effect(() => {
    if (!entity) expanded = false;
  });

  // ── MR entity tab state ─────────────────────────────────────────────────────
  let mrDetail = $state(null);
  let mrDetailLoading = $state(false);
  let mrDiff = $state(null);
  let mrDiffLoading = $state(false);
  let mrGates = $state(null);
  let mrGatesLoading = $state(false);
  let mrAttestation = $state(null);
  let mrAttestationLoading = $state(false);
  let mrTimeline = $state(null);
  let mrTimelineLoading = $state(false);
  let mrReviews = $state(null);
  let mrReviewsLoading = $state(false);
  let mrComments = $state(null);
  let mrCommentsLoading = $state(false);
  let mrDeps = $state(null);
  let mrDepsLoading = $state(false);
  let newCommentText = $state('');
  let submittingComment = $state(false);
  let newReviewDecision = $state('approved');
  let newReviewBody = $state('');
  let submittingReview = $state(false);
  let newMessageText = $state('');
  let sendingMessage = $state(false);

  // Agent/task name cache for cross-references
  let entityNameCache = $state({});

  // ── Task entity tab state ─────────────────────────────────────────────────
  let taskDetail = $state(null);
  let taskDetailLoading = $state(false);
  let taskAgents = $state(null);
  let taskAgentsLoading = $state(false);
  let taskMrs = $state(null);
  let taskMrsLoading = $state(false);

  // ── Agent entity tab state ─────────────────────────────────────────────────
  let agentDetail = $state(null);
  let agentDetailLoading = $state(false);
  let agentLogs = $state(null);
  let agentLogsLoading = $state(false);
  let agentMessages = $state(null);
  let agentMessagesLoading = $state(false);
  let agentWorkload = $state(null);
  let agentWorkloadLoading = $state(false);
  let agentTraceSpans = $state(null);

  // Reset MR/agent/task data when entity changes
  $effect(() => {
    if (entity?.type === 'mr') {
      mrDetail = null;
      mrDiff = null;
      mrGates = null;
      mrAttestation = null;
      mrTimeline = null;
      mrReviews = null;
      mrComments = null;
      mrDeps = null;
    }
    if (entity?.type === 'agent') {
      agentDetail = null;
      agentLogs = null;
      agentMessages = null;
      agentWorkload = null;
    }
    if (entity?.type === 'task') {
      taskDetail = null;
      taskAgents = null;
      taskMrs = null;
    }
  });

  // Load MR data per tab
  $effect(() => {
    if (entity?.type !== 'mr') return;
    const id = entity.id;

    if (activeTab === 'info' && !mrDetail && !mrDetailLoading) {
      mrDetailLoading = true;
      mrDepsLoading = true;
      Promise.all([
        api.mergeRequest(id),
        api.mrDependencies(id).catch(() => null),
        api.mrGates(id).catch(() => []),
        api.mrTimeline(id).catch(() => []),
        api.mrDiff(id).catch(() => null),
      ]).then(async ([d, deps, gates, timeline, diff]) => {
        mrDetail = d;
        mrDeps = deps;
        // Enrich diff_stats from the diff endpoint if MR response lacks them
        if (!d?.diff_stats && diff) {
          mrDetail = { ...mrDetail, diff_stats: { files_changed: diff.files_changed ?? 0, insertions: diff.insertions ?? 0, deletions: diff.deletions ?? 0 } };
        }
        // Pre-cache diff for the diff tab
        if (!mrDiff && diff) mrDiff = diff;
        // Pre-cache timeline for the timeline tab
        const rawTimeline = Array.isArray(timeline) ? timeline : (timeline?.events ?? []);
        if (!mrTimeline) mrTimeline = rawTimeline;
        // Resolve task_id via agent's current_task_id if MR lacks it
        const agentId = d?.author_agent_id ?? d?.agent_id;
        if (!d?.task_id && agentId) {
          try {
            const ag = await api.agent(agentId);
            const taskId = ag?.current_task_id ?? ag?.task_id;
            if (taskId) mrDetail = { ...mrDetail, task_id: taskId };
          } catch { /* best effort */ }
        }
        // Pre-compute gate summary for info tab
        const gateList = Array.isArray(gates) ? gates : (gates?.gates ?? []);
        if (gateList.length > 0) {
          const passed = gateList.filter(g => g.status === 'Passed' || g.status === 'passed').length;
          const failed = gateList.filter(g => g.status === 'Failed' || g.status === 'failed').length;
          const total = gateList.length;
          // Also store gate names for display
          const gateNames = gateList.map(g => ({ name: g.gate_name ?? g.name ?? 'Gate', status: g.status, required: g.required }));
          mrDetail = { ...mrDetail, _gateSummary: { passed, failed, total, gates: gateNames } };
        }
        // Pre-cache gates for the gates tab
        if (!mrGates) {
          mrGates = gateList.map(r => ({
            ...r,
            name: r.gate_name ?? r.name,
          }));
        }
        // Fetch commit signature for merged MRs
        const repoId = d?.repository_id ?? d?.repo_id;
        const mergeSha = d?.merge_commit_sha;
        if (repoId && mergeSha) {
          api.commitSignature(repoId, mergeSha).then(sig => {
            if (sig) mrDetail = { ...mrDetail, _commitSig: sig };
          }).catch(() => {});
        }
        // Build mini status story from timeline
        if (mrTimeline.length > 0) {
          const events = mrTimeline.slice(-4).map(evt => {
            const evtType = evt.event_type ?? evt.type ?? evt.event;
            return { label: timelineEventLabel(evtType), variant: timelineEventVariant(evtType), time: evt.timestamp ?? evt.created_at };
          });
          mrDetail = { ...mrDetail, _statusStory: events };
        }
      }).catch(() => { mrDetail = null; })
        .finally(() => { mrDetailLoading = false; mrDepsLoading = false; });
    }
    if (activeTab === 'diff' && !mrDiff && !mrDiffLoading) {
      mrDiffLoading = true;
      api.mrDiff(id)
        .then((d) => { mrDiff = d; })
        .catch(() => { mrDiff = null; })
        .finally(() => { mrDiffLoading = false; });
    }
    if (activeTab === 'gates' && !mrGates && !mrGatesLoading) {
      mrGatesLoading = true;
      const mr = mrDetail ?? entity.data ?? {};
      const repoId = mr.repository_id ?? mr.repo_id ?? entity.data?.repository_id ?? entity.data?.repo_id;
      Promise.all([
        api.mrGates(id),
        repoId ? api.repoGates(repoId).catch(() => []) : Promise.resolve([]),
      ])
        .then(([results, defs]) => {
          const raw = Array.isArray(results) ? results : (results?.gates ?? []);
          const defMap = Object.fromEntries((Array.isArray(defs) ? defs : []).map(g => [g.id, g]));
          // Server now enriches results with gate_name/gate_type/required/command,
          // but fall back to client-side join for older servers
          mrGates = raw.map(r => {
            const def = defMap[r.gate_id] ?? {};
            return {
              ...r,
              name: r.gate_name ?? def.name ?? r.name,
              gate_type: r.gate_type ?? def.gate_type,
              required: r.required ?? def.required,
              command: r.command ?? def.command,
              _result_id: r.id,
            };
          });
        })
        .catch(() => { mrGates = []; })
        .finally(() => { mrGatesLoading = false; });
    }
    if (activeTab === 'attestation' && !mrAttestation && !mrAttestationLoading) {
      mrAttestationLoading = true;
      api.mrAttestation(id)
        .then((d) => { mrAttestation = d; })
        .catch(() => { mrAttestation = null; })
        .finally(() => { mrAttestationLoading = false; });
    }
    if (activeTab === 'timeline' && !mrTimeline && !mrTimelineLoading) {
      mrTimelineLoading = true;
      api.mrTimeline(id)
        .then((d) => { mrTimeline = Array.isArray(d) ? d : (d?.events ?? []); })
        .catch(() => { mrTimeline = []; })
        .finally(() => { mrTimelineLoading = false; });
    }
    if (activeTab === 'reviews' && !mrReviews && !mrReviewsLoading) {
      mrReviewsLoading = true;
      mrCommentsLoading = true;
      Promise.all([
        api.mrReviews(id).catch(() => []),
        api.mrComments(id).catch(() => []),
      ]).then(([revs, cmts]) => {
        mrReviews = Array.isArray(revs) ? revs : [];
        mrComments = Array.isArray(cmts) ? cmts : [];
      }).finally(() => { mrReviewsLoading = false; mrCommentsLoading = false; });
    }
  });

  /** Normalize server agent fields to UI-expected names */
  function normalizeAgent(d) {
    if (!d) return d;
    return {
      ...d,
      task_id: d.task_id ?? d.current_task_id,
      created_at: d.created_at ?? d.spawned_at,
      completed_at: d.completed_at ?? (d.status === 'idle' || d.status === 'stopped' || d.status === 'completed' ? (d.last_heartbeat ?? d.spawned_at) : undefined),
    };
  }

  // Load agent data per tab
  $effect(() => {
    if (entity?.type !== 'agent') return;
    const id = entity.id;

    if (activeTab === 'info' && !agentDetail && !agentDetailLoading) {
      agentDetailLoading = true;
      Promise.all([
        api.agent(id),
        api.agentLogs(id, 5, 0).catch(() => []),
        api.agentContainer(id).catch(() => null),
        api.agentWorkload(id).catch(() => null),
        api.agentTouchedPaths(id).catch(() => null),
        api.agentCard(id).catch(() => null),
      ]).then(([d, logs, container, workload, touchedPaths, card]) => {
        const norm = normalizeAgent(d);
        agentDetail = norm ? { ...norm, _container: container, _workload: workload, _touchedPaths: touchedPaths, _card: card } : norm;
        // Pre-cache a few recent logs for the info view
        if (!agentLogs) agentLogs = Array.isArray(logs) ? logs : (logs?.logs ?? logs?.entries ?? []);
      }).catch(() => { agentDetail = null; })
        .finally(() => { agentDetailLoading = false; });
    }
    if (activeTab === 'trace' && !agentLogs && !agentLogsLoading) {
      agentLogsLoading = true;
      const ag = agentDetail ?? entity.data ?? {};
      const mrId = ag.mr_id;
      Promise.all([
        api.agentLogs(id),
        mrId ? api.mrTrace(mrId).catch(() => null) : Promise.resolve(null),
      ]).then(([d, trace]) => {
        agentLogs = Array.isArray(d) ? d : (d?.logs ?? d?.entries ?? []);
        if (trace?.spans) agentTraceSpans = trace.spans;
      }).catch(() => { agentLogs = []; })
        .finally(() => { agentLogsLoading = false; });
    }
    if (activeTab === 'chat' && !agentMessages && !agentMessagesLoading) {
      agentMessagesLoading = true;
      api.agentMessages(id)
        .then((d) => { agentMessages = Array.isArray(d) ? d : (d?.messages ?? []); })
        .catch(() => { agentMessages = []; })
        .finally(() => { agentMessagesLoading = false; });
    }
    if (activeTab === 'history' && !agentWorkload && !agentWorkloadLoading) {
      agentWorkloadLoading = true;
      Promise.all([
        api.agent(id).then(normalizeAgent).catch(() => null),
        api.agentLogs(id, 500, 0).catch(() => []),
      ]).then(([ag, logs]) => {
        agentWorkload = { agent: ag };
        if (!agentLogs || agentLogs.length < 5) {
          agentLogs = Array.isArray(logs) ? logs : (logs?.logs ?? logs?.entries ?? []);
        }
      }).catch(() => { agentWorkload = null; })
        .finally(() => { agentWorkloadLoading = false; });
    }
  });

  // Load task data per tab
  $effect(() => {
    if (entity?.type !== 'task') return;
    const id = entity.id;

    if (activeTab === 'info' && !taskDetail && !taskDetailLoading) {
      taskDetailLoading = true;
      api.task(id)
        .then((d) => { taskDetail = d; })
        .catch(() => { taskDetail = null; })
        .finally(() => { taskDetailLoading = false; });
    }
    if (activeTab === 'activity' && !taskAgents && !taskAgentsLoading) {
      taskAgentsLoading = true;
      taskMrsLoading = true;
      const tk = taskDetail ?? entity.data ?? {};
      const wsId = tk.workspace_id;
      const repoId = tk.repo_id ?? tk.repository_id;
      Promise.all([
        api.agents({ workspaceId: wsId, repoId }).then(list => {
          const all = Array.isArray(list) ? list : [];
          return all.filter(a => (a.task_id ?? a.current_task_id) === id);
        }).catch(() => []),
        api.mergeRequests(repoId ? { repository_id: repoId } : {}).then(list => {
          const all = Array.isArray(list) ? list : [];
          return all.filter(m => m.task_id === id);
        }).catch(() => []),
      ]).then(([agents, mrs]) => {
        taskAgents = agents;
        taskMrs = mrs;
      }).finally(() => { taskAgentsLoading = false; taskMrsLoading = false; });
    }
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

  // ── Spec approval actions ──────────────────────────────────────────────
  let approving = $state(false);
  let revoking = $state(false);
  let rejecting = $state(false);

  async function approveCurrentSpec() {
    if (!entity || approving) return;
    const sha = entity.data?.current_sha;
    const path = entity.id;
    if (!sha || !path) { toastError($t('detail_panel.approve_missing_sha')); return; }
    approving = true;
    try {
      await api.approveSpec(path, sha);
      toastSuccess($t('detail_panel.spec_approved'));
      // Update local state to reflect approval
      if (entity.data) entity = { ...entity, data: { ...entity.data, approval_status: 'approved' } };
    } catch (e) {
      toastError($t('detail_panel.approval_failed', { values: { error: e.message } }));
    } finally {
      approving = false;
    }
  }

  async function revokeCurrentSpec() {
    if (!entity || revoking) return;
    const path = entity.id;
    if (!path) return;
    revoking = true;
    try {
      await api.revokeSpec(path, 'Revoked from detail panel');
      toastSuccess($t('detail_panel.spec_revoked'));
      if (entity.data) entity = { ...entity, data: { ...entity.data, approval_status: 'pending' } };
    } catch (e) {
      toastError($t('detail_panel.revocation_failed', { values: { error: e.message } }));
    } finally {
      revoking = false;
    }
  }

  async function rejectCurrentSpec() {
    if (!entity || rejecting) return;
    const path = entity.id;
    if (!path) return;
    rejecting = true;
    try {
      await api.rejectSpec(path, 'Rejected from detail panel');
      toastSuccess('Spec rejected');
      if (entity.data) entity = { ...entity, data: { ...entity.data, approval_status: 'rejected' } };
    } catch (e) {
      toastError('Rejection failed: ' + (e.message ?? e));
    } finally {
      rejecting = false;
    }
  }

  // Architecture tab state (S2: spec detail mini canvas + predict loop)
  let archNodes = $state([]);
  let archEdges = $state([]);
  let archLoading = $state(false);
  let archLoaded = $state(false); // prevents re-fetch after empty result
  let archError = $state(null);
  let archGhostOverlays = $state([]);

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
      archNodes = [];
      archEdges = [];
      archLoaded = false;
      archError = null;
      archGhostOverlays = [];
    }
  });

  // Load data for the active spec tab
  $effect(() => {
    if (entity?.type !== 'spec') return;
    const path = entity.id;
    const repoId = entity.data?.repo_id ?? null;

    if ((activeTab === 'content' || activeTab === 'edit') && !specDetail && !specDetailLoading) {
      specDetailLoading = true;
      api.specContent(path, repoId)
        .then(async (d) => {
          specDetail = d;
          editContent = d?.content ?? '';
          // If no content but we got a repo_id from the ledger, try fetching content with repo context
          if (!d?.content && d?.repo_id && !repoId) {
            try {
              const withContent = await api.specContent(path, d.repo_id);
              if (withContent?.content) {
                specDetail = { ...d, ...withContent };
                editContent = withContent.content;
              }
            } catch { /* best effort */ }
          }
        })
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
    if (activeTab === 'architecture' && repoId && !archLoaded && !archLoading) {
      loadArchGraph(repoId, path);
    }
  });

  async function loadArchGraph(repoId, specPath) {
    // Snapshot entity identity to detect stale results after entity switch
    const loadingEntityId = entity?.id;
    archLoading = true;
    archError = null;
    try {
      const graph = await api.repoGraph(repoId);
      // Discard result if entity changed while this fetch was in-flight
      if (entity?.id !== loadingEntityId) return;
      const allNodes = graph?.nodes ?? [];
      const allEdges = graph?.edges ?? [];
      // Filter to nodes governed by this spec (spec_path match)
      const specNodes = allNodes.filter((n) => n.spec_path === specPath);
      const specNodeIds = new Set(specNodes.map((n) => n.id));
      // Include edges where both endpoints are in this spec's node set
      const specEdges = allEdges.filter(
        (e) => specNodeIds.has(e.source_id ?? e.source) && specNodeIds.has(e.target_id ?? e.target)
      );
      archNodes = specNodes;
      archEdges = specEdges;
      archLoaded = true;
    } catch (e) {
      if (entity?.id !== loadingEntityId) return;
      archError = e.message ?? 'Failed to load graph';
      archLoaded = true; // mark loaded even on error so we don't retry automatically
    } finally {
      if (entity?.id === loadingEntityId) archLoading = false;
    }
  }

  // Debounced graphPredict: when editContent changes while on the architecture tab,
  // run a predict call 800ms after the user stops typing.
  $effect(() => {
    const content = editContent;
    if (!content || entity?.type !== 'spec') return;
    const repoId = entity.data?.repo_id;
    const specId = entity.id;
    if (!repoId || !archNodes.length) return;
    const timer = setTimeout(async () => {
      try {
        const result = await api.graphPredict(repoId, {
          spec_path: specId,
          draft_content: content,
        });
        const overlays = result?.predictions ?? result?.overlays ?? [];
        archGhostOverlays = overlays.map((p) => ({
          nodeId: p.node_id ?? p.nodeId,
          type: p.change_type ?? p.type ?? 'modified',
        }));
      } catch {
        // Silent failure — predictions are best-effort
      }
    }, 800);
    // Cleanup: clear timer on effect re-run (new keypress) or component unmount
    return () => clearTimeout(timer);
  });

  function expandToCanvas() {
    if (!goToRepoTab || !entity) return;
    // Navigate to Architecture tab; S3 reads highlight_spec to pre-select nodes
    goToRepoTab('architecture', { highlight_spec: entity.id });
  }

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
      toastError($t('detail_panel.llm_assist_failed', { values: { error: e.message } }));
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
      toastSuccess($t('detail_panel.spec_saved', { values: { mr_id: result.mr_id } }));
    } catch (e) {
      toastError($t('detail_panel.save_failed', { values: { error: e.message } }));
    } finally {
      saving = false;
    }
  }

  // Helpers for spec tabs
  function specStatusColor(s) {
    if (s === 'approved')   return 'success';
    if (s === 'pending')    return 'warning';
    if (s === 'rejected')   return 'danger';
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

  /** Truncate a UUID/SHA to 8 chars for display. Full value shown in title. */
  function shortId(id) {
    if (!id) return '—';
    return id.length > 12 ? id.slice(0, 8) + '...' : id;
  }

  /** Copy text to clipboard and show a toast. */
  async function copyId(value) {
    if (!value) return;
    try {
      await navigator.clipboard.writeText(value);
      toastSuccess('Copied to clipboard');
    } catch { /* clipboard unavailable */ }
  }

  /** Navigate to an entity in the detail panel. */
  function navigateTo(type, id, data) {
    openDetailPanel?.({ type, id, data: data ?? {} });
  }

  /** Queue entity name resolution outside of template rendering. */
  function queueNameResolution(type, id) {
    if (!id) return;
    const key = `${type}:${id}`;
    if (entityNameCache[key] !== undefined) return;
    // Use queueMicrotask to avoid state mutation during template rendering
    queueMicrotask(() => {
      if (entityNameCache[key] !== undefined) return;
      entityNameCache = { ...entityNameCache, [key]: null };
      const fetcher = type === 'agent' ? api.agent(id).then(a => a?.name) :
                      type === 'task' ? api.task(id).then(t => t?.title) :
                      type === 'repo' ? api.repo(id).then(r => r?.name) :
                      type === 'mr' ? api.mergeRequest(id).then(m => m?.title) :
                      Promise.resolve(null);
      fetcher.then(name => {
        if (name) entityNameCache = { ...entityNameCache, [key]: name };
      }).catch(() => {});
    });
  }

  function entityName(type, id) {
    if (!id) return shortId(id);
    const cached = entityNameCache[`${type}:${id}`];
    if (cached) return cached;
    queueNameResolution(type, id);
    return shortId(id);
  }

  async function enqueueMr() {
    if (!entity || enqueueing) return;
    enqueueing = true;
    try {
      await api.enqueue(entity.id);
      toastSuccess('MR enqueued for merge');
      // Reload MR detail to reflect new status
      const updated = await api.mergeRequest(entity.id).catch(() => null);
      if (updated) {
        mrDetail = updated;
        if (entity.data) entity = { ...entity, data: { ...entity.data, ...updated } };
      }
    } catch (e) {
      toastError('Failed to enqueue: ' + (e.message ?? e));
    } finally {
      enqueueing = false;
    }
  }

  async function submitComment() {
    if (!newCommentText.trim() || !entity || submittingComment) return;
    submittingComment = true;
    try {
      await api.submitComment(entity.id, { author_agent_id: 'human-reviewer', body: newCommentText.trim() });
      toastSuccess('Comment added');
      newCommentText = '';
      // Reload comments
      const cmts = await api.mrComments(entity.id).catch(() => []);
      mrComments = Array.isArray(cmts) ? cmts : [];
    } catch (e) {
      toastError('Failed to add comment: ' + (e.message ?? e));
    } finally {
      submittingComment = false;
    }
  }

  async function submitReview() {
    if (!entity || submittingReview) return;
    submittingReview = true;
    try {
      await api.submitReview(entity.id, {
        reviewer_agent_id: 'human-reviewer',
        decision: newReviewDecision,
        body: newReviewBody.trim() || undefined,
      });
      toastSuccess('Review submitted');
      newReviewBody = '';
      // Reload reviews
      const revs = await api.mrReviews(entity.id).catch(() => []);
      mrReviews = Array.isArray(revs) ? revs : [];
    } catch (e) {
      toastError('Failed to submit review: ' + (e.message ?? e));
    } finally {
      submittingReview = false;
    }
  }

  async function sendMessage() {
    if (!newMessageText?.trim() || !entity || sendingMessage) return;
    sendingMessage = true;
    try {
      await api.sendAgentMessage(entity.id, { content: newMessageText.trim(), kind: 'FreeText' });
      toastSuccess('Message sent');
      newMessageText = '';
      // Reload messages
      const msgs = await api.agentMessages(entity.id).catch(() => []);
      agentMessages = Array.isArray(msgs) ? msgs : (msgs?.messages ?? []);
    } catch (e) {
      toastError('Failed to send message: ' + (e.message ?? e));
    } finally {
      sendingMessage = false;
    }
  }

  /** Map timeline event types to human-readable labels and icons */
  function timelineEventLabel(evt) {
    const map = {
      'created': 'MR created',
      'mr_created': 'MR created',
      'commit_pushed': 'Commits pushed',
      'gate_started': 'Gate started',
      'gate_passed': 'Gate passed',
      'gate_failed': 'Gate failed',
      'GateResult': 'Gate completed',
      'enqueued': 'Enqueued for merge',
      'merged': 'Merged',
      'Merged': 'Merged to main',
      'closed': 'Closed',
      'review_submitted': 'Review submitted',
      'comment_added': 'Comment added',
      'graph_extracted': 'Graph extracted',
      'GraphDelta': 'Architecture updated',
      'GitPush': 'Code pushed',
      'attestation_created': 'Attestation signed',
    };
    return map[evt] ?? evt?.replace(/_/g, ' ') ?? 'Event';
  }

  function timelineEventVariant(evt) {
    if (evt === 'merged' || evt === 'Merged' || evt === 'gate_passed') return 'success';
    if (evt === 'gate_failed' || evt === 'closed') return 'danger';
    if (evt?.startsWith('gate_') || evt === 'GateResult') return 'warning';
    if (evt === 'GraphDelta' || evt === 'graph_extracted') return 'info';
    return 'info';
  }

  /** Extract human-readable detail from a timeline event's detail field */
  function timelineDetailText(evt) {
    const detail = evt.detail ?? evt.details;
    if (!detail) return evt.message ?? null;
    if (typeof detail === 'string') return detail;
    // GateResult events have {gate, status}
    if (detail.gate) {
      const status = detail.status === 'pass' || detail.status === 'passed' ? 'passed' : detail.status === 'fail' || detail.status === 'failed' ? 'failed' : detail.status;
      return `${detail.gate}: ${status}`;
    }
    // GitPush events have {branch, sha, ...}
    if (detail.branch) return `Branch: ${detail.branch}${detail.sha ? ' @ ' + detail.sha.slice(0, 7) : ''}`;
    // GraphDelta events have {added, removed, changed}
    if (detail.added !== undefined || detail.removed !== undefined) {
      const parts = [];
      if (detail.added) parts.push(`+${detail.added} nodes`);
      if (detail.removed) parts.push(`-${detail.removed} nodes`);
      if (detail.changed) parts.push(`~${detail.changed} changed`);
      return parts.join(', ') || null;
    }
    return JSON.stringify(detail);
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside
  class="detail-panel"
  class:expanded
  class:open={!!entity}
  role="dialog"
  aria-label={$t('detail_panel.title')}
  aria-modal={expanded ? 'true' : undefined}
  tabindex="-1"
  onkeydown={handleKeydown}
  bind:this={panelEl}
>
  {#if entity && expanded && showEditorSplit}
    <EditorSplit
      bind:content={editContent}
      onChange={(v) => { editContent = v; }}
      repoId={entity.data?.repo_id ?? null}
      specPath={entity.id}
      ghostOverlays={archGhostOverlays}
      onClose={closeEditorSplit}
      context="spec"
    />
  {:else if entity}
    <div class="panel-header">
      {#if onback}
        <button
          class="panel-btn panel-back"
          onclick={onback}
          aria-label="Go back"
          title="Go back to previous entity"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <polyline points="15 18 9 12 15 6"/>
          </svg>
        </button>
      {/if}
      <div class="panel-entity">
        <span class="entity-type">{entity.type}</span>
        <span class="entity-id">{entity.data?.name ?? entity.data?.title ?? entity.id}</span>
      </div>
      <div class="panel-actions">
        <button
          class="panel-btn"
          onclick={popout}
          aria-label={expanded ? $t('detail_panel.collapse') : $t('detail_panel.pop_out')}
          title={expanded ? $t('detail_panel.collapse_label') : $t('detail_panel.pop_out_label')}
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
          <span class="sr-only">{expanded ? $t('detail_panel.collapse_label') : $t('detail_panel.pop_out_label')}</span>
        </button>
        <button
          class="panel-btn panel-close"
          onclick={close}
          aria-label={$t('detail_panel.close')}
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
          {#if entity.type === 'mr'}
            {#if mrDetailLoading && !entity.data}
              <div class="spec-skeleton">
                {#each Array(5) as _}<Skeleton width="100%" height="1.2rem" />{/each}
              </div>
            {:else}
              {@const mr = mrDetail ?? entity.data ?? {}}
              <dl class="entity-meta">
                <dt>Title</dt><dd>{mr.title ?? '—'}</dd>
                <dt>Status</dt>
                <dd>
                  <Badge value={mr.status ?? 'unknown'} variant={mr.status === 'merged' ? 'success' : mr.status === 'open' ? 'info' : 'muted'} />
                  {#if mr._statusStory?.length > 0}
                    <span class="status-story">
                      {#each mr._statusStory as step, i}
                        <span class="status-step status-step-{step.variant}">{step.label}</span>{#if i < mr._statusStory.length - 1}<span class="status-step-arrow">→</span>{/if}
                      {/each}
                    </span>
                  {/if}
                </dd>
                <dt>ID</dt><dd class="mono copyable" title="Click to copy: {entity.id}" onclick={() => copyId(entity.id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(entity.id); }}>{shortId(entity.id)}</dd>
                {#if mr.description}
                  <dt>Description</dt><dd class="task-description">{mr.description}</dd>
                {/if}
                {#if mr.source_branch}
                  <dt>Branch</dt><dd class="mono">{mr.source_branch} → {mr.target_branch ?? 'main'}</dd>
                {/if}
                {#if mr.diff_stats}
                  <dt>Changes</dt>
                  <dd>
                    <span class="diff-stat-inline">{mr.diff_stats.files_changed ?? 0} files</span>
                    <span class="diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                    <span class="diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                  </dd>
                {/if}
                {#if mr.spec_ref}
                  {@const specPath = mr.spec_ref.split('@')[0]}
                  <dt>Spec</dt><dd><button class="entity-link mono" title={mr.spec_ref} onclick={() => navigateTo('spec', specPath, { path: specPath, repo_id: mr.repository_id ?? mr.repo_id })}>{specPath.split('/').pop()}</button></dd>
                {/if}
                {#if mr.author_agent_id}
                  <dt>Agent</dt><dd><button class="entity-link mono" title={mr.author_agent_id} onclick={() => navigateTo('agent', mr.author_agent_id)}>{entityName('agent', mr.author_agent_id)}</button></dd>
                {:else if mr.agent_id}
                  <dt>Agent</dt><dd><button class="entity-link mono" title={mr.agent_id} onclick={() => navigateTo('agent', mr.agent_id)}>{entityName('agent', mr.agent_id)}</button></dd>
                {/if}
                {#if mr.repository_id ?? mr.repo_id}
                  <dt>Repo</dt><dd class="mono" title={mr.repository_id ?? mr.repo_id}>{entityName('repo', mr.repository_id ?? mr.repo_id)}</dd>
                {/if}
                {#if mr.author_id && mr.author_id !== mr.author_agent_id}
                  <dt>Author</dt><dd class="mono" title={mr.author_id}>{shortId(mr.author_id)}</dd>
                {/if}
                {#if mr.task_id}
                  <dt>Task</dt><dd><button class="entity-link" title={mr.task_id} onclick={() => navigateTo('task', mr.task_id)}>{entityName('task', mr.task_id)}</button></dd>
                {/if}
                {#if mr.has_conflicts}
                  <dt>Conflicts</dt><dd><Badge value="conflicts" variant="danger" /></dd>
                {/if}
                {#if mr.depends_on?.length}
                  <dt>Depends on</dt>
                  <dd>
                    {#each mr.depends_on as depId, i}
                      <button class="entity-link mono" title={depId} onclick={() => navigateTo('mr', depId)}>{entityName('mr', depId)}</button>{#if i < mr.depends_on.length - 1}, {/if}
                    {/each}
                  </dd>
                {/if}
                {#if mr.atomic_group}
                  <dt>Atomic group</dt><dd class="mono">{mr.atomic_group}</dd>
                {/if}
                {#if mr.created_at}
                  <dt>Created</dt><dd>{fmtDate(mr.created_at)}</dd>
                {/if}
                {#if mr.merged_at ?? mr.updated_at}
                  <dt>{mr.status === 'merged' ? 'Merged' : 'Updated'}</dt><dd>{fmtDate(mr.merged_at ?? mr.updated_at)}</dd>
                {/if}
              </dl>

              <!-- Provenance chain -->
              {@const specPath = mr.spec_ref?.split('@')[0]}
              {@const agentId = mr.author_agent_id ?? mr.agent_id}
              {#if specPath || mr.task_id || agentId}
                <div class="provenance-chain">
                  <span class="provenance-label">Provenance</span>
                  <div class="provenance-flow">
                    {#if specPath}
                      <button class="provenance-node provenance-spec" onclick={() => navigateTo('spec', specPath, { path: specPath, repo_id: mr.repository_id ?? mr.repo_id })} title={specPath}>
                        <span class="provenance-icon prov-icon-spec"></span>
                        <span class="provenance-type">Spec</span>
                        <span class="provenance-name">{specPath.split('/').pop()}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    {#if mr.task_id}
                      <button class="provenance-node provenance-task" onclick={() => navigateTo('task', mr.task_id)} title={mr.task_id}>
                        <span class="provenance-icon prov-icon-task"></span>
                        <span class="provenance-type">Task</span>
                        <span class="provenance-name">{entityName('task', mr.task_id)}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    {#if agentId}
                      <button class="provenance-node provenance-agent" onclick={() => navigateTo('agent', agentId)} title={agentId}>
                        <span class="provenance-icon prov-icon-agent"></span>
                        <span class="provenance-type">Agent</span>
                        <span class="provenance-name">{entityName('agent', agentId)}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    <span class="provenance-node provenance-mr provenance-current">
                      <span class="provenance-icon prov-icon-mr"></span>
                      <span class="provenance-type">MR</span>
                      <span class="provenance-name">{mr.status ?? 'open'}</span>
                    </span>
                    {#if mr.status === 'merged' && mr.diff_stats}
                      <span class="provenance-arrow">&#x2192;</span>
                      <span class="provenance-node provenance-code">
                        <span class="provenance-icon prov-icon-code"></span>
                        <span class="provenance-type">Code</span>
                        <span class="provenance-name">+{mr.diff_stats.insertions ?? 0} -{mr.diff_stats.deletions ?? 0}</span>
                      </span>
                    {/if}
                  </div>
                </div>
              {/if}

              <!-- Gate Summary -->
              {#if mr._gateSummary}
                <div class="gate-summary-bar">
                  <span class="gate-summary-label">Gates</span>
                  <div class="gate-summary-pills">
                    {#if mr._gateSummary.passed > 0}
                      <span class="gate-pill gate-pill-pass">{mr._gateSummary.passed} passed</span>
                    {/if}
                    {#if mr._gateSummary.failed > 0}
                      <span class="gate-pill gate-pill-fail">{mr._gateSummary.failed} failed</span>
                    {/if}
                    {#if mr._gateSummary.total - mr._gateSummary.passed - mr._gateSummary.failed > 0}
                      <span class="gate-pill gate-pill-pending">{mr._gateSummary.total - mr._gateSummary.passed - mr._gateSummary.failed} pending</span>
                    {/if}
                  </div>
                  {#if mr._gateSummary.gates?.length > 0}
                    <div class="gate-detail-list">
                      {#each mr._gateSummary.gates as gate}
                        {@const passed = gate.status === 'Passed' || gate.status === 'passed'}
                        {@const failed = gate.status === 'Failed' || gate.status === 'failed'}
                        <span class="gate-detail-item" class:gate-pass={passed} class:gate-fail={failed}>
                          <span class="gate-check">{passed ? '✓' : failed ? '✗' : '○'}</span>
                          <span class="gate-detail-name">{gate.name}</span>
                          {#if gate.required === false}<span class="gate-advisory-tag">advisory</span>{/if}
                        </span>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}

              <!-- Dependencies -->
              {#if mrDeps && ((mrDeps.depends_on?.length ?? 0) > 0 || (mrDeps.dependents?.length ?? 0) > 0)}
                <div class="mr-deps-section">
                  {#if mrDeps.depends_on?.length > 0}
                    <span class="progress-section-label">Blocked by</span>
                    <ul class="task-list">
                      {#each mrDeps.depends_on as depId}
                        <li class="task-item clickable-row" onclick={() => navigateTo('mr', depId)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('mr', depId); }}>
                          <span class="task-title">{entityName('mr', depId)}</span>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                  {#if mrDeps.dependents?.length > 0}
                    <span class="progress-section-label">Blocks</span>
                    <ul class="task-list">
                      {#each mrDeps.dependents as depId}
                        <li class="task-item clickable-row" onclick={() => navigateTo('mr', depId)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('mr', depId); }}>
                          <span class="task-title">{entityName('mr', depId)}</span>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              {/if}

              <!-- MR Actions -->
              {#if mr.status === 'open'}
                <div class="mr-actions">
                  {#if mr.queue_position != null}
                    <span class="queue-position">
                      <Badge value="queued" variant="warning" />
                      <span class="queue-pos-text">Position {mr.queue_position + 1} in merge queue</span>
                    </span>
                  {:else}
                    <Button variant="primary" onclick={enqueueMr} disabled={enqueueing}>
                      {enqueueing ? 'Enqueueing...' : 'Enqueue for Merge'}
                    </Button>
                  {/if}
                </div>
              {:else if mr.status === 'merged'}
                <div class="mr-merged-info">
                  <Badge value="merged" variant="success" />
                  {#if mr.merge_commit_sha}
                    <span class="mono copyable" title="Click to copy merge SHA: {mr.merge_commit_sha}" onclick={() => copyId(mr.merge_commit_sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(mr.merge_commit_sha); }}>{mr.merge_commit_sha.slice(0, 7)}</span>
                  {/if}
                  {#if mr._commitSig}
                    <span class="sig-badge" title="Commit signed with {mr._commitSig.algorithm ?? 'unknown'}">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
                      signed
                    </span>
                  {/if}
                </div>
              {/if}
            {/if}
          {:else if entity.type === 'agent'}
            {#if agentDetailLoading && !entity.data}
              <div class="spec-skeleton">
                {#each Array(5) as _}<Skeleton width="100%" height="1.2rem" />{/each}
              </div>
            {:else}
              {@const ag = agentDetail ?? entity.data ?? {}}
              <dl class="entity-meta">
                <dt>Name</dt><dd>{ag.name ?? entity.id}</dd>
                <dt>Status</dt><dd><Badge value={ag.status ?? 'unknown'} variant={ag.status === 'active' ? 'success' : ag.status === 'idle' || ag.status === 'completed' ? 'info' : ag.status === 'failed' || ag.status === 'dead' ? 'danger' : ag.status === 'stopped' ? 'muted' : 'muted'} /></dd>
                <dt>ID</dt><dd class="mono copyable" title="Click to copy: {entity.id}" onclick={() => copyId(entity.id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(entity.id); }}>{shortId(entity.id)}</dd>
                {#if ag.agent_type}
                  <dt>Type</dt><dd>{ag.agent_type}</dd>
                {/if}
                {#if ag._card?.description}
                  <dt>Description</dt><dd class="task-description">{ag._card.description}</dd>
                {/if}
                {#if ag._card?.capabilities?.length > 0}
                  <dt>Capabilities</dt>
                  <dd class="agent-caps">
                    {#each ag._card.capabilities as cap}
                      <span class="cap-tag">{cap}</span>
                    {/each}
                  </dd>
                {/if}
                {#if ag._card?.protocols?.length > 0}
                  <dt>Protocols</dt>
                  <dd class="agent-caps">
                    {#each ag._card.protocols as proto}
                      <span class="cap-tag cap-proto">{proto}</span>
                    {/each}
                  </dd>
                {/if}
                {#if ag.branch}
                  <dt>Branch</dt><dd class="mono">{ag.branch}</dd>
                {/if}
                {#if ag.task_id}
                  <dt>Task</dt><dd><button class="entity-link" title={ag.task_id} onclick={() => navigateTo('task', ag.task_id)}>{entityName('task', ag.task_id)}</button></dd>
                {/if}
                {#if ag.repo_id}
                  <dt>Repo</dt><dd class="mono" title={ag.repo_id}>{entityName('repo', ag.repo_id)}</dd>
                {/if}
                {#if ag.mr_id}
                  <dt>MR</dt><dd><button class="entity-link mono" title={ag.mr_id} onclick={() => navigateTo('mr', ag.mr_id)}>{entityName('mr', ag.mr_id)}</button></dd>
                {/if}
                {#if ag.workspace_id}
                  <dt>Workspace</dt><dd class="mono" title={ag.workspace_id}>{shortId(ag.workspace_id)}</dd>
                {/if}
                {#if ag.created_at}
                  <dt>Spawned</dt><dd>{fmtDate(ag.created_at)}</dd>
                {/if}
                {#if ag.completed_at}
                  <dt>Completed</dt><dd>{fmtDate(ag.completed_at)}</dd>
                  {@const dur = ag.completed_at - ag.created_at}
                  <dt>Duration</dt><dd>{dur < 60 ? `${Math.round(dur)}s` : dur < 3600 ? `${Math.round(dur / 60)}m` : `${Math.round(dur / 3600)}h ${Math.round((dur % 3600) / 60)}m`}</dd>
                {:else if ag.created_at}
                  {@const elapsed = Math.round((Date.now() / 1000 - ag.created_at) / 60)}
                  <dt>Running</dt><dd>{elapsed < 60 ? `${elapsed}m` : `${Math.round(elapsed / 60)}h ${elapsed % 60}m`}</dd>
                {/if}
              </dl>

              <!-- Workload attestation (compute target, hostname, alive) -->
              {#if ag._workload}
                <div class="agent-container-info">
                  <span class="progress-section-label">Runtime</span>
                  <dl class="entity-meta">
                    {#if ag._workload.compute_target}
                      <dt>Compute</dt><dd>{ag._workload.compute_target}</dd>
                    {/if}
                    {#if ag._workload.hostname}
                      <dt>Host</dt><dd class="mono">{ag._workload.hostname}</dd>
                    {/if}
                    {#if ag._workload.alive !== undefined}
                      <dt>Alive</dt><dd><Badge value={ag._workload.alive ? 'yes' : 'no'} variant={ag._workload.alive ? 'success' : 'muted'} /></dd>
                    {/if}
                    {#if ag._workload.pid}
                      <dt>PID</dt><dd class="mono">{ag._workload.pid}</dd>
                    {/if}
                    {#if ag._workload.attested_at}
                      <dt>Attested</dt><dd>{fmtDate(ag._workload.attested_at)}</dd>
                    {/if}
                  </dl>
                </div>
              {/if}

              <!-- Container info if available -->
              {#if ag._container}
                <div class="agent-container-info">
                  <span class="progress-section-label">Container</span>
                  <dl class="entity-meta">
                    {#if ag._container.image}
                      <dt>Image</dt><dd class="mono">{ag._container.image}</dd>
                    {/if}
                    {#if ag._container.runtime}
                      <dt>Runtime</dt><dd>{ag._container.runtime}</dd>
                    {/if}
                    {#if ag._container.exit_code !== undefined && ag._container.exit_code !== null}
                      <dt>Exit</dt><dd><Badge value={String(ag._container.exit_code)} variant={ag._container.exit_code === 0 ? 'success' : 'danger'} /></dd>
                    {/if}
                  </dl>
                </div>
              {/if}

              <!-- Touched paths (files written by this agent) -->
              {#if ag._touchedPaths}
                {@const paths = Array.isArray(ag._touchedPaths) ? ag._touchedPaths : (ag._touchedPaths?.paths ?? ag._touchedPaths?.files ?? [])}
                {#if paths.length > 0}
                  <div class="agent-container-info">
                    <span class="progress-section-label">Files Modified</span>
                    <div class="touched-paths-list">
                      {#each paths.slice(0, 10) as p}
                        <span class="touched-path mono">{typeof p === 'string' ? p : (p.path ?? p.file ?? JSON.stringify(p))}</span>
                      {/each}
                      {#if paths.length > 10}
                        <span class="touched-path-more">+{paths.length - 10} more files</span>
                      {/if}
                    </div>
                  </div>
                {/if}
              {/if}

              <!-- Provenance chain for agent -->
              {#if ag.task_id || ag.mr_id}
                <div class="provenance-chain">
                  <span class="provenance-label">Provenance</span>
                  <div class="provenance-flow">
                    {#if ag.task_id}
                      <button class="provenance-node provenance-task" onclick={() => navigateTo('task', ag.task_id)} title={ag.task_id}>
                        <span class="provenance-icon prov-icon-task"></span>
                        <span class="provenance-type">Task</span>
                        <span class="provenance-name">{entityName('task', ag.task_id)}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    <span class="provenance-node provenance-agent provenance-current">
                      <span class="provenance-icon prov-icon-agent"></span>
                      <span class="provenance-type">Agent</span>
                      <span class="provenance-name">{ag.status ?? 'active'}</span>
                    </span>
                    {#if ag.mr_id}
                      <span class="provenance-arrow">&#x2192;</span>
                      <button class="provenance-node provenance-mr" onclick={() => navigateTo('mr', ag.mr_id)} title={ag.mr_id}>
                        <span class="provenance-icon prov-icon-mr"></span>
                        <span class="provenance-type">MR</span>
                        <span class="provenance-name">{entityName('mr', ag.mr_id)}</span>
                      </button>
                    {/if}
                  </div>
                </div>
              {/if}

              <!-- Recent logs preview -->
              {#if Array.isArray(agentLogs) && agentLogs.length > 0}
                <div class="agent-recent-logs">
                  <span class="progress-section-label">Recent Activity</span>
                  <div class="trace-list trace-list-compact">
                    {#each agentLogs.slice(0, 3) as entry}
                      <div class="trace-entry">
                        {#if entry.timestamp || entry.created_at}
                          <span class="trace-time">{fmtDate(entry.timestamp ?? entry.created_at)}</span>
                        {/if}
                        <span class="trace-msg">{entry.message ?? entry.content ?? entry.line ?? JSON.stringify(entry)}</span>
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}
            {/if}
          {:else if entity.type === 'task'}
            {#if taskDetailLoading && !entity.data}
              <div class="spec-skeleton">
                {#each Array(5) as _}<Skeleton width="100%" height="1.2rem" />{/each}
              </div>
            {:else}
              {@const tk = taskDetail ?? entity.data ?? {}}
              <dl class="entity-meta">
                <dt>Title</dt><dd>{tk.title ?? '—'}</dd>
                <dt>Status</dt>
                <dd>
                  <Badge value={tk.status ?? 'unknown'} variant={taskStatusColor(tk.status)} />
                  {#if tk.status && tk.status !== 'backlog'}
                    <span class="status-story">
                      <span class="status-step status-step-info">Created</span>
                      <span class="status-step-arrow">→</span>
                      {#if tk.assigned_to}
                        <span class="status-step status-step-warning">Assigned</span>
                        <span class="status-step-arrow">→</span>
                      {/if}
                      <span class="status-step status-step-{taskStatusColor(tk.status)}">{tk.status}</span>
                    </span>
                  {/if}
                </dd>
                <dt>ID</dt><dd class="mono copyable" title="Click to copy: {entity.id}" onclick={() => copyId(entity.id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(entity.id); }}>{shortId(entity.id)}</dd>
                {#if tk.priority}
                  <dt>Priority</dt><dd><Badge value={tk.priority} variant={tk.priority === 'high' || tk.priority === 'critical' ? 'danger' : tk.priority === 'low' ? 'muted' : 'warning'} /></dd>
                {/if}
                {#if tk.task_type}
                  <dt>Type</dt><dd>{tk.task_type}</dd>
                {/if}
                {#if tk.description}
                  <dt>Description</dt><dd class="task-description">{tk.description}</dd>
                {/if}
                {#if tk.spec_path}
                  <dt>Spec</dt><dd><button class="entity-link mono" title={tk.spec_path} onclick={() => navigateTo('spec', tk.spec_path, { path: tk.spec_path, repo_id: tk.repo_id })}>{tk.spec_path.split('/').pop()}</button></dd>
                {/if}
                {#if tk.branch}
                  <dt>Branch</dt><dd class="mono">{tk.branch}</dd>
                {/if}
                {#if tk.assigned_to}
                  <dt>Agent</dt><dd><button class="entity-link mono" title={tk.assigned_to} onclick={() => navigateTo('agent', tk.assigned_to)}>{entityName('agent', tk.assigned_to)}</button></dd>
                {/if}
                {#if tk.repo_id}
                  <dt>Repo</dt><dd class="mono" title={tk.repo_id}>{entityName('repo', tk.repo_id)}</dd>
                {/if}
                {#if tk.labels?.length > 0}
                  <dt>Labels</dt><dd>{tk.labels.join(', ')}</dd>
                {/if}
                {#if tk.created_at}
                  <dt>Created</dt><dd>{fmtDate(tk.created_at)}</dd>
                {/if}
                {#if tk.updated_at}
                  <dt>Updated</dt><dd>{fmtDate(tk.updated_at)}</dd>
                {/if}
              </dl>

              <!-- Provenance chain for task -->
              {#if tk.spec_path || tk.assigned_to}
                <div class="provenance-chain">
                  <span class="provenance-label">Provenance</span>
                  <div class="provenance-flow">
                    {#if tk.spec_path}
                      <button class="provenance-node provenance-spec" onclick={() => navigateTo('spec', tk.spec_path, { path: tk.spec_path, repo_id: tk.repo_id })} title={tk.spec_path}>
                        <span class="provenance-icon prov-icon-spec"></span>
                        <span class="provenance-type">Spec</span>
                        <span class="provenance-name">{tk.spec_path.split('/').pop()}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    <span class="provenance-node provenance-task provenance-current">
                      <span class="provenance-icon prov-icon-task"></span>
                      <span class="provenance-type">Task</span>
                      <span class="provenance-name">{tk.status ?? 'backlog'}</span>
                    </span>
                    {#if tk.assigned_to}
                      <span class="provenance-arrow">&#x2192;</span>
                      <button class="provenance-node provenance-agent" onclick={() => navigateTo('agent', tk.assigned_to)} title={tk.assigned_to}>
                        <span class="provenance-icon prov-icon-agent"></span>
                        <span class="provenance-type">Agent</span>
                        <span class="provenance-name">{entityName('agent', tk.assigned_to)}</span>
                      </button>
                    {/if}
                  </div>
                </div>
              {/if}
            {/if}
          {:else if entity.type === 'commit'}
            {@const c = entity.data ?? {}}
            {@const sha = c.sha ?? c.id ?? entity.id}
            <dl class="entity-meta">
              <dt>SHA</dt><dd class="mono copyable" title="Click to copy: {sha}" onclick={() => copyId(sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(sha); }}>{sha.slice(0, 12)}...</dd>
              {#if c.message ?? c.summary}
                <dt>Message</dt><dd class="task-description">{c.message ?? c.summary}</dd>
              {/if}
              {#if c.author ?? c.author_name}
                <dt>Author</dt><dd>{c.author ?? c.author_name}</dd>
              {/if}
              {#if c.author_email}
                <dt>Email</dt><dd class="mono">{c.author_email}</dd>
              {/if}
              {#if c.timestamp ?? c.authored_at ?? c.date}
                <dt>Date</dt><dd>{fmtDate(c.timestamp ?? c.authored_at ?? c.date)}</dd>
              {/if}
              {#if c.agent_id}
                <dt>Agent</dt><dd><button class="entity-link mono" title={c.agent_id} onclick={() => navigateTo('agent', c.agent_id)}>{entityName('agent', c.agent_id)}</button></dd>
              {/if}
              {#if c.branch}
                <dt>Branch</dt><dd class="mono">{c.branch}</dd>
              {/if}
              {#if c.parents?.length > 0}
                <dt>Parents</dt><dd class="mono">{c.parents.map(p => p.slice(0, 7)).join(', ')}</dd>
              {/if}
            </dl>
            {#if c.agent_id}
              <div class="provenance-chain">
                <span class="provenance-label">Provenance</span>
                <div class="provenance-flow">
                  <button class="provenance-node provenance-agent" onclick={() => navigateTo('agent', c.agent_id)} title={c.agent_id}>
                    <span class="provenance-icon prov-icon-agent"></span>
                    <span class="provenance-type">Agent</span>
                    <span class="provenance-name">{entityName('agent', c.agent_id)}</span>
                  </button>
                  <span class="provenance-arrow">&#x2192;</span>
                  <span class="provenance-node provenance-code provenance-current">
                    <span class="provenance-icon prov-icon-code"></span>
                    <span class="provenance-type">Commit</span>
                    <span class="provenance-name">{sha.slice(0, 7)}</span>
                  </span>
                </div>
              </div>
            {/if}
          {:else}
            <dl class="entity-meta">
              <dt>{$t('detail_panel.type')}</dt><dd>{entity.type}</dd>
              <dt>{$t('detail_panel.id')}</dt><dd class="mono" title={entity.id}>{shortId(entity.id)}</dd>
              {#if entity.data?.status}
                <dt>{$t('detail_panel.status')}</dt><dd>{entity.data.status}</dd>
              {/if}
              {#if entity.data?.created_at}
                <dt>{$t('detail_panel.created')}</dt><dd>{fmtDate(entity.data.created_at)}</dd>
              {/if}
              {#if entity.data?.spec_path}
                <dt>{$t('detail_panel.spec')}</dt><dd class="mono">{entity.data.spec_path}</dd>
              {/if}
            </dl>
          {/if}
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
                <dt>{$t('detail_panel.status')}</dt>
                <dd>
                  <Badge value={entity.data.approval_status} variant={specStatusColor(entity.data.approval_status)} />
                </dd>
              {/if}
              {#if entity.data?.owner}
                <dt>{$t('detail_panel.owner')}</dt><dd class="mono">{entity.data.owner}</dd>
              {/if}
              {#if entity.data?.updated_at}
                <dt>{$t('detail_panel.updated')}</dt><dd>{fmtDate(entity.data.updated_at)}</dd>
              {/if}
            </dl>
            <div class="spec-approval-actions" data-testid="spec-approval-actions">
              {#if entity.data?.approval_status === 'approved'}
                <button
                  class="approval-btn revoke"
                  onclick={revokeCurrentSpec}
                  disabled={revoking}
                  data-testid="spec-revoke-btn"
                >
                  {revoking ? $t('detail_panel.revoking') : $t('detail_panel.revoke_approval')}
                </button>
              {:else if entity.data?.approval_status === 'rejected'}
                <button
                  class="approval-btn approve"
                  onclick={approveCurrentSpec}
                  disabled={approving || !entity.data?.current_sha}
                  data-testid="spec-approve-btn"
                >
                  {approving ? $t('detail_panel.approving') : 'Re-approve'}
                </button>
              {:else}
                <button
                  class="approval-btn approve"
                  onclick={approveCurrentSpec}
                  disabled={approving || !entity.data?.current_sha}
                  data-testid="spec-approve-btn"
                >
                  {approving ? $t('detail_panel.approving') : $t('detail_panel.approve')}
                </button>
                <button
                  class="approval-btn revoke"
                  onclick={rejectCurrentSpec}
                  disabled={rejecting}
                  data-testid="spec-reject-btn"
                >
                  {rejecting ? 'Rejecting...' : 'Reject'}
                </button>
              {/if}
            </div>
            <div class="spec-content-box">
              <pre class="spec-content-pre">{specDetail.content}</pre>
            </div>
          {:else}
            {@const sd = specDetail ?? entity.data ?? {}}
            <dl class="spec-meta-list">
              <dt>{$t('detail_panel.path')}</dt><dd class="mono">{entity.id}</dd>
              {#if sd.title}
                <dt>{$t('detail_panel.title')}</dt><dd>{sd.title}</dd>
              {/if}
              {#if sd.approval_status}
                <dt>{$t('detail_panel.status')}</dt>
                <dd><Badge value={sd.approval_status} variant={specStatusColor(sd.approval_status)} /></dd>
              {/if}
              {#if sd.owner}
                <dt>{$t('detail_panel.owner')}</dt><dd class="mono">{sd.owner}</dd>
              {/if}
              {#if sd.kind}
                <dt>{$t('detail_panel.kind')}</dt><dd>{sd.kind}</dd>
              {/if}
              {#if sd.current_sha}
                <dt>{$t('detail_panel.sha')}</dt><dd class="mono">{sd.current_sha.slice(0, 7)}</dd>
              {/if}
              {#if sd.drift_status && sd.drift_status !== 'none'}
                <dt>Drift</dt><dd><Badge value={sd.drift_status} variant={sd.drift_status === 'drifted' ? 'warning' : 'muted'} /></dd>
              {/if}
              {#if sd.repo_id}
                <dt>Repo</dt><dd class="mono">{entityName('repo', sd.repo_id)}</dd>
              {/if}
              {#if sd.updated_at}
                <dt>{$t('detail_panel.updated')}</dt><dd>{fmtDate(sd.updated_at)}</dd>
              {/if}
            </dl>
            <!-- Approval actions even when content is not available -->
            <div class="spec-approval-actions" data-testid="spec-approval-actions">
              {#if sd.approval_status === 'approved'}
                <button class="approval-btn revoke" onclick={revokeCurrentSpec} disabled={revoking}>
                  {revoking ? $t('detail_panel.revoking') : $t('detail_panel.revoke_approval')}
                </button>
              {:else if sd.approval_status === 'rejected'}
                <button class="approval-btn approve" onclick={approveCurrentSpec} disabled={approving || !sd.current_sha}>
                  {approving ? $t('detail_panel.approving') : 'Re-approve'}
                </button>
              {:else if sd.current_sha}
                <button class="approval-btn approve" onclick={approveCurrentSpec} disabled={approving}>
                  {approving ? $t('detail_panel.approving') : $t('detail_panel.approve')}
                </button>
                <button class="approval-btn revoke" onclick={rejectCurrentSpec} disabled={rejecting}>
                  {rejecting ? 'Rejecting...' : 'Reject'}
                </button>
              {/if}
            </div>
            {#if sd.linked_tasks?.length > 0}
              <span class="progress-section-label">Linked Tasks</span>
              <ul class="task-list">
                {#each sd.linked_tasks as taskId}
                  <li class="task-item clickable-row" onclick={() => navigateTo('task', taskId)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('task', taskId); }}>
                    <span class="task-title">{entityName('task', taskId)}</span>
                  </li>
                {/each}
              </ul>
            {/if}
            {#if sd.linked_mrs?.length > 0}
              <span class="progress-section-label">Linked MRs</span>
              <ul class="task-list">
                {#each sd.linked_mrs as mrId}
                  <li class="task-item clickable-row" onclick={() => navigateTo('mr', mrId)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('mr', mrId); }}>
                    <span class="task-title">{entityName('mr', mrId)}</span>
                  </li>
                {/each}
              </ul>
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
              placeholder={$t('detail_panel.spec_placeholder')}
              aria-label={$t('detail_panel.spec_editor')}
              spellcheck="false"
            ></textarea>

            {#if llmSuggestion}
              <div class="suggestion-block" role="region" aria-label={$t('detail_panel.suggested_change')}>
                <div class="suggestion-hdr">
                  <span class="suggestion-lbl">{$t('detail_panel.suggested_change')}</span>
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
                  <Button variant="primary" onclick={acceptSuggestion}>{$t('detail_panel.accept')}</Button>
                  <Button variant="secondary" onclick={editSuggestion}>{$t('detail_panel.edit_btn')}</Button>
                  <Button variant="secondary" onclick={dismissSuggestion}>{$t('detail_panel.dismiss')}</Button>
                </div>
              </div>
            {/if}

            {#if llmStreaming && llmExplanation}
              <div class="llm-streaming" aria-live="polite">
                <span class="streaming-lbl">{$t('detail_panel.thinking')}</span>
                <p class="streaming-txt">{llmExplanation}<span class="blink-cursor" aria-hidden="true"></span></p>
              </div>
            {/if}

            <div class="llm-input-area">
              <div class="recipient-line">{$t('detail_panel.edit_spec_recipient', { values: { name: entity.data?.title || entity.id } })}</div>
              <div class="llm-row">
                <textarea
                  class="llm-textarea"
                  bind:value={llmInstruction}
                  placeholder={$t('detail_panel.llm_placeholder')}
                  rows="2"
                  disabled={llmStreaming}
                  onkeydown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); sendLlmInstruction(); } }}
                  aria-label={$t('detail_panel.llm_instruction')}
                ></textarea>
                <button
                  class="llm-send"
                  onclick={sendLlmInstruction}
                  disabled={!llmInstruction.trim() || llmStreaming || !entity.data?.repo_id}
                  aria-label={$t('detail_panel.send_to_llm')}
                  aria-busy={llmStreaming}
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
                  <span class="sr-only">{$t('detail_panel.send')}</span>
                </button>
              </div>
              {#if !entity.data?.repo_id}
                <p class="llm-hint warn">{$t('detail_panel.llm_requires_repo')}</p>
              {:else}
                <p class="llm-hint">{$t('detail_panel.llm_hint')}</p>
              {/if}
            </div>

            {#if entity.data?.repo_id}
              <div class="save-bar">
                <Button variant="secondary" onclick={openEditorSplit} aria-label={$t('detail_panel.preview_aria')}>
                  {$t('detail_panel.preview')}
                </Button>
                <Button variant="primary" onclick={saveSpec} disabled={saving || !editContent.trim()}>
                  {saving ? $t('detail_panel.saving') : $t('detail_panel.save_create_mr')}
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
            {@const totalTasks = specProgress.total_tasks ?? (specProgress.tasks?.length ?? 0)}
            {@const done = specProgress.completed_tasks ?? 0}
            {@const pct = totalTasks > 0 ? Math.round((done / totalTasks) * 100) : 0}
            <div class="progress-summary">
              <span class="progress-big">{done}/{totalTasks}</span>
              <span class="progress-lbl">{$t('detail_panel.tasks_complete')}</span>
              {#if specProgress.merged_mrs}
                <span class="progress-mrs">{specProgress.merged_mrs} MR{specProgress.merged_mrs !== 1 ? 's' : ''} merged</span>
              {/if}
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
              <span class="progress-section-label">Tasks</span>
              <ul class="task-list">
                {#each specProgress.tasks as task}
                  <li class="task-item clickable-row" onclick={() => navigateTo('task', task.id ?? task.task_id, task)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('task', task.id ?? task.task_id, task); }}>
                    <Badge value={task.status} variant={taskStatusColor(task.status)} />
                    <span class="task-title">{task.title}</span>
                    {#if task.priority && task.priority !== 'medium'}
                      <span class="task-priority priority-{task.priority}">{task.priority}</span>
                    {/if}
                    {#if task.agent_id}
                      <span class="task-agent mono" title={task.agent_id}>{task.agent_id.slice(0, 8)}</span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="no-data">{$t('detail_panel.no_tasks')}</p>
            {/if}
            {#if specProgress.mrs?.length > 0}
              <span class="progress-section-label">Merge Requests</span>
              <ul class="task-list">
                {#each specProgress.mrs as mr}
                  <li class="task-item clickable-row" onclick={() => navigateTo('mr', mr.id ?? mr.mr_id, mr)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('mr', mr.id ?? mr.mr_id, mr); }}>
                    <Badge value={mr.status} variant={mr.status === 'merged' ? 'success' : mr.status === 'open' ? 'info' : 'muted'} />
                    <span class="task-title">{mr.title}</span>
                    {#if mr.source_branch}
                      <span class="task-agent mono">{mr.source_branch}</span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
          {:else}
            <p class="no-data">{$t('detail_panel.progress_requires_repo')}</p>
          {/if}
        </div>

      {:else if activeTab === 'links'}
        <div class="tab-pane">
          {#if specLinksLoading}
            <div class="spec-skeleton">
              {#each Array(4) as _}<Skeleton width="100%" height="1.5rem" />{/each}
            </div>
          {:else}
            {#each [Array.isArray(specLinks) ? specLinks : (specLinks?.links ?? [])] as linkArray}
              {#if linkArray.length > 0}
                <ul class="links-list">
                  {#each linkArray as link}
                    {@const target = typeof link === 'string' ? link : (link.target_path ?? link.target ?? JSON.stringify(link))}
                    {@const kind = typeof link === 'object' ? (link.kind ?? link.link_type ?? link.type) : null}
                    {@const direction = typeof link === 'object' ? link.direction : null}
                    {@const isConflict = kind === 'conflicts_with' || kind === 'conflicts'}
                    <li class="link-item" class:link-conflict={isConflict}>
                      {#if kind}
                        <Badge
                          value={kind.replace(/_/g, ' ')}
                          variant={isConflict ? 'danger' : 'info'}
                        />
                      {/if}
                      <span class="link-direction">{direction === 'inbound' ? '← from' : '→ to'}</span>
                      <button class="entity-link mono" title="Navigate to {target}" onclick={() => navigateTo('spec', target, { path: target, repo_id: entity?.data?.repo_id })}>{target.split('/').pop()}</button>
                      <span class="link-full-path mono">{target}</span>
                    </li>
                  {/each}
                </ul>
              {:else}
                <p class="no-data">{$t('detail_panel.no_links')}</p>
              {/if}
            {/each}
          {/if}
        </div>

      {:else if activeTab === 'spec'}
        <div class="tab-pane">
          <EmptyState title={$t('detail_panel.spec_not_loaded')} description={$t('detail_panel.spec_not_loaded_desc', { values: { path: entity.data?.spec_path ?? '' } })} />
        </div>

      {:else if activeTab === 'chat'}
        <div class="tab-pane">
          {#if entity.type === 'agent'}
            {#if agentMessagesLoading}
              <div class="spec-skeleton">
                {#each Array(5) as _}<Skeleton width="100%" height="1.5rem" />{/each}
              </div>
            {:else if Array.isArray(agentMessages) && agentMessages.length > 0}
              <div class="messages-list">
                {#each agentMessages as msg}
                  <div class="message-item">
                    <div class="message-header">
                      {#if msg.kind ?? msg.message_type}
                        <Badge value={msg.kind ?? msg.message_type} variant={
                          (msg.kind === 'TaskAssignment' || msg.message_type === 'TaskAssignment') ? 'info' :
                          (msg.kind === 'Escalation' || msg.message_type === 'Escalation') ? 'danger' :
                          (msg.kind === 'StatusUpdate' || msg.message_type === 'StatusUpdate') ? 'warning' :
                          (msg.kind === 'ReviewRequest' || msg.message_type === 'ReviewRequest') ? 'info' :
                          'muted'
                        } />
                      {/if}
                      {#if msg.sender_id ?? msg.from}
                        <span class="message-sender mono">{msg.sender_id ?? msg.from}</span>
                      {/if}
                      {#if msg.timestamp ?? msg.created_at}
                        <span class="message-time">{fmtDate(msg.timestamp ?? msg.created_at)}</span>
                      {/if}
                    </div>
                    <p class="message-body">{msg.content ?? msg.message ?? msg.body ?? JSON.stringify(msg)}</p>
                  </div>
                {/each}
              </div>
            {:else}
              <p class="no-data">No messages for this agent</p>
            {/if}

            <!-- Send message form -->
            <div class="message-form">
              <span class="progress-section-label">Send Message</span>
              <textarea
                class="comment-textarea"
                bind:value={newMessageText}
                placeholder="Send a message to this agent..."
                rows="2"
                disabled={sendingMessage}
              ></textarea>
              <div class="comment-form-actions">
                <Button variant="primary" size="sm" onclick={sendMessage} disabled={!newMessageText?.trim() || sendingMessage}>
                  {sendingMessage ? 'Sending...' : 'Send'}
                </Button>
              </div>
            </div>
          {:else}
            <EmptyState title={$t('detail_panel.no_conversation')} description={$t('detail_panel.start_conversation')} />
          {/if}
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
                        variant={ev.event === 'approved' ? 'success' : ev.event === 'rejected' || ev.event === 'invalidated' || ev.event === 'revoked' ? 'danger' : 'muted'}
                      />
                      <span class="history-user mono">{ev.user_id || ev.approver_id || '—'}</span>
                      <span class="history-time">{fmtDate(ev.timestamp || ev.approved_at)}</span>
                    </div>
                    {#if ev.sha || ev.spec_sha}
                      <span class="history-sha mono">{(ev.sha || ev.spec_sha).slice(0, 7)}</span>
                    {/if}
                    {#if ev.reason}
                      <p class="history-reason">{ev.reason}</p>
                    {/if}
                  </div>
                {/each}
              </div>
            {:else}
              <p class="no-data">{$t('detail_panel.no_approvals')}</p>
            {/if}
          {:else if entity.type === 'agent'}
            {#if agentWorkloadLoading}
              <div class="spec-skeleton">
                {#each Array(4) as _}<Skeleton width="100%" height="1.5rem" />{/each}
              </div>
            {:else if agentWorkload}
              {@const ag = normalizeAgent(agentWorkload.agent) ?? agentDetail ?? entity.data ?? {}}
              <!-- Agent execution logs (full log stream) -->
              {#if Array.isArray(agentLogs) && agentLogs.length > 0}
                <div class="trace-list">
                  {#each agentLogs as entry}
                    <div class="trace-entry">
                      {#if entry.timestamp || entry.created_at}
                        <span class="trace-time">{fmtDate(entry.timestamp ?? entry.created_at)}</span>
                      {/if}
                      <span class="trace-msg">{entry.message ?? entry.content ?? entry.line ?? JSON.stringify(entry)}</span>
                    </div>
                  {/each}
                </div>
              {:else}
                <p class="no-data">No execution logs available for this agent</p>
              {/if}
            {:else}
              <p class="no-data">No log data available</p>
            {/if}
          {:else}
            <EmptyState title={$t('detail_panel.no_history')} description={$t('detail_panel.no_history_desc')} />
          {/if}
        </div>

      {:else if activeTab === 'architecture'}
        <div class="tab-pane arch-tab">
          {#if archLoading}
            <div class="arch-loading-wrap">
              <Skeleton width="100%" height="220px" />
              <p class="arch-loading-label">{$t('detail_panel.loading_arch')}</p>
            </div>
          {:else if archError}
            <div class="arch-error" role="alert">
              <p class="arch-error-msg">{archError}</p>
              <Button
                variant="secondary"
                onclick={() => { archLoaded = false; archNodes = []; loadArchGraph(entity.data?.repo_id, entity.id); }}
              >{$t('common.retry')}</Button>
            </div>
          {:else}
            <div class="arch-mini-header">
              <span class="arch-mini-label">
                {$t('detail_panel.nodes_governed', { values: { count: archNodes.length } })}
              </span>
              {#if archGhostOverlays.length}
                <span class="arch-predict-badge">
                  {$t('detail_panel.predicted_changes', { values: { count: archGhostOverlays.length } })}
                </span>
              {/if}
            </div>
            <div class="arch-canvas-container" data-testid="arch-mini-canvas-wrap">
              <ArchPreviewCanvas
                nodes={archNodes}
                edges={archEdges}
                ghostOverlays={archGhostOverlays}
                size="mini"
              />
            </div>
            {#if goToRepoTab}
              <div class="arch-expand-wrap">
                <Button
                  variant="secondary"
                  onclick={expandToCanvas}
                  disabled={!archNodes.length}
                >
                  {$t('detail_panel.expand_to_canvas')}
                </Button>
              </div>
            {/if}
          {/if}
        </div>

      {:else if activeTab === 'diff'}
        <div class="tab-pane">
          {#if mrDiffLoading}
            <div class="spec-skeleton">
              {#each Array(6) as _}<Skeleton width="100%" height="1.2rem" />{/each}
            </div>
          {:else if mrDiff}
            <div class="diff-summary">
              <span class="diff-stat">{mrDiff.files_changed ?? 0} files changed</span>
              <span class="diff-ins">+{mrDiff.insertions ?? 0}</span>
              <span class="diff-del">-{mrDiff.deletions ?? 0}</span>
            </div>
            {#if mrDiff.files?.length > 0}
              <!-- File tree summary (like GitHub) -->
              <div class="diff-file-tree">
                {#each mrDiff.files as file}
                  <span class="diff-tree-item">
                    <span class="diff-tree-status diff-tree-status-{file.status ?? 'modified'}">{file.status === 'added' ? '+' : file.status === 'deleted' ? '-' : '~'}</span>
                    <span class="diff-tree-path mono">{file.path}</span>
                  </span>
                {/each}
              </div>
              <div class="diff-file-list">
                {#each mrDiff.files as file, idx}
                  <details class="diff-file" open={mrDiff.files.length <= 5}>
                    <summary class="diff-file-header">
                      <Badge value={file.status ?? 'modified'} variant={file.status === 'added' ? 'success' : file.status === 'deleted' ? 'danger' : 'info'} />
                      <span class="diff-file-path mono">{file.path}</span>
                      {#if file.insertions != null || file.deletions != null}
                        <span class="diff-file-stats">
                          {#if file.insertions}<span class="diff-ins">+{file.insertions}</span>{/if}
                          {#if file.deletions}<span class="diff-del">-{file.deletions}</span>{/if}
                        </span>
                      {/if}
                    </summary>
                    {#if file.patch}
                      <div class="diff-patch">{#each file.patch.split('\n') as line}<span class={line.startsWith('+') ? 'diff-line-add' : line.startsWith('-') ? 'diff-line-del' : line.startsWith('@@') ? 'diff-line-hunk' : 'diff-line'}>{line}
</span>{/each}</div>
                    {:else}
                      <p class="diff-no-patch">Binary file or no patch data available</p>
                    {/if}
                  </details>
                {/each}
              </div>
            {:else}
              <p class="no-data">No file details available</p>
            {/if}
          {:else}
            <p class="no-data">No diff data available</p>
          {/if}
        </div>

      {:else if activeTab === 'gates'}
        <div class="tab-pane">
          {#if mrGatesLoading}
            <div class="spec-skeleton">
              {#each Array(3) as _}<Skeleton width="100%" height="2rem" />{/each}
            </div>
          {:else if Array.isArray(mrGates) && mrGates.length > 0}
            <ul class="gates-list">
              {#each mrGates as gate}
                {@const duration = (gate.started_at && gate.finished_at) ? Math.round((gate.finished_at - gate.started_at) * 1000) : gate.duration_ms}
                {@const gateName = gate.name ?? gate.gate_name ?? (gate.gate_id ? shortId(gate.gate_id) : 'Gate')}
                {@const gateStatus = (gate.status === 'Passed' || gate.status === 'passed') ? 'passed' : (gate.status === 'Failed' || gate.status === 'failed') ? 'failed' : (gate.status === 'Running' || gate.status === 'running') ? 'running' : gate.status ?? 'pending'}
                <li class="gate-item gate-item-{gateStatus}">
                  <div class="gate-row">
                    <span class="gate-status-icon">{gateStatus === 'passed' ? '✓' : gateStatus === 'failed' ? '✗' : gateStatus === 'running' ? '⟳' : '○'}</span>
                    <span class="gate-name" title={gate.gate_id ?? ''}>{gateName}</span>
                    {#if gate.gate_type}
                      <span class="gate-type-badge">{gate.gate_type.replace(/_/g, ' ')}</span>
                    {/if}
                    {#if gate.required !== undefined}
                      <span class="gate-required-badge" class:advisory={!gate.required}>
                        {gate.required ? 'required' : 'advisory'}
                      </span>
                    {/if}
                    {#if duration}
                      <span class="gate-duration">{duration < 1000 ? duration + 'ms' : (duration / 1000).toFixed(1) + 's'}</span>
                    {/if}
                  </div>
                  {#if gate.command}
                    <div class="gate-cmd-row">
                      <span class="gate-cmd-label">Command</span>
                      <code class="gate-cmd mono">{gate.command}</code>
                    </div>
                  {/if}
                  {#if gate.output}
                    <details class="gate-output-details" open={gateStatus === 'failed'}>
                      <summary class="gate-output-label">Output</summary>
                      <pre class="gate-output">{gate.output}</pre>
                    </details>
                  {/if}
                  {#if gate.error}
                    <details class="gate-output-details" open>
                      <summary class="gate-output-label gate-error-label">Error</summary>
                      <pre class="gate-output gate-error">{gate.error}</pre>
                    </details>
                  {/if}
                  {#if gate.started_at}
                    <span class="gate-timing">{fmtDate(gate.started_at)}{#if gate.finished_at} — {fmtDate(gate.finished_at)}{/if}</span>
                  {/if}
                </li>
              {/each}
            </ul>
          {:else}
            <p class="no-data">No gates configured for this merge request</p>
          {/if}
        </div>

      {:else if activeTab === 'attestation'}
        <div class="tab-pane">
          {#if mrAttestationLoading}
            <div class="spec-skeleton">
              {#each Array(4) as _}<Skeleton width="100%" height="1.2rem" />{/each}
            </div>
          {:else if mrAttestation}
            {@const att = mrAttestation.attestation ?? mrAttestation}
            <div class="attestation-block">
              <div class="attestation-header">
                <Badge value="Signed" variant="success" />
                {#if att.attestation_version}
                  <span class="att-version">v{att.attestation_version}</span>
                {/if}
              </div>
              <dl class="entity-meta">
                {#if att.merge_commit_sha}
                  <dt>Merge commit</dt><dd class="mono copyable" title="Click to copy: {att.merge_commit_sha}" onclick={() => copyId(att.merge_commit_sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(att.merge_commit_sha); }}>{att.merge_commit_sha.slice(0, 12)}...</dd>
                {/if}
                {#if att.merged_at}
                  <dt>Merged at</dt><dd>{fmtDate(att.merged_at)}</dd>
                {/if}
                {#if att.spec_ref}
                  {@const attSpecPath = att.spec_ref.split('@')[0]}
                  <dt>Spec</dt><dd><button class="entity-link mono" title={att.spec_ref} onclick={() => navigateTo('spec', attSpecPath, { path: attSpecPath })}>{attSpecPath.split('/').pop()}</button></dd>
                {/if}
                {#if att.spec_fully_approved !== undefined}
                  <dt>Spec approved</dt><dd><Badge value={att.spec_fully_approved ? 'yes' : 'no'} variant={att.spec_fully_approved ? 'success' : 'warning'} /></dd>
                {/if}
                {#if att.author_agent_id}
                  <dt>Agent</dt><dd><button class="entity-link mono" title={att.author_agent_id} onclick={() => navigateTo('agent', att.author_agent_id)}>{entityName('agent', att.author_agent_id)}</button></dd>
                {/if}
                {#if att.mr_id}
                  <dt>MR</dt><dd><button class="entity-link mono" title={att.mr_id} onclick={() => navigateTo('mr', att.mr_id)}>{entityName('mr', att.mr_id)}</button></dd>
                {/if}
                {#if att.task_id}
                  <dt>Task</dt><dd><button class="entity-link" title={att.task_id} onclick={() => navigateTo('task', att.task_id)}>{entityName('task', att.task_id)}</button></dd>
                {/if}
                {#if att.repo_id}
                  <dt>Repo</dt><dd class="mono" title={att.repo_id}>{entityName('repo', att.repo_id)}</dd>
                {/if}
                {#if att.gate_results?.length > 0}
                  {@const passed = att.gate_results.filter(g => g.status === 'Passed' || g.status === 'passed').length}
                  {@const total = att.gate_results.length}
                  <dt>Gates</dt>
                  <dd>
                    <Badge value="{passed}/{total} passed" variant={passed === total ? 'success' : 'warning'} />
                  </dd>
                {/if}
              </dl>
              <!-- Expand attestation gate results -->
              {#if att.gate_results?.length > 0}
                <details class="att-gates-detail">
                  <summary class="progress-section-label">Gate Results ({att.gate_results.length})</summary>
                  <ul class="gates-list">
                    {#each att.gate_results as gate}
                      {@const gStatus = (gate.status === 'Passed' || gate.status === 'passed') ? 'passed' : (gate.status === 'Failed' || gate.status === 'failed') ? 'failed' : gate.status ?? 'unknown'}
                      <li class="gate-item gate-item-{gStatus}">
                        <div class="gate-row">
                          <span class="gate-status-icon">{gStatus === 'passed' ? '✓' : gStatus === 'failed' ? '✗' : '○'}</span>
                          <span class="gate-name">{gate.gate_name ?? gate.name ?? shortId(gate.gate_id)}</span>
                          {#if gate.gate_type}
                            <span class="gate-type-badge">{gate.gate_type.replace(/_/g, ' ')}</span>
                          {/if}
                          {#if gate.required !== undefined}
                            <span class="gate-required-badge" class:advisory={!gate.required}>
                              {gate.required ? 'required' : 'advisory'}
                            </span>
                          {/if}
                        </div>
                        {#if gate.output}
                          <pre class="gate-output">{gate.output}</pre>
                        {/if}
                      </li>
                    {/each}
                  </ul>
                </details>
              {/if}
              {#if mrAttestation.signature}
                <div class="att-sig-block">
                  <span class="att-sig-label">Ed25519 Signature</span>
                  <code class="att-sig-value mono copyable" title="Click to copy full signature" onclick={() => copyId(mrAttestation.signature)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(mrAttestation.signature); }}>{mrAttestation.signature.slice(0, 24)}...</code>
                </div>
              {/if}
            </div>
          {:else}
            <p class="no-data">No attestation bundle available for this merge request</p>
          {/if}
        </div>

      {:else if activeTab === 'timeline'}
        <div class="tab-pane">
          {#if mrTimelineLoading}
            <div class="spec-skeleton">
              {#each Array(5) as _}<Skeleton width="100%" height="1.5rem" />{/each}
            </div>
          {:else if Array.isArray(mrTimeline) && mrTimeline.length > 0}
            <div class="timeline-list">
              {#each mrTimeline as evt, i}
                {@const evtType = evt.event_type ?? evt.type ?? evt.event}
                {@const detailText = timelineDetailText(evt)}
                {@const prevTime = i > 0 ? (mrTimeline[i-1].timestamp ?? mrTimeline[i-1].created_at) : null}
                {@const thisTime = evt.timestamp ?? evt.created_at}
                {@const elapsed = (prevTime && thisTime) ? Math.round(thisTime - prevTime) : null}
                <div class="timeline-item">
                  <div class="timeline-connector">
                    <div class="timeline-dot timeline-dot-{timelineEventVariant(evtType)}"></div>
                    {#if i < mrTimeline.length - 1}<div class="timeline-line"></div>{/if}
                  </div>
                  <div class="timeline-content">
                    <div class="timeline-header">
                      <Badge value={timelineEventLabel(evtType)} variant={timelineEventVariant(evtType)} />
                      <span class="timeline-time">{fmtDate(thisTime)}</span>
                      {#if elapsed && elapsed > 0}
                        <span class="timeline-elapsed">+{elapsed < 60 ? elapsed + 's' : elapsed < 3600 ? Math.round(elapsed / 60) + 'm' : Math.round(elapsed / 3600) + 'h'}</span>
                      {/if}
                    </div>
                    {#if evt.actor || evt.actor_id || evt.agent_id}
                      <button class="entity-link timeline-actor mono" onclick={() => navigateTo('agent', evt.actor_id ?? evt.agent_id)} title={evt.actor_id ?? evt.agent_id}>
                        {evt.actor ?? entityName('agent', evt.actor_id ?? evt.agent_id)}
                      </button>
                    {/if}
                    {#if detailText}
                      <p class="timeline-detail">{detailText}</p>
                    {/if}
                    {#if evt.gate_name ?? evt.detail?.gate}
                      <button class="entity-link timeline-gate-ref mono" onclick={() => { activeTab = 'gates'; }} title="View gate details">{evt.gate_name ?? evt.detail?.gate}</button>
                    {/if}
                    {#if evt.sha || evt.commit_sha}
                      {@const sha = evt.sha ?? evt.commit_sha}
                      <span class="timeline-sha mono copyable" title="Click to copy: {sha}" onclick={() => copyId(sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(sha); }}>{sha.slice(0, 7)}</span>
                    {/if}
                    {#if evt.mr_id}
                      <button class="entity-link mono" onclick={() => navigateTo('mr', evt.mr_id)} title={evt.mr_id}>MR {shortId(evt.mr_id)}</button>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {:else}
            <p class="no-data">No timeline events for this merge request</p>
          {/if}
        </div>

      {:else if activeTab === 'reviews'}
        <div class="tab-pane">
          {#if mrReviewsLoading}
            <div class="spec-skeleton">
              {#each Array(3) as _}<Skeleton width="100%" height="2rem" />{/each}
            </div>
          {:else}
            {#if Array.isArray(mrReviews) && mrReviews.length > 0}
              <span class="progress-section-label">Reviews</span>
              <div class="reviews-list">
                {#each mrReviews as review}
                  <div class="review-item">
                    <div class="review-header">
                      <Badge
                        value={review.decision ?? review.status ?? 'review'}
                        variant={
                          (review.decision === 'approved' || review.status === 'approved') ? 'success' :
                          (review.decision === 'changes_requested' || review.status === 'changes_requested') ? 'danger' : 'info'
                        }
                      />
                      <span class="review-author mono">{review.reviewer ?? review.reviewer_agent_id ?? review.user_id ?? shortId(review.reviewer_id)}</span>
                      <span class="review-time">{fmtDate(review.created_at ?? review.timestamp)}</span>
                    </div>
                    {#if review.body}
                      <p class="review-body">{review.body}</p>
                    {/if}
                  </div>
                {/each}
              </div>
            {:else}
              <p class="no-data no-data-sm">No reviews yet</p>
            {/if}

            {#if Array.isArray(mrComments) && mrComments.length > 0}
              <span class="progress-section-label">Comments</span>
              <div class="reviews-list">
                {#each mrComments as comment}
                  <div class="review-item comment-item">
                    <div class="review-header">
                      <span class="review-author mono">{comment.author ?? comment.author_agent_id ?? comment.user_id ?? shortId(comment.author_id)}</span>
                      <span class="review-time">{fmtDate(comment.created_at ?? comment.timestamp)}</span>
                    </div>
                    {#if comment.body}
                      <p class="review-body">{comment.body}</p>
                    {/if}
                  </div>
                {/each}
              </div>
            {:else}
              <p class="no-data no-data-sm">No comments yet</p>
            {/if}

            <!-- Comment submission form -->
            <div class="comment-form">
              <span class="progress-section-label">Add Comment</span>
              <textarea
                class="comment-textarea"
                bind:value={newCommentText}
                placeholder="Write a comment..."
                rows="2"
                disabled={submittingComment}
              ></textarea>
              <div class="comment-form-actions">
                <Button variant="primary" size="sm" onclick={submitComment} disabled={!newCommentText.trim() || submittingComment}>
                  {submittingComment ? 'Posting...' : 'Comment'}
                </Button>
              </div>
            </div>

            <!-- Review submission form -->
            <div class="comment-form">
              <span class="progress-section-label">Submit Review</span>
              <div class="review-form-row">
                <select class="review-decision-select" bind:value={newReviewDecision}>
                  <option value="approved">Approve</option>
                  <option value="changes_requested">Request Changes</option>
                </select>
              </div>
              <textarea
                class="comment-textarea"
                bind:value={newReviewBody}
                placeholder="Review comment (optional)..."
                rows="2"
                disabled={submittingReview}
              ></textarea>
              <div class="comment-form-actions">
                <Button
                  variant={newReviewDecision === 'approved' ? 'primary' : 'secondary'}
                  size="sm"
                  onclick={submitReview}
                  disabled={submittingReview}
                >
                  {submittingReview ? 'Submitting...' : newReviewDecision === 'approved' ? 'Approve' : 'Request Changes'}
                </Button>
              </div>
            </div>
          {/if}
        </div>

      {:else if activeTab === 'trace'}
        <div class="tab-pane">
          {#if agentLogsLoading}
            <div class="spec-skeleton">
              {#each Array(5) as _}<Skeleton width="100%" height="1.5rem" />{/each}
            </div>
          {:else}
            <!-- Structured trace spans from MR trace -->
            {#if Array.isArray(agentTraceSpans) && agentTraceSpans.length > 0}
              <span class="progress-section-label">Execution Trace ({agentTraceSpans.length} spans)</span>
              <div class="trace-spans">
                {#each agentTraceSpans as span}
                  {@const durMs = (span.end_ms && span.start_ms) ? span.end_ms - span.start_ms : null}
                  <div class="trace-span" class:trace-span-root={!span.parent_span_id}>
                    <div class="trace-span-header">
                      <span class="trace-span-name">{span.name}</span>
                      {#if durMs != null}
                        <span class="trace-span-dur">{durMs < 1000 ? durMs + 'ms' : (durMs / 1000).toFixed(1) + 's'}</span>
                      {/if}
                    </div>
                    {#if span.attributes}
                      {@const attrs = typeof span.attributes === 'string' ? JSON.parse(span.attributes) : span.attributes}
                      <div class="trace-span-attrs">
                        {#each Object.entries(attrs).slice(0, 5) as [key, val]}
                          <span class="trace-span-attr"><span class="trace-attr-key">{key}:</span> <span class="trace-attr-val">{typeof val === 'object' ? JSON.stringify(val) : String(val)}</span></span>
                        {/each}
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}

            <!-- Raw log entries -->
            {#if Array.isArray(agentLogs) && agentLogs.length > 0}
              <span class="progress-section-label">Log Output ({agentLogs.length} entries)</span>
              <div class="trace-list">
                {#each agentLogs as entry}
                  <div class="trace-entry">
                    {#if entry.timestamp || entry.created_at}
                      <span class="trace-time">{fmtDate(entry.timestamp ?? entry.created_at)}</span>
                    {/if}
                    <span class="trace-msg">{entry.message ?? entry.content ?? entry.line ?? JSON.stringify(entry)}</span>
                  </div>
                {/each}
              </div>
            {:else if !agentTraceSpans?.length}
              <p class="no-data">No trace data available for this agent</p>
            {/if}
          {/if}
        </div>

      {:else if activeTab === 'activity'}
        <div class="tab-pane">
          {#if taskAgentsLoading || taskMrsLoading}
            <div class="spec-skeleton">
              {#each Array(4) as _}<Skeleton width="100%" height="1.5rem" />{/each}
            </div>
          {:else}
            <!-- Provenance summary -->
            {@const tk = taskDetail ?? entity.data ?? {}}
            {#if tk.spec_path || (taskAgents?.length > 0) || (taskMrs?.length > 0)}
              <div class="task-activity-summary">
                <span class="progress-section-label">Progress</span>
                <div class="provenance-flow">
                  {#if tk.spec_path}
                    <button class="provenance-node provenance-spec" onclick={() => navigateTo('spec', tk.spec_path, { path: tk.spec_path, repo_id: tk.repo_id })} title={tk.spec_path}>
                      <span class="provenance-type">Spec</span>
                      <span class="provenance-name">{tk.spec_path.split('/').pop()}</span>
                    </button>
                    <span class="provenance-arrow">→</span>
                  {/if}
                  <span class="provenance-node provenance-task provenance-current">
                    <span class="provenance-type">Task</span>
                    <span class="provenance-name">{tk.status ?? 'backlog'}</span>
                  </span>
                  {#if taskAgents?.length > 0}
                    <span class="provenance-arrow">→</span>
                    <span class="provenance-node provenance-agent">
                      <span class="provenance-type">Agents</span>
                      <span class="provenance-name">{taskAgents.length}</span>
                    </span>
                  {/if}
                  {#if taskMrs?.length > 0}
                    <span class="provenance-arrow">→</span>
                    <span class="provenance-node provenance-mr">
                      <span class="provenance-type">MRs</span>
                      <span class="provenance-name">{taskMrs.filter(m => m.status === 'merged').length}/{taskMrs.length} merged</span>
                    </span>
                  {/if}
                </div>
              </div>
            {/if}

            {#if Array.isArray(taskAgents) && taskAgents.length > 0}
              <span class="progress-section-label">Agents ({taskAgents.length})</span>
              <ul class="task-list">
                {#each taskAgents as agent}
                  {@const agNorm = normalizeAgent(agent)}
                  {@const dur = agNorm.completed_at && agNorm.created_at ? Math.round(agNorm.completed_at - agNorm.created_at) : null}
                  <li class="task-item clickable-row" onclick={() => navigateTo('agent', agent.id, agent)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('agent', agent.id, agent); }}>
                    <Badge value={agent.status ?? 'active'} variant={agent.status === 'active' ? 'success' : (agent.status === 'idle' || agent.status === 'completed') ? 'info' : agent.status === 'failed' ? 'danger' : 'muted'} />
                    <span class="task-title">{agent.name ?? shortId(agent.id)}</span>
                    {#if agent.branch}
                      <span class="task-agent mono">{agent.branch}</span>
                    {/if}
                    {#if dur}
                      <span class="task-duration">{dur < 60 ? dur + 's' : dur < 3600 ? Math.round(dur / 60) + 'm' : Math.round(dur / 3600) + 'h'}</span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="no-data no-data-sm">No agents assigned to this task</p>
            {/if}

            {#if Array.isArray(taskMrs) && taskMrs.length > 0}
              <span class="progress-section-label">Merge Requests ({taskMrs.length})</span>
              <ul class="task-list">
                {#each taskMrs as mr}
                  <li class="task-item clickable-row" onclick={() => navigateTo('mr', mr.id, mr)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('mr', mr.id, mr); }}>
                    <Badge value={mr.status ?? 'open'} variant={mr.status === 'merged' ? 'success' : mr.status === 'open' ? 'info' : 'muted'} />
                    <span class="task-title">{mr.title ?? shortId(mr.id)}</span>
                    {#if mr.source_branch}
                      <span class="task-agent mono">{mr.source_branch}</span>
                    {/if}
                    {#if mr.diff_stats}
                      <span class="diff-stat-compact">
                        <span class="diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                        <span class="diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                      </span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="no-data no-data-sm">No merge requests linked to this task</p>
            {/if}
          {/if}
        </div>

      {:else if activeTab === 'ask-why'}
        <div class="tab-pane ask-why">
          {#if entity.data?.conversation_sha}
            <button
              class="start-interrogation"
              onclick={startInterrogation}
              disabled={interrogationLoading}
              aria-describedby="ask-why-hint"
            >
              {interrogationLoading ? $t('detail_panel.ask_why_starting') : $t('detail_panel.ask_why_spawn')}
            </button>
            <p class="ask-why-hint" id="ask-why-hint">{$t('detail_panel.ask_why_hint')}</p>
            {#if interrogationAgentId}
              <a class="view-spawned-link" href="/explorer?detail=agent:{interrogationAgentId}">{$t('detail_panel.ask_why_view_agent')}</a>
            {/if}
          {:else}
            <p class="ask-why-unavailable">{$t('detail_panel.ask_why_unavailable')}</p>
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
    transition: width var(--transition-normal) ease-out, min-width var(--transition-normal) ease-out;
    flex-shrink: 0;
  }

  .detail-panel.open {
    width: 40%;
    min-width: 320px;
    max-width: 560px;
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
    gap: var(--space-1);
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
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .panel-back {
    flex-shrink: 0;
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

  .agent-caps {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    white-space: normal;
  }

  .cap-tag {
    display: inline-block;
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-primary-bg, rgba(59, 130, 246, 0.1));
    color: var(--color-primary, #3b82f6);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    border: 1px solid var(--color-primary-border, rgba(59, 130, 246, 0.2));
  }

  .cap-proto {
    background: var(--color-surface-alt, rgba(139, 92, 246, 0.1));
    color: var(--color-text-secondary, #8b5cf6);
    border-color: rgba(139, 92, 246, 0.2);
  }

  .copyable {
    cursor: pointer;
    transition: color var(--transition-fast);
    position: relative;
  }

  .copyable:hover {
    color: var(--color-primary);
    text-decoration: underline;
    text-decoration-style: dotted;
  }

  .copyable:active {
    color: var(--color-link);
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
    color: var(--color-text-inverse);
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
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .ask-why-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: var(--space-3) 0 0;
  }

  .view-spawned-link {
    font-size: var(--text-xs);
    color: var(--color-primary);
    text-decoration: underline;
    text-underline-offset: 2px;
    margin-top: var(--space-2);
    transition: opacity var(--transition-fast);
  }

  .view-spawned-link:hover {
    opacity: 0.8;
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
    padding-top: var(--space-1);
  }

  .spec-meta-list dd {
    margin: 0;
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .spec-approval-actions {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2) 0;
  }

  .approval-btn {
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .approval-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .approval-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .approval-btn.approve {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-success) 40%, transparent);
    color: var(--color-success);
  }

  .approval-btn.approve:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-success) 25%, transparent);
    border-color: var(--color-success);
  }

  .approval-btn.revoke {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 40%, transparent);
    color: var(--color-danger);
  }

  .approval-btn.revoke:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-danger) 25%, transparent);
    border-color: var(--color-danger);
  }

  .spec-content-box {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    background: var(--color-surface-elevated);
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
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .spec-editor-textarea:focus-visible {
    border-color: var(--color-focus);
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
    gap: var(--space-1);
  }

  .diff-badge {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    padding: var(--space-1);
    border-radius: var(--radius-sm);
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
    margin-bottom: var(--space-1);
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
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  .llm-textarea:focus-visible {
    border-color: var(--color-focus);
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
    color: var(--color-text-inverse);
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .llm-send:hover:not(:disabled) { background: var(--color-primary-hover); }
  .llm-send:disabled { opacity: 0.4; cursor: not-allowed; }

  .llm-send:focus-visible {
    outline: 2px solid var(--color-focus);
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
    font-size: var(--text-2xl);
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
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--color-success);
    border-radius: var(--radius-sm);
    transition: width var(--transition-slow);
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

  .task-priority {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    padding: 1px var(--space-1);
    border-radius: var(--radius-sm);
    flex-shrink: 0;
  }

  .task-duration {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  .task-activity-summary {
    margin-bottom: var(--space-3);
  }

  .task-priority.priority-high,
  .task-priority.priority-critical {
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
  }

  .task-priority.priority-low {
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
  }

  .progress-mrs {
    font-size: var(--text-xs);
    color: var(--color-success);
    margin-left: auto;
  }

  .progress-section-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
    margin-top: var(--space-2);
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

  .link-conflict {
    border-color: color-mix(in srgb, var(--color-danger) 30%, transparent);
    background: color-mix(in srgb, var(--color-danger) 5%, var(--color-surface-elevated));
  }

  .link-direction {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .link-full-path {
    font-size: 10px;
    color: var(--color-text-muted);
    opacity: 0.6;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .link-kind {
    font-size: var(--text-xs);
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

  .link-direction {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    flex-shrink: 0;
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

  .view-spawned-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .start-interrogation:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* ── Architecture tab (S2: spec detail mini canvas) ────────────────────────── */
  .arch-tab {
    gap: var(--space-3);
    padding: 0;
  }

  .arch-loading-wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
  }

  .arch-loading-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-align: center;
    margin: 0;
  }

  .arch-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-4);
  }

  .arch-error-msg {
    font-size: var(--text-sm);
    color: var(--color-danger);
    margin: 0;
    text-align: center;
  }

  .arch-mini-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .arch-mini-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .arch-predict-badge {
    font-size: var(--text-xs);
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent);
    border-radius: var(--radius-sm);
    padding: 1px var(--space-2);
  }

  .arch-canvas-container {
    flex: 1;
    min-height: 240px;
    overflow: hidden;
  }

  .arch-expand-wrap {
    display: flex;
    justify-content: flex-end;
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  /* ── MR Diff tab ─────────────────────────────────────────────────────────── */
  .diff-summary {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .diff-stat { font-weight: 600; color: var(--color-text); }
  .diff-stat-inline { font-size: var(--text-xs); color: var(--color-text-secondary); margin-right: var(--space-1); }
  .diff-ins { color: var(--color-success); font-family: var(--font-mono); font-size: var(--text-xs); }
  .diff-del { color: var(--color-danger); font-family: var(--font-mono); font-size: var(--text-xs); }

  /* ── Diff file tree (GitHub-style summary) ─────────────────────────────── */
  .diff-file-tree {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    padding: var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    margin-bottom: var(--space-2);
  }

  .diff-tree-item {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
  }

  .diff-tree-status {
    font-weight: 700;
    width: 12px;
    text-align: center;
  }

  .diff-tree-status-added { color: var(--color-success); }
  .diff-tree-status-deleted { color: var(--color-danger); }
  .diff-tree-status-modified { color: var(--color-info); }

  .diff-tree-path {
    color: var(--color-text-muted);
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .diff-no-patch {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    padding: var(--space-2) var(--space-3);
    margin: 0;
  }

  .diff-file-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .diff-file {
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .diff-file-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    cursor: pointer;
    list-style: none;
  }

  .diff-file-header::-webkit-details-marker { display: none; }
  .diff-file-header::marker { content: ''; }

  .diff-file[open] .diff-file-header {
    border-bottom: 1px solid var(--color-border);
  }

  .diff-file-header:hover {
    background: color-mix(in srgb, var(--color-surface-elevated) 80%, var(--color-border));
  }

  .diff-file-path {
    flex: 1;
    font-size: var(--text-xs);
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .diff-file-stats {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .diff-patch {
    margin: 0;
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--color-text);
    max-height: 400px;
    overflow-y: auto;
  }

  .diff-line { display: block; }
  .diff-line-add { display: block; background: color-mix(in srgb, var(--color-success) 12%, transparent); color: var(--color-success); }
  .diff-line-del { display: block; background: color-mix(in srgb, var(--color-danger) 12%, transparent); color: var(--color-danger); }
  .diff-line-hunk { display: block; color: var(--color-info); font-weight: 500; background: color-mix(in srgb, var(--color-info) 8%, transparent); }

  /* ── MR Gates tab ────────────────────────────────────────────────────────── */
  .gates-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .gate-item {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-left: 3px solid var(--color-border);
  }

  .gate-item-passed { border-left-color: var(--color-success, #22c55e); }
  .gate-item-failed { border-left-color: var(--color-danger, #ef4444); }
  .gate-item-running { border-left-color: var(--color-warning, #f59e0b); }

  .gate-status-icon {
    font-size: var(--text-sm);
    flex-shrink: 0;
    width: 1.2em;
    text-align: center;
  }

  .gate-item-passed .gate-status-icon { color: var(--color-success, #22c55e); }
  .gate-item-failed .gate-status-icon { color: var(--color-danger, #ef4444); }
  .gate-item-running .gate-status-icon { color: var(--color-warning, #f59e0b); }

  .gate-output-details summary {
    cursor: pointer;
    user-select: none;
  }

  .gate-error-label { color: var(--color-danger, #ef4444); }

  .gate-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .gate-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
    flex: 1;
  }

  .gate-duration {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .gate-cmd {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface);
    border-radius: var(--radius-sm);
    display: block;
  }

  .gate-output {
    margin: 0;
    padding: var(--space-2);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--color-text-muted);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    max-height: 150px;
    overflow-y: auto;
  }

  .gate-error { color: var(--color-danger); }

  .gate-type-badge {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: 1px var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
  }

  .gate-required-badge {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-danger) 12%, transparent);
    color: var(--color-danger);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .gate-required-badge.advisory {
    background: color-mix(in srgb, var(--color-text-muted) 12%, transparent);
    color: var(--color-text-muted);
    border-color: var(--color-border);
  }

  .gate-cmd-row,
  .gate-output-row {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .gate-cmd-label,
  .gate-output-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .gate-timing {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  /* ── MR Attestation tab ──────────────────────────────────────────────────── */
  .attestation-block {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .attestation-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .att-version {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .att-sig-block {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .att-sig-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .att-sig-value {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    word-break: break-all;
    line-height: 1.5;
  }

  /* ── Agent Trace tab ─────────────────────────────────────────────────────── */
  .trace-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .trace-entry {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2);
    border-bottom: 1px solid var(--color-border);
    font-size: var(--text-xs);
  }

  .trace-entry:last-child { border-bottom: none; }

  .trace-time {
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .trace-msg {
    color: var(--color-text);
    word-break: break-word;
  }

  /* ── Agent messages ─────────────────────────────────────────────── */
  .messages-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .message-item {
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface);
  }

  .message-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
    flex-wrap: wrap;
  }

  .message-sender {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .message-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .message-body {
    font-size: var(--text-sm);
    color: var(--color-text);
    margin: 0;
    word-break: break-word;
  }

  .message-form {
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }

  /* ── Trace spans ────────────────────────────────────────────────── */
  .trace-spans {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-bottom: var(--space-3);
  }

  .trace-span {
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    border-left: 3px solid var(--color-primary, #3b82f6);
    font-size: var(--text-xs);
  }

  .trace-span-root {
    border-left-color: var(--color-success, #22c55e);
    background: var(--color-surface-elevated);
  }

  .trace-span-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-2);
  }

  .trace-span-name {
    font-weight: 500;
    color: var(--color-text);
    font-family: var(--font-mono);
  }

  .trace-span-dur {
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  .trace-span-attrs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin-top: var(--space-1);
  }

  .trace-span-attr {
    font-size: 10px;
    color: var(--color-text-muted);
  }

  .trace-attr-key {
    color: var(--color-text-secondary);
    font-family: var(--font-mono);
  }

  .trace-attr-val {
    font-family: var(--font-mono);
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    display: inline-block;
    vertical-align: bottom;
  }

  /* ── Clickable entity links ─────────────────────────────────────────────── */
  .entity-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--color-primary);
    cursor: pointer;
    font: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
    text-decoration-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
    transition: color var(--transition-fast), text-decoration-color var(--transition-fast);
  }

  .entity-link:hover {
    color: var(--color-primary-hover, var(--color-primary));
    text-decoration-color: var(--color-primary);
  }

  .entity-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .entity-link.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .clickable-row {
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .clickable-row:hover {
    background: var(--color-surface-hover, color-mix(in srgb, var(--color-primary) 5%, transparent));
    border-color: color-mix(in srgb, var(--color-primary) 30%, transparent);
  }

  .clickable-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* ── Timeline tab ───────────────────────────────────────────────────────── */
  .timeline-list {
    display: flex;
    flex-direction: column;
  }

  .timeline-item {
    display: flex;
    gap: var(--space-3);
    min-height: 48px;
  }

  .timeline-connector {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 16px;
    flex-shrink: 0;
    padding-top: var(--space-2);
  }

  .timeline-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 2px solid var(--color-border-strong);
    background: var(--color-surface);
    flex-shrink: 0;
    z-index: 1;
  }

  .timeline-dot-success { border-color: var(--color-success); background: color-mix(in srgb, var(--color-success) 20%, transparent); }
  .timeline-dot-danger { border-color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 20%, transparent); }
  .timeline-dot-warning { border-color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 20%, transparent); }
  .timeline-dot-info { border-color: var(--color-info); background: color-mix(in srgb, var(--color-info) 20%, transparent); }

  .timeline-line {
    width: 2px;
    flex: 1;
    background: var(--color-border);
    margin-top: var(--space-1);
  }

  .timeline-content {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-1) 0 var(--space-3) 0;
    flex: 1;
    min-width: 0;
  }

  .timeline-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .timeline-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .timeline-elapsed {
    font-size: 10px;
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    opacity: 0.7;
  }

  .timeline-actor {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .timeline-detail {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.4;
  }

  .timeline-gate-ref {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: 1px var(--space-1);
    background: var(--color-surface-elevated);
    border-radius: var(--radius-sm);
    width: fit-content;
  }

  .timeline-sha {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: 1px var(--space-1);
    background: color-mix(in srgb, var(--color-info) 8%, transparent);
    border-radius: var(--radius-sm);
    width: fit-content;
  }

  /* ── Reviews tab ────────────────────────────────────────────────────────── */
  .reviews-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .review-item {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .review-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .review-author {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .review-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .review-body {
    font-size: var(--text-sm);
    color: var(--color-text);
    margin: 0;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .comment-item {
    border-left: 3px solid var(--color-border-strong);
  }

  .no-data-sm {
    padding: var(--space-2) 0;
    font-size: var(--text-xs);
  }

  .task-description {
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.5;
  }

  /* ── Status story (how did it get here) ──────────────────────────────── */
  .status-story {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-top: var(--space-1);
    flex-wrap: wrap;
  }

  .status-step {
    font-size: 10px;
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    white-space: nowrap;
  }

  .status-step-success { background: color-mix(in srgb, var(--color-success) 12%, transparent); color: var(--color-success); }
  .status-step-danger { background: color-mix(in srgb, var(--color-danger) 12%, transparent); color: var(--color-danger); }
  .status-step-warning { background: color-mix(in srgb, var(--color-warning) 12%, transparent); color: var(--color-warning); }
  .status-step-info { background: color-mix(in srgb, var(--color-info) 12%, transparent); color: var(--color-info); }

  .status-step-arrow {
    font-size: 9px;
    color: var(--color-text-muted);
    margin: 0 1px;
  }

  /* ── Agent container info ──────────────────────────────────────────────── */
  .agent-container-info,
  .agent-recent-logs {
    margin-top: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .trace-list-compact {
    max-height: 120px;
    overflow-y: auto;
  }

  /* ── Comment/Review form ─────────────────────────────────────────────────── */
  .comment-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
  }

  .comment-textarea {
    width: 100%;
    min-height: 48px;
    max-height: 120px;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    resize: vertical;
    box-sizing: border-box;
  }

  .comment-textarea:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    border-color: var(--color-focus);
  }

  .comment-textarea:disabled { opacity: 0.6; cursor: not-allowed; }

  .comment-form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .review-form-row {
    display: flex;
    gap: var(--space-2);
  }

  .review-decision-select {
    appearance: none;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-5) var(--space-1) var(--space-2);
    cursor: pointer;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12'%3E%3Cpath fill='%23888' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right var(--space-1) center;
    background-size: var(--space-3);
  }

  .review-decision-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── MR Actions ──────────────────────────────────────────────────────── */
  .mr-actions {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-3);
    padding-top: var(--space-3);
    border-top: 1px solid var(--color-border);
    align-items: center;
  }

  .queue-position {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .queue-pos-text {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .mr-merged-info {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
    font-size: var(--text-sm);
  }

  .sig-badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px var(--space-2);
    border-radius: var(--radius-full);
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-success) 30%, transparent);
    color: var(--color-success);
    font-size: var(--text-xs);
    font-weight: 500;
  }

  /* ── Gate summary bar ──────────────────────────────────────────────────── */
  .gate-summary-bar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    margin-top: var(--space-2);
  }

  .gate-summary-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .gate-summary-pills {
    display: flex;
    gap: var(--space-1);
  }

  .gate-pill {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 1px var(--space-2);
    border-radius: var(--radius-full);
  }

  .gate-pill-pass {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
  }

  .gate-pill-fail {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    color: var(--color-danger);
  }

  .gate-pill-pending {
    background: color-mix(in srgb, var(--color-text-muted) 15%, transparent);
    color: var(--color-text-muted);
  }

  /* ── Gate detail list (individual gate results) ─────────────────────────── */
  .gate-detail-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: var(--space-2);
  }

  .gate-detail-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    padding: 2px 0;
  }

  .gate-detail-item.gate-pass .gate-check { color: var(--color-success); }
  .gate-detail-item.gate-fail .gate-check { color: var(--color-danger); }
  .gate-check { font-weight: 600; width: 14px; text-align: center; }
  .gate-detail-name { font-weight: 500; }

  .gate-advisory-tag {
    font-size: 9px;
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-text-muted) 12%, transparent);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  /* ── Touched paths ─────────────────────────────────────────────────────── */
  .touched-paths-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .touched-path {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    padding: 2px 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .touched-path-more {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    padding: 2px 0;
  }

  /* ── MR deps section ───────────────────────────────────────────────────── */
  .mr-deps-section {
    margin-top: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  /* ── Provenance chain ─────────────────────────────────────────────────── */
  .provenance-chain {
    margin-top: var(--space-3);
    padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .provenance-label {
    display: block;
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    margin-bottom: var(--space-2);
  }

  .provenance-flow {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: wrap;
  }

  .provenance-node {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: var(--space-2);
    border-radius: var(--radius);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    cursor: pointer;
    font: inherit;
    transition: border-color var(--transition-fast), background var(--transition-fast);
    min-width: 56px;
  }

  button.provenance-node:hover {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 5%, transparent);
  }

  button.provenance-node:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .provenance-current {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    cursor: default;
  }

  .provenance-icon {
    width: 16px;
    height: 16px;
    display: inline-block;
  }

  .prov-icon-spec { background: var(--color-info); mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Cpath d='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z'/%3E%3Cpolyline points='14 2 14 8 20 8'/%3E%3C/svg%3E") center/contain no-repeat; -webkit-mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Cpath d='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z'/%3E%3Cpolyline points='14 2 14 8 20 8'/%3E%3C/svg%3E") center/contain no-repeat; }
  .prov-icon-task { background: var(--color-warning); mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Cpolyline points='9 11 12 14 22 4'/%3E%3Cpath d='M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11'/%3E%3C/svg%3E") center/contain no-repeat; -webkit-mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Cpolyline points='9 11 12 14 22 4'/%3E%3Cpath d='M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11'/%3E%3C/svg%3E") center/contain no-repeat; }
  .prov-icon-agent { background: var(--color-success); mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Ccircle cx='12' cy='12' r='3'/%3E%3Cpath d='M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z'/%3E%3C/svg%3E") center/contain no-repeat; -webkit-mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Ccircle cx='12' cy='12' r='3'/%3E%3Cpath d='M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z'/%3E%3C/svg%3E") center/contain no-repeat; }
  .prov-icon-mr { background: var(--color-primary); mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Ccircle cx='18' cy='18' r='3'/%3E%3Ccircle cx='6' cy='6' r='3'/%3E%3Cpath d='M6 21V9a9 9 0 0 0 9 9'/%3E%3C/svg%3E") center/contain no-repeat; -webkit-mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Ccircle cx='18' cy='18' r='3'/%3E%3Ccircle cx='6' cy='6' r='3'/%3E%3Cpath d='M6 21V9a9 9 0 0 0 9 9'/%3E%3C/svg%3E") center/contain no-repeat; }
  .prov-icon-code { background: var(--color-text-secondary); mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Cpolyline points='16 18 22 12 16 6'/%3E%3Cpolyline points='8 6 2 12 8 18'/%3E%3C/svg%3E") center/contain no-repeat; -webkit-mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='2'%3E%3Cpolyline points='16 18 22 12 16 6'/%3E%3Cpolyline points='8 6 2 12 8 18'/%3E%3C/svg%3E") center/contain no-repeat; }

  .provenance-type {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-muted);
  }

  .provenance-name {
    font-size: var(--text-xs);
    color: var(--color-text);
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: center;
  }

  .provenance-arrow {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }

  /* ── Spec history reason ────────────────────────────────────────────────── */
  .history-reason {
    font-size: var(--text-xs);
    color: var(--color-danger);
    margin: 0;
    padding: var(--space-1) var(--space-2);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
    border-radius: var(--radius-sm);
    line-height: 1.4;
  }
</style>
