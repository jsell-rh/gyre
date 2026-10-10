<script>
  import ArchPreviewCanvas from './ArchPreviewCanvas.svelte';
  import ConcurrentEditBanner from './ConcurrentEditBanner.svelte';
  import SpecConflictDialog from './SpecConflictDialog.svelte';
  import SpecDiffView from './SpecDiffView.svelte';
  import { api } from './api.js';
  import { toastError, toastSuccess, toastInfo } from './toast.svelte.js';
  import { t } from 'svelte-i18n';

  /**
   * EditorSplit — full-width editor + architecture preview split layout
   * (ui-layout.md §2 Editor Split).
   *
   * Used by DetailPanel (spec editing pop-out) and MetaSpecs (meta-spec editing,
   * §9 preview loop). Left panel: spec textarea + inline LLM chat. Right panel:
   * architectural impact preview with Architecture (default) and Code Diff tabs.
   *
   * Spec ref: ui-layout.md §2 Editor Split,
   *           §3 LLM-Assisted Spec Editing (Accept/Edit/Dismiss),
   *           §9 Meta-specs Preview Loop (Editing → Preview Running → Preview Complete)
   *
   * Props:
   *   content       — string — current text content (bindable)
   *   onChange       — (newContent: string) => void — called on every change
   *   repoId        — string | null
   *   specPath      — string | null — e.g. 'specs/system/auth.md'
   *   ghostOverlays — array of { nodeId, type: 'new'|'modified'|'removed' }
   *   onClose       — () => void — called when user dismisses (Back or Esc)
   *   context       — 'spec' | 'meta-spec' — display label
   *   workspaceId   — string | null — workspace scope for the presence banner (HSI §7)
   *                   and the meta-spec preview endpoints (§9)
   *   wsStore       — WebSocket store ({ send, sessionId, onMessage }) for live presence
   *   selfUserId    — string | null — current user id (excludes own sessions)
   *   baseSha       — string | null — spec current_sha at load (optimistic concurrency)
   *   targetSpecs   — array of { path } — selectable preview targets (§9 State 1).
   *                   When provided (meta-spec context) the right panel shows the
   *                   target spec selector in the editing state.
   *   onpublish     — () => void — Publish hook (meta-spec approval flow, §9 State 3)
   */
  let {
    content = $bindable(''),
    onChange = undefined,
    repoId = null,
    specPath = null,
    ghostOverlays = [],
    onClose = undefined,
    context = 'spec',
    workspaceId = null,
    wsStore = null,
    selfUserId = null,
    baseSha = null,
    targetSpecs = [],
    onpublish = undefined,
  } = $props();

  // 409 conflict body from the last save attempt, or null (HSI §7).
  let specConflict = $state(null);

  // ── Graph data (lazy-loaded) ───────────────────────────────────────────────
  // The fast preview combines the repo's current graph (real nodes/edges from
  // the knowledge graph) with graphPredict ghost overlays (ui-layout.md §2
  // Editor Split — Phase 1 fast preview; Phase 2 thorough preview computes a
  // real ArchitecturalDelta server-side).
  let graphNodes = $state([]);
  let graphEdges = $state([]);
  let graphLoading = $state(false);
  let graphLoaded = $state(false);
  // Ghost overlays derived from graphPredict predictions — same normalization
  // DetailPanel uses (predictions → { nodeId, type }).
  let derivedOverlays = $state([]);

  // Effective overlays: caller-supplied ghosts (e.g. DetailPanel arch tab)
  // take precedence; otherwise the locally predicted ones.
  let overlays = $derived(ghostOverlays.length ? ghostOverlays : derivedOverlays);

  // ── Preview state machine (ui-layout.md §2/§9) ─────────────────────────────
  // editing → preview_running → preview_complete (Iterate returns to editing
  // with results still visible, §9 State 3). A preview that fails, times out,
  // or is unavailable never presents State 3 — it returns to editing (the
  // Architecture tab keeps the fast graphPredict overlays) with an error banner.
  let previewState = $state('editing');
  let previewError = $state(null);
  let selectedSpecPaths = $state([]);
  let previewPollTimer = null;
  let previewProgress = $state([]);
  let codeDiffFiles = $state([]); // [{ path, diff: [{op,text}] }]
  let previewRunning = $state(false);
  let activeImpactTab = $state('architecture');
  // Last meta-spec preview result (real PreviewResponse shape: preview_id,
  // state, specs, blast_radius, structural_impact) rendered in the right pane.
  let metaPreviewResult = $state(null);

  const selectableSpecs = $derived(
    Array.isArray(targetSpecs) ? targetSpecs.filter((s) => s?.path) : [],
  );

  $effect(() => {
    // Load graph once when repoId is available
    if (repoId && !graphLoaded && !graphLoading) {
      loadGraph();
    }
  });

  // Stop any preview poll when the component is destroyed.
  $effect(() => {
    return () => stopPreviewPoll();
  });

  async function loadGraph() {
    if (!repoId) return;
    graphLoading = true;
    try {
      // Base graph: real nodes/edges from the knowledge graph. The spec-path
      // filter mirrors the DetailPanel architecture tab: nodes governed by
      // this spec plus edges between them. Falls back to the full graph when
      // no node carries this spec_path (e.g. new/unlinked specs).
      let nodes = [];
      let edges = [];
      try {
        const graph = await api.repoGraph(repoId);
        const allNodes = graph?.nodes ?? [];
        const allEdges = graph?.edges ?? [];
        const specNodes = specPath ? allNodes.filter((n) => n.spec_path === specPath) : allNodes;
        if (specNodes.length) {
          const specNodeIds = new Set(specNodes.map((n) => n.id));
          edges = allEdges.filter(
            (e) => specNodeIds.has(e.source_id ?? e.source) && specNodeIds.has(e.target_id ?? e.target),
          );
        } else {
          specNodes.push(...allNodes);
          edges = allEdges;
        }
        nodes = specNodes;
      } catch {
        // graceful: fall through to predictions only
      }

      // Fast preview: ask the LLM what the draft would change structurally,
      // and render those predictions as ghost overlays on the real graph.
      // Prediction-only nodes (LLM-invented names) are appended so the
      // canvas isn't empty for specs with no graph linkage yet.
      try {
        const result = await api.graphPredict(repoId, {
          spec_path: specPath,
          draft_content: content || undefined,
        });
        const predictions = result?.predictions ?? result?.overlays ?? [];
        const predOverlays = predictions
          .map((p) => ({
            nodeId: p.node_id ?? p.nodeId ?? p.name ?? p.qualified_name,
            type: p.change_type ?? p.type ?? 'modified',
          }))
          .filter((p) => p.nodeId);
        derivedOverlays = predOverlays;
        if (nodes.length) {
          // Match predictions to existing graph nodes by name; append
          // synthetic ghost nodes for unmatched predictions.
          const byName = new Map(nodes.map((n) => [n.name ?? n.label ?? n.id, n]));
          const synthetic = [];
          for (const p of predOverlays) {
            const existing = byName.get(p.nodeId);
            if (!existing && p.type !== 'removed') {
              synthetic.push({ id: p.nodeId, name: p.nodeId, node_type: p.node_type ?? 'type' });
            }
          }
          nodes = [...nodes, ...synthetic];
        } else {
          nodes = predOverlays
            .filter((p) => p.type !== 'removed')
            .map((p) => ({ id: p.nodeId, name: p.nodeId, node_type: 'type' }));
          edges = [];
        }
      } catch {
        // graceful: show base graph without ghost overlays; user can still edit
      }

      graphNodes = nodes;
      graphEdges = edges;
      graphLoaded = true;
    } finally {
      graphLoading = false;
    }
  }

  // ── Preview workflow (§2/§9) ────────────────────────────────────────────────

  function toggleSpec(path) {
    if (selectedSpecPaths.includes(path)) {
      selectedSpecPaths = selectedSpecPaths.filter((p) => p !== path);
    } else {
      selectedSpecPaths = [...selectedSpecPaths, path];
    }
  }

  function selectAllSpecs() {
    selectedSpecPaths = selectableSpecs.map((s) => s.path);
  }

  function clearAllSpecs() {
    selectedSpecPaths = [];
  }

  const canRunPreview = $derived.by(() => {
    if (previewState !== 'editing' || previewRunning) return false;
    // Meta-spec context: at least one target spec must be selected (§9 State 1).
    if (context === 'meta-spec') return selectedSpecPaths.length > 0;
    // Spec context: needs a repo + spec path.
    return Boolean(repoId && specPath);
  });

  /**
   * Start the thorough preview: the server commits the draft to a throwaway
   * branch, agents implement, and the resulting graph delta + code diff are
   * reported (ui-layout.md §2 Editor Split right panel).
   */
  async function runPreview() {
    if (!canRunPreview) return;
    previewRunning = true;
    previewError = null;
    stopPreviewPoll();
    try {
      if (context === 'meta-spec' && workspaceId) {
        // Meta-spec preview loop (§9): preview the persona change against the
        // selected target specs. Real PreviewResponse shape: { preview_id,
        // state, specs, blast_radius, structural_impact }.
        let res;
        try {
          res = await api.previewPersona(workspaceId, {
            persona_id: specPath ?? undefined,
            content,
            spec_paths: selectedSpecPaths,
          });
        } catch (e) {
          // Preview service unavailable — return to editing; never present
          // §9 State 3 without a real result.
          failPreview($t('editor_split.preview_unavailable'), e);
          return;
        }
        if (res?.preview_id) {
          pollPreviewPersona(res.preview_id, res);
        } else if (res) {
          applyPreviewResult(res);
          finishPreview();
        } else {
          failPreview($t('editor_split.preview_unavailable'));
        }
      } else if (repoId && specPath) {
        // Spec preview: thorough preview on a throwaway branch.
        const result = await api.thoroughPreview(repoId, {
          spec_path: specPath,
          draft_content: content,
        });
        if (result?.task_id) {
          pollPreviewTask(result.task_id);
        } else if (result) {
          applyPreviewResult(result);
          finishPreview();
        } else {
          failPreview($t('editor_split.preview_fallback'));
        }
      }
    } catch (e) {
      // Thorough preview unavailable — back to editing; the Architecture tab
      // keeps the fast graphPredict overlays (Phase 1, ui-layout.md §2).
      failPreview($t('editor_split.preview_fallback'), e);
    } finally {
      previewRunning = false;
    }
  }

  function pollPreviewTask(taskId) {
    previewState = 'preview_running';
    previewProgress = selectedSpecPaths.length
      ? selectedSpecPaths.map((path) => ({ path, status: 'running' }))
      : [{ path: specPath ?? '', status: 'running' }];
    let elapsed = 0;
    previewPollTimer = setInterval(async () => {
      elapsed += 10000;
      try {
        const status = await api.taskStatus(taskId);
        if (status?.status === 'completed') {
          applyPreviewResult(status);
          finishPreview();
          return;
        }
        if (status?.status === 'failed') {
          failPreview(
            $t('editor_split.preview_failed', { values: { error: status?.error ?? 'agent execution failed' } }),
          );
          return;
        }
      } catch {
        // transient poll error — keep polling
      }
      if (elapsed >= 300000) {
        failPreview($t('editor_split.preview_timeout'));
      }
    }, 10000);
  }

  function pollPreviewPersona(previewId, initial) {
    previewState = 'preview_running';
    previewProgress = (initial?.specs?.length ? initial.specs : selectedSpecPaths).map?.((pathOrSpec) =>
      typeof pathOrSpec === 'string' ? { path: pathOrSpec, status: 'running' } : pathOrSpec,
    ) ?? [];
    if (!previewProgress.length) {
      previewProgress = selectedSpecPaths.map((path) => ({ path, status: 'running' }));
    }
    if (initial) applyPreviewResult(initial);
    let elapsed = 0;
    previewPollTimer = setInterval(async () => {
      elapsed += 1500;
      try {
        const status = await api.previewPersonaStatus(workspaceId, previewId);
        previewProgress = status.specs ?? previewProgress;
        if (status.state === 'complete') {
          applyPreviewResult(status);
          finishPreview();
          return;
        }
        if (status.state === 'failed' || status.state === 'error') {
          failPreview($t('editor_split.preview_failed', { values: { error: status.state } }));
          return;
        }
      } catch (e) {
        // Status endpoint failing mid-poll is a failed preview, not a
        // completed one — back to editing.
        failPreview($t('editor_split.preview_unavailable'), e);
        return;
      }
      if (elapsed >= 30000) {
        failPreview($t('editor_split.preview_timeout'));
      }
    }, 1500);
  }

  /**
   * Map a completed preview result onto the Architecture (ghost overlays /
   * node delta) and Code Diff tabs. Accepts the shapes returned by
   * thoroughPreview/taskStatus ({predictions, code_diff}) and the real
   * previewPersona PreviewResponse ({preview_id, state, specs,
   * blast_radius, structural_impact}) — the meta-spec result is kept whole
   * for the right-pane summary (§9 State 3).
   */
  function applyPreviewResult(result) {
    if (!result) return;
    // Meta-spec preview results (real server shape) are stored for the
    // Architecture pane summary and per-spec Code Diff listing.
    if (result.preview_id) {
      metaPreviewResult = result;
    }
    // Structural delta → ghost overlays on the Architecture tab.
    const preds = result.predictions ?? [];
    if (Array.isArray(preds) && preds.length) {
      derivedOverlays = preds
        .map((p) => ({
          nodeId: p.node_id ?? p.nodeId ?? p.name ?? p.qualified_name,
          type: p.change_type ?? p.type ?? p.action ?? 'modified',
        }))
        .filter((p) => p.nodeId);
      // Unmatched prediction-only nodes get appended in loadGraph; for the
      // thorough result, append synthetic nodes so the delta is visible even
      // without a graph linkage.
      if (graphNodes.length) {
        const byName = new Set(graphNodes.map((n) => n.name ?? n.id));
        const synthetic = derivedOverlays
          .filter((p) => p.type !== 'removed' && !byName.has(p.nodeId))
          .map((p) => ({ id: p.nodeId, name: p.nodeId, node_type: 'type' }));
        if (synthetic.length) graphNodes = [...graphNodes, ...synthetic];
      } else {
        graphNodes = derivedOverlays
          .filter((p) => p.type !== 'removed')
          .map((p) => ({ id: p.nodeId, name: p.nodeId, node_type: 'type' }));
      }
    }
    // Line-level diff → Code Diff tab.
    const files = result.code_diff ?? result.specs_diff ?? [];
    if (Array.isArray(files) && files.length) {
      codeDiffFiles = files.map((f) => ({
        path: f.path ?? f.spec_path ?? '',
        diff: Array.isArray(f.diff)
          ? f.diff
          : [{ op: 'context', text: String(f.diff ?? '') }],
      }));
    }
    if (result.specs?.length) previewProgress = result.specs;
  }

  function finishPreview() {
    stopPreviewPoll();
    previewState = 'preview_complete';
  }

  /**
   * A failed/unavailable/timed-out preview: back to the editing state with an
   * error banner in the right pane. The Architecture tab keeps whatever the
   * fast graphPredict pass loaded — §9 State 3 is never presented without a
   * real completed result.
   */
  function failPreview(message, err) {
    stopPreviewPoll();
    previewState = 'editing';
    previewProgress = [];
    previewError = err
      ? `${message} (${err?.message ?? err})`
      : message;
    if (err) console.warn('[EditorSplit] preview failed:', err);
  }

  function cancelPreview() {
    stopPreviewPoll();
    previewState = 'editing';
    previewProgress = [];
  }

  /** Iterate (§9 State 3): back to editing with results still visible. */
  function iteratePreview() {
    stopPreviewPoll();
    previewState = 'editing';
  }

  function stopPreviewPoll() {
    if (previewPollTimer) {
      clearInterval(previewPollTimer);
      previewPollTimer = null;
    }
  }

  // ── LLM chat ───────────────────────────────────────────────────────────────
  let llmInstruction = $state('');
  let llmStreaming = $state(false);
  let llmExplanation = $state('');
  let llmSuggestion = $state(null); // { diff: [...], explanation: string } | null
  let saving = $state(false);

  async function sendLlmInstruction() {
    if (!llmInstruction.trim() || llmStreaming) return;
    if (!repoId) return;
    const instruction = llmInstruction.trim();
    llmInstruction = '';
    llmStreaming = true;
    llmExplanation = '';
    llmSuggestion = null;

    try {
      const resp = await api.specsAssist(repoId, {
        spec_path: specPath,
        instruction,
        draft_content: content || undefined,
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
            // Track the SSE event type from `event:` lines (server format:
            // `event: partial\ndata: {...}\n\n` — see specs_assist.rs).
            if (line.startsWith('event:')) {
              currentEvent = line.slice(6).trim();
              continue;
            }
            if (!line.startsWith('data: ') && !line.startsWith('data:')) continue;
            const raw = line.startsWith('data: ') ? line.slice(6) : line.slice(5);
            if (raw === '[DONE]') { done = true; break; }
            try {
              const parsed = JSON.parse(raw);
              const evtType = parsed.event ?? parsed.type ?? currentEvent;
              if (evtType === 'partial' || (!evtType && parsed.text != null)) {
                llmExplanation += parsed.text ?? parsed.explanation ?? '';
              } else if (evtType === 'complete') {
                llmSuggestion = {
                  diff: parsed.diff ?? [],
                  explanation: parsed.explanation ?? llmExplanation,
                };
                done = true; break;
              } else if (evtType === 'error') {
                throw new Error(parsed.error ?? parsed.message ?? 'LLM error');
              }
            } catch (pe) {
              if (pe.message && !pe.message.startsWith('Unexpected token')) throw pe;
            }
            currentEvent = '';
          }
        }
      }
    } catch (e) {
      toastError($t('editor_split.llm_assist_failed', { values: { error: e.message } }));
    } finally {
      llmStreaming = false;
    }
  }

  /**
   * Apply the LLM's diff ops to the editor content (in-memory only — the
   * human must still Save; ui-layout.md §3 step 4).
   */
  function applyDiffOps(c, diff) {
    let out = c;
    for (const op of diff ?? []) {
      if (op.op === 'add') {
        const idx = out.indexOf(op.path);
        if (idx !== -1) {
          const lineEnd = out.indexOf('\n', idx + op.path.length);
          const insertAt = lineEnd !== -1 ? lineEnd + 1 : out.length;
          out = out.slice(0, insertAt) + op.content + '\n' + out.slice(insertAt);
        } else {
          out += '\n' + op.content;
        }
      } else if (op.op === 'replace') {
        const idx = out.indexOf(op.path);
        if (idx !== -1) {
          const end = findSectionEnd(out, idx + op.path.length);
          out = out.slice(0, idx) + op.path + '\n' + op.content + out.slice(end);
        }
      } else if (op.op === 'remove') {
        const idx = out.indexOf(op.path);
        if (idx !== -1) {
          const end = findSectionEnd(out, idx + op.path.length);
          out = out.slice(0, idx) + out.slice(end);
        }
      }
    }
    return out;
  }

  function acceptSuggestion() {
    if (!llmSuggestion) return;
    const c = applyDiffOps(content, llmSuggestion.diff);
    content = c;
    onChange?.(c);
    llmSuggestion = null;
  }

  // Edit: copy the suggested text into the editor for manual refinement
  // (ui-layout.md §3 LLM-Assisted Spec Editing step 5).
  function editSuggestion() {
    if (!llmSuggestion) return;
    if (llmSuggestion.diff?.length) {
      const c = content + '\n\n' + llmSuggestion.diff.map((d) => d.content).filter(Boolean).join('\n\n');
      content = c;
      onChange?.(c);
    }
    llmSuggestion = null;
  }

  function dismissSuggestion() { llmSuggestion = null; }

  function findSectionEnd(c, from) {
    const rest = c.slice(from);
    const match = rest.match(/\n(#{1,6} )/);
    if (match?.index !== undefined) return from + match.index + 1;
    return c.length;
  }

  async function saveSpec({ overwrite = false } = {}) {
    if (!repoId || !specPath || saving) return;
    saving = true;
    try {
      const result = await api.specsSave(repoId, {
        spec_path: specPath,
        content,
        message: `Update ${specPath} via editor split`,
        base_sha: baseSha ?? undefined,
        overwrite,
      });
      if (result?.conflict) {
        // Another editor's change landed since load — surface the conflict dialog.
        specConflict = result.conflict;
        return;
      }
      specConflict = null;
      toastSuccess($t('editor_split.spec_saved', { values: { mr_id: result.mr_id } }));
    } catch (e) {
      toastError($t('editor_split.save_failed', { values: { error: e.message } }));
    } finally {
      saving = false;
    }
  }

  // Conflict dialog resolvers (HSI §7).
  function conflictOverwrite() {
    saveSpec({ overwrite: true });
  }

  function conflictDiscard() {
    // Take the server's current version, dropping local edits.
    const serverContent = specConflict?.current_content ?? '';
    content = serverContent;
    onChange?.(serverContent);
    specConflict = null;
  }

  function conflictClose() {
    specConflict = null;
  }

  let textareaEl = $state(null);

  // Markdown toolbar (ui-layout.md §2 "standard markdown editor with
  // toolbar"): each action wraps or prefixes the textarea selection.
  const TOOLBAR_BUTTONS = [
    { id: 'bold', label: 'B', wrap: '**' },
    { id: 'italic', label: 'I', wrap: '_' },
    { id: 'heading', label: 'H', wrap: '## ', prefix: true },
    { id: 'list', label: '•', wrap: '- ', prefix: true },
    { id: 'code', label: '</>', wrap: '`' },
    { id: 'link', label: '🔗', wrap: null, template: '[title](url)' },
    { id: 'quote', label: '"', wrap: null, template: '> ' },
    { id: 'undo', label: '⤺', action: 'undo' },
    { id: 'redo', label: '⤻', action: 'redo' },
  ];

  /**
   * Apply a toolbar action to the textarea selection and update content.
   * Boring by design: wrap/prefix/template only, no parsing.
   */
  function applyToolbarAction(btn) {
    if (!textareaEl) return;
    const { selectionStart: start, selectionEnd: end } = textareaEl;
    if (btn.action === 'undo') {
      textareaEl.focus();
      document.execCommand('undo');
      return;
    }
    if (btn.action === 'redo') {
      textareaEl.focus();
      document.execCommand('redo');
      return;
    }
    if (btn.template) {
      const insert = btn.template;
      const next = content.slice(0, start) + insert + content.slice(end);
      content = next;
      onChange?.(next);
      queueMicrotask(() => {
        textareaEl.focus();
        textareaEl.setSelectionRange(start + insert.length, start + insert.length);
      });
      return;
    }
    const wrap = btn.wrap ?? '';
    const prefixMode = Boolean(btn.prefix);
    let next;
    let selStart;
    let selEnd;
    if (prefixMode) {
      // Insert at line start(s): walk back to the line start (or each
      // selected line's start) and prepend.
      const lineStart = content.lastIndexOf('\n', start - 1) + 1;
      const selected = content.slice(lineStart, end);
      const prefixed = selected.replace(/^/gm, wrap);
      next = content.slice(0, lineStart) + prefixed + content.slice(end);
      selStart = start + wrap.length;
      selEnd = end + wrap.length * (selected.split('\n').length);
    } else {
      const selected = content.slice(start, end);
      next = content.slice(0, start) + wrap + selected + wrap + content.slice(end);
      selStart = start + wrap.length;
      selEnd = selStart + selected.length;
    }
    content = next;
    onChange?.(next);
    queueMicrotask(() => {
      textareaEl.focus();
      textareaEl.setSelectionRange(selStart, selEnd);
    });
  }

  function handleContentInput(e) {
    content = e.target.value;
    onChange?.(content);
  }

  function handleKeydown(e) {
    if (e.key !== 'Escape') return;
    if (e._escConsumed) return; // handled by an inner surface (conflict dialog)
    e._escConsumed = true;      // DetailPanel's panel div must not also react
    e.preventDefault();
    e.stopPropagation();
    onClose?.();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="editor-split" role="region" aria-label={$t('editor_split.editor_split_view')}>
  <!-- Back button -->
  <div class="split-header">
    <button class="back-btn" onclick={() => onClose?.()} aria-label={$t('editor_split.close_editor')}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <path d="M19 12H5M12 19l-7-7 7-7"/>
      </svg>
      {$t('editor_split.back')}
    </button>
    <span class="split-label">{context === 'meta-spec' ? $t('editor_split.meta_spec_editor') : $t('editor_split.spec_editor')}{specPath ? ` — ${specPath}` : ''}</span>
    <div class="header-actions">
      {#if previewState === 'preview_complete'}
        <button class="preview-btn" onclick={iteratePreview} data-testid="iterate-btn">{$t('editor_split.iterate')}</button>
        {#if context === 'meta-spec' && onpublish}
          <button class="save-btn" onclick={() => onpublish?.()} data-testid="publish-btn">{$t('editor_split.publish')}</button>
        {/if}
      {:else if previewState !== 'preview_running'}
        {#if context === 'meta-spec' || (repoId && specPath)}
          <button
            class="preview-btn"
            onclick={runPreview}
            disabled={!canRunPreview || previewRunning}
            aria-busy={previewRunning}
            data-testid="preview-btn"
          >
            {previewRunning ? $t('editor_split.preview_starting') : $t('editor_split.preview')}
          </button>
        {/if}
        {#if context === 'meta-spec' && onpublish}
          <button class="save-btn" onclick={() => onpublish?.()} data-testid="publish-btn">{$t('editor_split.publish')}</button>
        {/if}
      {/if}
      {#if repoId && specPath && context === 'spec'}
        <button
          class="save-btn"
          onclick={() => saveSpec()}
          disabled={saving || !content.trim()}
          aria-busy={saving}
        >
          {saving ? $t('editor_split.saving') : $t('editor_split.save_create_mr')}
        </button>
      {/if}
    </div>
  </div>

  <!-- Split panes -->
  <div class="split-panes">
    <!-- Left: Editor + LLM chat -->
    <div class="pane pane-left">
      {#if context === 'spec' && specPath && workspaceId}
        <ConcurrentEditBanner {specPath} {workspaceId} {wsStore} {selfUserId} />
      {/if}
      {#if previewState === 'preview_running'}
        <!-- Editor locked during preview (§9 State 2): the draft is shown
             read-only — verbatim, no diff styling (the draft is markdown,
             not a diff). -->
        <div class="locked-editor" role="region" aria-label={$t('editor_split.editor_locked_aria')} data-testid="editor-locked">
          {#each content.split('\n') as line}
            <div class="locked-line">{line}</div>
          {/each}
        </div>
      {:else}
        <div class="md-toolbar" role="toolbar" aria-label={$t('editor_split.md_toolbar_aria')} data-testid="md-toolbar">
          {#each TOOLBAR_BUTTONS as btn (btn.id)}
            <button
              class="md-btn"
              type="button"
              onclick={() => applyToolbarAction(btn)}
              aria-label={btn.aria ?? btn.label}
              title={btn.aria ?? btn.label}
              data-testid={`md-${btn.id}`}
            >{btn.label}</button>
          {/each}
        </div>
        <textarea
          class="split-textarea"
          value={content}
          oninput={handleContentInput}
          placeholder={$t('editor_split.spec_placeholder')}
          aria-label={$t('editor_split.spec_editor')}
          spellcheck="false"
          bind:this={textareaEl}
          data-testid="editor-split-textarea"
        ></textarea>
      {/if}

      {#if llmSuggestion}
        <div class="suggestion-block" role="region" aria-label={$t('editor_split.llm_suggestion')} data-testid="llm-suggestion">
          <div class="suggestion-hdr">
            <span class="suggestion-lbl">{$t('editor_split.suggested_change')}</span>
            <button class="dismiss-btn" onclick={dismissSuggestion} aria-label={$t('editor_split.dismiss_suggestion')}>✕</button>
          </div>
          {#if llmSuggestion.explanation}
            <p class="suggestion-expl">{llmSuggestion.explanation}</p>
          {/if}
          {#if llmSuggestion.diff?.length}
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
            <button class="accept-btn" onclick={acceptSuggestion} data-testid="accept-suggestion">{$t('editor_split.accept')}</button>
            {#if llmSuggestion.diff?.length}
              <button class="dismiss-btn-sm" onclick={editSuggestion} data-testid="edit-suggestion">{$t('editor_split.edit')}</button>
            {/if}
            <button class="dismiss-btn-sm" onclick={dismissSuggestion} data-testid="dismiss-suggestion">{$t('common.dismiss')}</button>
          </div>
        </div>
      {/if}

      {#if llmStreaming && llmExplanation}
        <div class="llm-streaming" aria-live="polite">
          <span class="streaming-lbl">{$t('editor_split.thinking')}</span>
          <p class="streaming-txt">{llmExplanation}<span class="blink-cursor" aria-hidden="true"></span></p>
        </div>
      {/if}

      <div class="llm-input-area">
        <div class="recipient-line">
          {$t('editor_split.edit_label')} {context === 'meta-spec' ? $t('editor_split.meta_spec') : $t('editor_split.spec')}{specPath ? `: "${specPath}" ▸` : ' ▸'}
        </div>
        <div class="llm-row">
          <textarea
            class="llm-textarea"
            bind:value={llmInstruction}
            placeholder={$t('editor_split.llm_placeholder')}
            rows="2"
            disabled={llmStreaming || !repoId}
            onkeydown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); sendLlmInstruction(); } }}
            aria-label={$t('editor_split.llm_instruction')}
            data-testid="llm-input"
          ></textarea>
          <button
            class="llm-send"
            onclick={sendLlmInstruction}
            disabled={!llmInstruction.trim() || llmStreaming || !repoId}
            aria-label={$t('editor_split.send_to_llm')}
            aria-busy={llmStreaming}
            data-testid="llm-send-btn"
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
            <span class="sr-only">{$t('editor_split.send')}</span>
          </button>
        </div>
        {#if !repoId}
          <p class="llm-hint warn">{$t('editor_split.llm_requires_repo')}</p>
        {:else}
          <p class="llm-hint">{$t('editor_split.llm_hint')}</p>
        {/if}
      </div>
    </div>

    <!-- Divider -->
    <div class="split-divider" aria-hidden="true"></div>

    <!-- Right: Architecture preview / preview states -->
    <div class="pane pane-right" data-testid="arch-preview-pane">
      {#if context === 'meta-spec' && previewState === 'editing' && selectableSpecs.length}
        <!-- Target spec selector (§9 State 1) -->
        <div class="spec-selector" data-testid="spec-selector">
          <div class="spec-selector-header">
            <span class="pane-title">{$t('editor_split.preview_against')}</span>
            <div class="spec-selector-shortcuts">
              <button class="link-btn" onclick={selectAllSpecs}>{$t('editor_split.select_all')}</button>
              <button class="link-btn" onclick={clearAllSpecs}>{$t('editor_split.clear')}</button>
            </div>
          </div>
          <div class="spec-checklist">
            {#each selectableSpecs as spec (spec.path)}
              <label class="spec-check-item">
                <input
                  type="checkbox"
                  checked={selectedSpecPaths.includes(spec.path)}
                  onchange={() => toggleSpec(spec.path)}
                />
                <span class="spec-check-path">{spec.path}</span>
              </label>
            {/each}
          </div>
        </div>
      {:else if previewState === 'preview_running'}
        <!-- Preview Running (§9 State 2): per-spec progress indicators -->
        <div class="preview-progress" data-testid="preview-running" aria-live="polite">
          <div class="progress-header" role="status">{$t('editor_split.preview_running')}</div>
          <div class="progress-list">
            {#each previewProgress as item (item.path)}
              <div class="progress-item">
                <span class="progress-icon" aria-hidden="true">{item.status === 'complete' ? '✓' : '◐'}</span>
                <span class="progress-path">{item.path}</span>
                <span class="progress-status">{item.status === 'complete' ? $t('editor_split.status_complete') : $t('editor_split.agent_implementing')}</span>
              </div>
            {/each}
          </div>
          <div class="progress-summary">{$t('editor_split.progress_label', { values: { done: previewProgress.filter((p) => p.status === 'complete').length, total: previewProgress.length } })}</div>
          <button class="dismiss-btn-sm cancel-preview" onclick={cancelPreview} data-testid="cancel-preview">{$t('editor_split.cancel_preview')}</button>
        </div>
      {:else}
        <!-- Preview Complete / spec context: Architecture (default) + Code Diff tabs -->
        {#if previewError}
          <div class="preview-error" role="alert" data-testid="preview-error">
            <strong>{$t('editor_split.preview_error_label')}:</strong> {previewError}
          </div>
        {/if}
        <div class="pane-header">
          <div class="impact-tabs" role="tablist" aria-label={$t('editor_split.impact_view_aria')}>
            <button
              class="impact-tab"
              role="tab"
              id="editor-split-tab-arch"
              aria-controls="editor-split-panel-arch"
              aria-selected={activeImpactTab === 'architecture'}
              class:active={activeImpactTab === 'architecture'}
              tabindex={activeImpactTab === 'architecture' ? 0 : -1}
              onclick={() => (activeImpactTab = 'architecture')}
              data-testid="tab-architecture"
            >{$t('editor_split.architecture')}</button>
            <button
              class="impact-tab"
              role="tab"
              id="editor-split-tab-diff"
              aria-controls="editor-split-panel-diff"
              aria-selected={activeImpactTab === 'code-diff'}
              class:active={activeImpactTab === 'code-diff'}
              tabindex={activeImpactTab === 'code-diff' ? 0 : -1}
              onclick={() => (activeImpactTab = 'code-diff')}
              data-testid="tab-code-diff"
            >{$t('editor_split.code_diff')}</button>
          </div>
          {#if graphLoading}
            <span class="loading-chip" aria-live="polite">{$t('editor_split.loading_graph')}</span>
          {:else if overlays.length}
            <span class="overlay-chip">{$t('editor_split.predicted_changes', { values: { count: overlays.length } })}</span>
          {/if}
        </div>
        {#if activeImpactTab === 'architecture'}
          <div
            class="canvas-wrap"
            role="tabpanel"
            id="editor-split-panel-arch"
            aria-labelledby="editor-split-tab-arch"
          >
            {#if context === 'meta-spec' && metaPreviewResult}
              <!-- Meta-spec preview (§9 State 3): the real PreviewResponse —
                   structural impact + blast radius the server computed. -->
              <div class="meta-impact" data-testid="meta-impact">
                <div class="meta-impact-section">
                  <h4 class="meta-impact-title">{$t('editor_split.architecture')}</h4>
                  <dl class="meta-impact-list">
                    <div class="meta-impact-row"><dt>{$t('editor_split.impact_scope')}</dt><dd>{metaPreviewResult.structural_impact?.scope ?? '—'}</dd></div>
                    <div class="meta-impact-row"><dt>{$t('editor_split.impact_affected_spec_count')}</dt><dd>{metaPreviewResult.structural_impact?.affected_spec_count ?? '—'}</dd></div>
                    <div class="meta-impact-row"><dt>{$t('editor_split.impact_affected_workspaces')}</dt><dd>{metaPreviewResult.blast_radius?.affected_workspace_count ?? metaPreviewResult.blast_radius?.affected_workspaces?.length ?? 0}</dd></div>
                    <div class="meta-impact-row"><dt>{$t('editor_split.impact_affected_repos')}</dt><dd>{metaPreviewResult.blast_radius?.affected_repos?.length ?? 0}</dd></div>
                  </dl>
                </div>
                {#if metaPreviewResult.blast_radius?.affected_repos?.length}
                  <div class="meta-impact-section">
                    <h4 class="meta-impact-title">{$t('editor_split.impact_affected_repos')}</h4>
                    <ul class="meta-repo-list">
                      {#each metaPreviewResult.blast_radius.affected_repos as repo (repo.id)}
                        <li class="meta-repo-item">
                          <span class="meta-repo-id">{repo.id}</span>
                          {#if repo.reason}
                            <span class="meta-repo-reason">{$t('editor_split.impact_reason')}: {repo.reason}</span>
                          {/if}
                        </li>
                      {/each}
                    </ul>
                  </div>
                {/if}
              </div>
            {:else}
              <ArchPreviewCanvas
                nodes={graphNodes}
                edges={graphEdges}
                ghostOverlays={overlays}
                size="full"
              />
            {/if}
          </div>
        {:else}
          <div
            class="code-diff-wrap"
            role="tabpanel"
            id="editor-split-panel-diff"
            aria-labelledby="editor-split-tab-diff"
            data-testid="code-diff-panel"
          >
            {#if codeDiffFiles.length}
              {#each codeDiffFiles as file (file.path)}
                <div class="code-diff-file">
                  <div class="code-diff-path">{file.path}</div>
                  <SpecDiffView diff={file.diff} />
                </div>
              {/each}
            {:else if context === 'meta-spec' && previewState === 'preview_complete'}
              <!-- Meta-spec previews report per-spec completion; the endpoint
                   produces no line-level diff, so list the spec statuses. -->
              {#if metaPreviewResult?.specs?.length}
                <div class="meta-spec-statuses" data-testid="meta-spec-statuses">
                  {#each metaPreviewResult.specs as s (s.path)}
                    <div class="meta-spec-status">
                      <span class="meta-spec-path">{s.path}</span>
                      <span class="meta-spec-state">{s.status}</span>
                    </div>
                  {/each}
                </div>
                <p class="code-diff-empty">{$t('editor_split.no_line_diff_available')}</p>
              {:else}
                <p class="code-diff-empty">{$t('editor_split.no_code_diff')}</p>
              {/if}
            {:else if previewState === 'preview_complete'}
              <p class="code-diff-empty">{$t('editor_split.no_code_diff')}</p>
            {:else}
              <p class="code-diff-empty">{$t('editor_split.run_preview_for_diff')}</p>
            {/if}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

<SpecConflictDialog
  conflict={specConflict}
  onOverwrite={conflictOverwrite}
  onDiscard={conflictDiscard}
  onClose={conflictClose}
/>

<style>
  .editor-split {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-surface);
    overflow: hidden;
  }

  .split-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
    min-height: 44px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }

  .back-btn:hover {
    color: var(--color-text);
    border-color: var(--color-text-muted);
  }

  .back-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .split-label {
    flex: 1;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .preview-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    white-space: nowrap;
    transition: border-color var(--transition-fast);
  }

  .preview-btn:hover:not(:disabled) { border-color: var(--color-primary); }
  .preview-btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .preview-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .save-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    font-weight: 500;
    white-space: nowrap;
    transition: background var(--transition-fast);
  }

  .save-btn:hover:not(:disabled) { background: var(--color-primary-hover); }
  .save-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  .save-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .split-panes {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .pane {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
  }

  .pane-left {
    flex: 1;
    border-right: none;
  }

  .pane-right {
    flex: 1;
    background: var(--color-surface, #0f172a);
  }

  .split-divider {
    width: 1px;
    background: var(--color-border);
    flex-shrink: 0;
  }

  .split-textarea {
    flex: 1;
    width: 100%;
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border: none;
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    resize: none;
    box-sizing: border-box;
    min-height: 200px;
  }

  .split-textarea:focus:not(:focus-visible) { outline: none; }
  .split-textarea:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* Editor locked during preview (§9 State 2) */
  .locked-editor {
    flex: 1;
    overflow: auto;
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.6;
    min-height: 200px;
  }

  .locked-line { white-space: pre-wrap; color: var(--color-text-secondary); }

  /* Markdown toolbar (ui-layout.md §2) */
  .md-toolbar {
    display: flex;
    gap: 2px;
    align-items: center;
    padding: 4px var(--space-3);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-1, transparent);
  }
  .md-btn {
    min-width: 26px;
    height: 24px;
    padding: 0 6px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--color-text-secondary);
    font-size: 12px;
    font-family: var(--font-mono, monospace);
    cursor: pointer;
  }
  .md-btn:hover { background: var(--color-surface-3, rgba(127,127,127,.15)); color: var(--color-text); }
  .md-btn:active { transform: translateY(1px); }

  /* Preview error banner (right pane) */
  .preview-error {
    margin: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--color-danger, #cc3340);
    border-radius: 6px;
    background: rgba(204, 51, 64, 0.08);
    color: var(--color-text);
    font-size: 12px;
  }

  /* Meta-spec preview impact summary (real PreviewResponse fields) */
  .meta-impact {
    overflow: auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .meta-impact-title { margin: 0 0 var(--space-2); font-size: 12px; font-weight: 600; color: var(--color-text-secondary); text-transform: uppercase; letter-spacing: 0.04em; }
  .meta-impact-list { margin: 0; display: flex; flex-direction: column; gap: var(--space-2); }
  .meta-impact-row { display: flex; justify-content: space-between; gap: var(--space-3); font-size: 13px; }
  .meta-impact-row dt { color: var(--color-text-secondary); }
  .meta-impact-row dd { margin: 0; font-family: var(--font-mono, monospace); }
  .meta-repo-list { margin: 0; padding: 0; list-style: none; display: flex; flex-direction: column; gap: var(--space-2); }
  .meta-repo-item { display: flex; flex-direction: column; gap: 2px; font-size: 13px; }
  .meta-repo-id { font-family: var(--font-mono, monospace); }
  .meta-repo-reason { color: var(--color-text-secondary); font-size: 12px; }
  .meta-spec-statuses { display: flex; flex-direction: column; gap: var(--space-2); padding: var(--space-3); }
  .meta-spec-status { display: flex; justify-content: space-between; gap: var(--space-3); font-size: 13px; }
  .meta-spec-path { font-family: var(--font-mono, monospace); }
  .meta-spec-state { color: var(--color-text-secondary); }

  /* LLM suggestion block */
  .suggestion-block {
    margin: var(--space-2) var(--space-3);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--color-primary) 5%, transparent);
  }

  .suggestion-hdr {
    display: flex;
    align-items: center;
    justify-content: space-between;
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
    padding: 0 var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .diff-op {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    flex-wrap: wrap;
  }

  .diff-badge {
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    font-size: 10px;
    text-transform: uppercase;
    font-weight: 600;
    flex-shrink: 0;
  }

  .diff-op-add .diff-badge { background: color-mix(in srgb, var(--color-success, #2e9e5b) 20%, transparent); color: var(--color-success, #2e9e5b); }
  .diff-op-remove .diff-badge { background: color-mix(in srgb, var(--color-danger, #cc3340) 20%, transparent); color: var(--color-danger, #cc3340); }
  .diff-op-replace .diff-badge { background: color-mix(in srgb, var(--color-warning) 20%, transparent); color: var(--color-warning); }

  .diff-path {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .diff-content {
    margin: 0;
    flex: 1;
    min-width: 0;
    white-space: pre-wrap;
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-surface-elevated);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-2);
  }

  .suggestion-btns {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
  }

  .accept-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius-sm);
    color: var(--color-text-inverse);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
  }

  .accept-btn:hover { opacity: 0.9; }
  .accept-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .dismiss-btn {
    background: none;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: 0 var(--space-1);
    font-size: var(--text-xs);
  }

  .dismiss-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .dismiss-btn-sm {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
  }

  .dismiss-btn-sm:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* LLM streaming */
  .llm-streaming {
    margin: var(--space-2) var(--space-3);
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

  /* LLM input */
  .llm-input-area {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3) var(--space-4);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .recipient-line {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .llm-textarea:focus:not(:focus-visible) { outline: none; }
  .llm-textarea:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
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
  .llm-send:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .llm-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .llm-hint.warn { color: var(--color-warning); }

  .spin { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  /* Right pane */
  .pane-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .impact-tabs {
    display: flex;
    gap: var(--space-1);
  }

  .impact-tab {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    font-weight: 500;
  }

  .impact-tab:hover { color: var(--color-text); }
  .impact-tab.active {
    color: var(--color-primary);
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
  }
  .impact-tab:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .pane-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .loading-chip {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .overlay-chip {
    font-size: 10px;
    padding: 1px 5px;
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 30%, transparent);
    border-radius: var(--radius-sm);
    color: var(--color-warning);
    font-family: var(--font-mono);
  }

  .canvas-wrap {
    flex: 1;
    overflow: hidden;
    min-height: 0;
  }

  /* Target spec selector (§9 State 1) */
  .spec-selector {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
    padding: var(--space-3);
    gap: var(--space-2);
  }

  .spec-selector-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .spec-selector-shortcuts {
    display: flex;
    gap: var(--space-2);
  }

  .link-btn {
    background: none;
    border: none;
    color: var(--color-primary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    padding: 0;
  }

  .link-btn:hover { text-decoration: underline; }
  .link-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .spec-checklist {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    overflow: auto;
    padding: var(--space-1) 0;
  }

  .spec-check-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text);
  }

  .spec-check-item:hover { background: var(--color-surface-elevated); }

  /* Preview Running (§9 State 2) */
  .preview-progress {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    overflow: auto;
  }

  .progress-header {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .progress-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .progress-item {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .progress-icon { width: 1em; flex-shrink: 0; }
  .progress-path { font-family: var(--font-mono); color: var(--color-text); }
  .progress-status { color: var(--color-text-muted); }

  .progress-summary {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .cancel-preview { align-self: flex-start; }

  /* Code Diff tab */
  .code-diff-wrap {
    flex: 1;
    overflow: auto;
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-height: 0;
  }

  .code-diff-file {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .code-diff-path {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text);
  }

  .code-diff-empty {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
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

  @media (prefers-reduced-motion: reduce) {
    .spin { animation: none; }
    .blink-cursor { animation: none; }
    .back-btn, .save-btn, .llm-send, .llm-textarea { transition: none; }
  }
</style>
