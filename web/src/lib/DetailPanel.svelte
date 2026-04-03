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
  import { entityName as sharedEntityName, shortId as sharedShortId, formatSha, formatId as sharedFormatId } from './entityNames.svelte.js';
  import { relativeTime, absoluteTime, formatDuration, formatDate } from './timeFormat.js';
  import { mrStatusJourney } from './statusTooltips.js';
  import { toastSuccess, toastError } from './toast.svelte.js';
  import { detectLang, highlightLine } from './syntaxHighlight.js';
  import { renderMarkdown } from './markdown.js';

  const goToRepoTab = getContext('goToRepoTab') ?? null;
  const openDetailPanel = getContext('openDetailPanel') ?? null;
  const goToEntityDetailCtx = getContext('goToEntityDetail') ?? null;

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
    fullPage = false,
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
  let conversationData = $state(null);
  let conversationLoading = $state(false);

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
        { id: 'commits',     label: 'Commits' },
        { id: 'timeline',    label: 'Timeline' },
        { id: 'gates',       label: $t('detail_panel.tabs.gates') },
        { id: 'trace',       label: 'Trace' },
        { id: 'reviews',     label: 'Reviews' },
        { id: 'attestation', label: $t('detail_panel.tabs.attestation') },
      );
      // Always show ask-why for MRs — conversation_sha is loaded async from attestation
      result.push({
        id: 'ask-why',
        label: $t('detail_panel.tabs.ask_why'),
      });
      return result;
    }

    if (type === 'agent') {
      result.push(
        { id: 'chat',    label: $t('detail_panel.tabs.chat') },
        { id: 'history', label: 'Logs' },
        { id: 'trace',   label: $t('detail_panel.tabs.trace') },
      );
      // Always show ask-why for agents — conversation_sha is loaded async from agent detail
      result.push({
        id: 'ask-why',
        label: $t('detail_panel.tabs.ask_why'),
      });
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
      // Show ask-why if task has an assigned agent (conversation provenance)
      if (data.assigned_to) {
        result.push({ id: 'ask-why', label: $t('detail_panel.tabs.ask_why') });
      }
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
  // MRs default to 'info' (overview with provenance chain, status journey,
  // gates summary — more useful than raw diff for an autonomous platform).
  // Specs default to 'content'. Everything else defaults to first tab.
  $effect(() => {
    if (entity) {
      const freshTabs = computeTabs(entity);
      // Honor _openTab hint from caller (e.g. clicking gate badges → gates tab)
      const requestedTab = entity.data?._openTab;
      if (requestedTab && freshTabs.some(t => t.id === requestedTab)) {
        activeTab = requestedTab;
      } else if (entity.type === 'mr' && freshTabs.some(t => t.id === 'info')) {
        activeTab = 'info';
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

  // ── Spec preview for MR info tab ─────────────────────────────────────────────
  let mrSpecPreview = $state(null);
  let mrSpecPreviewLoading = $state(false);

  async function loadMrSpecPreview(specRef, repoId) {
    if (mrSpecPreview || mrSpecPreviewLoading) return;
    const specPath = specRef?.split('@')[0];
    if (!specPath) return;
    mrSpecPreviewLoading = true;
    try {
      const data = await api.specContent(specPath, repoId);
      mrSpecPreview = data;
    } catch {
      mrSpecPreview = null;
    } finally {
      mrSpecPreviewLoading = false;
    }
  }

  // ── Spec preview for task info tab ────────────────────────────────────────────
  let taskSpecPreview = $state(null);
  let taskSpecPreviewLoading = $state(false);

  async function loadTaskSpecPreview(specPath, repoId) {
    if (taskSpecPreview || taskSpecPreviewLoading) return;
    if (!specPath) return;
    taskSpecPreviewLoading = true;
    try {
      const data = await api.specContent(specPath, repoId);
      taskSpecPreview = data;
    } catch {
      taskSpecPreview = null;
    } finally {
      taskSpecPreviewLoading = false;
    }
  }

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
  let mrTrace = $state(null);
  let mrTraceLoading = $state(false);
  let mrReviews = $state(null);
  let mrReviewsLoading = $state(false);
  let mrComments = $state(null);
  let mrCommentsLoading = $state(false);
  let mrDeps = $state(null);
  let mrDepsLoading = $state(false);
  let mrCommits = $state(null);
  let mrCommitsLoading = $state(false);
  let newCommentText = $state('');
  let submittingComment = $state(false);
  let newReviewDecision = $state('approved');
  let newReviewBody = $state('');
  let submittingReview = $state(false);
  let newMessageText = $state('');
  let sendingMessage = $state(false);

  // Agent/task name cache for cross-references
  // Entity name cache is now a shared singleton in entityNames.svelte.js

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
  let agentLogFilter = $state('');
  let agentLogStreaming = $state(false);
  let agentLogStreamSource = null;
  let logListEl = $state(null);

  // Reset MR/agent/task data when entity changes
  $effect(() => {
    if (entity?.type === 'mr') {
      mrDetail = null;
      mrDiff = null;
      mrGates = null;
      mrAttestation = null;
      mrTimeline = null;
      mrTrace = null;
      mrReviews = null;
      mrComments = null;
      mrDeps = null;
      mrCommits = null;
      mrSpecPreview = null;
    }
    if (entity?.type === 'agent') {
      agentDetail = null;
      agentLogs = null;
      agentMessages = null;
      agentWorkload = null;
      agentLogFilter = '';
    }
    if (entity?.type === 'task') {
      taskDetail = null;
      taskAgents = null;
      taskMrs = null;
      taskSpecPreview = null;
    }
    conversationData = null;
    interrogationAgentId = null;
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
        // Extract merged_at from timeline if MR response doesn't include it
        if (d?.status === 'merged' && !d?.merged_at) {
          const mergedEvt = rawTimeline.find(evt => {
            const t = evt.event_type ?? evt.type ?? evt.event;
            return t === 'Merged' || t === 'merged';
          });
          if (mergedEvt?.timestamp) {
            mrDetail = { ...mrDetail, merged_at: mergedEvt.timestamp };
          }
        }
        // Resolve task_id via agent's current_task_id if MR lacks it
        const agentId = d?.author_agent_id ?? d?.agent_id;
        if (!d?.task_id && agentId) {
          try {
            const ag = await api.agent(agentId);
            const taskId = ag?.current_task_id ?? ag?.task_id;
            if (taskId) mrDetail = { ...mrDetail, task_id: taskId };
          } catch { /* best effort */ }
        }
        // Fetch repo gate definitions to enrich gate results with names/types
        const repoId = d?.repository_id ?? d?.repo_id;
        let gateDefs = [];
        if (repoId) {
          try { gateDefs = await api.repoGates(repoId); } catch { /* best effort */ }
        }
        const gateDefMap = Object.fromEntries((Array.isArray(gateDefs) ? gateDefs : []).map(g => [g.id, g]));
        // Pre-compute gate summary for info tab
        const gateList = Array.isArray(gates) ? gates : (gates?.gates ?? []);
        // Enrich gate results with definition data
        const enrichedGates = gateList.map(r => {
          const def = gateDefMap[r.gate_id] ?? {};
          return {
            ...r,
            name: r.gate_name ?? def.name ?? r.name,
            gate_type: r.gate_type ?? def.gate_type,
            required: r.required ?? def.required,
            command: r.command ?? def.command,
            _result_id: r.id,
          };
        });
        if (enrichedGates.length > 0) {
          const passed = enrichedGates.filter(g => g.status === 'Passed' || g.status === 'passed').length;
          const failed = enrichedGates.filter(g => g.status === 'Failed' || g.status === 'failed').length;
          const total = enrichedGates.length;
          const gateNames = enrichedGates.map(g => ({ name: g.name ?? 'Gate', status: g.status, required: g.required, gate_type: g.gate_type, output: g.output, error: g.error, command: g.command, duration_ms: g.duration_ms, started_at: g.started_at, finished_at: g.finished_at }));
          mrDetail = { ...mrDetail, _gateSummary: { passed, failed, total, gates: gateNames } };
        }
        // Pre-cache enriched gates for the gates tab
        if (!mrGates) {
          mrGates = enrichedGates;
        }
        // Fetch attestation for merged MRs to extract conversation_sha
        if (d?.status === 'merged') {
          api.mrAttestation(id).then(att => {
            if (att && !att.error) {
              // Enrich attestation gate results with names from repo gate definitions
              const attData = att.attestation ?? att;
              if (attData.gate_results?.length > 0 && gateDefMap) {
                attData.gate_results = attData.gate_results.map(g => {
                  const def = gateDefMap[g.gate_id] ?? {};
                  return { ...g, gate_name: g.gate_name ?? def.name, gate_type: g.gate_type ?? def.gate_type, required: g.required ?? def.required };
                });
              }
              mrAttestation = att;
              const convSha = attData.conversation_sha ?? att.conversation_sha;
              if (convSha) mrDetail = { ...mrDetail, conversation_sha: convSha };
            }
          }).catch(() => {});
        }
        // Fetch commit signature for merged MRs
        const mergeSha = d?.merge_commit_sha;
        if (repoId && mergeSha) {
          api.commitSignature(repoId, mergeSha).then(sig => {
            if (sig) mrDetail = { ...mrDetail, _commitSig: sig };
          }).catch(() => {});
        }
        // Fetch graph nodes for merged MRs (architecture impact)
        if (d?.status === 'merged' && repoId) {
          api.repoGraphTypes(repoId).then(nodes => {
            const arr = Array.isArray(nodes) ? nodes : (nodes?.nodes ?? []);
            if (arr.length > 0) {
              mrDetail = { ...mrDetail, _graphNodes: arr.slice(0, 20) };
            }
          }).catch(() => {});
        }
        // Build status story from timeline events, or fall back to MR fields
        if (rawTimeline.length > 0) {
          const events = rawTimeline.slice(-4).map(evt => {
            const evtType = evt.event_type ?? evt.type ?? evt.event;
            return { label: timelineEventLabel(evtType), variant: timelineEventVariant(evtType, evt), time: evt.timestamp ?? evt.created_at };
          });
          mrDetail = { ...mrDetail, _statusStory: events };
        } else {
          // Fallback: synthesize journey from MR fields so it's always visible
          const story = [{ label: 'Created', variant: 'info', time: d?.created_at }];
          if (enrichedGates.length > 0) {
            const failed = enrichedGates.filter(g => g.status === 'Failed' || g.status === 'failed').length;
            const passed = enrichedGates.filter(g => g.status === 'Passed' || g.status === 'passed').length;
            const total = enrichedGates.length;
            if (failed > 0) {
              story.push({ label: `${failed}/${total} gates failed`, variant: 'danger', time: null });
            } else if (passed === total && total > 0) {
              story.push({ label: `${total} gates passed`, variant: 'success', time: null });
            } else {
              story.push({ label: `Gates: ${passed}/${total}`, variant: 'warning', time: null });
            }
          }
          if (d?.queue_position != null) {
            story.push({ label: `Queued (#${d.queue_position + 1})`, variant: 'warning', time: null });
          }
          if (d?.status === 'merged') {
            story.push({ label: 'Merged', variant: 'success', time: d?.merged_at });
          } else if (d?.status === 'closed') {
            story.push({ label: 'Closed', variant: 'danger', time: d?.updated_at });
          } else {
            story.push({ label: 'Open', variant: 'info', time: null });
          }
          mrDetail = { ...mrDetail, _statusStory: story };
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
      const mr = mrDetail ?? entity.data ?? {};
      const mrStatus = mr.status ?? entity.data?.status;
      // Skip fetching attestation for open/non-merged MRs — it doesn't exist yet
      if (mrStatus && mrStatus !== 'merged') {
        mrAttestation = null;
        return;
      }
      mrAttestationLoading = true;
      const repoId = mr.repository_id ?? mr.repo_id ?? entity.data?.repository_id ?? entity.data?.repo_id;
      Promise.all([
        api.mrAttestation(id),
        repoId ? api.repoGates(repoId).catch(() => []) : Promise.resolve([]),
      ]).then(([d, defs]) => {
        // Ignore error responses like {error: "no attestation found..."}
        if (d && !d.error) {
          // Enrich attestation gate results with names from repo gate definitions
          const defMap = Object.fromEntries((Array.isArray(defs) ? defs : []).map(g => [g.id, g]));
          const att = d.attestation ?? d;
          if (att.gate_results?.length > 0) {
            att.gate_results = att.gate_results.map(g => {
              const def = defMap[g.gate_id] ?? {};
              return {
                ...g,
                gate_name: g.gate_name ?? def.name,
                gate_type: g.gate_type ?? def.gate_type,
                required: g.required ?? def.required,
                command: g.command ?? def.command,
              };
            });
          }
          mrAttestation = d;
        } else {
          mrAttestation = null;
        }
      })
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
    if (activeTab === 'trace' && !mrTrace && !mrTraceLoading) {
      mrTraceLoading = true;
      api.mrTrace(id)
        .then((d) => { mrTrace = d && !d.error ? d : null; })
        .catch(() => { mrTrace = null; })
        .finally(() => { mrTraceLoading = false; });
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
    if (activeTab === 'commits' && !mrCommits && !mrCommitsLoading) {
      mrCommitsLoading = true;
      const mr = mrDetail ?? entity.data ?? {};
      const repoId = mr.repository_id ?? mr.repo_id ?? entity.data?.repository_id ?? entity.data?.repo_id;
      const branch = mr.source_branch ?? entity.data?.source_branch;
      const agentId = mr.author_agent_id ?? entity.data?.author_agent_id;
      Promise.all([
        repoId && branch ? api.repoCommits(repoId, branch, 50).catch(() => []) : Promise.resolve([]),
        repoId ? api.repoAgentCommits(repoId).catch(() => []) : Promise.resolve([]),
      ]).then(([branchCommits, agentCommitRecords]) => {
        const commits = Array.isArray(branchCommits) ? branchCommits : [];
        const agentMap = {};
        (Array.isArray(agentCommitRecords) ? agentCommitRecords : []).forEach(ac => {
          if (ac.sha) agentMap[ac.sha] = ac;
          if (ac.commit_sha) agentMap[ac.commit_sha] = ac;
        });
        mrCommits = commits.map(c => ({
          ...c,
          _agentRecord: agentMap[c.sha] ?? agentMap[c.id] ?? null,
        }));
      }).catch(() => { mrCommits = []; })
        .finally(() => { mrCommitsLoading = false; });
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
        api.costsByAgent(id).catch(() => []),
      ]).then(async ([d, logs, container, workload, touchedPaths, card, costs]) => {
        const norm = normalizeAgent(d);
        const costEntries = Array.isArray(costs) ? costs : [];
        const totalTokens = costEntries.reduce((sum, c) => sum + (c.input_tokens ?? 0) + (c.output_tokens ?? 0), 0);
        const totalCost = costEntries.reduce((sum, c) => sum + (c.cost_usd ?? c.amount ?? 0), 0);
        const modelUsage = {};
        for (const c of costEntries) {
          const m = c.model ?? 'unknown';
          if (!modelUsage[m]) modelUsage[m] = { input: 0, output: 0, cost: 0 };
          modelUsage[m].input += c.input_tokens ?? 0;
          modelUsage[m].output += c.output_tokens ?? 0;
          modelUsage[m].cost += c.cost_usd ?? c.amount ?? 0;
        }
        const _costs = costEntries.length > 0 ? { totalTokens, totalCost, models: modelUsage, entries: costEntries } : null;
        agentDetail = norm ? { ...norm, _container: container, _workload: workload, _touchedPaths: touchedPaths, _card: card, _costs } : norm;
        // Pre-cache a few recent logs for the info view
        if (!agentLogs) agentLogs = Array.isArray(logs) ? logs : (logs?.logs ?? logs?.entries ?? []);
        // Resolve spec_path via task if available
        const taskId = norm?.task_id;
        if (taskId && !norm?.spec_path) {
          try {
            const task = await api.task(taskId);
            if (task?.spec_path) {
              agentDetail = { ...agentDetail, spec_path: task.spec_path, _taskTitle: task.title };
            }
          } catch { /* best effort */ }
        }
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

  // SSE live log streaming for active agents
  $effect(() => {
    const isActive = entity?.type === 'agent' && activeTab === 'history';
    const agentStatus = agentDetail?.status ?? entity?.data?.status;
    const isLive = agentStatus === 'active' || agentStatus === 'running' || agentStatus === 'spawning';

    if (isActive && isLive && entity?.id && typeof EventSource !== 'undefined') {
      const token = localStorage.getItem('gyre_token') ?? '';
      const url = api.agentLogStreamUrl(entity.id);
      const es = new EventSource(`${url}?token=${encodeURIComponent(token)}`);
      agentLogStreaming = true;
      agentLogStreamSource = es;

      es.onmessage = (evt) => {
        try {
          const parsed = JSON.parse(evt.data);
          const entry = typeof parsed === 'string' ? { message: parsed } : parsed;
          agentLogs = [...(agentLogs ?? []), entry];
          // Auto-scroll to bottom
          queueMicrotask(() => {
            if (logListEl) logListEl.scrollTop = logListEl.scrollHeight;
          });
        } catch {
          agentLogs = [...(agentLogs ?? []), { message: evt.data }];
        }
      };
      es.onerror = () => {
        agentLogStreaming = false;
        es.close();
        agentLogStreamSource = null;
      };

      return () => {
        es.close();
        agentLogStreamSource = null;
        agentLogStreaming = false;
      };
    } else {
      // Cleanup if conditions change
      if (agentLogStreamSource) {
        agentLogStreamSource.close();
        agentLogStreamSource = null;
        agentLogStreaming = false;
      }
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

  // Load conversation provenance for ask-why tab
  // Use enriched mrDetail or agentDetail as source, since conversation_sha is loaded asynchronously
  let convResolutionAttempted = $state(false);
  let effectiveConvSha = $derived(entity?.data?.conversation_sha ?? mrDetail?.conversation_sha ?? agentDetail?.conversation_sha ?? null);

  // Reset convResolutionAttempted when entity changes
  $effect(() => {
    if (entity) convResolutionAttempted = false;
  });

  $effect(() => {
    if (activeTab !== 'ask-why') return;
    const convSha = effectiveConvSha;

    // If no conversation_sha yet, try to resolve it from the attestation or agent
    if (!convSha && !conversationLoading && !convResolutionAttempted) {
      convResolutionAttempted = true;
      conversationLoading = true;

      (async () => {
        try {
          // For MRs: try attestation first
          if (entity?.type === 'mr') {
            const att = mrAttestation ?? await api.mrAttestation(entity.id).catch(() => null);
            if (!mrAttestation && att && !att.error) mrAttestation = att;
            const attConvSha = att?.attestation?.conversation_sha ?? att?.conversation_sha;
            if (attConvSha) {
              if (mrDetail) mrDetail = { ...mrDetail, conversation_sha: attConvSha };
              const data = await api.conversationProvenance(attConvSha).catch(() => null);
              conversationData = data;
              return;
            }
          }
          // Try via agent (works for MRs, agents, and tasks with assigned agents)
          const agentId = entity?.data?.author_agent_id ?? mrDetail?.author_agent_id ?? agentDetail?.id ?? (entity?.type === 'task' ? (taskDetail?.assigned_to ?? entity?.data?.assigned_to) : null);
          if (agentId) {
            const ag = agentDetail ?? await api.agent(agentId).catch(() => null);
            if (ag?.conversation_sha) {
              const data = await api.conversationProvenance(ag.conversation_sha).catch(() => null);
              conversationData = data;
              return;
            }
          }
          conversationData = null;
        } catch {
          conversationData = null;
        } finally {
          conversationLoading = false;
        }
      })();
      return;
    }

    if (!convSha || conversationData || conversationLoading) return;
    conversationLoading = true;
    api.conversationProvenance(convSha)
      .then((d) => { conversationData = d; })
      .catch(() => { conversationData = null; })
      .finally(() => { conversationLoading = false; });
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
    const reason = prompt('Rejection reason (required):', '');
    if (reason === null) return;
    if (!reason.trim()) {
      toastError('A rejection reason is required');
      return;
    }
    rejecting = true;
    try {
      await api.rejectSpec(path, reason.trim());
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
    // Normalize: strip leading "specs/" prefix — the API route already includes /specs/
    const path = (entity.id ?? '').replace(/^specs\//, '');
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
          // Preload spec history for rejected specs to show rejection reason banner
          const status = entity?.data?.approval_status;
          if ((status === 'rejected' || status === 'revoked') && !specHistory && !specHistoryLoading) {
            api.specHistoryRepo(path, repoId).then(h => {
              specHistory = Array.isArray(h) ? h : [];
            }).catch(() => {});
          }
          // Preload spec progress for journey visualization
          if (!specProgress && !specProgressLoading) {
            api.specProgress(path, repoId).then(p => {
              specProgress = p;
            }).catch(() => {});
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
          let currentEvent = '';
          for (const line of lines) {
            // Track SSE event type from `event:` lines
            if (line.startsWith('event:')) {
              currentEvent = line.slice(6).trim();
              continue;
            }
            if (!line.startsWith('data: ') && !line.startsWith('data:')) continue;
            const raw = line.startsWith('data: ') ? line.slice(6) : line.slice(5);
            if (raw === '[DONE]') { done = true; break; }
            try {
              const parsed = JSON.parse(raw);
              // Use event type from either SSE event: line or from JSON payload
              const evtType = parsed.event ?? parsed.type ?? currentEvent;
              if (evtType === 'partial' || (!evtType && parsed.text != null)) {
                llmExplanation += parsed.text ?? parsed.explanation ?? '';
              } else if (evtType === 'complete') {
                // Complete event: if it has a full text field, use it as the suggestion
                const fullText = parsed.text ?? parsed.explanation ?? llmExplanation;
                llmSuggestion = {
                  diff: parsed.diff ?? [],
                  explanation: fullText,
                };
                done = true; break;
              } else if (evtType === 'error') {
                throw new Error(parsed.message ?? 'LLM error');
              }
            } catch (pe) {
              if (pe.message && !pe.message.startsWith('Unexpected token')) throw pe;
            }
            currentEvent = '';
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
    if (s === 'review')      return 'info';
    if (s === 'blocked')     return 'danger';
    if (s === 'cancelled')   return 'muted';
    return 'neutral';
  }

  function fmtDate(ts) {
    if (!ts) return '—';
    const rel = relativeTime(ts);
    const abs = formatDate(ts);
    return rel ? `${rel} (${abs})` : abs;
  }

  /** Truncate a UUID/SHA to 8 chars for display. Full value shown in title. */
  function shortId(id) {
    if (!id) return '—';
    return sharedShortId(id);
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
    if (fullPage && goToEntityDetailCtx) {
      goToEntityDetailCtx(type, id, data ?? {});
    } else {
      openDetailPanel?.({ type, id, data: data ?? {} });
    }
  }

  // Entity name resolution uses shared singleton cache
  function entityName(type, id) {
    if (!id) return shortId(id);
    return sharedEntityName(type, id);
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
    const ag = agentDetail ?? entity.data ?? {};
    const wsId = ag.workspace_id;
    if (!wsId) {
      toastError('Agent has no workspace — cannot send message');
      return;
    }
    sendingMessage = true;
    try {
      await api.sendAgentMessage(wsId, entity.id, { content: newMessageText.trim(), kind: 'FreeText' });
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

  // ── Spawn agent for task ──────────────────────────────────────────────
  let spawnAgentLoading = $state(false);

  async function spawnAgentForTask() {
    if (!entity || spawnAgentLoading) return;
    const tk = taskDetail ?? entity.data ?? {};
    const repoId = tk.repo_id ?? tk.repository_id;
    if (!repoId) { toastError('Task has no repository — cannot spawn agent'); return; }
    spawnAgentLoading = true;
    try {
      const branchSlug = (tk.title ?? 'task').toLowerCase().replace(/[^a-z0-9]+/g, '-').slice(0, 30);
      const result = await api.spawnAgent({
        name: `agent-${branchSlug}`,
        repo_id: repoId,
        task_id: entity.id,
        branch: `feat/${branchSlug}`,
      });
      const agentId = result?.agent?.id;
      toastSuccess('Agent spawned for this task');
      // Update task to show the new agent
      if (agentId) {
        taskDetail = { ...tk, assigned_to: agentId };
        // Navigate to the new agent
        navigateTo('agent', agentId, result.agent);
      }
    } catch (e) {
      toastError('Failed to spawn agent: ' + (e.message ?? e));
    } finally {
      spawnAgentLoading = false;
    }
  }

  async function updateTaskStatusFromDetail(tk, newStatus) {
    try {
      await api.updateTaskStatus(entity.id, newStatus);
      toastSuccess(`Task → ${newStatus.replace(/_/g, ' ')}`);
      taskDetail = { ...tk, status: newStatus };
    } catch (e) {
      toastError('Failed to update: ' + (e.message ?? e));
    }
  }

  /** Explain spec approval status in human terms */
  function specStatusExplain(spec) {
    if (!spec?.approval_status) return '';
    switch (spec.approval_status) {
      case 'pending': return 'Awaiting human approval before agents can begin work';
      case 'approved': return 'Approved — agents can create tasks and implement this spec';
      case 'rejected': return 'Rejected — no further implementation should proceed';
      case 'draft': return 'Synced from repo but not yet submitted for approval';
      case 'deprecated': return 'No longer active — superseded by a newer spec';
      case 'implemented': return 'All linked tasks have been completed';
      default: return '';
    }
  }

  /** Explain task status in human terms */
  function taskStatusExplain(tk) {
    if (!tk?.status) return '';
    switch (tk.status) {
      case 'backlog': return 'Waiting to be picked up by an agent';
      case 'in_progress': return tk.assigned_to ? `Being worked on by ${entityName('agent', tk.assigned_to)}` : 'An agent is actively implementing this';
      case 'done': return 'Implementation complete — code has been submitted';
      case 'blocked': return tk.depends_on?.length ? `Blocked by ${tk.depends_on.length} dependency/ies` : 'Waiting for a dependency or human input';
      case 'cancelled': return 'Cancelled — the associated spec may have been rejected';
      case 'review': return 'Implementation submitted, awaiting review';
      default: return '';
    }
  }

  /** Explain MR status in human terms */
  function mrStatusExplain(mr) {
    if (!mr?.status) return '';
    switch (mr.status) {
      case 'open': {
        if (mr._gateSummary?.failed > 0) return `Blocked — ${mr._gateSummary.failed} gate(s) failed`;
        if (mr.queue_position) return `In merge queue at position #${mr.queue_position}`;
        if (mr.has_conflicts) return 'Has merge conflicts with the target branch';
        return 'Ready for review or to be enqueued for merge';
      }
      case 'merged': return mr.merge_commit_sha ? `Merged as ${mr.merge_commit_sha.slice(0, 7)}` : 'Successfully merged into the target branch';
      case 'closed': return 'Closed without merging — may have been superseded';
      case 'queued': return `Waiting in the merge queue — gates must pass before merge`;
      default: return '';
    }
  }

  /** Explain agent status in human terms — the "why" behind the status badge */
  function agentStatusExplain(ag) {
    if (!ag?.status) return '';
    const durStr = (() => {
      if (!ag.created_at) return '';
      const end = ag.completed_at ?? (Date.now() / 1000);
      const dur = Math.max(0, end - ag.created_at);
      if (dur < 60) return ` after ${Math.round(dur)}s`;
      if (dur < 3600) return ` after ${Math.floor(dur / 60)}m ${Math.round(dur % 60)}s`;
      return ` after ${Math.floor(dur / 3600)}h ${Math.round((dur % 3600) / 60)}m`;
    })();
    switch (ag.status) {
      case 'active': return ag.branch ? `Working on branch ${ag.branch}` : 'Actively executing its task';
      case 'idle': return ag.mr_id ? `Finished${durStr} — MR created for review` : `Finished${durStr}`;
      case 'completed': return ag.mr_id ? `Completed${durStr} — MR submitted` : `Completed${durStr}`;
      case 'failed': {
        const exit = ag._container?.exit_code;
        if (exit === 137) return `Killed${durStr} — out of memory or resource limit exceeded`;
        if (exit === 143) return `Terminated${durStr} by the system (SIGTERM)`;
        if (exit === 1) return `Failed${durStr} — see logs for details`;
        return `Failed${durStr} — see logs`;
      }
      case 'dead': return `Process died${durStr} — may have crashed or been killed`;
      case 'stopped': return `Stopped${durStr} by an operator or budget limit`;
      default: return '';
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
      'GateResult': 'Gate check',
      'enqueued': 'Enqueued for merge',
      'MergeQueueEnqueued': 'Enqueued for merge',
      'merged': 'Merged',
      'Merged': 'Merged to main',
      'closed': 'Closed',
      'review_submitted': 'Review submitted',
      'comment_added': 'Comment added',
      'graph_extracted': 'Graph extracted',
      'GraphDelta': 'Architecture updated',
      'GraphExtraction': 'Architecture extracted',
      'GitPush': 'Code pushed',
      'SpecLifecycleTrigger': 'Spec lifecycle triggered',
      'AgentSpawned': 'Agent spawned',
      'AgentCompleted': 'Agent completed',
      'ConversationTurn': 'Agent conversation',
      'attestation_created': 'Attestation signed',
    };
    return map[evt] ?? evt?.replace(/_/g, ' ') ?? 'Event';
  }

  function timelineEventVariant(evt, evtObj) {
    if (evt === 'merged' || evt === 'Merged' || evt === 'gate_passed' || evt === 'AgentCompleted') return 'success';
    if (evt === 'gate_failed' || evt === 'closed') return 'danger';
    if (evt === 'GateResult') {
      const status = evtObj?.detail?.status;
      if (status === 'pass' || status === 'passed') return 'success';
      if (status === 'fail' || status === 'failed') return 'danger';
      return 'warning';
    }
    if (evt?.startsWith('gate_')) return 'warning';
    if (evt === 'GraphDelta' || evt === 'graph_extracted' || evt === 'GraphExtraction') return 'info';
    if (evt === 'AgentSpawned' || evt === 'SpecLifecycleTrigger') return 'info';
    if (evt === 'MergeQueueEnqueued' || evt === 'enqueued') return 'warning';
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
    // Spec lifecycle events have {spec_path, task_id}
    if (detail.spec_path) {
      const specName = detail.spec_path.split('/').pop();
      return specName + (detail.task_id ? ' — task created' : '');
    }
    // GraphExtraction events have {commit_sha, nodes_added, nodes_modified}
    if (detail.nodes_added !== undefined || detail.nodes_modified !== undefined) {
      const parts = [];
      if (detail.nodes_added) parts.push(`+${detail.nodes_added} nodes`);
      if (detail.nodes_modified) parts.push(`~${detail.nodes_modified} modified`);
      if (detail.commit_sha) parts.push(`@ ${detail.commit_sha.slice(0, 7)}`);
      return parts.join(', ') || null;
    }
    // Agent events have {agent_id, commit_sha, files_changed, status, persona}
    if (detail.agent_id) {
      const parts = [];
      if (detail.persona) parts.push(detail.persona);
      if (detail.status) parts.push(detail.status);
      if (detail.commit_sha) parts.push(detail.commit_sha.slice(0, 7));
      if (detail.files_changed) parts.push(`${detail.files_changed} files`);
      return parts.length > 0 ? parts.join(', ') : null;
    }
    // MergeQueueEnqueued events have {position}
    if (detail.position !== undefined) {
      return `Queue position: #${detail.position + 1}`;
    }
    // Merged events with empty detail
    if (Object.keys(detail).length === 0) return null;
    // Generic fallback: format key-value pairs instead of raw JSON
    const parts = [];
    for (const [k, v] of Object.entries(detail)) {
      if (v === null || v === undefined) continue;
      // Truncate long UUIDs/SHAs
      const display = typeof v === 'string' && v.length > 20 ? v.slice(0, 8) + '...' : v;
      parts.push(`${k.replace(/_/g, ' ')}: ${display}`);
    }
    return parts.length > 0 ? parts.join(' · ') : null;
  }

  /** Format a log entry object into a human-readable string */
  function formatLogEntry(entry) {
    if (typeof entry === 'string') return entry;
    const msg = entry.message ?? entry.content ?? entry.line ?? entry.text;
    if (msg) return msg;
    // Format structured entries nicely instead of raw JSON
    const parts = [];
    for (const [k, v] of Object.entries(entry)) {
      if (k === 'timestamp' || k === 'created_at' || k === 'level' || k === 'id') continue;
      if (v === null || v === undefined) continue;
      parts.push(`${k}: ${typeof v === 'object' ? JSON.stringify(v) : v}`);
    }
    return parts.length > 0 ? parts.join(' | ') : JSON.stringify(entry);
  }

  /** Flatten structured hunk objects into a line array with computed line numbers */
  function computeHunkLines(hunks, fileStatus) {
    const result = [];
    for (const hunk of hunks) {
      // Parse hunk header for starting line numbers: @@ -old,count +new,count @@
      const hdrMatch = (hunk.header ?? '').match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      let oldLine = hdrMatch ? parseInt(hdrMatch[1], 10) : 1;
      let newLine = hdrMatch ? parseInt(hdrMatch[2], 10) : 1;
      // Detect new-file hunks: @@ -0,0 +N,... @@ means all lines are additions
      const isNewFile = (hdrMatch && parseInt(hdrMatch[1], 10) === 0) || (fileStatus ?? '').toLowerCase() === 'added';
      // Detect deleted-file hunks: @@ -N,... +0,0 @@ means all lines are deletions
      const isDeletedFile = (hdrMatch && parseInt(hdrMatch[2], 10) === 0) || (fileStatus ?? '').toLowerCase() === 'deleted';
      const hunkHeader = hunk.header ?? (hunk.old_start != null ? `@@ -${hunk.old_start},${hunk.old_count ?? 0} +${hunk.new_start ?? 1},${hunk.new_count ?? (hunk.lines?.length ?? 0)} @@` : '');
      if (hunkHeader) result.push({ type: 'hunk', header: hunkHeader });
      for (const line of (hunk.lines ?? [])) {
        // Fix misclassified lines: server may mark all lines as "context" for added/deleted files
        let lt = line.type === 'add' ? 'add' : line.type === 'delete' ? 'del' : 'ctx';
        if (lt === 'ctx' && isNewFile) lt = 'add';
        if (lt === 'ctx' && isDeletedFile) lt = 'del';
        if (lt === 'add') {
          result.push({ type: 'line', lineType: lt, oldNum: '', newNum: newLine, content: line.content });
          newLine++;
        } else if (lt === 'del') {
          result.push({ type: 'line', lineType: lt, oldNum: oldLine, newNum: '', content: line.content });
          oldLine++;
        } else {
          result.push({ type: 'line', lineType: lt, oldNum: oldLine, newNum: newLine, content: line.content });
          oldLine++;
          newLine++;
        }
      }
    }
    return result;
  }

  /** Parse unified diff patch into line objects with line numbers */
  function parsePatchLines(patch) {
    if (!patch) return [];
    const lines = patch.split('\n');
    let oldLine = 0;
    let newLine = 0;
    return lines.map(line => {
      const hunkMatch = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (hunkMatch) {
        oldLine = parseInt(hunkMatch[1], 10);
        newLine = parseInt(hunkMatch[2], 10);
        return { text: line, type: 'hunk', oldNum: '', newNum: '' };
      }
      if (line.startsWith('+')) {
        const result = { text: line, type: 'add', oldNum: '', newNum: newLine };
        newLine++;
        return result;
      }
      if (line.startsWith('-')) {
        const result = { text: line, type: 'del', oldNum: oldLine, newNum: '' };
        oldLine++;
        return result;
      }
      // Context line
      const result = { text: line, type: 'ctx', oldNum: oldLine, newNum: newLine };
      if (line.length > 0 || oldLine > 0) { oldLine++; newLine++; }
      return result;
    });
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class={fullPage ? 'detail-page' : 'detail-panel'}
  class:expanded={!fullPage && expanded}
  class:open={!!entity}
  role={fullPage ? undefined : 'dialog'}
  aria-label={fullPage ? undefined : $t('detail_panel.title')}
  aria-modal={!fullPage && expanded ? 'true' : undefined}
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
        <span class="entity-type">{entity.type === 'mr' ? 'Merge Request' : entity.type === 'spec' ? 'Specification' : entity.type === 'agent' ? 'Agent' : entity.type === 'task' ? 'Task' : entity.type === 'node' ? 'Architecture Node' : entity.type === 'commit' ? 'Commit' : entity.type}</span>
        <span class="entity-id">{entity.data?.name ?? entity.data?.title ?? (entity.type === 'spec' ? (entity.id?.split('/').pop()?.replace(/\.md$/, '') ?? entity.id) : entity.type === 'commit' ? ((entity.data?.sha ?? entity.id ?? '').slice(0, 7)) : entityName(entity.type, entity.id))}</span>
      </div>
      <div class="panel-actions">
        {#if !fullPage}
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
        {/if}
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
              <!-- Prominent status journey block — click steps to see details -->
              {#if mr._statusStory?.length > 0}
                <div class="mr-status-journey">
                  <div class="status-journey-track">
                    {#each mr._statusStory as step, i}
                      {@const stepTab = step.label?.toLowerCase().includes('gate') ? 'gates' : step.label?.toLowerCase().includes('merged') ? 'attestation' : step.label?.toLowerCase().includes('created') ? 'timeline' : step.label?.toLowerCase().includes('commit') ? 'commits' : null}
                      <button
                        class="status-journey-node status-journey-node-{step.variant}"
                        class:status-journey-clickable={stepTab}
                        title={stepTab ? `Click to view ${stepTab}${step.time ? ' — ' + absoluteTime(step.time) : ''}` : (step.time ? absoluteTime(step.time) : '')}
                        onclick={() => { if (stepTab) activeTab = stepTab; }}
                      >
                        <span class="status-journey-dot"></span>
                        <span class="status-journey-label">{step.label}</span>
                        {#if step.time}
                          <span class="status-journey-time">{relativeTime(step.time) || formatDate(step.time)}</span>
                        {/if}
                      </button>
                      {#if i < mr._statusStory.length - 1}
                        <span class="status-journey-connector"></span>
                      {/if}
                    {/each}
                  </div>
                  {#if mr.merge_commit_sha}
                    <div class="status-journey-sha">
                      <code class="sha-badge mono copyable" title="Click to copy: {mr.merge_commit_sha}" onclick={() => copyId(mr.merge_commit_sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(mr.merge_commit_sha); }}>{mr.merge_commit_sha.slice(0, 7)}</code>
                    </div>
                  {/if}
                </div>
              {/if}

              <!-- Prominent diff stats banner -->
              {#if mr.diff_stats}
                <button class="mr-diff-stats-banner" onclick={() => { activeTab = 'diff'; }} title="View full diff">
                  <span class="diff-stats-banner-files">{mr.diff_stats.files_changed ?? 0} file{(mr.diff_stats.files_changed ?? 0) === 1 ? '' : 's'} changed</span>
                  <span class="diff-stats-banner-ins">+{mr.diff_stats.insertions ?? 0}</span>
                  <span class="diff-stats-banner-del">-{mr.diff_stats.deletions ?? 0}</span>
                </button>
              {/if}

              <!-- Agent summary (from attestation) -->
              {#if mrAttestation?.attestation?.completion_summary ?? mrAttestation?.completion_summary}
                <div class="mr-agent-summary">
                  <p class="mr-agent-summary-text">{mrAttestation.attestation?.completion_summary ?? mrAttestation.completion_summary}</p>
                </div>
              {/if}

              <!-- Status Journey Stepper -->
              {@const journey = mrStatusJourney(mr)}
              {#if journey.length > 1}
                <div class="status-journey">
                  {#each journey as step, i}
                    <div class="journey-step journey-step-{step.status}">
                      <span class="journey-dot">{step.status === 'done' ? '✓' : step.status === 'failed' ? '✗' : step.status === 'active' ? '●' : '○'}</span>
                      <span class="journey-label">{step.step}</span>
                      {#if step.detail}<span class="journey-detail">{step.detail}</span>{/if}
                    </div>
                    {#if i < journey.length - 1}
                      <span class="journey-connector" class:journey-connector-done={step.status === 'done'}></span>
                    {/if}
                  {/each}
                </div>
              {/if}

              <dl class="entity-meta">
                <dt>Title</dt><dd>{mr.title ?? '—'}</dd>
                <dt>Status</dt>
                <dd>
                  <Badge value={mr.status ?? 'unknown'} variant={mr.status === 'merged' ? 'success' : mr.status === 'open' ? 'info' : 'muted'} />
                  <span class="status-explain">{mrStatusExplain(mr)}</span>
                </dd>
                <dt>ID</dt><dd class="mono copyable" title="Click to copy: {entity.id}" onclick={() => copyId(entity.id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(entity.id); }}>{sharedFormatId('mr', entity.id)}</dd>
                {#if mr.description}
                  <dt>Description</dt><dd class="task-description">{mr.description}</dd>
                {/if}
                {#if mr.source_branch}
                  <dt>Branch</dt><dd class="mono">{mr.source_branch} → {mr.target_branch ?? 'main'}</dd>
                {/if}
                {#if mr.diff_stats}
                  <dt>Changes</dt>
                  <dd>
                    <button class="entity-link diff-stat-link" onclick={() => { activeTab = 'diff'; }} title="View diff">
                      <span class="diff-stat-inline">{mr.diff_stats.files_changed ?? 0} files</span>
                      <span class="diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                      <span class="diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                    </button>
                  </dd>
                {/if}
                {#if mr.spec_ref}
                  {@const specPath = mr.spec_ref.split('@')[0]}
                  {@const specSha = mr.spec_ref.includes('@') ? mr.spec_ref.split('@')[1] : null}
                  <dt>Spec</dt>
                  <dd>
                    <button class="entity-link" title={mr.spec_ref} onclick={() => navigateTo('spec', specPath, { path: specPath, repo_id: mr.repository_id ?? mr.repo_id })}>{specPath.split('/').pop()?.replace(/\.md$/, '')}</button>
                    {#if specSha}<code class="sha-badge mono" title="Pinned at spec version {specSha}">@{specSha.slice(0, 7)}</code>{/if}
                  </dd>
                {/if}
                {#if mr.author_agent_id}
                  <dt>Agent</dt><dd><button class="entity-link mono" title={mr.author_agent_id} onclick={() => navigateTo('agent', mr.author_agent_id)}>{entityName('agent', mr.author_agent_id)}</button></dd>
                {:else if mr.agent_id}
                  <dt>Agent</dt><dd><button class="entity-link mono" title={mr.agent_id} onclick={() => navigateTo('agent', mr.agent_id)}>{entityName('agent', mr.agent_id)}</button></dd>
                {/if}
                {#if mr.repository_id ?? mr.repo_id}
                  <dt>Repo</dt><dd class="mono copyable" title="Click to copy: {mr.repository_id ?? mr.repo_id}" onclick={() => copyId(mr.repository_id ?? mr.repo_id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(mr.repository_id ?? mr.repo_id); }}>{entityName('repo', mr.repository_id ?? mr.repo_id)}</dd>
                {/if}
                {#if mr.author_id && mr.author_id !== mr.author_agent_id}
                  <dt>Author</dt><dd class="mono copyable" title="Click to copy: {mr.author_id}" onclick={() => copyId(mr.author_id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(mr.author_id); }}>{mr.author_id === 'human-reviewer' || mr.author_id === 'system' ? mr.author_id : entityName('agent', mr.author_id)}</dd>
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
                      <button class="provenance-node provenance-spec" onclick={() => navigateTo('spec', specPath, { path: specPath, repo_id: mr.repository_id ?? mr.repo_id })} title="Open spec: {specPath}">
                        <span class="provenance-icon prov-icon-spec"></span>
                        <span class="provenance-type">Spec</span>
                        <span class="provenance-name">{specPath.split('/').pop()?.replace(/\.md$/, '') ?? specPath}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    {#if mr.task_id}
                      <button class="provenance-node provenance-task" onclick={() => navigateTo('task', mr.task_id)} title="Open task: {entityName('task', mr.task_id)}">
                        <span class="provenance-icon prov-icon-task"></span>
                        <span class="provenance-type">Task</span>
                        <span class="provenance-name">{entityName('task', mr.task_id)}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    {#if agentId}
                      <button class="provenance-node provenance-agent" onclick={() => navigateTo('agent', agentId)} title="Open agent: {entityName('agent', agentId)}">
                        <span class="provenance-icon prov-icon-agent"></span>
                        <span class="provenance-type">Agent</span>
                        <span class="provenance-name">{entityName('agent', agentId)}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    <span class="provenance-node provenance-mr provenance-current">
                      <span class="provenance-icon prov-icon-mr"></span>
                      <span class="provenance-type">MR</span>
                      <span class="provenance-name">{mr.title ?? mr.status ?? 'open'}</span>
                    </span>
                    {#if mr.status === 'merged'}
                      <span class="provenance-arrow">&#x2192;</span>
                      <span class="provenance-node provenance-code">
                        <span class="provenance-icon prov-icon-code"></span>
                        <span class="provenance-type">Merged</span>
                        <span class="provenance-name">{mr.diff_stats ? `+${mr.diff_stats.insertions ?? 0} -${mr.diff_stats.deletions ?? 0}` : (mr.merged_at ? relativeTime(mr.merged_at) || formatDate(mr.merged_at) : 'merged')}</span>
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
                        {@const duration = (gate.started_at && gate.finished_at) ? Math.round((gate.finished_at - gate.started_at) * 1000) : gate.duration_ms}
                        <button class="gate-detail-item" class:gate-pass={passed} class:gate-fail={failed} onclick={() => { activeTab = 'gates'; }} title="View gate details">
                          <span class="gate-check">{passed ? '✓' : failed ? '✗' : '○'}</span>
                          <span class="gate-detail-name">{gate.gate_name ?? gate.name ?? (gate.gate_type ? gate.gate_type.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase()) : (gate.command ? gate.command.split(' ')[0].split('/').pop() : 'Quality Gate'))}</span>
                          {#if gate.gate_type}<span class="gate-type-tag">{gate.gate_type.replace(/_/g, ' ')}</span>{/if}
                          {#if gate.required === false}<span class="gate-advisory-tag">advisory</span>{/if}
                          {#if duration}<span class="gate-duration-tag">{duration < 1000 ? duration + 'ms' : (duration / 1000).toFixed(1) + 's'}</span>{/if}
                          {#if gate.command}<span class="gate-cmd-tag mono">{gate.command}</span>{/if}
                        </button>
                        {#if failed && (gate.error || gate.output)}
                          <div class="gate-inline-error">
                            <pre class="gate-inline-error-text">{(gate.error || gate.output || '').slice(0, 300)}{(gate.error || gate.output || '').length > 300 ? '...' : ''}</pre>
                          </div>
                        {/if}
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}

              <!-- Spec Preview (collapsible) -->
              {#if mr.spec_ref}
                {@const previewSpecPath = mr.spec_ref.split('@')[0]}
                <details class="spec-preview-section" ontoggle={(e) => { if (e.target.open) loadMrSpecPreview(mr.spec_ref, mr.repository_id ?? mr.repo_id); }}>
                  <summary class="spec-preview-summary">
                    <span class="progress-section-label">Spec: {previewSpecPath.split('/').pop()}</span>
                  </summary>
                  <div class="spec-preview-body">
                    {#if mrSpecPreviewLoading}
                      <Skeleton width="100%" height="60px" />
                    {:else if mrSpecPreview?.content}
                      <div class="spec-content-rendered spec-preview-rendered">{@html renderMarkdown(mrSpecPreview.content)}</div>
                    {:else}
                      <p class="no-data no-data-sm">Spec content not available. <button class="entity-link" onclick={() => navigateTo('spec', previewSpecPath, { path: previewSpecPath, repo_id: mr.repository_id ?? mr.repo_id })}>Open spec →</button></p>
                    {/if}
                  </div>
                </details>
              {/if}

              <!-- Dependencies -->
              {#if mrDeps && ((mrDeps.depends_on?.length ?? 0) > 0 || (mrDeps.dependents?.length ?? 0) > 0)}
                <div class="mr-deps-section">
                  {#if mrDeps.depends_on?.length > 0}
                    <span class="progress-section-label">Depends on ({mrDeps.depends_on.length})</span>
                    <p class="deps-explain">This MR cannot merge until these are merged first</p>
                    <ul class="task-list">
                      {#each mrDeps.depends_on as depId}
                        <li class="task-item clickable-row" onclick={() => navigateTo('mr', depId)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('mr', depId); }}>
                          <span class="dep-arrow dep-arrow-in">←</span>
                          <span class="task-title">{entityName('mr', depId)}</span>
                          <span class="dep-id mono">{sharedFormatId('mr', depId)}</span>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                  {#if mrDeps.dependents?.length > 0}
                    <span class="progress-section-label">Blocks ({mrDeps.dependents.length})</span>
                    <p class="deps-explain">These MRs are waiting for this one to merge</p>
                    <ul class="task-list">
                      {#each mrDeps.dependents as depId}
                        <li class="task-item clickable-row" onclick={() => navigateTo('mr', depId)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('mr', depId); }}>
                          <span class="dep-arrow dep-arrow-out">→</span>
                          <span class="task-title">{entityName('mr', depId)}</span>
                          <span class="dep-id mono">{sharedFormatId('mr', depId)}</span>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              {/if}

              <!-- MR Quick Actions -->
              <div class="mr-quick-links">
                <button class="mr-explore-btn" onclick={() => { activeTab = 'diff'; }} title="View code changes">View Diff</button>
                <button class="mr-explore-btn" onclick={() => { activeTab = 'gates'; }} title="View quality gate results">View Gates</button>
                <button class="mr-explore-btn" onclick={() => { activeTab = 'timeline'; }} title="View full event timeline">Timeline</button>
                {#if mr.status === 'merged'}
                  <button class="mr-explore-btn" onclick={() => { activeTab = 'attestation'; }} title="View signed merge attestation">Attestation</button>
                {/if}
                <button class="mr-explore-btn" onclick={() => { activeTab = 'ask-why'; }} title="Explore agent reasoning">Ask Why</button>
              </div>

              <!-- Architecture Impact (merged MRs) -->
              {#if mr.status === 'merged' && mr._graphNodes?.length > 0}
                <details class="spec-preview-section">
                  <summary class="spec-preview-summary">
                    <span class="progress-section-label">Architecture Impact ({mr._graphNodes.length} types extracted)</span>
                  </summary>
                  <div class="graph-impact-list">
                    {#each mr._graphNodes as node}
                      <button class="graph-impact-node" onclick={() => { if (goToRepoTab) { goToRepoTab('architecture', { nodeId: node.id ?? node.name }); close(); } }} title="View in architecture explorer">
                        <span class="graph-impact-type">{node.node_type ?? 'Type'}</span>
                        <span class="graph-impact-name mono">{node.name ?? node.qualified_name}</span>
                        {#if node.file_path}<span class="graph-impact-file">{node.file_path}</span>{/if}
                      </button>
                    {/each}
                  </div>
                </details>
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
                    <code class="sha-badge mono copyable" title="Click to copy: {mr.merge_commit_sha}" onclick={() => copyId(mr.merge_commit_sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(mr.merge_commit_sha); }}>{mr.merge_commit_sha.slice(0, 7)}</code>
                  {/if}
                  {#if mr._commitSig}
                    <span class="sig-badge" title="Commit signed with {mr._commitSig.algorithm ?? 'unknown'}">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12" aria-hidden="true"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
                      signed
                    </span>
                  {/if}
                  <button class="mr-explore-btn" onclick={() => { activeTab = 'diff'; }} title="View code changes in this merge request">
                    View Diff
                  </button>
                  {#if goToRepoTab}
                    <button class="mr-explore-btn" onclick={() => { goToRepoTab('code', { subTab: 'provenance' }); close(); }} title="View agent provenance for this code">
                      View Provenance
                    </button>
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
              {#if ag.status && ag.status !== 'pending'}
                {@const phases = (() => {
                  const p = [{ label: 'Spawned', variant: 'info', time: ag.created_at }];
                  p.push({ label: 'Active', variant: 'success', time: ag.created_at });
                  if (ag._touchedPaths) {
                    const paths = Array.isArray(ag._touchedPaths) ? ag._touchedPaths : (ag._touchedPaths?.paths ?? ag._touchedPaths?.files ?? []);
                    if (paths.length > 0) p.push({ label: `${paths.length} files`, variant: 'warning', time: null });
                  }
                  if (ag.status !== 'active') {
                    p.push({ label: ag.status === 'completed' ? 'Completed' : ag.status === 'failed' ? 'Failed' : ag.status === 'dead' ? 'Dead' : ag.status === 'idle' ? 'Idle' : ag.status === 'stopped' ? 'Stopped' : ag.status, variant: ag.status === 'completed' ? 'success' : ag.status === 'idle' ? 'info' : ag.status === 'failed' || ag.status === 'dead' ? 'danger' : 'muted', time: ag.completed_at });
                  }
                  if (ag.mr_id) p.push({ label: 'MR Created', variant: 'success', time: null });
                  return p;
                })()}
                <div class="mr-status-journey">
                  <div class="status-journey-track">
                    {#each phases as step, i}
                      <div class="status-journey-node status-journey-node-{step.variant}" title={step.time ? fmtDate(step.time) : ''}>
                        <span class="status-journey-dot"></span>
                        <span class="status-journey-label">{step.label}</span>
                        {#if step.time}
                          <span class="status-journey-time">{fmtDate(step.time)}</span>
                        {/if}
                      </div>
                      {#if i < phases.length - 1}
                        <span class="status-journey-connector"></span>
                      {/if}
                    {/each}
                  </div>
                </div>
              {/if}
              <!-- Agent provenance chain -->
              {#if ag.spec_path || ag.task_id || ag.mr_id}
                <div class="provenance-chain">
                  <span class="provenance-label">Provenance</span>
                  <div class="provenance-flow">
                    {#if ag.spec_path}
                      <button class="provenance-node provenance-spec" onclick={() => navigateTo('spec', ag.spec_path, { path: ag.spec_path, repo_id: ag.repo_id })} title={ag.spec_path}>
                        <span class="provenance-icon prov-icon-spec"></span>
                        <span class="provenance-type">Spec</span>
                        <span class="provenance-name">{ag.spec_path.split('/').pop()?.replace(/\.md$/, '')}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    {#if ag.task_id}
                      <button class="provenance-node provenance-task" onclick={() => navigateTo('task', ag.task_id)} title={ag._taskTitle ?? ag.task_id}>
                        <span class="provenance-icon prov-icon-task"></span>
                        <span class="provenance-type">Task</span>
                        <span class="provenance-name">{ag._taskTitle ?? entityName('task', ag.task_id)}</span>
                      </button>
                      <span class="provenance-arrow">&#x2192;</span>
                    {/if}
                    <span class="provenance-node provenance-agent provenance-current">
                      <span class="provenance-icon prov-icon-agent"></span>
                      <span class="provenance-type">Agent</span>
                      <span class="provenance-name">{ag.name ?? sharedFormatId('agent', entity.id)}</span>
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

              <!-- Prominent cost/token summary -->
              {#if ag._costs && (ag._costs.totalTokens > 0 || ag._costs.totalCost > 0)}
                <div class="agent-cost-summary">
                  <span class="cost-summary-item">
                    <span class="cost-summary-value">{ag._costs.totalTokens.toLocaleString()}</span>
                    <span class="cost-summary-label">tokens</span>
                  </span>
                  {#if ag._costs.totalCost > 0}
                    <span class="cost-summary-item">
                      <span class="cost-summary-value">${ag._costs.totalCost.toFixed(4)}</span>
                      <span class="cost-summary-label">cost</span>
                    </span>
                  {/if}
                  {#if Object.keys(ag._costs.models ?? {}).length > 0}
                    <span class="cost-summary-item">
                      <span class="cost-summary-value">{Object.keys(ag._costs.models).length}</span>
                      <span class="cost-summary-label">{Object.keys(ag._costs.models).length === 1 ? 'model' : 'models'}</span>
                    </span>
                  {/if}
                </div>
              {/if}

              <dl class="entity-meta">
                <dt>Name</dt><dd>{ag.name ?? sharedFormatId('agent', entity.id)}</dd>
                <dt>Status</dt>
                <dd>
                  <Badge value={ag.status ?? 'unknown'} variant={ag.status === 'active' ? 'success' : ag.status === 'idle' || ag.status === 'completed' ? 'info' : ag.status === 'failed' || ag.status === 'dead' ? 'danger' : ag.status === 'stopped' ? 'muted' : 'muted'} />
                  <span class="status-explain">{agentStatusExplain(ag)}</span>
                </dd>
                <dt>ID</dt><dd class="mono copyable" title="Click to copy: {entity.id}" onclick={() => copyId(entity.id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(entity.id); }}>{sharedFormatId('agent', entity.id)}</dd>
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
                  <dt>Task</dt><dd><button class="entity-link" title={ag._taskTitle ?? ag.task_id} onclick={() => navigateTo('task', ag.task_id)}>{ag._taskTitle ?? entityName('task', ag.task_id)}</button></dd>
                {/if}
                {#if ag.spec_path}
                  <dt>Spec</dt><dd><button class="entity-link mono" title={ag.spec_path} onclick={() => navigateTo('spec', ag.spec_path, { path: ag.spec_path, repo_id: ag.repo_id })}>{ag.spec_path.split('/').pop()}</button></dd>
                {/if}
                {#if ag.repo_id}
                  <dt>Repo</dt><dd class="mono copyable" title="Click to copy: {ag.repo_id}" onclick={() => copyId(ag.repo_id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(ag.repo_id); }}>{entityName('repo', ag.repo_id)}</dd>
                {/if}
                {#if ag.mr_id}
                  <dt>MR</dt><dd><button class="entity-link mono" title={ag.mr_id} onclick={() => navigateTo('mr', ag.mr_id)}>{entityName('mr', ag.mr_id)}</button></dd>
                {/if}
                {#if ag.workspace_id}
                  <dt>Workspace</dt><dd class="mono copyable" title="Click to copy: {ag.workspace_id}" onclick={() => copyId(ag.workspace_id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(ag.workspace_id); }}>{entityName('workspace', ag.workspace_id)}</dd>
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
                    {#if ag._workload.alive !== undefined}
                      <dt>Status</dt>
                      <dd>
                        {#if ag._workload.alive}
                          <Badge value="Live" variant="success" />
                        {:else}
                          <Badge value="Exited" variant="muted" />
                          {#if ag._container?.exit_code !== undefined && ag._container?.exit_code !== null}
                            <span class="exit-code-explain">(code {ag._container.exit_code}{ag._container.exit_code === 0 ? ' — clean' : ag._container.exit_code === 137 ? ' — killed (OOM/SIGKILL)' : ag._container.exit_code === 143 ? ' — terminated (SIGTERM)' : ag._container.exit_code === 1 ? ' — error' : ''})</span>
                          {/if}
                        {/if}
                      </dd>
                    {/if}
                    {#if ag._workload.compute_target}
                      <dt>Running on</dt><dd>{ag._workload.compute_target}</dd>
                    {/if}
                    {#if ag._workload.hostname}
                      <dt>Hostname</dt><dd class="mono">{ag._workload.hostname}</dd>
                    {/if}
                    {#if ag._workload.pid}
                      <dt>Process</dt><dd class="mono">PID {ag._workload.pid}</dd>
                    {/if}
                    {#if ag._workload.attested_at}
                      <dt>Last Heartbeat</dt><dd>{fmtDate(ag._workload.attested_at)}</dd>
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
                        {@const filePath = typeof p === 'string' ? p : (p.path ?? p.file ?? JSON.stringify(p))}
                        {@const fileStatus = typeof p === 'object' ? (p.status ?? p.change_type ?? 'modified') : 'modified'}
                        {@const statusLower = fileStatus.toLowerCase()}
                        {@const isDeleted = statusLower === 'deleted' || statusLower === 'removed'}
                        {#if goToRepoTab && !isDeleted}
                          <button class="touched-path touched-path-link mono" onclick={() => { goToRepoTab('code', { subTab: 'files', file: filePath }); close(); }} title="View blame for {filePath}">
                            <span class="touched-path-status touched-path-status-{statusLower === 'added' || statusLower === 'created' || statusLower === 'new' ? 'added' : 'modified'}">{statusLower === 'added' || statusLower === 'created' || statusLower === 'new' ? '+' : '~'}</span>
                            {filePath}
                          </button>
                        {:else}
                          <span class="touched-path mono">
                            <span class="touched-path-status touched-path-status-{isDeleted ? 'deleted' : statusLower === 'added' || statusLower === 'created' || statusLower === 'new' ? 'added' : 'modified'}">{isDeleted ? '-' : statusLower === 'added' || statusLower === 'created' || statusLower === 'new' ? '+' : '~'}</span>
                            {filePath}
                          </span>
                        {/if}
                      {/each}
                      {#if paths.length > 10}
                        <span class="touched-path-more">+{paths.length - 10} more files</span>
                      {/if}
                    </div>
                  </div>
                {/if}
              {/if}

              <!-- LLM Usage & Cost -->
              {#if ag._costs}
                <div class="agent-container-info">
                  <span class="progress-section-label">LLM Usage</span>
                  {#if Object.keys(ag._costs.models).length > 0}
                    <table class="cost-table">
                      <thead>
                        <tr>
                          <th>Model</th>
                          <th class="cost-col-right">Input</th>
                          <th class="cost-col-right">Output</th>
                          <th class="cost-col-right">Cost</th>
                        </tr>
                      </thead>
                      <tbody>
                        {#each Object.entries(ag._costs.models) as [model, usage]}
                          <tr>
                            <td class="mono">{model}</td>
                            <td class="cost-col-right">{usage.input.toLocaleString()}</td>
                            <td class="cost-col-right">{usage.output.toLocaleString()}</td>
                            <td class="cost-col-right mono">{usage.cost > 0 ? `$${usage.cost.toFixed(4)}` : '—'}</td>
                          </tr>
                        {/each}
                      </tbody>
                      <tfoot>
                        <tr class="cost-total-row">
                          <td>Total</td>
                          <td class="cost-col-right" colspan="2">{ag._costs.totalTokens.toLocaleString()} tokens</td>
                          <td class="cost-col-right mono">{ag._costs.totalCost > 0 ? `$${ag._costs.totalCost.toFixed(4)}` : '—'}</td>
                        </tr>
                      </tfoot>
                    </table>
                  {:else}
                    <dl class="entity-meta">
                      <dt>Total Tokens</dt><dd>{ag._costs.totalTokens.toLocaleString()}</dd>
                      {#if ag._costs.totalCost > 0}
                        <dt>Total Cost</dt><dd>${ag._costs.totalCost.toFixed(4)}</dd>
                      {/if}
                    </dl>
                  {/if}
                </div>
              {/if}

              <!-- Agent Quick Actions -->
              <div class="mr-quick-links">
                <button class="mr-explore-btn" onclick={() => { activeTab = 'chat'; }} title="Send messages to this agent">Chat</button>
                <button class="mr-explore-btn" onclick={() => { activeTab = 'history'; }} title="View execution logs">Logs</button>
                <button class="mr-explore-btn" onclick={() => { activeTab = 'trace'; }} title="View execution trace">Trace</button>
                <button class="mr-explore-btn" onclick={() => { activeTab = 'ask-why'; }} title="Explore agent reasoning and decisions">Ask Why</button>
              </div>

              <!-- Conversation provenance link -->
              {#if ag.conversation_sha}
                <div class="agent-container-info">
                  <span class="progress-section-label">Decision Trail</span>
                  <p class="conv-sha-info">
                    This agent's reasoning was recorded.
                    <button class="entity-link" onclick={() => { activeTab = 'ask-why'; }} title="View the agent's conversation and decision-making process">View Conversation →</button>
                  </p>
                  <code class="sha-badge mono copyable" title="Click to copy conversation SHA: {ag.conversation_sha}" onclick={() => copyId(ag.conversation_sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(ag.conversation_sha); }}>{ag.conversation_sha.slice(0, 12)}...</code>
                </div>
              {/if}

              <!-- Recent logs preview -->
              {#if Array.isArray(agentLogs) && agentLogs.length > 0}
                <div class="agent-recent-logs">
                  <span class="progress-section-label">Recent Activity ({agentLogs.length} entries)</span>
                  <div class="trace-list trace-list-compact">
                    {#each agentLogs.slice(0, 3) as entry}
                      <div class="trace-entry">
                        {#if entry.timestamp || entry.created_at}
                          <span class="trace-time">{fmtDate(entry.timestamp ?? entry.created_at)}</span>
                        {/if}
                        <span class="trace-msg">{formatLogEntry(entry)}</span>
                      </div>
                    {/each}
                  </div>
                  {#if agentLogs.length > 3}
                    <button class="view-all-logs-btn" onclick={() => { activeTab = 'history'; }}>View all {agentLogs.length} log entries →</button>
                  {/if}
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
              <!-- Status journey visualization -->
              {#if tk.status && tk.status !== 'backlog'}
                {@const taskPhases = (() => {
                  const p = [{ label: 'Created', variant: 'info', time: tk.created_at }];
                  if (tk.assigned_to) p.push({ label: 'Assigned', variant: 'warning', time: null });
                  if (tk.status === 'in_progress' || tk.status === 'review' || tk.status === 'done') {
                    p.push({ label: 'In Progress', variant: 'warning', time: null });
                  }
                  if (tk.status === 'review') p.push({ label: 'Review', variant: 'info', time: null });
                  if (tk.status === 'done') p.push({ label: 'Done', variant: 'success', time: tk.updated_at });
                  if (tk.status === 'blocked') p.push({ label: 'Blocked', variant: 'danger', time: null });
                  if (tk.status === 'cancelled') p.push({ label: 'Cancelled', variant: 'muted', time: null });
                  return p;
                })()}
                <div class="mr-status-journey">
                  <div class="status-journey-track">
                    {#each taskPhases as step, i}
                      <div class="status-journey-node status-journey-node-{step.variant}" title={step.time ? fmtDate(step.time) : ''}>
                        <span class="status-journey-dot"></span>
                        <span class="status-journey-label">{step.label}</span>
                        {#if step.time}
                          <span class="status-journey-time">{fmtDate(step.time)}</span>
                        {/if}
                      </div>
                      {#if i < taskPhases.length - 1}
                        <span class="status-journey-connector"></span>
                      {/if}
                    {/each}
                  </div>
                </div>
              {/if}
              <!-- Prominent spec link banner -->
              {#if tk.spec_path}
                <div class="task-spec-banner">
                  <button class="task-spec-banner-link" onclick={() => navigateTo('spec', tk.spec_path, { path: tk.spec_path, repo_id: tk.repo_id })} title="Open spec: {tk.spec_path}">
                    <span class="task-spec-banner-icon">&#128196;</span>
                    <span class="task-spec-banner-name">{tk.spec_path.split('/').pop()?.replace(/\.md$/, '')}</span>
                    <span class="task-spec-banner-arrow">&#x2192;</span>
                  </button>
                </div>
              {/if}
              <dl class="entity-meta">
                <dt>Title</dt><dd>{tk.title ?? '—'}</dd>
                <dt>Status</dt>
                <dd>
                  <Badge value={tk.status ?? 'unknown'} variant={taskStatusColor(tk.status)} />
                  <span class="status-explain">{taskStatusExplain(tk)}</span>
                </dd>
                <dt>ID</dt><dd class="mono copyable" title="Click to copy: {entity.id}" onclick={() => copyId(entity.id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(entity.id); }}>{sharedFormatId('task', entity.id)}</dd>
                {#if tk.priority}
                  <dt>Priority</dt>
                  <dd>
                    <span class="priority-indicator priority-{tk.priority}">
                      {#if tk.priority === 'critical'}&#x26A0;{:else if tk.priority === 'high'}&#x2191;{:else if tk.priority === 'low'}&#x2193;{:else}&#x2022;{/if}
                    </span>
                    <Badge value={tk.priority} variant={tk.priority === 'high' || tk.priority === 'critical' ? 'danger' : tk.priority === 'low' ? 'muted' : 'warning'} />
                  </dd>
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
                  <dt>Repo</dt><dd class="mono copyable" title="Click to copy: {tk.repo_id}" onclick={() => copyId(tk.repo_id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(tk.repo_id); }}>{entityName('repo', tk.repo_id)}</dd>
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

              <!-- Spec preview (collapsible) -->
              {#if tk.spec_path}
                <details class="spec-preview-section" ontoggle={(e) => { if (e.target.open) loadTaskSpecPreview(tk.spec_path, tk.repo_id); }}>
                  <summary class="spec-preview-summary">
                    <span class="progress-section-label">Spec: {tk.spec_path.split('/').pop()}</span>
                  </summary>
                  <div class="spec-preview-body">
                    {#if taskSpecPreviewLoading}
                      <Skeleton width="100%" height="60px" />
                    {:else if taskSpecPreview?.content}
                      <div class="spec-content-rendered spec-preview-rendered">{@html renderMarkdown(taskSpecPreview.content)}</div>
                    {:else}
                      <p class="no-data no-data-sm">Spec content not available. <button class="entity-link" onclick={() => navigateTo('spec', tk.spec_path, { path: tk.spec_path, repo_id: tk.repo_id })}>Open spec →</button></p>
                    {/if}
                  </div>
                </details>
              {/if}

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

              <!-- Task actions -->
              <div class="mr-actions">
                <!-- Status transitions -->
                {#if tk.status === 'backlog'}
                  <Button variant="primary" size="sm" onclick={() => updateTaskStatusFromDetail(tk, 'in_progress')}>Start</Button>
                {:else if tk.status === 'in_progress'}
                  <Button variant="primary" size="sm" onclick={() => updateTaskStatusFromDetail(tk, 'done')}>Mark Done</Button>
                  <Button variant="secondary" size="sm" onclick={() => updateTaskStatusFromDetail(tk, 'blocked')}>Block</Button>
                {:else if tk.status === 'blocked'}
                  <Button variant="primary" size="sm" onclick={() => updateTaskStatusFromDetail(tk, 'in_progress')}>Unblock</Button>
                {:else if tk.status === 'review'}
                  <Button variant="primary" size="sm" onclick={() => updateTaskStatusFromDetail(tk, 'done')}>Approve</Button>
                  <Button variant="secondary" size="sm" onclick={() => updateTaskStatusFromDetail(tk, 'in_progress')}>Needs Work</Button>
                {:else if tk.status === 'done'}
                  <Button variant="secondary" size="sm" onclick={() => updateTaskStatusFromDetail(tk, 'in_progress')}>Reopen</Button>
                {/if}
                <!-- Spawn agent for unassigned tasks -->
                {#if !tk.assigned_to && tk.repo_id && (tk.status === 'backlog' || tk.status === 'in_progress')}
                  <Button variant="primary" onclick={spawnAgentForTask} disabled={spawnAgentLoading}>
                    {spawnAgentLoading ? 'Spawning...' : 'Spawn Agent'}
                  </Button>
                {/if}
              </div>

              <!-- Quick view: linked agents and MRs (loaded eagerly) -->
              {#if !taskAgents && !taskAgentsLoading}
                <!-- Trigger activity data load for preview -->
                {(() => {
                  const wsId = tk.workspace_id;
                  const rId = tk.repo_id ?? tk.repository_id;
                  const tId = entity.id;
                  queueMicrotask(() => {
                    if (taskAgents || taskAgentsLoading) return;
                    taskAgentsLoading = true;
                    taskMrsLoading = true;
                    Promise.all([
                      api.agents({ workspaceId: wsId, repoId: rId }).then(list => {
                        const all = Array.isArray(list) ? list : [];
                        return all.filter(a => (a.task_id ?? a.current_task_id) === tId);
                      }).catch(() => []),
                      api.mergeRequests(rId ? { repository_id: rId } : {}).then(list => {
                        const all = Array.isArray(list) ? list : [];
                        return all.filter(m => m.task_id === tId);
                      }).catch(() => []),
                    ]).then(([agents, mrs]) => {
                      taskAgents = agents;
                      taskMrs = mrs;
                    }).finally(() => { taskAgentsLoading = false; taskMrsLoading = false; });
                  });
                  return '';
                })()}
              {/if}
              {#if Array.isArray(taskAgents) && taskAgents.length > 0}
                <div class="task-linked-entities">
                  <span class="progress-section-label">Agents ({taskAgents.length})</span>
                  <ul class="task-list">
                    {#each taskAgents.slice(0, 3) as agent}
                      <li class="task-item clickable-row" onclick={() => navigateTo('agent', agent.id, agent)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('agent', agent.id, agent); }}>
                        <Badge value={agent.status ?? 'active'} variant={agent.status === 'active' ? 'success' : (agent.status === 'idle' || agent.status === 'completed') ? 'info' : agent.status === 'failed' ? 'danger' : 'muted'} />
                        <span class="task-title">{agent.name ?? sharedFormatId('agent', agent.id)}</span>
                        {#if agent.branch}<span class="task-agent mono">{agent.branch}</span>{/if}
                      </li>
                    {/each}
                    {#if taskAgents.length > 3}
                      <li class="task-item"><button class="view-all-logs-btn" onclick={() => { activeTab = 'activity'; }}>View all {taskAgents.length} agents →</button></li>
                    {/if}
                  </ul>
                </div>
              {/if}
              {#if Array.isArray(taskMrs) && taskMrs.length > 0}
                <div class="task-linked-entities">
                  <span class="progress-section-label">Merge Requests ({taskMrs.length})</span>
                  <ul class="task-list">
                    {#each taskMrs.slice(0, 3) as mr}
                      <li class="task-item clickable-row" onclick={() => navigateTo('mr', mr.id, mr)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('mr', mr.id, mr); }}>
                        <Badge value={mr.status ?? 'open'} variant={mr.status === 'merged' ? 'success' : mr.status === 'open' ? 'info' : 'muted'} />
                        <span class="task-title">{mr.title ?? sharedFormatId('mr', mr.id)}</span>
                        {#if mr.diff_stats}
                          <span class="diff-stat-compact">
                            <span class="diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                            <span class="diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                          </span>
                        {/if}
                      </li>
                    {/each}
                    {#if taskMrs.length > 3}
                      <li class="task-item"><button class="view-all-logs-btn" onclick={() => { activeTab = 'activity'; }}>View all {taskMrs.length} MRs →</button></li>
                    {/if}
                  </ul>
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
              {#if c.spec_ref}
                {@const commitSpecPath = c.spec_ref.split('@')[0]}
                <dt>Spec</dt><dd><button class="entity-link mono" title={c.spec_ref} onclick={() => navigateTo('spec', commitSpecPath, { path: commitSpecPath })}>{commitSpecPath.split('/').pop()}</button></dd>
              {/if}
              {#if c.branch}
                <dt>Branch</dt><dd class="mono">{c.branch}</dd>
              {/if}
              {#if c.parents?.length > 0}
                <dt>Parents</dt><dd class="mono">{c.parents.map(p => p.slice(0, 7)).join(', ')}</dd>
              {/if}
              {#if c.conversation_sha}
                <dt>Conversation</dt><dd><code class="sha-badge mono copyable" title="Click to copy: {c.conversation_sha}" onclick={() => copyId(c.conversation_sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(c.conversation_sha); }}>{c.conversation_sha.slice(0, 7)}</code></dd>
              {/if}
            </dl>

            <!-- Provenance chain -->
            {#if c.spec_ref || c.agent_id}
              <div class="provenance-chain">
                <span class="provenance-label">Provenance</span>
                <div class="provenance-flow">
                  {#if c.spec_ref}
                    {@const commitSpecPath2 = c.spec_ref.split('@')[0]}
                    <button class="provenance-node provenance-spec" onclick={() => navigateTo('spec', commitSpecPath2, { path: commitSpecPath2 })} title={c.spec_ref}>
                      <span class="provenance-icon prov-icon-spec"></span>
                      <span class="provenance-type">Spec</span>
                      <span class="provenance-name">{commitSpecPath2.split('/').pop()}</span>
                    </button>
                    <span class="provenance-arrow">&#x2192;</span>
                  {/if}
                  {#if c.agent_id}
                    <button class="provenance-node provenance-agent" onclick={() => navigateTo('agent', c.agent_id)} title={c.agent_id}>
                      <span class="provenance-icon prov-icon-agent"></span>
                      <span class="provenance-type">Agent</span>
                      <span class="provenance-name">{entityName('agent', c.agent_id)}</span>
                    </button>
                    <span class="provenance-arrow">&#x2192;</span>
                  {/if}
                  <span class="provenance-node provenance-code provenance-current">
                    <span class="provenance-icon prov-icon-code"></span>
                    <span class="provenance-type">Commit</span>
                    <span class="provenance-name">{sha.slice(0, 7)}</span>
                  </span>
                </div>
              </div>
            {/if}

            <!-- Investigate: spawn interrogation agent from this commit's context -->
            {#if c.agent_id && c.conversation_sha}
              <div class="commit-investigate">
                <button
                  class="investigate-btn"
                  onclick={startInterrogation}
                  disabled={interrogationLoading}
                  title="Spawn an agent with this commit's conversation context to investigate decisions"
                >
                  {interrogationLoading ? 'Spawning...' : 'Investigate this commit'}
                </button>
                <p class="investigate-hint">Resume the agent conversation that produced this commit</p>
                {#if interrogationAgentId}
                  <button class="entity-link" onclick={() => navigateTo('agent', interrogationAgentId)}>View spawned agent →</button>
                {/if}
              </div>
            {/if}
          {:else}
            <dl class="entity-meta">
              <dt>{$t('detail_panel.type')}</dt><dd>{entity.type}</dd>
              <dt>{$t('detail_panel.id')}</dt><dd class="mono" title={entity.id}>{sharedFormatId(entity.type, entity.id)}</dd>
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
            {#if entity.data?.approval_status === 'rejected'}
              {@const rejectEvent = (specHistory ?? []).find(ev => ev.event === 'rejected' || ev.event === 'revoked')}
              <div class="spec-rejected-banner" role="alert">
                <span class="rejected-banner-icon">&#x26D4;</span>
                <div class="rejected-banner-content">
                  <span class="rejected-banner-title">This spec has been rejected</span>
                  {#if rejectEvent?.reason ?? rejectEvent?.revocation_reason}
                    <span class="rejected-banner-reason">{rejectEvent.reason ?? rejectEvent.revocation_reason}</span>
                  {/if}
                  {#if rejectEvent?.user_id ?? rejectEvent?.approver_id ?? rejectEvent?.revoked_by}
                    <span class="rejected-banner-by">by {rejectEvent.user_id ?? rejectEvent.approver_id ?? rejectEvent.revoked_by} {rejectEvent.timestamp ? '· ' + fmtDate(rejectEvent.timestamp ?? rejectEvent.approved_at) : ''}</span>
                  {/if}
                </div>
              </div>
            {/if}
            <!-- Spec status journey -->
            {@const specStatus = entity.data?.approval_status ?? 'draft'}
            {@const specJourney = (() => {
              const steps = [{ label: 'Synced', variant: 'info', done: true }];
              if (specStatus === 'pending' || specStatus === 'approved' || specStatus === 'rejected' || specStatus === 'implemented') {
                steps.push({ label: 'Pending', variant: specStatus === 'pending' ? 'warning' : 'info', done: specStatus !== 'pending' || specStatus === 'pending' });
              }
              if (specStatus === 'approved' || specStatus === 'implemented') {
                steps.push({ label: 'Approved', variant: 'success', done: true });
              } else if (specStatus === 'rejected') {
                steps.push({ label: 'Rejected', variant: 'danger', done: true });
              }
              if (specStatus === 'approved' || specStatus === 'implemented') {
                const tasksDone = specProgress?.tasks_done ?? 0;
                const tasksTotal = specProgress?.tasks_total ?? 0;
                const mrs = specProgress?.mrs ?? [];
                const mergedMrs = mrs.filter(m => m.status === 'merged').length;
                if (tasksTotal > 0) {
                  steps.push({ label: `${tasksDone}/${tasksTotal} tasks`, variant: tasksDone === tasksTotal ? 'success' : 'warning', done: tasksDone > 0 });
                }
                if (mrs.length > 0) {
                  steps.push({ label: `${mergedMrs}/${mrs.length} MRs merged`, variant: mergedMrs === mrs.length ? 'success' : 'warning', done: mergedMrs > 0 });
                }
                if (specStatus === 'implemented') {
                  steps.push({ label: 'Implemented', variant: 'success', done: true });
                }
              }
              return steps;
            })()}
            {#if specJourney.length > 1}
              <div class="mr-status-journey">
                <div class="status-journey-track">
                  {#each specJourney as step, i}
                    <div class="status-journey-node status-journey-node-{step.variant}">
                      <span class="status-journey-dot"></span>
                      <span class="status-journey-label">{step.label}</span>
                    </div>
                    {#if i < specJourney.length - 1}
                      <span class="status-journey-connector"></span>
                    {/if}
                  {/each}
                </div>
              </div>
            {/if}
            <dl class="spec-meta-list">
              {#if entity.data?.approval_status}
                <dt>{$t('detail_panel.status')}</dt>
                <dd>
                  <Badge value={entity.data.approval_status} variant={specStatusColor(entity.data.approval_status)} />
                  <span class="status-explain">{specStatusExplain(entity.data)}</span>
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
            <div class="spec-content-box spec-content-rendered">
              {@html renderMarkdown(specDetail.content)}
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
                <dd>
                  <Badge value={sd.approval_status} variant={specStatusColor(sd.approval_status)} />
                  <span class="status-explain">{specStatusExplain(sd)}</span>
                </dd>
              {/if}
              {#if sd.owner}
                <dt>{$t('detail_panel.owner')}</dt><dd class="mono">{sd.owner}</dd>
              {/if}
              {#if sd.kind}
                <dt>{$t('detail_panel.kind')}</dt><dd>{sd.kind}</dd>
              {/if}
              {#if sd.current_sha}
                <dt>{$t('detail_panel.sha')}</dt><dd><code class="sha-badge mono copyable" title="Click to copy: {sd.current_sha}" onclick={() => copyId(sd.current_sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(sd.current_sha); }}>{sd.current_sha.slice(0, 7)}</code></dd>
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
                  <span class="suggestion-lbl">{llmSuggestion.diff?.length > 0 ? $t('detail_panel.suggested_change') : 'LLM Response'}</span>
                </div>
                {#if llmSuggestion.explanation}
                  <div class="suggestion-expl">{@html renderMarkdown(llmSuggestion.explanation)}</div>
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
                  {#if llmSuggestion.diff?.length > 0}
                    <Button variant="primary" onclick={acceptSuggestion}>{$t('detail_panel.accept')}</Button>
                    <Button variant="secondary" onclick={editSuggestion}>{$t('detail_panel.edit_btn')}</Button>
                  {/if}
                  <Button variant="secondary" onclick={dismissSuggestion}>{$t('detail_panel.dismiss')}</Button>
                </div>
              </div>
            {/if}

            {#if llmStreaming}
              <div class="llm-streaming" aria-live="polite">
                <span class="streaming-lbl">{$t('detail_panel.thinking')}</span>
                {#if llmExplanation}
                  <p class="streaming-txt">{llmExplanation}<span class="blink-cursor" aria-hidden="true"></span></p>
                {:else}
                  <p class="streaming-txt"><span class="blink-cursor" aria-hidden="true"></span></p>
                {/if}
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
              <span class="progress-section-label">Tasks ({specProgress.tasks.length})</span>
              <ul class="task-list">
                {#each specProgress.tasks as task}
                  <li class="task-item clickable-row" onclick={() => navigateTo('task', task.id ?? task.task_id, task)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('task', task.id ?? task.task_id, task); }}>
                    <Badge value={task.status} variant={taskStatusColor(task.status)} />
                    <span class="task-title">{task.title}</span>
                    {#if task.priority}
                      <span class="task-priority priority-{task.priority}">{task.priority}</span>
                    {/if}
                    {#if task.agent_id}
                      <button class="entity-link mono" title={task.agent_id} onclick={(e) => { e.stopPropagation(); navigateTo('agent', task.agent_id); }}>{entityName('agent', task.agent_id)}</button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="no-data">{$t('detail_panel.no_tasks')}</p>
            {/if}
            {#if specProgress.mrs?.length > 0}
              <span class="progress-section-label">Merge Requests ({specProgress.mrs.length})</span>
              <ul class="task-list">
                {#each specProgress.mrs as mr}
                  <li class="task-item clickable-row" onclick={() => navigateTo('mr', mr.id ?? mr.mr_id, mr)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('mr', mr.id ?? mr.mr_id, mr); }}>
                    <Badge value={mr.status} variant={mr.status === 'merged' ? 'success' : mr.status === 'open' ? 'info' : 'muted'} />
                    <span class="task-title">{mr.title}</span>
                    {#if mr.gate_summary}
                      <span class="mr-gate-summary mono">{mr.gate_summary}</span>
                    {:else if mr.gates_passed != null && mr.gates_total != null}
                      <span class="mr-gate-summary mono">{mr.gates_passed}/{mr.gates_total} gates</span>
                    {/if}
                    {#if mr.additions != null || mr.deletions != null}
                      <span class="mr-diff-stats mono">
                        {#if mr.additions != null}<span class="diff-add">+{mr.additions}</span>{/if}
                        {#if mr.deletions != null}<span class="diff-del">-{mr.deletions}</span>{/if}
                      </span>
                    {/if}
                    {#if mr.source_branch}
                      <span class="task-agent mono">{mr.source_branch}</span>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if specProgress.agents?.length > 0}
              <span class="progress-section-label">Agents ({specProgress.agents.length})</span>
              <ul class="task-list">
                {#each specProgress.agents as agent}
                  {@const agentId = agent.id ?? agent.agent_id}
                  <li class="task-item clickable-row" onclick={() => navigateTo('agent', agentId, agent)} tabindex="0" role="button" onkeydown={(e) => { if (e.key === 'Enter') navigateTo('agent', agentId, agent); }}>
                    <Badge value={agent.status ?? 'unknown'} variant={agent.status === 'running' ? 'info' : agent.status === 'completed' ? 'success' : agent.status === 'failed' ? 'danger' : 'muted'} />
                    <span class="task-title">{entityName('agent', agentId)}</span>
                    {#if agent.task_id}
                      <button class="entity-link mono" title={agent.task_id} onclick={(e) => { e.stopPropagation(); navigateTo('task', agent.task_id); }}>{entityName('task', agent.task_id)}</button>
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
            {@const linkArray = Array.isArray(specLinks) ? specLinks : (specLinks?.links ?? [])}
            {@const inbound = linkArray.filter(l => (typeof l === 'object' ? l.direction : null) === 'inbound')}
            {@const outbound = linkArray.filter(l => (typeof l === 'object' ? l.direction : null) !== 'inbound')}

            {#if linkArray.length > 0}
              <!-- Visual spec relationship diagram -->
              <div class="spec-link-diagram">
                {#if inbound.length > 0}
                  <div class="link-column link-column-inbound">
                    {#each inbound as link}
                      {@const target = typeof link === 'string' ? link : (link.target_path ?? link.target)}
                      {@const kind = typeof link === 'object' ? (link.kind ?? link.link_type ?? link.type) : null}
                      <button class="link-node link-node-inbound" onclick={() => navigateTo('spec', target, { path: target, repo_id: entity?.data?.repo_id })} title={target}>
                        <span class="link-node-name">{target?.split('/').pop()}</span>
                        {#if kind}<span class="link-node-kind">{kind.replace(/_/g, ' ')}</span>{/if}
                      </button>
                    {/each}
                  </div>
                  <div class="link-arrows">
                    {#each inbound as _}
                      <span class="link-arrow">→</span>
                    {/each}
                  </div>
                {/if}
                <div class="link-column link-column-center">
                  <div class="link-node link-node-current">
                    <span class="link-node-name">{entity?.id?.split('/').pop()}</span>
                    <span class="link-node-kind">current</span>
                  </div>
                </div>
                {#if outbound.length > 0}
                  <div class="link-arrows">
                    {#each outbound as _}
                      <span class="link-arrow">→</span>
                    {/each}
                  </div>
                  <div class="link-column link-column-outbound">
                    {#each outbound as link}
                      {@const target = typeof link === 'string' ? link : (link.target_path ?? link.target)}
                      {@const kind = typeof link === 'object' ? (link.kind ?? link.link_type ?? link.type) : null}
                      {@const isConflict = kind === 'conflicts_with' || kind === 'conflicts'}
                      <button class="link-node link-node-outbound" class:link-node-conflict={isConflict} onclick={() => navigateTo('spec', target, { path: target, repo_id: entity?.data?.repo_id })} title={target}>
                        <span class="link-node-name">{target?.split('/').pop()}</span>
                        {#if kind}<span class="link-node-kind">{kind.replace(/_/g, ' ')}</span>{/if}
                      </button>
                    {/each}
                  </div>
                {/if}
              </div>

              <!-- Detailed list below diagram -->
              <span class="progress-section-label" style="margin-top: var(--space-4)">All Links ({linkArray.length})</span>
              <ul class="links-list">
                {#each linkArray as link}
                  {@const target = typeof link === 'string' ? link : (link.target_path ?? link.target ?? JSON.stringify(link))}
                  {@const kind = typeof link === 'object' ? (link.kind ?? link.link_type ?? link.type) : null}
                  {@const direction = typeof link === 'object' ? link.direction : null}
                  {@const isConflict = kind === 'conflicts_with' || kind === 'conflicts'}
                  {@const isImplements = kind === 'implements' || kind === 'implemented_by'}
                  {@const isExtends = kind === 'extends' || kind === 'extended_by'}
                  {@const approvalStatus = typeof link === 'object' ? (link.approval_status ?? link.status) : null}
                  <li class="link-item" class:link-conflict={isConflict}>
                    {#if kind}
                      <span class="link-type-icon" class:link-type-conflict={isConflict} class:link-type-implements={isImplements} class:link-type-extends={isExtends}>
                        {#if isConflict}&#9888;{:else if isImplements}&#8594;{:else if isExtends}&#8599;{:else}&#8596;{/if}
                      </span>
                      <Badge
                        value={kind.replace(/_/g, ' ')}
                        variant={isConflict ? 'danger' : isImplements ? 'success' : 'info'}
                      />
                    {/if}
                    <span class="link-direction">{direction === 'inbound' ? '<- from' : '-> to'}</span>
                    <button class="entity-link mono" title="Navigate to {target}" onclick={() => navigateTo('spec', target, { path: target, repo_id: entity?.data?.repo_id })}>{target.split('/').pop()}</button>
                    <span class="link-full-path mono">{target}</span>
                    {#if approvalStatus}
                      <Badge
                        value={approvalStatus}
                        variant={approvalStatus === 'approved' ? 'success' : approvalStatus === 'rejected' ? 'danger' : approvalStatus === 'draft' ? 'warning' : 'muted'}
                      />
                    {/if}
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="no-data">{$t('detail_panel.no_links')}</p>
            {/if}
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
              <p class="no-data">No messages yet. You can send typed messages (FreeText, TaskAssignment, ReviewRequest) to active agents via the workspace message bus.</p>
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
              <div class="history-list history-timeline">
                {#each specHistory as ev, idx}
                  {@const eventType = ev.event ?? ev.action ?? 'unknown'}
                  {@const isApproved = eventType === 'approved'}
                  {@const isRejected = eventType === 'rejected' || eventType === 'invalidated'}
                  {@const isRevoked = eventType === 'revoked'}
                  <div class="history-item history-timeline-item">
                    <div class="history-timeline-marker" class:marker-approved={isApproved} class:marker-rejected={isRejected} class:marker-revoked={isRevoked}>
                      {#if isApproved}
                        <span class="timeline-icon" aria-label="Approved">&#10003;</span>
                      {:else if isRejected}
                        <span class="timeline-icon" aria-label="Rejected">&#10007;</span>
                      {:else if isRevoked}
                        <span class="timeline-icon" aria-label="Revoked">!</span>
                      {:else}
                        <span class="timeline-icon" aria-label={eventType}>&#8226;</span>
                      {/if}
                    </div>
                    {#if idx < specHistory.length - 1}
                      <div class="history-timeline-line"></div>
                    {/if}
                    <div class="history-timeline-content">
                      <div class="history-row">
                        <Badge
                          value={eventType}
                          variant={isApproved ? 'success' : isRejected || isRevoked ? 'danger' : 'muted'}
                        />
                        {#if ev.user_id || ev.approver_id}
                          {@const reviewerId = ev.user_id || ev.approver_id}
                          {#if reviewerId === 'human-reviewer' || reviewerId === 'system'}
                            <span class="history-user mono" title={reviewerId}>{reviewerId}</span>
                          {:else}
                            <button class="entity-link mono" title={reviewerId} onclick={() => navigateTo('agent', reviewerId)}>{entityName('agent', reviewerId)}</button>
                          {/if}
                        {:else}
                          <span class="history-user mono">--</span>
                        {/if}
                        {#if ev.timestamp || ev.approved_at}
                          {@const evTs = ev.timestamp || ev.approved_at}
                          <span class="history-time" title={formatDate(evTs)}>{relativeTime(evTs) || formatDate(evTs)}</span>
                        {:else}
                          <span class="history-time">--</span>
                        {/if}
                      </div>
                      {#if ev.sha || ev.spec_sha}
                        {@const evSha = ev.sha || ev.spec_sha}
                        <code class="sha-badge mono copyable" title="Click to copy: {evSha}" onclick={() => copyId(evSha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(evSha); }}>{evSha.slice(0, 7)}</code>
                      {/if}
                      {#if ev.reason}
                        <p class="history-reason">{ev.reason}</p>
                      {/if}
                    </div>
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
              {#if Array.isArray(agentLogs) && agentLogs.length > 0}
                {@const logLevels = [...new Set(agentLogs.map(e => e.level ?? (e.message?.startsWith('ERROR') || e.message?.startsWith('error') ? 'error' : e.message?.startsWith('WARN') || e.message?.startsWith('warn') ? 'warn' : 'info')))]}
                {#if agentLogStreaming}
                  <div class="log-live-indicator">
                    <span class="log-live-dot"></span>
                    <span class="log-live-text">Live — streaming new log entries</span>
                  </div>
                {/if}
                <div class="log-filter-bar">
                  <input
                    type="text"
                    class="log-filter-input"
                    placeholder="Filter logs..."
                    bind:value={agentLogFilter}
                  />
                  {#if logLevels.length > 1}
                    <div class="log-level-pills">
                      {#each logLevels as lvl}
                        {@const levelLower = (lvl ?? 'info').toLowerCase()}
                        <button class="log-level-pill log-level-{levelLower}" class:active={agentLogFilter === `level:${levelLower}`} onclick={() => { agentLogFilter = agentLogFilter === `level:${levelLower}` ? '' : `level:${levelLower}`; }} title="Filter to {levelLower} logs">
                          {levelLower}
                        </button>
                      {/each}
                    </div>
                  {/if}
                  {#if agentLogFilter}
                    {@const matchCount = agentLogs.filter(e => {
                      if (agentLogFilter.startsWith('level:')) {
                        const filterLevel = agentLogFilter.slice(6);
                        const entryLevel = (e.level ?? (e.message?.startsWith('ERROR') || e.message?.startsWith('error') ? 'error' : e.message?.startsWith('WARN') || e.message?.startsWith('warn') ? 'warn' : 'info')).toLowerCase();
                        return entryLevel === filterLevel;
                      }
                      const txt = e.message ?? e.content ?? e.line ?? JSON.stringify(e);
                      return txt.toLowerCase().includes(agentLogFilter.toLowerCase());
                    }).length}
                    <span class="log-filter-count">{matchCount}/{agentLogs.length}</span>
                  {/if}
                </div>
                {@const filteredLogs = agentLogFilter ? agentLogs.filter(e => {
                  if (agentLogFilter.startsWith('level:')) {
                    const filterLevel = agentLogFilter.slice(6);
                    const entryLevel = (e.level ?? (e.message?.startsWith('ERROR') || e.message?.startsWith('error') ? 'error' : e.message?.startsWith('WARN') || e.message?.startsWith('warn') ? 'warn' : 'info')).toLowerCase();
                    return entryLevel === filterLevel;
                  }
                  const txt = e.message ?? e.content ?? e.line ?? JSON.stringify(e);
                  return txt.toLowerCase().includes(agentLogFilter.toLowerCase());
                }) : agentLogs}
                <div class="log-terminal" bind:this={logListEl}>
                  {#each filteredLogs as entry}
                    {@const entryLevel = (entry.level ?? (entry.message?.startsWith('ERROR') || entry.message?.startsWith('error') ? 'error' : entry.message?.startsWith('WARN') || entry.message?.startsWith('warn') ? 'warn' : 'info')).toLowerCase()}
                    <div class="log-line" class:log-line-error={entryLevel === 'error'} class:log-line-warn={entryLevel === 'warn' || entryLevel === 'warning'}>
                      {#if entry.timestamp || entry.created_at}
                        <span class="log-ts">{fmtDate(entry.timestamp ?? entry.created_at)}</span>
                      {/if}
                      {#if entry.level}
                        <span class="log-level-badge log-level-{entryLevel}">{entryLevel}</span>
                      {/if}
                      <span class="log-msg">{formatLogEntry(entry)}</span>
                    </div>
                  {/each}
                  {#if filteredLogs.length === 0}
                    <p class="no-data">No logs matching "{agentLogFilter}"</p>
                  {/if}
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
              <!-- File tree summary (like GitHub — click to jump) -->
              <div class="diff-file-tree">
                <div class="diff-tree-actions">
                  <button class="diff-expand-btn" onclick={() => { mrDiff.files.forEach((_, i) => { const el = document.getElementById(`diff-file-${i}`); if (el) el.open = true; }); }} title="Expand all files">Expand all</button>
                  <button class="diff-expand-btn" onclick={() => { mrDiff.files.forEach((_, i) => { const el = document.getElementById(`diff-file-${i}`); if (el) el.open = false; }); }} title="Collapse all files">Collapse all</button>
                </div>
                {#each mrDiff.files as file, idx}
                  {@const statusLower = (file.status ?? 'modified').toLowerCase()}
                  {@const treeAdds = file.insertions ?? 0}
                  {@const treeDels = file.deletions ?? 0}
                  <button class="diff-tree-item diff-tree-clickable" onclick={() => { const el = document.getElementById(`diff-file-${idx}`); if (el) { el.open = true; el.scrollIntoView({ behavior: 'smooth', block: 'start' }); } }} title="Jump to {file.path}">
                    <span class="diff-tree-status diff-tree-status-{statusLower}">{statusLower === 'added' ? '+' : statusLower === 'deleted' ? '-' : '~'}</span>
                    <span class="diff-tree-path mono">{file.path}</span>
                    {#if treeAdds > 0 || treeDels > 0}
                      <span class="diff-tree-stats">
                        {#if treeAdds > 0}<span class="diff-ins">+{treeAdds}</span>{/if}
                        {#if treeDels > 0}<span class="diff-del">-{treeDels}</span>{/if}
                      </span>
                    {/if}
                  </button>
                {/each}
              </div>
              <div class="diff-file-list">
                {#each mrDiff.files as file, idx}
                  {@const fileStatusLower = (file.status ?? 'modified').toLowerCase()}
                  {@const fileAdds = file.insertions ?? (file.hunks ? file.hunks.reduce((sum, h) => sum + h.lines.filter(l => l.type === 'add').length, 0) : null)}
                  {@const fileDels = file.deletions ?? (file.hunks ? file.hunks.reduce((sum, h) => sum + h.lines.filter(l => l.type === 'delete').length, 0) : null)}
                  <details class="diff-file" id="diff-file-{idx}" open={mrDiff.files.length <= 10}>
                    <summary class="diff-file-header">
                      <Badge value={fileStatusLower} variant={fileStatusLower === 'added' ? 'success' : fileStatusLower === 'deleted' ? 'danger' : 'info'} />
                      <span class="diff-file-path mono">{file.path}</span>
                      <button class="diff-copy-path" onclick={(e) => { e.preventDefault(); e.stopPropagation(); navigator.clipboard.writeText(file.path); e.target.textContent = 'Copied!'; setTimeout(() => { e.target.textContent = 'Copy path'; }, 1500); }} title="Copy file path">Copy path</button>
                      {#if goToRepoTab && fileStatusLower !== 'deleted'}
                        <button class="diff-blame-link" onclick={(e) => { e.preventDefault(); e.stopPropagation(); goToRepoTab('code', { subTab: 'files', file: file.path }); close(); }} title="View blame & agent attribution for {file.path}">Blame</button>
                      {/if}
                      {#if fileAdds != null || fileDels != null}
                        <span class="diff-file-stats">
                          {#if fileAdds}<span class="diff-ins">+{fileAdds}</span>{/if}
                          {#if fileDels}<span class="diff-del">-{fileDels}</span>{/if}
                        </span>
                      {/if}
                    </summary>
                    {#if file.patch}
                      {@const lang = detectLang(file.path ?? '')}
                      <table class="diff-table">
                        <tbody>
                          {#each parsePatchLines(file.patch) as pline}
                            <tr class="diff-tr diff-tr-{pline.type}">
                              <td class="diff-gutter diff-gutter-old">{pline.oldNum}</td>
                              <td class="diff-gutter diff-gutter-new">{pline.newNum}</td>
                              <td class="diff-code">{#if pline.type === 'hunk'}<span class="diff-hunk-text">{pline.text}</span>{:else}<span class="diff-prefix">{pline.text.charAt(0)}</span>{@html highlightLine(pline.text.slice(1), lang)}{/if}</td>
                            </tr>
                          {/each}
                        </tbody>
                      </table>
                    {:else if file.hunks?.length > 0}
                      {@const lang = detectLang(file.path ?? '')}
                      {@const hunkLines = computeHunkLines(file.hunks, file.status)}
                      <table class="diff-table">
                        <tbody>
                          {#each hunkLines as hline}
                            {#if hline.type === 'hunk'}
                              <tr class="diff-tr diff-tr-hunk">
                                <td class="diff-gutter diff-gutter-old"></td>
                                <td class="diff-gutter diff-gutter-new"></td>
                                <td class="diff-code"><span class="diff-hunk-text">{hline.header}</span></td>
                              </tr>
                            {:else}
                              {@const lineType = hline.lineType}
                              {@const prefix = lineType === 'add' ? '+' : lineType === 'del' ? '-' : ' '}
                              <tr class="diff-tr diff-tr-{lineType}">
                                <td class="diff-gutter diff-gutter-old">{hline.oldNum}</td>
                                <td class="diff-gutter diff-gutter-new">{hline.newNum}</td>
                                <td class="diff-code"><span class="diff-prefix">{prefix}</span>{@html highlightLine(hline.content, lang)}</td>
                              </tr>
                            {/if}
                          {/each}
                        </tbody>
                      </table>
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
            <p class="no-data">No diff data available — the merge request may have been merged directly or the source branch has been deleted.</p>
          {/if}
        </div>

      {:else if activeTab === 'commits'}
        <div class="tab-pane">
          {#if mrCommitsLoading}
            <div class="spec-skeleton">
              {#each Array(5) as _}<Skeleton width="100%" height="1.5rem" />{/each}
            </div>
          {:else if Array.isArray(mrCommits) && mrCommits.length > 0}
            <div class="commits-list">
              {#each mrCommits as commit}
                {@const sha = commit.sha ?? commit.id ?? ''}
                {@const agentId = commit._agentRecord?.agent_id ?? commit.agent_id}
                {@const specRef = commit._agentRecord?.spec_ref ?? commit.spec_ref}
                <div class="commit-item">
                  <div class="commit-header">
                    <code class="sha-badge mono copyable" title="Click to copy: {sha}" onclick={() => copyId(sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(sha); }}>{sha.slice(0, 7)}</code>
                    <span class="commit-message">{commit.message ?? commit.summary ?? '—'}</span>
                  </div>
                  <div class="commit-meta">
                    {#if agentId}
                      <button class="entity-link mono" onclick={() => navigateTo('agent', agentId)} title={agentId}>
                        <span class="commit-author-icon">&#x2699;</span> {entityName('agent', agentId)}
                      </button>
                    {:else if commit.author}
                      <span class="commit-author mono">{commit.author}</span>
                    {/if}
                    {#if specRef}
                      {@const specPath = specRef.split('@')[0]}
                      <button class="entity-link mono" onclick={() => navigateTo('spec', specPath, { path: specPath })} title={specRef}>
                        📋 {specPath.split('/').pop()}
                      </button>
                    {/if}
                    {#if commit.timestamp ?? commit.created_at}
                      {@const commitTs = commit.timestamp ?? commit.created_at}
                      <span class="commit-time" title={absoluteTime(commitTs)}>{relativeTime(commitTs) || formatDate(commitTs)}</span>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {:else}
            <p class="no-data">No commits found on this branch</p>
          {/if}
        </div>

      {:else if activeTab === 'gates'}
        <div class="tab-pane">
          {#if mrGatesLoading}
            <div class="spec-skeleton">
              {#each Array(3) as _}<Skeleton width="100%" height="2rem" />{/each}
            </div>
          {:else if Array.isArray(mrGates) && mrGates.length > 0}
            {@const totalGates = mrGates.length}
            {@const passedGates = mrGates.filter(g => g.status === 'Passed' || g.status === 'passed').length}
            {@const failedGates = mrGates.filter(g => g.status === 'Failed' || g.status === 'failed').length}
            {@const pendingGates = totalGates - passedGates - failedGates}
            {@const requiredGates = mrGates.filter(g => g.required !== false)}
            {@const requiredFailed = requiredGates.filter(g => g.status === 'Failed' || g.status === 'failed').length}
            <div class="gates-tab-header" class:gates-all-passed={failedGates === 0 && pendingGates === 0} class:gates-has-failures={failedGates > 0}>
              <span class="gates-tab-summary-icon">{failedGates > 0 ? '✗' : pendingGates > 0 ? '○' : '✓'}</span>
              <span class="gates-tab-summary-text">
                {#if requiredFailed > 0}
                  {requiredFailed} required gate{requiredFailed !== 1 ? 's' : ''} failed — merge blocked
                {:else if failedGates > 0}
                  {failedGates} advisory gate{failedGates !== 1 ? 's' : ''} failed — merge not blocked
                {:else if pendingGates > 0}
                  {passedGates} of {totalGates} gate{totalGates !== 1 ? 's' : ''} passed — {pendingGates} pending
                {:else}
                  All {totalGates} gate{totalGates !== 1 ? 's' : ''} passed
                {/if}
              </span>
            </div>
            <ul class="gates-list">
              {#each mrGates as gate}
                {@const duration = (gate.started_at && gate.finished_at) ? Math.round((gate.finished_at - gate.started_at) * 1000) : gate.duration_ms}
                {@const gateTypeLabel = gate.gate_type ? gate.gate_type.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase()) : ''}
                {@const gateName = gate.gate_name ?? gate.name ?? (gateTypeLabel || (gate.command ? gate.command.split(' ')[0].split('/').pop() : '') || 'Quality Gate')}
                {@const gateStatus = (gate.status === 'Passed' || gate.status === 'passed') ? 'passed' : (gate.status === 'Failed' || gate.status === 'failed') ? 'failed' : (gate.status === 'Running' || gate.status === 'running') ? 'running' : gate.status ?? 'pending'}
                {@const gateDesc = ({
                  test_command: 'Runs the test suite to verify correctness',
                  lint_command: 'Checks code style and formatting',
                  build_command: 'Compiles the project to verify it builds',
                  trace_capture: 'Captures execution traces for observability',
                  trace_validation: 'Validates execution traces match expected behavior',
                  spec_compliance: 'Verifies implementation matches the linked specification',
                  security_scan: 'Scans for known security vulnerabilities',
                  coverage_check: 'Checks test coverage meets threshold',
                })[gate.gate_type] ?? null}
                <li class="gate-item gate-item-{gateStatus}" class:gate-item-required={gate.required !== false}>
                  <div class="gate-row">
                    <span class="gate-status-icon">{gateStatus === 'passed' ? '✓' : gateStatus === 'failed' ? '✗' : gateStatus === 'running' ? '⟳' : '○'}</span>
                    <div class="gate-name-block">
                      <span class="gate-name" title={gate.gate_id ?? ''}>{gateName}</span>
                      {#if gateDesc}
                        <span class="gate-description">{gateDesc}</span>
                      {/if}
                    </div>
                    <span class="gate-status-badge gate-status-badge-{gateStatus}">
                      {#if gateStatus === 'passed'}
                        Passed
                      {:else if gateStatus === 'failed'}
                        Failed
                      {:else if gateStatus === 'running'}
                        Running
                      {:else}
                        <span class="gate-pending-pulse"></span>Waiting...
                      {/if}
                    </span>
                    {#if gate.required !== undefined}
                      <span class="gate-required-badge" class:advisory={!gate.required}>
                        {gate.required ? 'Required' : 'Advisory'}
                      </span>
                    {/if}
                    {#if duration}
                      <span class="gate-duration">{duration < 1000 ? duration + 'ms' : (duration / 1000).toFixed(1) + 's'}</span>
                    {/if}
                  </div>
                  {#if gate.command}
                    <div class="gate-cmd-row">
                      <span class="gate-cmd-label">$</span>
                      <code class="gate-cmd mono">{gate.command}</code>
                    </div>
                  {:else}
                    <div class="gate-cmd-row gate-cmd-configured">
                      <span class="gate-cmd-label gate-cmd-hint">Configured in repo settings</span>
                    </div>
                  {/if}
                  {#if gate.output}
                    {@const isJson = gate.output.trim().startsWith('{') || gate.output.trim().startsWith('[')}
                    {@const parsed = isJson ? (() => { try { return JSON.parse(gate.output); } catch { return null; } })() : null}
                    <details class="gate-output-details" open={gateStatus === 'failed'}>
                      <summary class="gate-output-label">
                        Output
                        <button class="gate-copy-btn" onclick={(e) => { e.stopPropagation(); e.preventDefault(); navigator.clipboard.writeText(gate.output); e.target.textContent = 'Copied!'; setTimeout(() => { e.target.textContent = 'Copy'; }, 1500); }} title="Copy output to clipboard">Copy</button>
                      </summary>
                      {#if parsed && (parsed.tests_passed !== undefined || parsed.total !== undefined || parsed.coverage !== undefined)}
                        <div class="gate-structured-output">
                          {#if parsed.tests_passed !== undefined}
                            <span class="gate-metric gate-metric-success">{parsed.tests_passed} passed</span>
                          {/if}
                          {#if parsed.tests_failed !== undefined && parsed.tests_failed > 0}
                            <span class="gate-metric gate-metric-fail">{parsed.tests_failed} failed</span>
                          {/if}
                          {#if parsed.total !== undefined}
                            <span class="gate-metric">{parsed.total} total</span>
                          {/if}
                          {#if parsed.coverage !== undefined}
                            <span class="gate-metric">{parsed.coverage}% coverage</span>
                          {/if}
                          {#if parsed.warnings !== undefined && parsed.warnings > 0}
                            <span class="gate-metric gate-metric-warn">{parsed.warnings} warnings</span>
                          {/if}
                        </div>
                      {/if}
                      <pre class="gate-output gate-terminal">{gate.output}</pre>
                    </details>
                  {/if}
                  {#if gate.error}
                    <details class="gate-output-details" open>
                      <summary class="gate-output-label gate-error-label">
                        Error
                        <button class="gate-copy-btn" onclick={(e) => { e.stopPropagation(); e.preventDefault(); navigator.clipboard.writeText(gate.error); e.target.textContent = 'Copied!'; setTimeout(() => { e.target.textContent = 'Copy'; }, 1500); }} title="Copy error to clipboard">Copy</button>
                      </summary>
                      <pre class="gate-output gate-terminal gate-error">{gate.error}</pre>
                    </details>
                  {/if}
                  {#if !gate.output && !gate.error && (gateStatus === 'passed' || gateStatus === 'failed')}
                    <p class="gate-no-output">No output captured for this gate run.</p>
                  {/if}
                  {#if gate.started_at}
                    <span class="gate-timing">{fmtDate(gate.started_at)}{#if gate.finished_at} — {fmtDate(gate.finished_at)}{/if}</span>
                  {/if}
                </li>
              {/each}
            </ul>
          {:else}
            <p class="no-data">No gate results for this merge request. Quality gates (tests, lint, traces) are configured per-repository in Settings and run automatically when an MR is enqueued.</p>
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
              <!-- Verified badge banner -->
              <div class="att-verified-banner" class:att-verified={!!mrAttestation.signature}>
                <div class="att-verified-icon">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="28" height="28">
                    <path d="M12 2L3 5v6c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V5L12 2z"/>
                    {#if mrAttestation.signature}<path d="M9 12l2 2 4-4" stroke-width="2"/>{/if}
                  </svg>
                </div>
                <div class="att-verified-text">
                  <span class="att-verified-title">{mrAttestation.signature ? 'Verified Attestation' : 'Attestation Bundle'}</span>
                  {#if att.attestation_version}
                    <span class="att-version-badge">v{att.attestation_version}</span>
                  {/if}
                </div>
                {#if att.merged_at}
                  <span class="att-merged-time">{fmtDate(att.merged_at)}</span>
                {/if}
              </div>

              <dl class="entity-meta">
                {#if att.merge_commit_sha}
                  <dt>Merge commit</dt>
                  <dd>
                    <code class="sha-badge mono copyable" title={att.merge_commit_sha} onclick={() => copyId(att.merge_commit_sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(att.merge_commit_sha); }}>{att.merge_commit_sha.slice(0, 7)}</code>
                  </dd>
                {/if}
                {#if att.spec_ref}
                  {@const attSpecPath = att.spec_ref.split('@')[0]}
                  {@const attSpecSha = att.spec_ref.includes('@') ? att.spec_ref.split('@')[1] : null}
                  <dt>Spec</dt>
                  <dd>
                    <button class="entity-link" title={att.spec_ref} onclick={() => navigateTo('spec', attSpecPath, { path: attSpecPath })}>{attSpecPath.split('/').pop()?.replace(/\.md$/, '')}</button>
                    {#if attSpecSha}<code class="sha-badge mono" title="Pinned at spec version {attSpecSha}">@{attSpecSha.slice(0, 7)}</code>{/if}
                  </dd>
                {/if}
                {#if att.spec_fully_approved !== undefined}
                  <dt>Spec approved</dt><dd><Badge value={att.spec_fully_approved ? 'yes' : 'no'} variant={att.spec_fully_approved ? 'success' : 'warning'} /></dd>
                {/if}
                {#if att.author_agent_id}
                  <dt>Author agent</dt>
                  <dd>
                    <button class="entity-link mono" title={att.author_agent_id} onclick={() => navigateTo('agent', att.author_agent_id)}>{entityName('agent', att.author_agent_id)}</button>
                  </dd>
                {/if}
                {#if att.mr_id}
                  <dt>MR</dt><dd><button class="entity-link mono" title={att.mr_id} onclick={() => navigateTo('mr', att.mr_id)}>{entityName('mr', att.mr_id)}</button></dd>
                {/if}
                {#if att.task_id}
                  <dt>Task</dt><dd><button class="entity-link" title={att.task_id} onclick={() => navigateTo('task', att.task_id)}>{entityName('task', att.task_id)}</button></dd>
                {/if}
                {#if att.repo_id}
                  <dt>Repo</dt><dd class="mono copyable" title="Click to copy: {att.repo_id}" onclick={() => copyId(att.repo_id)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(att.repo_id); }}>{entityName('repo', att.repo_id)}</dd>
                {/if}
                {#if att.conversation_sha}
                  <dt>Conversation</dt>
                  <dd>
                    <button class="entity-link mono" title="View agent reasoning for this merge" onclick={() => { activeTab = 'ask-why'; }}>
                      <code class="sha-badge mono">{att.conversation_sha.slice(0, 7)}</code>
                      <span class="att-conv-arrow">View reasoning</span>
                    </button>
                  </dd>
                {/if}
                {#if att.gate_results?.length > 0}
                  {@const passed = att.gate_results.filter(g => g.status === 'Passed' || g.status === 'passed').length}
                  {@const total = att.gate_results.length}
                  <dt>Gates</dt>
                  <dd class="att-gate-summary">
                    <Badge value="{passed}/{total} passed" variant={passed === total ? 'success' : 'warning'} />
                  </dd>
                {/if}
              </dl>

              <!-- Gate results detail list -->
              {#if att.gate_results?.length > 0}
                <details class="att-gates-detail" open>
                  <summary class="progress-section-label">Gate Results ({att.gate_results.length})</summary>
                  <ul class="gates-list">
                    {#each att.gate_results as gate}
                      {@const gStatus = (gate.status === 'Passed' || gate.status === 'passed') ? 'passed' : (gate.status === 'Failed' || gate.status === 'failed') ? 'failed' : gate.status ?? 'unknown'}
                      <li class="gate-item gate-item-{gStatus}">
                        <div class="gate-row">
                          <span class="gate-status-icon">{gStatus === 'passed' ? '✓' : gStatus === 'failed' ? '✗' : '○'}</span>
                          <span class="gate-name">{gate.gate_name ?? gate.name ?? (gate.gate_type ? gate.gate_type.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase()) : (gate.command ? gate.command.split(' ')[0].split('/').pop() : 'Quality Gate'))}</span>
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
              {#if att.completion_summary}
                <div class="att-completion-block">
                  <span class="progress-section-label">Agent Summary</span>
                  <p class="att-completion-text">{att.completion_summary}</p>
                </div>
              {/if}
              {#if att.meta_specs_used?.length > 0}
                <div class="att-meta-specs">
                  <span class="progress-section-label">Meta-specs Applied ({att.meta_specs_used.length})</span>
                  <div class="att-meta-list">
                    {#each att.meta_specs_used as ms}
                      <span class="cap-tag">{ms.name ?? ms.id ?? ms}</span>
                    {/each}
                  </div>
                </div>
              {/if}
              {#if mrAttestation.signature}
                <div class="att-sig-block">
                  <div class="att-sig-header">
                    <span class="att-sig-label">Ed25519 Signature</span>
                    <button class="att-sig-copy-btn" title="Copy full signature" onclick={() => copyId(mrAttestation.signature)}>
                      <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="5" y="5" width="8" height="8" rx="1"/><path d="M3 11V3h8"/></svg>
                      Copy
                    </button>
                  </div>
                  <code class="att-sig-value mono">{mrAttestation.signature.slice(0, 20)}...</code>
                </div>
              {/if}
            </div>
          {:else}
            {@const mrStatus = mrDetail?.status ?? entity.data?.status}
            {#if mrStatus !== 'merged'}
              <div class="attestation-pending">
                <div class="att-pending-icon">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="32" height="32"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
                </div>
                <p class="att-pending-title">Attestation will be created at merge</p>
                <p class="att-pending-desc">When this MR passes all required gates and is merged, Gyre will create a signed attestation bundle containing:</p>
                <ul class="att-pending-list">
                  <li>Merge commit SHA with Ed25519 signature</li>
                  <li>Gate results for each quality check</li>
                  <li>Spec binding verification</li>
                  <li>Agent identity and conversation provenance</li>
                </ul>
                <p class="att-pending-desc">This creates a tamper-evident audit trail from spec to code.</p>
              </div>
            {:else}
              <p class="no-data">No attestation bundle available for this merge request</p>
            {/if}
          {/if}
        </div>

      {:else if activeTab === 'timeline'}
        <div class="tab-pane">
          {#if mrTimelineLoading}
            <div class="spec-skeleton">
              {#each Array(5) as _}<Skeleton width="100%" height="1.5rem" />{/each}
            </div>
          {:else if Array.isArray(mrTimeline) && mrTimeline.length > 0}
            {@const firstTime = mrTimeline[0]?.timestamp ?? mrTimeline[0]?.created_at}
            {@const lastTime = mrTimeline[mrTimeline.length - 1]?.timestamp ?? mrTimeline[mrTimeline.length - 1]?.created_at}
            {@const totalDuration = (firstTime && lastTime) ? Math.round(lastTime - firstTime) : null}
            <div class="timeline-summary">
              <span class="timeline-summary-count">{mrTimeline.length} events</span>
              {#if totalDuration != null && totalDuration > 0}
                <span class="timeline-summary-duration">Total: {totalDuration < 60 ? totalDuration + 's' : totalDuration < 3600 ? Math.round(totalDuration / 60) + 'm' : (totalDuration / 3600).toFixed(1) + 'h'}</span>
              {/if}
              <span class="timeline-summary-range">{fmtDate(firstTime)} — {fmtDate(lastTime)}</span>
            </div>
            <div class="timeline-list">
              {#each mrTimeline as evt, i}
                {@const evtType = evt.event_type ?? evt.type ?? evt.event}
                {@const detailText = timelineDetailText(evt)}
                {@const prevTime = i > 0 ? (mrTimeline[i-1].timestamp ?? mrTimeline[i-1].created_at) : null}
                {@const thisTime = evt.timestamp ?? evt.created_at}
                {@const elapsed = (prevTime && thisTime) ? Math.round(thisTime - prevTime) : null}
                <div class="timeline-item">
                  <div class="timeline-connector">
                    <div class="timeline-dot timeline-dot-{timelineEventVariant(evtType, evt)}">
                      {#if evtType === 'created' || evtType === 'mr_created'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8" cy="8" r="5"/><path d="M8 5.5v5M5.5 8h5"/></svg>
                      {:else if evtType === 'merged' || evtType === 'Merged'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M5 3v6a4 4 0 004 4h2M5 3L3 5M5 3l2 2M11 7v6"/></svg>
                      {:else if evtType === 'closed'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8" cy="8" r="5"/><path d="M5.5 5.5l5 5M10.5 5.5l-5 5"/></svg>
                      {:else if evtType?.startsWith('gate_') || evtType === 'GateResult'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="3" y="2" width="10" height="12" rx="1"/><path d="M6 6h4M6 9h2"/></svg>
                      {:else if evtType === 'commit_pushed' || evtType === 'GitPush'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M8 12V4M8 4l-3 3M8 4l3 3"/></svg>
                      {:else if evtType === 'review_submitted'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M3 8.5l3 3 7-7"/></svg>
                      {:else if evtType === 'comment_added'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M3 3h10v7H6l-3 3V3z"/></svg>
                      {:else if evtType === 'enqueued' || evtType === 'MergeQueueEnqueued'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="3" y="3" width="10" height="10" rx="2"/><path d="M6 6h4M6 8h4M6 10h2"/></svg>
                      {:else if evtType === 'AgentSpawned' || evtType === 'AgentCompleted'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8" cy="6" r="3"/><path d="M4 13c0-2.2 1.8-4 4-4s4 1.8 4 4"/></svg>
                      {:else if evtType === 'GraphDelta' || evtType === 'graph_extracted' || evtType === 'GraphExtraction'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="4" cy="4" r="2"/><circle cx="12" cy="4" r="2"/><circle cx="8" cy="12" r="2"/><path d="M5.5 5.5L8 10M10.5 5.5L8 10"/></svg>
                      {:else if evtType === 'attestation_created'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M8 2L3 5v4c0 3.3 2.2 5.6 5 7 2.8-1.4 5-3.7 5-7V5L8 2z"/></svg>
                      {:else if evtType === 'SpecLifecycleTrigger'}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M4 2h8v12H4zM7 5h2M7 7h2"/></svg>
                      {:else}
                        <svg class="timeline-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8" cy="8" r="3"/></svg>
                      {/if}
                    </div>
                    {#if i < mrTimeline.length - 1}<div class="timeline-line"></div>{/if}
                  </div>
                  <div class="timeline-content">
                    <div class="timeline-header">
                      <Badge value={timelineEventLabel(evtType)} variant={timelineEventVariant(evtType, evt)} />
                      <span class="timeline-time" title={absoluteTime(thisTime)}>{relativeTime(thisTime) || formatDate(thisTime)}</span>
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
                      <code class="sha-badge mono copyable" title="Click to copy: {sha}" onclick={() => copyId(sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(sha); }}>{sha.slice(0, 7)}</code>
                    {/if}
                    {#if evt.mr_id}
                      <button class="entity-link mono" onclick={() => navigateTo('mr', evt.mr_id)} title={evt.mr_id}>{entityName('mr', evt.mr_id)}</button>
                    {/if}
                    {#if evt.detail?.spec_path}
                      {@const tlSpecPath = evt.detail.spec_path.replace(/^specs\//, '')}
                      <button class="entity-link timeline-spec-ref mono" onclick={() => navigateTo('spec', tlSpecPath, { path: tlSpecPath, repo_id: mrDetail?.repository_id ?? mrDetail?.repo_id })} title={evt.detail.spec_path}>{tlSpecPath.split('/').pop()}</button>
                    {/if}
                    {#if evt.detail?.task_id}
                      <button class="entity-link mono" onclick={() => navigateTo('task', evt.detail.task_id)} title={evt.detail.task_id}>{entityName('task', evt.detail.task_id)}</button>
                    {/if}
                    {#if evt.detail?.agent_id && !(evt.actor_id ?? evt.agent_id)}
                      <button class="entity-link mono" onclick={() => navigateTo('agent', evt.detail.agent_id)} title={evt.detail.agent_id}>{entityName('agent', evt.detail.agent_id)}</button>
                    {/if}
                    {#if evt.detail?.commit_sha && !(evt.sha || evt.commit_sha)}
                      <code class="sha-badge mono copyable" title="Click to copy: {evt.detail.commit_sha}" onclick={() => copyId(evt.detail.commit_sha)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(evt.detail.commit_sha); }}>{evt.detail.commit_sha.slice(0, 7)}</code>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {:else}
            <p class="no-data">No SDLC timeline events recorded yet. Events are created as the MR moves through the lifecycle: creation, enqueue, gate execution, merge, and attestation signing.</p>
          {/if}
        </div>

      {:else if activeTab === 'trace' && entity.type === 'mr'}
        <div class="tab-pane">
          {#if mrTraceLoading}
            <div class="spec-skeleton">
              {#each Array(5) as _}<Skeleton width="100%" height="1.5rem" />{/each}
            </div>
          {:else if mrTrace}
            {@const spans = mrTrace.spans ?? []}
            {@const traceId = mrTrace.trace_id ?? mrTrace.id ?? ''}
            {@const rootSpans = mrTrace.root_spans ?? spans.filter(s => !s.parent_span_id).length}
            {@const totalDurUs = spans.reduce((max, s) => Math.max(max, s.duration_us ?? 0), 0)}
            {@const totalDurMs = totalDurUs > 0 ? Math.round(totalDurUs / 1000) : (mrTrace.duration_ms ?? null)}
            <div class="trace-header-banner">
              <div class="trace-header-left">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="20" height="20"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg>
                <span class="trace-header-title">{spans.length} span{spans.length !== 1 ? 's' : ''}</span>
                {#if rootSpans > 0}
                  <span class="trace-header-roots">{rootSpans} root</span>
                {/if}
              </div>
              <div class="trace-header-right">
                {#if totalDurMs != null && totalDurMs > 0}
                  <span class="trace-header-duration">{totalDurMs < 1000 ? totalDurMs + 'ms' : totalDurMs < 60000 ? (totalDurMs / 1000).toFixed(1) + 's' : (totalDurMs / 60000).toFixed(1) + 'min'}</span>
                {/if}
                {#if traceId}
                  <code class="sha-badge mono copyable" title="Click to copy trace ID: {traceId}" onclick={() => copyId(traceId)} role="button" tabindex="0" onkeydown={(e) => { if (e.key === 'Enter') copyId(traceId); }}>trace:{traceId.length > 16 ? traceId.slice(0, 12) + '...' : traceId}</code>
                {/if}
              </div>
            </div>
            {#if spans.length > 0}
              {@const spanTree = (() => {
                // Build hierarchical tree from flat span list
                const byId = new Map(spans.map(s => [s.span_id ?? s.id, s]));
                const children = new Map();
                const roots = [];
                for (const s of spans) {
                  const pid = s.parent_span_id;
                  if (pid && byId.has(pid)) {
                    if (!children.has(pid)) children.set(pid, []);
                    children.get(pid).push(s);
                  } else {
                    roots.push(s);
                  }
                }
                // Compute max duration for waterfall bar scaling
                const maxDur = Math.max(...spans.map(s => s.duration_us ?? 0), 1);
                // Flatten tree into display list with depth
                const flat = [];
                function walk(node, depth) {
                  const durUs = node.duration_us ?? 0;
                  const durMs = durUs > 0 ? Math.round(durUs / 1000) : ((node.end_ms && node.start_ms) ? node.end_ms - node.start_ms : null);
                  flat.push({ ...node, _depth: depth, _durMs: durMs, _pct: Math.max((durUs / maxDur) * 100, 2) });
                  const kids = children.get(node.span_id ?? node.id) ?? [];
                  kids.forEach(c => walk(c, depth + 1));
                }
                roots.forEach(r => walk(r, 0));
                return flat;
              })()}
              <div class="trace-waterfall">
                {#each spanTree as span}
                  {@const isRoot = span._depth === 0}
                  {@const statusColor = span.status === 'error' ? 'var(--color-danger)' : span.graph_node_id ? 'var(--color-primary)' : 'var(--color-success)'}
                  <div class="trace-waterfall-row" class:trace-waterfall-root={isRoot} style="padding-left: {span._depth * 20 + 8}px">
                    <div class="trace-waterfall-info">
                      {#if span._depth > 0}<span class="trace-tree-guide" aria-hidden="true"></span>{/if}
                      <span class="trace-span-name">{span.operation_name ?? span.name ?? 'span'}</span>
                      {#if span.service_name}
                        <span class="trace-service-tag">{span.service_name}</span>
                      {/if}
                      {#if span.graph_node_id}
                        <button class="entity-link mono trace-graph-link" onclick={() => navigateTo('node', span.graph_node_id, { id: span.graph_node_id })} title="Linked to graph node: {span.graph_node_id}">
                          <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="4" cy="4" r="2"/><circle cx="12" cy="12" r="2"/><path d="M6 6l4 4"/></svg>
                          {span.graph_node_name ?? entityName('node', span.graph_node_id)}
                        </button>
                      {/if}
                    </div>
                    <div class="trace-waterfall-bar-container">
                      <div class="trace-waterfall-bar" style="width: {span._pct}%; background: {statusColor}"></div>
                    </div>
                    <span class="trace-waterfall-dur">{span._durMs != null ? (span._durMs < 1000 ? span._durMs + 'ms' : (span._durMs / 1000).toFixed(1) + 's') : ''}</span>
                  </div>
                {/each}
              </div>
            {:else}
              <p class="no-data">Trace captured but contains no spans</p>
            {/if}
          {:else}
            <div class="attestation-pending">
              <div class="att-pending-icon">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="32" height="32"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg>
              </div>
              <p class="att-pending-title">No trace data available</p>
              <p class="att-pending-desc">Trace data is captured when a <code>trace_capture</code> gate is configured on the repository. It records execution spans during gate evaluation, providing flow visualization of the merge process.</p>
            </div>
          {/if}
        </div>

      {:else if activeTab === 'reviews'}
        <div class="tab-pane">
          {#if mrReviewsLoading}
            <div class="spec-skeleton">
              {#each Array(3) as _}<Skeleton width="100%" height="2rem" />{/each}
            </div>
          {:else}
            <!-- Unified conversation: reviews + comments merged chronologically (like GitHub) -->
            {@const conversation = [
              ...(Array.isArray(mrReviews) ? mrReviews : []).map(r => ({ ...r, _kind: 'review', _time: r.created_at ?? r.timestamp ?? 0 })),
              ...(Array.isArray(mrComments) ? mrComments : []).map(c => ({ ...c, _kind: 'comment', _time: c.created_at ?? c.timestamp ?? 0 })),
            ].sort((a, b) => a._time - b._time)}

            {#if conversation.length > 0}
              <div class="conversation-list">
                {#each conversation as item}
                  {@const isApproved = item._kind === 'review' && (item.decision === 'approved' || item.status === 'approved')}
                  {@const isChangesRequested = item._kind === 'review' && (item.decision === 'changes_requested' || item.status === 'changes_requested')}
                  <div class="conversation-item conversation-item-{item._kind}" class:conversation-item-approved={isApproved} class:conversation-item-changes-requested={isChangesRequested}>
                    <div class="conversation-header">
                      {#if item._kind === 'review'}
                        <Badge
                          value={item.decision ?? item.status ?? 'review'}
                          variant={isApproved ? 'success' : isChangesRequested ? 'danger' : 'info'}
                        />
                        {@const reviewer = item.reviewer ?? (item.reviewer_agent_id ? entityName('agent', item.reviewer_agent_id) : item.user_id ?? item.reviewer_id ?? 'reviewer')}
                        {#if item.reviewer_agent_id}
                          <button class="entity-link mono conversation-author" onclick={() => navigateTo('agent', item.reviewer_agent_id)}>{reviewer}</button>
                        {:else}
                          <span class="conversation-author mono">{reviewer}</span>
                        {/if}
                        <span class="conversation-verb">reviewed</span>
                      {:else}
                        <span class="conversation-comment-icon">
                          <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M3 3h10v7H6l-3 3V3z"/></svg>
                        </span>
                        {@const author = item.author ?? (item.author_agent_id ? entityName('agent', item.author_agent_id) : item.user_id ?? item.author_id ?? 'author')}
                        {#if item.author_agent_id}
                          <button class="entity-link mono conversation-author" onclick={() => navigateTo('agent', item.author_agent_id)}>{entityName('agent', item.author_agent_id)}</button>
                        {:else}
                          <span class="conversation-author mono">{author}</span>
                        {/if}
                        <span class="conversation-verb">commented</span>
                      {/if}
                      <span class="conversation-time" title={absoluteTime(item._time)}>{relativeTime(item._time) || formatDate(item._time)}</span>
                    </div>
                    {#if item.body}
                      <p class="review-body">{item.body}</p>
                    {/if}
                  </div>
                {/each}
              </div>
            {:else}
              <p class="no-data no-data-sm">No conversation yet — be the first to comment or review</p>
            {/if}

            <!-- Unified submission area -->
            <div class="conversation-submit">
              <span class="conversation-submit-heading">Add to conversation</span>
              <textarea
                class="comment-textarea"
                bind:value={newCommentText}
                placeholder="Leave a comment or submit a review..."
                rows="4"
                disabled={submittingComment || submittingReview}
              ></textarea>
              <div class="conversation-actions">
                <Button variant="secondary" size="sm" onclick={submitComment} disabled={!newCommentText?.trim() || submittingComment}>
                  {submittingComment ? 'Posting...' : 'Comment'}
                </Button>
                <div class="review-submit-group">
                  <select class="review-decision-select" bind:value={newReviewDecision}>
                    <option value="approved">Approve</option>
                    <option value="changes_requested">Request Changes</option>
                  </select>
                  <Button
                    variant={newReviewDecision === 'approved' ? 'primary' : 'secondary'}
                    size="sm"
                    onclick={() => { newReviewBody = newCommentText; submitReview(); }}
                    disabled={submittingReview}
                  >
                    {submittingReview ? 'Submitting...' : newReviewDecision === 'approved' ? 'Approve' : 'Request Changes'}
                  </Button>
                </div>
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
                    <span class="trace-msg">{formatLogEntry(entry)}</span>
                  </div>
                {/each}
              </div>
            {:else if !agentTraceSpans?.length}
              <div class="attestation-pending">
                <div class="att-pending-icon">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="32" height="32"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg>
                </div>
                <p class="att-pending-title">No trace data yet</p>
                <p class="att-pending-desc">Trace data appears here when:</p>
                <ul class="att-pending-list">
                  <li>The agent completes and creates a merge request</li>
                  <li>The MR's repository has a <code>trace_capture</code> gate configured</li>
                  <li>The gate emits OTLP spans during merge queue processing</li>
                </ul>
                <p class="att-pending-desc">Log output from the agent will appear above once the agent posts logs via the Gyre API.</p>
              </div>
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
                    <span class="task-title">{agent.name ?? sharedFormatId('agent', agent.id)}</span>
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
                    <span class="task-title">{mr.title ?? sharedFormatId('mr', mr.id)}</span>
                    {#if mr.source_branch}
                      <span class="task-agent mono">{mr.source_branch}</span>
                    {/if}
                    {#if mr.diff_stats}
                      <span class="diff-stat-compact">
                        <span class="diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                        <span class="diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                      </span>
                    {/if}
                    {#if mr.merge_commit_sha}
                      <code class="sha-badge mono" title={mr.merge_commit_sha}>{mr.merge_commit_sha.slice(0, 7)}</code>
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
          {#if effectiveConvSha}
            <!-- Conversation History -->
            {#if conversationLoading}
              <div class="spec-skeleton">
                {#each Array(4) as _}<Skeleton width="100%" height="1.5rem" />{/each}
              </div>
            {:else if conversationData}
              {@const turns = conversationData.turns ?? conversationData.messages ?? []}
              {@const toolCalls = turns.filter(t => t.tool_name || t.role === 'tool')}
              {@const decisions = turns.filter(t => t.role === 'assistant' && (t.content ?? t.text ?? '').length > 50)}
              {#if turns.length > 0}
                <div class="conv-summary">
                  <span class="conv-summary-stat">{turns.length} turns</span>
                  {#if toolCalls.length > 0}<span class="conv-summary-stat">{toolCalls.length} tool calls</span>{/if}
                  {#if decisions.length > 0}<span class="conv-summary-stat">{decisions.length} reasoning steps</span>{/if}
                  {#if conversationData.model}<span class="conv-summary-stat">{conversationData.model}</span>{/if}
                </div>
                <span class="progress-section-label">Conversation ({turns.length} turns)</span>
                <div class="conversation-trace">
                  {#each turns as turn, i}
                    <div class="conv-turn" class:conv-turn-user={turn.role === 'user' || turn.role === 'human'} class:conv-turn-assistant={turn.role === 'assistant'}>
                      <div class="conv-turn-header">
                        <Badge value={turn.role ?? 'message'} variant={turn.role === 'assistant' ? 'info' : turn.role === 'user' || turn.role === 'human' ? 'warning' : 'muted'} />
                        {#if turn.timestamp}
                          <span class="conv-turn-time">{fmtDate(turn.timestamp)}</span>
                        {/if}
                        {#if turn.tool_name}
                          <span class="conv-turn-tool mono">{turn.tool_name}</span>
                        {/if}
                      </div>
                      <div class="conv-turn-content">
                        {turn.content ?? turn.text ?? turn.message ?? formatLogEntry(turn)}
                      </div>
                    </div>
                  {/each}
                </div>
              {:else}
                <p class="no-data no-data-sm">Conversation recorded but no turns available</p>
              {/if}
              {#if conversationData.model}
                <div class="conv-meta">
                  <span class="conv-meta-label">Model:</span> <span class="mono">{conversationData.model}</span>
                  {#if conversationData.total_tokens}
                    <span class="conv-meta-label">Tokens:</span> <span>{conversationData.total_tokens.toLocaleString()}</span>
                  {/if}
                </div>
              {/if}
            {/if}

            <!-- Spawn investigation agent -->
            <div class="ask-why-actions">
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
                <button class="entity-link" onclick={() => navigateTo('agent', interrogationAgentId)}>{$t('detail_panel.ask_why_view_agent')}</button>
              {/if}
            </div>
          {:else if mrDetailLoading || agentDetailLoading}
            <div class="spec-skeleton">
              {#each Array(3) as _}<Skeleton width="100%" height="1.2rem" />{/each}
              <p class="no-data no-data-sm">Loading conversation provenance...</p>
            </div>
          {:else if entity.data?.author_agent_id || mrDetail?.author_agent_id}
            {@const agId = entity.data?.author_agent_id ?? mrDetail?.author_agent_id}
            <div class="ask-why-no-conv">
              <p class="no-data">No recorded conversation for this agent session.</p>
              <p class="ask-why-hint">The agent may not have stored conversation provenance. You can still spawn an investigation agent to explore the context.</p>
              <div class="ask-why-actions">
                <button
                  class="start-interrogation"
                  onclick={startInterrogation}
                  disabled={interrogationLoading}
                >
                  {interrogationLoading ? $t('detail_panel.ask_why_starting') : $t('detail_panel.ask_why_spawn')}
                </button>
                {#if interrogationAgentId}
                  <button class="entity-link" onclick={() => navigateTo('agent', interrogationAgentId)}>{$t('detail_panel.ask_why_view_agent')}</button>
                {/if}
              </div>
            </div>
          {:else}
            <p class="ask-why-unavailable">{$t('detail_panel.ask_why_unavailable')}</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* Full-page entity detail mode — takes up the entire content area */
  .detail-page {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
    background: var(--color-surface);
    min-height: 0;
  }

  .detail-page .panel-header {
    padding: var(--space-4) var(--space-6);
  }

  .detail-page .panel-content {
    max-width: 1200px;
    margin: 0 auto;
    width: 100%;
  }

  .detail-page .entity-id {
    font-size: var(--text-xl);
  }

  .detail-page .entity-type {
    font-size: var(--text-sm);
  }

  /* Side panel mode */
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
    padding: var(--space-4);
  }

  .ask-why-actions {
    text-align: center;
    padding: var(--space-4) 0;
    margin-top: var(--space-4);
    border-top: 1px solid var(--color-border);
  }

  .conversation-trace {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-height: 400px;
    overflow-y: auto;
    margin-bottom: var(--space-3);
  }

  .conv-turn {
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    border: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .conv-turn-user {
    border-left: 3px solid var(--color-warning);
  }

  .conv-turn-assistant {
    border-left: 3px solid var(--color-primary);
  }

  .conv-turn-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
  }

  .conv-turn-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .conv-turn-tool {
    font-size: var(--text-xs);
    background: var(--color-border);
    padding: 1px var(--space-1);
    border-radius: var(--radius-sm);
  }

  .conv-turn-content {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 150px;
    overflow-y: auto;
  }

  .conv-sha-info {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin-bottom: var(--space-2);
  }

  .conv-summary {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    margin-bottom: var(--space-2);
  }

  .conv-summary-stat {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    padding: 2px 8px;
    background: var(--color-surface);
    border-radius: var(--radius-sm);
  }

  .conv-meta {
    display: flex;
    gap: var(--space-3);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: var(--space-2) 0;
  }

  .conv-meta-label {
    font-weight: 600;
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

  .commit-investigate {
    margin-top: var(--space-4);
    padding: var(--space-3);
    background: color-mix(in srgb, var(--color-primary) 5%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 20%, transparent);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .investigate-btn {
    padding: var(--space-2) var(--space-4);
    background: var(--color-primary);
    color: var(--color-text-inverse);
    border: none;
    border-radius: var(--radius);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    align-self: flex-start;
    transition: background var(--transition-fast);
  }

  .investigate-btn:hover { background: var(--color-primary-hover); }
  .investigate-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .investigate-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
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

  .spec-rejected-banner {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-left: 3px solid var(--color-danger);
    border-radius: var(--radius);
    margin-bottom: var(--space-3);
  }

  .rejected-banner-icon {
    font-size: 18px;
    flex-shrink: 0;
    margin-top: 1px;
  }

  .rejected-banner-content {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .rejected-banner-title {
    font-weight: 600;
    font-size: var(--text-sm);
    color: var(--color-danger);
  }

  .rejected-banner-reason {
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .rejected-banner-by {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
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

  /* Rendered markdown styles */
  .spec-content-rendered {
    padding: var(--space-4);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    line-height: 1.7;
    color: var(--color-text);
  }

  .spec-content-rendered :global(.md-h1) { font-size: var(--text-xl); font-weight: 700; margin: 0 0 var(--space-3); padding-bottom: var(--space-2); border-bottom: 1px solid var(--color-border); }
  .spec-content-rendered :global(.md-h2) { font-size: var(--text-lg); font-weight: 600; margin: var(--space-4) 0 var(--space-2); }
  .spec-content-rendered :global(.md-h3) { font-size: var(--text-base); font-weight: 600; margin: var(--space-3) 0 var(--space-1); }
  .spec-content-rendered :global(.md-h4),
  .spec-content-rendered :global(.md-h5),
  .spec-content-rendered :global(.md-h6) { font-size: var(--text-sm); font-weight: 600; margin: var(--space-2) 0 var(--space-1); }
  .spec-content-rendered :global(.md-p) { margin: 0 0 var(--space-2); }
  .spec-content-rendered :global(.md-blockquote) { margin: 0 0 var(--space-2); padding: var(--space-2) var(--space-3); border-left: 3px solid var(--color-primary); background: color-mix(in srgb, var(--color-primary) 5%, transparent); color: var(--color-text-secondary); font-style: italic; }
  .spec-content-rendered :global(.md-list) { margin: 0 0 var(--space-2); padding-left: var(--space-5); }
  .spec-content-rendered :global(.md-list li) { margin: 0 0 var(--space-1); }
  .spec-content-rendered :global(.md-code) { font-family: var(--font-mono); font-size: var(--text-xs); background: var(--color-surface); padding: 1px 4px; border-radius: var(--radius-sm); border: 1px solid var(--color-border); }
  .spec-content-rendered :global(.md-codeblock) { margin: 0 0 var(--space-2); padding: var(--space-3); background: var(--color-surface); border: 1px solid var(--color-border); border-radius: var(--radius); overflow-x: auto; }
  .spec-content-rendered :global(.md-codeblock code) { font-family: var(--font-mono); font-size: var(--text-xs); line-height: 1.5; }
  .spec-content-rendered :global(.md-hr) { border: none; border-top: 1px solid var(--color-border); margin: var(--space-4) 0; }
  .spec-content-rendered :global(a) { color: var(--color-link); text-decoration: underline; text-underline-offset: 2px; }
  .spec-content-rendered :global(strong) { font-weight: 600; }
  .spec-content-rendered :global(em) { font-style: italic; }

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

  .task-linked-entities {
    margin-top: var(--space-3);
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
  /* ── Spec link diagram ──────────────────────────────────────────────── */
  .spec-link-diagram {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-4) var(--space-2);
    overflow-x: auto;
  }

  .link-column {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: center;
  }

  .link-arrows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: center;
    color: var(--color-text-muted);
    font-size: var(--text-lg);
  }

  .link-node {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-elevated, var(--color-bg));
    cursor: pointer;
    font-size: var(--text-xs);
    min-width: 80px;
    text-align: center;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }

  .link-node:hover {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 5%, var(--color-surface-elevated, var(--color-bg)));
  }

  .link-node-current {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 10%, var(--color-surface-elevated, var(--color-bg)));
    cursor: default;
    font-weight: 600;
  }

  .link-node-conflict {
    border-color: var(--color-danger);
  }

  .link-node-name {
    font-weight: 500;
    color: var(--color-text);
  }

  .link-node-kind {
    font-size: 10px;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

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
  .diff-stat-link {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: 2px var(--space-2);
    cursor: pointer;
    font: inherit;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }
  .diff-stat-link:hover {
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    border-color: color-mix(in srgb, var(--color-primary) 25%, transparent);
  }
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

  .diff-tree-actions {
    display: flex;
    gap: var(--space-2);
    width: 100%;
    margin-bottom: var(--space-1);
  }

  .diff-expand-btn {
    background: transparent;
    border: none;
    color: var(--color-link, var(--color-primary));
    font-size: var(--text-xs);
    font-family: var(--font-body);
    cursor: pointer;
    padding: 0;
  }

  .diff-expand-btn:hover { text-decoration: underline; }

  .diff-tree-item {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
  }

  .diff-tree-clickable {
    background: none;
    border: none;
    cursor: pointer;
    padding: 1px 2px;
    border-radius: var(--radius-sm);
    color: inherit;
    font: inherit;
    transition: background var(--transition-fast);
  }

  .diff-tree-clickable:hover {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
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
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .diff-tree-stats {
    display: flex;
    gap: 2px;
    font-size: 10px;
    flex-shrink: 0;
  }

  .diff-copy-path {
    font-size: 10px;
    padding: 0 4px;
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    font-family: var(--font-body);
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .diff-file-header:hover .diff-copy-path,
  .diff-copy-path:focus { opacity: 1; }
  .diff-copy-path:hover { border-color: var(--color-primary); color: var(--color-text); }

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

  .diff-blame-link {
    background: none;
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    padding: 1px var(--space-2);
    opacity: 0;
    transition: opacity var(--transition-fast), background var(--transition-fast);
    flex-shrink: 0;
  }
  .diff-file-header:hover .diff-blame-link { opacity: 1; }
  .diff-blame-link:hover {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    border-color: var(--color-primary);
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

  .diff-table {
    width: 100%;
    border-collapse: collapse;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    line-height: 1.5;
    max-height: 500px;
    overflow-y: auto;
    display: block;
  }
  .diff-table tbody { display: table; width: 100%; }
  .diff-tr { border: none; }
  .diff-gutter {
    width: 1px;
    min-width: 40px;
    padding: 0 var(--space-2);
    text-align: right;
    color: var(--color-text-muted);
    user-select: none;
    white-space: nowrap;
    vertical-align: top;
    border-right: 1px solid var(--color-border);
    opacity: 0.6;
  }
  .diff-gutter-old { border-right: none; }
  .diff-code {
    padding: 0 var(--space-3);
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--color-text);
  }
  .diff-prefix {
    user-select: none;
    color: var(--color-text-muted);
    display: inline-block;
    width: 1ch;
  }
  .diff-hunk-text { font-style: italic; }
  .diff-tr-add .diff-code { background: color-mix(in srgb, var(--color-success) 8%, transparent); }
  .diff-tr-add .diff-prefix { color: var(--color-success); font-weight: 600; }
  .diff-tr-add .diff-gutter { background: color-mix(in srgb, var(--color-success) 6%, transparent); }
  .diff-tr-del .diff-code { background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .diff-tr-del .diff-prefix { color: var(--color-danger); font-weight: 600; }
  .diff-tr-del .diff-gutter { background: color-mix(in srgb, var(--color-danger) 6%, transparent); }
  .diff-tr-hunk .diff-code { color: var(--color-info); font-weight: 500; background: color-mix(in srgb, var(--color-info) 6%, transparent); }
  .diff-tr-hunk .diff-gutter { background: color-mix(in srgb, var(--color-info) 4%, transparent); }
  .diff-tr-ctx:hover .diff-code,
  .diff-tr-add:hover .diff-code,
  .diff-tr-del:hover .diff-code { filter: brightness(0.95); }

  /* Syntax highlighting tokens in diffs */
  .diff-code :global(.hl-kw) { color: #c678dd; }
  .diff-code :global(.hl-str) { color: #98c379; }
  .diff-code :global(.hl-cmt) { color: #5c6370; font-style: italic; }
  .diff-code :global(.hl-num) { color: #d19a66; }

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
    align-items: flex-start;
    gap: var(--space-2);
  }

  .gate-name-block {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .gate-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .gate-description {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    line-height: 1.3;
  }

  .gate-item-required {
    border-left: 2px solid var(--color-primary);
  }

  .gate-item-required.gate-item-failed {
    border-left-color: var(--color-danger);
  }

  .gate-item-required.gate-item-passed {
    border-left-color: var(--color-success);
  }

  .gate-cmd-configured {
    opacity: 0.6;
  }

  .gate-cmd-hint {
    font-style: italic;
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }

  .gate-structured-output {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: var(--space-2);
    margin-bottom: var(--space-1);
  }

  .gate-metric {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
  }

  .gate-metric-success { color: var(--color-success); background: color-mix(in srgb, var(--color-success) 10%, transparent); }
  .gate-metric-fail { color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 10%, transparent); }
  .gate-metric-warn { color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 10%, transparent); }

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
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .gate-copy-btn {
    font-size: 10px;
    padding: 1px 6px;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    cursor: pointer;
    text-transform: none;
    letter-spacing: normal;
    margin-left: auto;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .gate-copy-btn:hover {
    background: var(--color-border);
    border-color: var(--color-primary);
  }

  .gate-no-output {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    margin: var(--space-1) 0 0 var(--space-4);
  }

  .gate-timing {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .gates-tab-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3);
    border-radius: var(--radius);
    border: 1px solid var(--color-border);
    margin-bottom: var(--space-3);
    background: var(--color-surface-elevated);
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .gates-tab-header.gates-all-passed {
    border-color: color-mix(in srgb, var(--color-success) 40%, transparent);
    background: color-mix(in srgb, var(--color-success) 8%, var(--color-surface-elevated));
  }

  .gates-tab-header.gates-all-passed .gates-tab-summary-icon {
    color: var(--color-success, #22c55e);
  }

  .gates-tab-header.gates-has-failures {
    border-color: color-mix(in srgb, var(--color-danger) 40%, transparent);
    background: color-mix(in srgb, var(--color-danger) 8%, var(--color-surface-elevated));
  }

  .gates-tab-header.gates-has-failures .gates-tab-summary-icon {
    color: var(--color-danger, #ef4444);
  }

  .gates-tab-summary-icon {
    font-size: var(--text-base);
    font-weight: 700;
    flex-shrink: 0;
    color: var(--color-text-muted);
  }

  .gates-tab-summary-text {
    color: var(--color-text);
  }

  .gate-status-badge {
    font-size: 10px;
    font-weight: 700;
    padding: 1px var(--space-2);
    border-radius: var(--radius-full);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .gate-status-badge-passed {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success, #22c55e);
  }

  .gate-status-badge-failed {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    color: var(--color-danger, #ef4444);
  }

  .gate-status-badge-running {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning, #f59e0b);
  }

  .gate-status-badge-pending {
    background: color-mix(in srgb, var(--color-text-muted) 12%, transparent);
    color: var(--color-text-muted);
  }

  .gate-pending-pulse {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    animation: gatePulse 1.5s ease-in-out infinite;
  }

  @keyframes gatePulse {
    0%, 100% { opacity: 0.3; }
    50% { opacity: 1; }
  }

  .gate-terminal {
    background: #1a1a2e;
    color: #d4d4d8;
    border-color: #2a2a3e;
    max-height: 250px;
    scrollbar-width: thin;
    scrollbar-color: #3a3a4e #1a1a2e;
  }

  .gate-terminal.gate-error {
    color: #fca5a5;
    background: #1a1017;
    border-color: #3d1f2e;
  }

  /* ── MR Attestation tab ──────────────────────────────────────────────────── */
  .attestation-block {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .att-verified-banner {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: color-mix(in srgb, var(--color-text-muted) 6%, transparent);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }
  .att-verified-banner.att-verified {
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 30%, transparent);
  }
  .att-verified-icon {
    flex-shrink: 0;
    color: var(--color-text-muted);
  }
  .att-verified-banner.att-verified .att-verified-icon {
    color: var(--color-success);
  }
  .att-verified-text {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 1;
    min-width: 0;
  }
  .att-verified-title {
    font-weight: 600;
    font-size: var(--text-sm);
    color: var(--color-text);
  }
  .att-version-badge {
    font-size: 10px;
    font-family: var(--font-mono);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }
  .att-merged-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .att-conv-arrow {
    font-size: var(--text-xs);
    color: var(--color-primary);
    margin-left: var(--space-1);
  }

  .att-sig-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .att-sig-copy-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    color: var(--color-text-muted);
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 2px 8px;
    cursor: pointer;
    transition: all var(--transition-fast);
  }
  .att-sig-copy-btn:hover {
    color: var(--color-text);
    border-color: var(--color-border-focus);
    background: var(--color-surface-elevated);
  }

  .att-completion-block {
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-info) 5%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-info) 20%, transparent);
    border-radius: var(--radius);
    margin-bottom: var(--space-2);
  }

  .att-completion-text {
    font-size: var(--text-sm);
    color: var(--color-text);
    margin: var(--space-1) 0 0;
    line-height: 1.5;
  }

  .att-meta-specs {
    margin-bottom: var(--space-2);
  }

  .att-meta-list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin-top: var(--space-1);
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

  .mr-diff-stats-banner {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    margin-bottom: var(--space-3);
    cursor: pointer;
    font-size: var(--text-sm);
    width: 100%;
    text-align: left;
    color: var(--color-text-secondary);
    transition: border-color 0.15s;
  }
  .mr-diff-stats-banner:hover {
    border-color: var(--color-border-focus);
    color: var(--color-text);
  }
  .diff-stats-banner-files {
    font-weight: 500;
  }
  .diff-stats-banner-ins {
    color: var(--color-success, #22c55e);
    font-weight: 600;
    font-family: var(--font-mono);
  }
  .diff-stats-banner-del {
    color: var(--color-danger, #ef4444);
    font-weight: 600;
    font-family: var(--font-mono);
  }

  .mr-agent-summary {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
    margin-bottom: var(--space-4);
  }

  .mr-agent-summary-text {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
    margin: 0;
    white-space: pre-wrap;
  }

  .spec-preview-section {
    margin: var(--space-4) 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  .spec-preview-summary {
    padding: var(--space-3) var(--space-4);
    cursor: pointer;
    background: var(--color-surface-elevated);
    border-radius: var(--radius);
  }

  .spec-preview-summary:hover {
    background: var(--color-surface-hover);
  }

  .spec-preview-body {
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
  }

  .graph-impact-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
  }

  .graph-impact-node {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .graph-impact-node:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-primary);
  }

  .graph-impact-type {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--color-text-muted);
    letter-spacing: 0.04em;
    flex-shrink: 0;
    min-width: 48px;
  }

  .graph-impact-name {
    font-weight: 500;
    color: var(--color-text);
  }

  .graph-impact-file {
    color: var(--color-text-muted);
    margin-left: auto;
    font-family: var(--font-mono);
    font-size: 10px;
  }

  .spec-preview-content {
    font-size: var(--text-xs);
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 300px;
    overflow-y: auto;
    color: var(--color-text-secondary);
    margin: 0;
  }

  .attestation-pending {
    text-align: center;
    padding: var(--space-6) var(--space-4);
  }

  .att-pending-icon {
    color: var(--color-text-muted);
    margin-bottom: var(--space-3);
  }

  .att-pending-title {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-3) 0;
  }

  .att-pending-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: var(--space-2) 0;
    max-width: 400px;
    margin-left: auto;
    margin-right: auto;
  }

  .att-pending-list {
    text-align: left;
    display: inline-block;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: var(--space-3) 0;
    padding-left: var(--space-6);
  }

  .att-pending-list li {
    margin-bottom: var(--space-1);
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
    font-size: var(--text-xs);
  }

  .trace-span-dur {
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  /* ── Trace waterfall (hierarchical flame graph style) ───────────────── */
  .trace-header-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-info) 6%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-info) 20%, transparent);
    border-radius: var(--radius);
    margin-bottom: var(--space-3);
  }
  .trace-header-left {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-secondary);
  }
  .trace-header-left svg {
    flex-shrink: 0;
    color: var(--color-info);
  }
  .trace-header-title {
    font-weight: 600;
    font-size: var(--text-sm);
  }
  .trace-header-roots {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }
  .trace-header-right {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .trace-header-duration {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    padding: 1px 8px;
    background: var(--color-surface-elevated);
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
  }

  .trace-waterfall {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .trace-waterfall-row {
    display: grid;
    grid-template-columns: minmax(180px, 40%) 1fr 60px;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--color-border);
    font-size: var(--text-xs);
    transition: background var(--transition-fast);
  }

  .trace-waterfall-row:last-child { border-bottom: none; }
  .trace-waterfall-row:hover { background: var(--color-surface-elevated); }

  .trace-waterfall-root {
    background: color-mix(in srgb, var(--color-success) 4%, transparent);
    font-weight: 500;
  }

  .trace-waterfall-info {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
    overflow: hidden;
  }

  .trace-tree-guide {
    display: inline-block;
    width: 8px;
    height: 12px;
    border-left: 1px solid var(--color-border-strong);
    border-bottom: 1px solid var(--color-border-strong);
    margin-right: 2px;
    flex-shrink: 0;
  }

  .trace-service-tag {
    font-size: 9px;
    padding: 0 4px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-elevated);
    color: var(--color-text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .trace-graph-link {
    font-size: 9px;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0 3px;
    flex-shrink: 0;
  }

  .trace-waterfall-bar-container {
    height: 14px;
    background: var(--color-surface-elevated);
    border-radius: 2px;
    overflow: hidden;
    position: relative;
  }

  .trace-waterfall-bar {
    height: 100%;
    border-radius: 2px;
    min-width: 2px;
    opacity: 0.7;
    transition: width var(--transition-fast);
  }

  .trace-waterfall-dur {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--color-text-muted);
    text-align: right;
    white-space: nowrap;
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
  .timeline-summary {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    margin-bottom: var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .timeline-summary-count {
    font-weight: 600;
    color: var(--color-text);
  }

  .timeline-summary-duration {
    font-family: var(--font-mono);
    color: var(--color-text-muted);
  }

  .timeline-summary-range {
    color: var(--color-text-muted);
    margin-left: auto;
  }

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
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2px solid var(--color-border-strong);
    background: var(--color-surface);
    flex-shrink: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .timeline-icon {
    width: 12px;
    height: 12px;
    color: var(--color-text-muted);
  }

  .timeline-dot-success { border-color: var(--color-success); background: color-mix(in srgb, var(--color-success) 20%, transparent); }
  .timeline-dot-success .timeline-icon { color: var(--color-success); }
  .timeline-dot-danger { border-color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 20%, transparent); }
  .timeline-dot-danger .timeline-icon { color: var(--color-danger); }
  .timeline-dot-warning { border-color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 20%, transparent); }
  .timeline-dot-warning .timeline-icon { color: var(--color-warning); }
  .timeline-dot-info { border-color: var(--color-info); background: color-mix(in srgb, var(--color-info) 20%, transparent); }
  .timeline-dot-info .timeline-icon { color: var(--color-info); }

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

  .conversation-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .conversation-item {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .conversation-item-review {
    border-left: 3px solid var(--color-info);
  }

  .conversation-item-approved {
    border-left-color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 5%, var(--color-surface-elevated));
  }

  .conversation-item-changes-requested {
    border-left-color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 5%, var(--color-surface-elevated));
  }

  .conversation-item-comment {
    border-left: 3px solid var(--color-border-strong);
  }

  .conversation-comment-icon {
    color: var(--color-text-muted);
    display: flex;
    align-items: center;
  }

  .conversation-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .conversation-author {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    font-weight: 500;
  }

  .conversation-verb {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .conversation-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: auto;
  }

  .conversation-submit {
    margin-top: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-4);
  }

  .conversation-submit-heading {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    margin-bottom: var(--space-1);
  }

  .conversation-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .review-submit-group {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .commits-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .commit-item {
    padding: var(--space-2) var(--space-3);
    border-left: 2px solid var(--color-border);
    transition: border-color var(--transition-fast);
  }

  .commit-item:hover {
    border-left-color: var(--color-primary);
  }

  .commit-header {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }

  .commit-message {
    font-size: var(--text-sm);
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .commit-meta {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-1);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .commit-author {
    color: var(--color-text-secondary);
  }

  .commit-author-icon {
    font-size: var(--text-xs);
  }

  .commit-time {
    margin-left: auto;
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

  .task-spec-banner {
    margin-bottom: var(--space-3);
  }

  .task-spec-banner-link {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-bg-secondary, var(--color-bg-hover));
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md, 8px);
    cursor: pointer;
    width: 100%;
    text-align: left;
    color: var(--color-text);
    font-size: var(--text-sm);
    transition: border-color 0.15s;
  }

  .task-spec-banner-link:hover {
    border-color: var(--color-accent, var(--color-primary));
  }

  .task-spec-banner-icon {
    font-size: 1.1em;
    flex-shrink: 0;
  }

  .task-spec-banner-name {
    font-weight: 600;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .task-spec-banner-arrow {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .priority-indicator {
    margin-right: var(--space-1);
    font-weight: 700;
  }

  .priority-critical { color: var(--color-danger, #e53e3e); }
  .priority-high { color: var(--color-danger, #e53e3e); }
  .priority-low { color: var(--color-text-muted); }

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

  /* ── MR status journey (prominent block at top of info tab) ────────── */
  .mr-status-journey {
    background: var(--color-surface-raised, var(--color-bg-alt));
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-3);
    margin-bottom: var(--space-3);
  }

  .status-journey-track {
    display: flex;
    align-items: flex-start;
    gap: 0;
    flex-wrap: wrap;
  }

  .status-journey-node {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    min-width: 60px;
    background: transparent;
    border: none;
    padding: var(--space-1);
    border-radius: var(--radius);
    cursor: default;
    font-family: inherit;
    transition: background var(--transition-fast);
  }

  .status-journey-clickable {
    cursor: pointer;
  }

  .status-journey-clickable:hover {
    background: var(--color-surface-elevated);
  }

  .status-journey-clickable:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
  }

  .status-journey-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--color-text-muted);
  }

  .status-journey-node-success .status-journey-dot { background: var(--color-success); }
  .status-journey-node-danger .status-journey-dot  { background: var(--color-danger); }
  .status-journey-node-warning .status-journey-dot { background: var(--color-warning); }
  .status-journey-node-info .status-journey-dot    { background: var(--color-info); }

  .status-journey-label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text);
    text-align: center;
    white-space: nowrap;
  }

  .status-journey-time {
    font-size: 10px;
    color: var(--color-text-muted);
    text-align: center;
  }

  .status-journey-connector {
    width: 20px;
    height: 2px;
    background: var(--color-border);
    margin-top: 4px;
    flex-shrink: 0;
  }

  .status-journey-sha {
    margin-top: var(--space-2);
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  /* ── SHA badge (code-like copyable badge) ──────────────────────────── */
  .sha-badge {
    display: inline-block;
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-info) 25%, transparent);
    color: var(--color-info);
    padding: 1px 6px;
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background 0.15s;
  }
  .sha-badge:hover {
    background: color-mix(in srgb, var(--color-info) 20%, transparent);
  }

  /* ── Attestation gate name list ────────────────────────────────────── */
  .att-gate-summary {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-wrap: wrap;
  }

  .att-gate-names {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
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

  .view-all-logs-btn {
    background: none;
    border: none;
    color: var(--color-primary);
    font-size: var(--text-xs);
    cursor: pointer;
    padding: var(--space-1) 0;
    text-align: left;
  }

  .view-all-logs-btn:hover {
    text-decoration: underline;
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
    flex-wrap: wrap;
  }

  .mr-quick-links {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
    padding: var(--space-3) 0;
    border-top: 1px solid var(--color-border);
    margin-top: var(--space-3);
  }

  .mr-explore-btn {
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-body);
    padding: var(--space-1) var(--space-3);
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .mr-explore-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 18%, transparent);
    border-color: var(--color-primary);
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
    background: none;
    border: none;
    cursor: pointer;
    font-family: var(--font-body);
  }

  .gate-detail-item:hover {
    color: var(--color-text);
    text-decoration: underline;
  }

  .gate-detail-item.gate-pass .gate-check { color: var(--color-success); }
  .gate-detail-item.gate-fail .gate-check { color: var(--color-danger); }
  .gate-check { font-weight: 600; width: 14px; text-align: center; }
  .gate-detail-name { font-weight: 500; }

  .gate-type-tag {
    font-size: 9px;
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-info) 12%, transparent);
    color: var(--color-info);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .gate-advisory-tag {
    font-size: 9px;
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-text-muted) 12%, transparent);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .gate-duration-tag {
    font-size: 9px;
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .gate-cmd-tag {
    font-size: 9px;
    color: var(--color-text-muted);
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .gate-inline-error {
    margin: -2px 0 4px 22px;
    padding: var(--space-1) var(--space-2);
    background: color-mix(in srgb, var(--color-danger) 6%, var(--color-surface));
    border: 1px solid color-mix(in srgb, var(--color-danger) 20%, var(--color-border));
    border-radius: var(--radius-sm);
  }

  .gate-inline-error-text {
    margin: 0;
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--color-danger);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 80px;
    overflow-y: auto;
    line-height: 1.3;
  }

  /* ── Touched paths ─────────────────────────────────────────────────────── */
  .touched-paths-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .touched-path {
    display: flex;
    align-items: center;
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    padding: 2px 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .touched-path-link {
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    border-radius: var(--radius-sm);
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .touched-path-link:hover {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    color: var(--color-primary);
  }

  .touched-path-more {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    padding: 2px 0;
  }

  /* ── Agent cost breakdown ───────────────────────────────────────────── */
  .cost-model-breakdown {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin-top: var(--space-1);
  }

  .cost-model-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    padding: 2px 0;
  }

  .cost-model-name {
    color: var(--color-text-secondary);
    min-width: 120px;
  }

  .cost-model-tokens {
    color: var(--color-text-muted);
  }

  .cost-model-cost {
    color: var(--color-text);
    font-family: var(--font-mono);
    margin-left: auto;
  }

  .cost-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-xs);
  }

  .cost-table th {
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--color-border);
    text-align: left;
  }

  .cost-table td {
    padding: var(--space-1) var(--space-2);
    color: var(--color-text-secondary);
  }

  .cost-col-right {
    text-align: right;
  }

  .cost-total-row {
    border-top: 1px solid var(--color-border);
    font-weight: 600;
  }

  .cost-total-row td {
    color: var(--color-text);
    padding-top: var(--space-2);
  }

  .agent-cost-summary {
    display: flex;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-4);
    background: var(--color-bg-secondary, var(--color-bg-hover));
    border-radius: var(--radius-md, 8px);
    border: 1px solid var(--color-border);
    margin-bottom: var(--space-3);
  }

  .cost-summary-item {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }

  .cost-summary-value {
    font-size: var(--text-lg, 1.125rem);
    font-weight: 700;
    color: var(--color-text);
    font-variant-numeric: tabular-nums;
  }

  .cost-summary-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .status-explain {
    display: block;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-top: 2px;
    font-style: italic;
  }

  /* Status Journey Stepper */
  .status-journey {
    display: flex;
    align-items: center;
    gap: 0;
    padding: var(--space-3) var(--space-2);
    margin-bottom: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow-x: auto;
  }

  .journey-step {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
    min-width: 60px;
  }

  .journey-dot {
    font-size: var(--text-sm);
    font-weight: 700;
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: var(--color-surface-elevated);
    border: 2px solid var(--color-border);
  }

  .journey-step-done .journey-dot {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    border-color: var(--color-success);
    color: var(--color-success);
  }

  .journey-step-failed .journey-dot {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    border-color: var(--color-danger);
    color: var(--color-danger);
  }

  .journey-step-active .journey-dot {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border-color: var(--color-primary);
    color: var(--color-primary);
  }

  .journey-step-pending .journey-dot {
    color: var(--color-text-muted);
  }

  .journey-label {
    font-size: 10px;
    font-weight: 600;
    color: var(--color-text);
  }

  .journey-detail {
    font-size: 9px;
    color: var(--color-text-muted);
  }

  .journey-connector {
    flex: 1;
    min-width: 20px;
    height: 2px;
    background: var(--color-border);
    margin: 0 2px;
    align-self: center;
    margin-bottom: 16px; /* offset for label height */
  }

  .journey-connector-done {
    background: var(--color-success);
  }

  .exit-code-explain {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: var(--space-1);
  }

  .touched-path-status {
    display: inline-block;
    width: 14px;
    text-align: center;
    font-weight: 700;
    margin-right: var(--space-1);
    flex-shrink: 0;
  }

  .touched-path-status-added { color: var(--color-success); }
  .touched-path-status-modified { color: var(--color-warning); }
  .touched-path-status-deleted { color: var(--color-danger); }

  .log-filter-bar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .log-filter-input {
    flex: 1;
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
  }

  .log-filter-input::placeholder {
    color: var(--color-text-muted);
  }

  .log-filter-input:focus {
    outline: none;
    border-color: var(--color-primary);
    box-shadow: 0 0 0 1px var(--color-primary);
  }

  .log-filter-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .log-level-pills {
    display: flex;
    gap: var(--space-1);
  }

  .log-level-pill {
    padding: 1px var(--space-2);
    font-size: 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    text-transform: uppercase;
    font-weight: 600;
    letter-spacing: 0.05em;
    white-space: nowrap;
  }

  .log-level-pill.active {
    border-color: var(--color-primary);
    background: var(--color-primary);
    color: white;
  }

  .log-level-pill.log-level-error { border-color: var(--color-danger); }
  .log-level-pill.log-level-error.active { background: var(--color-danger); }
  .log-level-pill.log-level-warn, .log-level-pill.log-level-warning { border-color: var(--color-warning); }
  .log-level-pill.log-level-warn.active, .log-level-pill.log-level-warning.active { background: var(--color-warning); }

  .log-level-badge {
    font-size: 9px;
    padding: 0 var(--space-1);
    border-radius: 2px;
    text-transform: uppercase;
    font-weight: 700;
    letter-spacing: 0.05em;
    flex-shrink: 0;
  }

  .log-level-badge.log-level-error { background: var(--color-danger); color: white; }
  .log-level-badge.log-level-warn, .log-level-badge.log-level-warning { background: var(--color-warning); color: var(--color-text); }
  .log-level-badge.log-level-info { background: var(--color-info, var(--color-primary)); color: white; opacity: 0.7; }
  .log-level-badge.log-level-debug { background: var(--color-border); color: var(--color-text-muted); }

  .trace-entry-error {
    background: rgba(255, 50, 50, 0.05);
    border-left: 2px solid var(--color-danger);
  }

  .trace-entry-warn {
    background: rgba(255, 200, 0, 0.05);
    border-left: 2px solid var(--color-warning);
  }

  /* ── Terminal-style log viewer ─────────────────────────────────────────── */
  .log-terminal {
    background: #0d1117;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    max-height: 500px;
    overflow-y: auto;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.5;
    padding: var(--space-1) 0;
  }

  .log-line {
    display: flex;
    gap: var(--space-2);
    padding: 1px var(--space-3);
    color: #c9d1d9;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .log-line:hover {
    background: rgba(255, 255, 255, 0.04);
  }

  .log-line-error {
    background: rgba(248, 81, 73, 0.08);
    border-left: 2px solid #f85149;
  }

  .log-line-warn {
    background: rgba(210, 153, 34, 0.08);
    border-left: 2px solid #d29922;
  }

  .log-ts {
    color: #484f58;
    white-space: nowrap;
    flex-shrink: 0;
    user-select: none;
  }

  .log-msg {
    color: #c9d1d9;
    word-break: break-word;
  }

  .log-line-error .log-msg { color: #f85149; }
  .log-line-warn .log-msg { color: #d29922; }

  .log-live-indicator {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: color-mix(in srgb, var(--color-success) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-success) 25%, transparent);
    border-radius: var(--radius);
    margin-bottom: var(--space-2);
  }

  .log-live-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-success);
    animation: log-pulse 1.5s ease-in-out infinite;
    flex-shrink: 0;
  }

  @keyframes log-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .log-live-text {
    font-size: var(--text-xs);
    color: var(--color-success);
    font-weight: 600;
  }

  /* ── MR deps section ───────────────────────────────────────────────────── */
  .mr-deps-section {
    margin-top: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .deps-explain {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
    font-style: italic;
  }

  .dep-arrow {
    font-weight: 700;
    font-size: var(--text-sm);
    flex-shrink: 0;
    width: 16px;
    text-align: center;
  }

  .dep-arrow-in { color: var(--color-warning); }
  .dep-arrow-out { color: var(--color-info, #1e90ff); }

  .dep-id {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
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

  /* ── History timeline ──────────────────────────────────────────────────── */
  .history-timeline {
    position: relative;
  }

  .history-timeline-item {
    position: relative;
    padding-left: var(--space-6);
  }

  .history-timeline-marker {
    position: absolute;
    left: 0;
    top: var(--space-2);
    width: 1.25rem;
    height: 1.25rem;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--color-surface-elevated);
    border: 2px solid var(--color-border);
    z-index: 1;
  }

  .history-timeline-marker.marker-approved {
    border-color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 15%, var(--color-surface-elevated));
  }

  .history-timeline-marker.marker-rejected {
    border-color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 15%, var(--color-surface-elevated));
  }

  .history-timeline-marker.marker-revoked {
    border-color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 15%, var(--color-surface-elevated));
  }

  .timeline-icon {
    font-size: 10px;
    font-weight: 700;
    line-height: 1;
  }

  .marker-approved .timeline-icon {
    color: var(--color-success);
  }

  .marker-rejected .timeline-icon {
    color: var(--color-danger);
  }

  .marker-revoked .timeline-icon {
    color: var(--color-warning);
  }

  .history-timeline-line {
    position: absolute;
    left: calc(0.625rem - 1px);
    top: calc(var(--space-2) + 1.25rem);
    bottom: calc(-1 * var(--space-1));
    width: 2px;
    background: var(--color-border);
  }

  .history-timeline-content {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  /* ── MR diff stats in progress tab ─────────────────────────────────────── */
  .mr-gate-summary {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .mr-diff-stats {
    font-size: var(--text-xs);
    display: inline-flex;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .diff-add {
    color: var(--color-success);
  }

  .diff-del {
    color: var(--color-danger);
  }

  /* ── Link type icons ───────────────────────────────────────────────────── */
  .link-type-icon {
    font-size: var(--text-sm);
    flex-shrink: 0;
    color: var(--color-text-muted);
  }

  .link-type-icon.link-type-conflict {
    color: var(--color-danger);
  }

  .link-type-icon.link-type-implements {
    color: var(--color-success);
  }

  .link-type-icon.link-type-extends {
    color: var(--color-info);
  }
</style>
