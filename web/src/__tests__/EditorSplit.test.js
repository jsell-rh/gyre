import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, screen, waitFor, within } from '@testing-library/svelte';
import EditorSplit from '../lib/EditorSplit.svelte';

// Mock layout-engines so ArchPreviewCanvas renders without ELK
vi.mock('../lib/layout-engines.js', async () => {
  const { columnLayout } = await vi.importActual('../lib/layout-engines.js');
  return { columnLayout };
});

// Mock api
vi.mock('../lib/api.js', () => ({
  api: {
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    graphPredict: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    specLinks: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    mrDiff: vi.fn().mockResolvedValue({ files: [] }),
    specsAssist: vi.fn(),
    metaSpecsAssist: vi.fn(),
    specsSave: vi.fn().mockResolvedValue({ mr_id: 42 }),
    workspacePresence: vi.fn().mockResolvedValue([]),
    thoroughPreview: vi.fn(),
    taskStatus: vi.fn(),
    previewMetaSpec: vi.fn(),
    previewMetaSpecStatus: vi.fn(),
    deleteMetaSpecPreview: vi.fn(),
  },
}));

// Mock toast
vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  toastInfo: vi.fn(),
}));

describe('EditorSplit', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── Rendering ──────────────────────────────────────────────────────────────

  it('renders the editor textarea', () => {
    render(EditorSplit, { props: { content: 'hello world', repoId: 'repo-1', specPath: 'specs/auth.md' } });
    const ta = screen.getByTestId('editor-split-textarea');
    expect(ta).toBeTruthy();
    expect(ta.value).toBe('hello world');
  });

  it('renders the LLM input area', () => {
    render(EditorSplit, { props: { content: '', repoId: 'repo-1', specPath: 'specs/auth.md' } });
    expect(screen.getByTestId('llm-input')).toBeTruthy();
    expect(screen.getByTestId('llm-send-btn')).toBeTruthy();
  });

  it('gives toolbar buttons localized aria-labels', () => {
    render(EditorSplit, { props: { content: '', repoId: 'repo-1', specPath: 'specs/auth.md' } });
    // Localized from editor_split.md_<id>_aria — not the raw 'B'/'I' label.
    expect(screen.getByTestId('md-bold').getAttribute('aria-label')).toBe('Bold');
    expect(screen.getByTestId('md-italic').getAttribute('aria-label')).toBe('Italic');
    expect(screen.getByTestId('md-undo').getAttribute('aria-label')).toBe('Undo');
  });

  it('renders the architecture preview pane', () => {
    render(EditorSplit, { props: { content: '', repoId: 'repo-1', specPath: 'specs/auth.md' } });
    expect(screen.getByTestId('arch-preview-pane')).toBeTruthy();
  });

  it('renders Back button', () => {
    render(EditorSplit, { props: { content: '', repoId: null } });
    const btn = screen.getByRole('button', { name: /close editor split/i });
    expect(btn).toBeTruthy();
  });

  it('shows spec path in split label', () => {
    render(EditorSplit, { props: { content: '', repoId: 'repo-1', specPath: 'specs/system/auth.md' } });
    expect(document.body.textContent).toContain('specs/system/auth.md');
  });

  // ── Context prop ───────────────────────────────────────────────────────────

  it('shows meta-spec label when context=meta-spec', () => {
    render(EditorSplit, { props: { content: '', repoId: null, context: 'meta-spec' } });
    expect(document.body.textContent).toContain('Meta-spec editor');
  });

  it('shows spec label by default', () => {
    render(EditorSplit, { props: { content: '', repoId: null } });
    expect(document.body.textContent).toContain('Spec editor');
  });

  // ── onClose callback ───────────────────────────────────────────────────────

  it('calls onClose when Back button is clicked', async () => {
    const onClose = vi.fn();
    render(EditorSplit, { props: { content: '', repoId: null, onClose } });
    const btn = screen.getByRole('button', { name: /close editor split/i });
    await fireEvent.click(btn);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('calls onClose on Escape key', async () => {
    const onClose = vi.fn();
    render(EditorSplit, { props: { content: '', repoId: null, onClose } });
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledOnce();
  });

  // ── Content editing ────────────────────────────────────────────────────────

  it('calls onChange when textarea value changes', async () => {
    const onChange = vi.fn();
    render(EditorSplit, { props: { content: '', repoId: null, onChange } });
    const ta = screen.getByTestId('editor-split-textarea');
    await fireEvent.input(ta, { target: { value: 'new content' } });
    expect(onChange).toHaveBeenCalledWith('new content');
  });

  // ── LLM input state ────────────────────────────────────────────────────────

  it('disables LLM send button when no instruction text', () => {
    render(EditorSplit, { props: { content: '', repoId: 'repo-1' } });
    const btn = screen.getByTestId('llm-send-btn');
    expect(btn.disabled).toBe(true);
  });

  it('disables LLM input when no repoId', () => {
    render(EditorSplit, { props: { content: '', repoId: null } });
    const input = screen.getByTestId('llm-input');
    expect(input.disabled).toBe(true);
  });

  it('shows LLM hint warning when no repoId', () => {
    render(EditorSplit, { props: { content: '', repoId: null } });
    expect(document.body.textContent).toContain('LLM editing requires repo or workspace context');
  });

  it('shows ctrl+enter hint when repoId is set', () => {
    render(EditorSplit, { props: { content: '', repoId: 'repo-1' } });
    expect(document.body.textContent).toContain('Ctrl+Enter');
  });

  // ── §9 meta-spec context: workspace-scoped LLM chat ─────────────────────

  it('enables the LLM chat in meta-spec context with a workspace and no repo', () => {
    render(EditorSplit, {
      props: { content: '', repoId: null, context: 'meta-spec', workspaceId: 'ws-1', specPath: 'ms-1' },
    });
    // Component contract for the meta-spec context (workspace-scoped LLM
    // chat with no repo). Note: MetaSpecs no longer mounts EditorSplit — it
    // owns the §5 preview loop with its own inline editor, and DetailPanel
    // mounts EditorSplit with context="spec" — so this branch is not
    // currently production-mounted; the assertion covers the component's
    // built-in capability, not a MetaSpecs production surface.
    expect(screen.getByTestId('llm-input').disabled).toBe(false);
    const input = screen.getByTestId('llm-input');
    fireEvent.input(input, { target: { value: 'make error handling more defensive' } });
    expect(screen.getByTestId('llm-send-btn').disabled).toBe(false);
    expect(document.body.textContent).toContain('Ctrl+Enter');
  });

  it('keeps the LLM chat disabled in meta-spec context without a workspace', () => {
    render(EditorSplit, { props: { content: '', repoId: null, context: 'meta-spec' } });
    expect(screen.getByTestId('llm-input').disabled).toBe(true);
    expect(screen.getByTestId('llm-send-btn').disabled).toBe(true);
    expect(document.body.textContent).toContain('LLM editing requires repo or workspace context');
  });

  it('sends meta-spec chat instructions to the workspace assist endpoint and renders the suggestion', async () => {
    const { api } = await import('../lib/api.js');
    api.metaSpecsAssist.mockResolvedValueOnce(
      sseResponse([
        ['partial', { text: 'Adding defensive error handling.' }],
        ['complete', {
          diff: [{ op: 'add', path: '## Existing', content: '## Error Handling\n\nUse Result<T, E> everywhere.' }],
          explanation: 'Adding defensive error handling.',
        }],
      ]),
    );
    render(EditorSplit, {
      props: {
        content: 'You are a backend developer.\n\n## Existing\nWrite services.\n',
        repoId: null,
        context: 'meta-spec',
        workspaceId: 'ws-9',
        specPath: 'ms-backend-dev',
      },
    });
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'make error handling more defensive' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));

    // Routed to the workspace endpoint with the §9 draft, NOT specsAssist.
    expect(api.metaSpecsAssist).toHaveBeenCalledWith('ws-9', {
      instruction: 'make error handling more defensive',
      draft_content: 'You are a backend developer.\n\n## Existing\nWrite services.\n',
      meta_spec_id: 'ms-backend-dev',
    });
    expect(api.specsAssist).not.toHaveBeenCalled();

    const block = await screen.findByTestId('llm-suggestion');
    expect(block).toBeTruthy();
    // §9 State 3: Accept applies the suggestion diff to the draft in-memory.
    await fireEvent.click(screen.getByTestId('accept-suggestion'));
    await waitFor(() => expect(screen.queryByTestId('llm-suggestion')).toBeNull());
    expect(screen.getByTestId('editor-split-textarea').value).toContain('## Error Handling');
  });

  // ── Save button ────────────────────────────────────────────────────────────

  it('shows Save button when repoId and specPath are set', () => {
    render(EditorSplit, { props: { content: 'text', repoId: 'repo-1', specPath: 'specs/auth.md' } });
    expect(screen.getByRole('button', { name: /save/i })).toBeTruthy();
  });

  it('does not show Save button when no repoId', () => {
    render(EditorSplit, { props: { content: 'text', repoId: null, specPath: 'specs/auth.md' } });
    expect(screen.queryByRole('button', { name: /save/i })).toBeNull();
  });

  it('Save button is disabled when content is empty', () => {
    render(EditorSplit, { props: { content: '', repoId: 'repo-1', specPath: 'specs/auth.md' } });
    const btn = screen.getByRole('button', { name: /save/i });
    expect(btn.disabled).toBe(true);
  });

  // ── Ghost overlays indicator ───────────────────────────────────────────────

  it('shows overlay count chip when ghostOverlays are provided', async () => {
    const overlays = [
      { nodeId: 'n1', type: 'new' },
      { nodeId: 'n2', type: 'modified' },
    ];
    render(EditorSplit, { props: { content: '', repoId: 'repo-1', ghostOverlays: overlays } });
    // Wait for graphPredict to resolve so loading state clears
    await waitFor(() => {
      expect(document.body.textContent).toContain('2 predicted changes');
    });
  });

  it('does not show overlay chip when no overlays', () => {
    render(EditorSplit, { props: { content: '', repoId: 'repo-1', ghostOverlays: [] } });
    expect(document.body.textContent).not.toContain('predicted changes');
  });

  // ── Graph loading ──────────────────────────────────────────────────────────

  it('triggers graphPredict on mount when repoId is set (debounced, §3 fast tier)', async () => {
    const { api } = await import('../lib/api.js');
    render(EditorSplit, { props: { content: '', repoId: 'repo-1', specPath: 'specs/auth.md' } });
    // §3: the prediction call is debounced (1.2s after the edit pause),
    // so the mount-time call lands after the debounce window.
    await waitFor(() => {
      expect(api.graphPredict).toHaveBeenCalledWith('repo-1', expect.objectContaining({ spec_path: 'specs/auth.md' }));
    }, { timeout: 4000 });
  });

  it('does not trigger graphPredict when no repoId', async () => {
    const { api } = await import('../lib/api.js');
    render(EditorSplit, { props: { content: '', repoId: null } });
    // Give time for any effect to run
    await new Promise(r => setTimeout(r, 50));
    expect(api.graphPredict).not.toHaveBeenCalled();
  });

  // ── LLM suggestion flow ────────────────────────────────────────────────────

  it('shows suggestion block when llmSuggestion is set (via mock)', async () => {
    const { api } = await import('../lib/api.js');
    // Mock a streaming response that immediately gives a complete event
    const mockBody = {
      getReader: () => {
        let done = false;
        return {
          read: () => {
            if (done) return Promise.resolve({ value: undefined, done: true });
            done = true;
            const text = 'data: {"event":"complete","diff":[{"op":"add","path":"## New","content":"stuff"}],"explanation":"Added section"}\n';
            return Promise.resolve({ value: new TextEncoder().encode(text), done: false });
          },
        };
      },
    };
    api.specsAssist.mockResolvedValue({ ok: true, body: mockBody.body, ...mockBody });

    render(EditorSplit, { props: { content: 'initial', repoId: 'repo-1', specPath: 'specs/auth.md' } });

    const input = screen.getByTestId('llm-input');
    await fireEvent.input(input, { target: { value: 'add error section' } });

    const sendBtn = screen.getByTestId('llm-send-btn');
    await fireEvent.click(sendBtn);

    await waitFor(() => {
      expect(api.specsAssist).toHaveBeenCalled();
    });
  });

  // ── Conflict prevention (HSI §7) ────────────────────────────────────────────

  it('sends base_sha with the save for optimistic concurrency', async () => {
    const { api } = await import('../lib/api.js');
    api.specsSave.mockResolvedValueOnce({ mr_id: 7 });
    render(EditorSplit, {
      props: { content: 'text', repoId: 'repo-1', specPath: 'specs/auth.md', baseSha: 'sha-loaded' },
    });
    await fireEvent.click(screen.getByRole('button', { name: /save/i }));
    await waitFor(() => {
      expect(api.specsSave).toHaveBeenCalledWith(
        'repo-1',
        expect.objectContaining({ base_sha: 'sha-loaded', overwrite: false }),
      );
    });
  });

  it('opens the conflict dialog on a 409 conflict response', async () => {
    const { api } = await import('../lib/api.js');
    api.specsSave.mockResolvedValueOnce({
      conflict: {
        spec_path: 'specs/auth.md',
        current_content: 'server',
        submitted_content: 'mine',
        diff: [{ op: 'add', text: 'mine' }],
      },
    });
    render(EditorSplit, {
      props: { content: 'mine', repoId: 'repo-1', specPath: 'specs/auth.md', baseSha: 'stale' },
    });
    await fireEvent.click(screen.getByRole('button', { name: /save/i }));
    await waitFor(() => {
      expect(screen.getByTestId('spec-conflict-dialog')).toBeTruthy();
    });
  });

  it('force-saves with overwrite when the user chooses Overwrite', async () => {
    const { api } = await import('../lib/api.js');
    api.specsSave
      .mockResolvedValueOnce({
        conflict: {
          spec_path: 'specs/auth.md',
          current_content: 'server',
          submitted_content: 'mine',
          diff: [{ op: 'add', text: 'mine' }],
        },
      })
      .mockResolvedValueOnce({ mr_id: 99 });
    render(EditorSplit, {
      props: { content: 'mine', repoId: 'repo-1', specPath: 'specs/auth.md', baseSha: 'stale' },
    });
    await fireEvent.click(screen.getByRole('button', { name: /save/i }));
    await waitFor(() => expect(screen.getByTestId('spec-conflict-dialog')).toBeTruthy());

    await fireEvent.click(screen.getByTestId('conflict-overwrite-btn'));
    await waitFor(() => {
      expect(api.specsSave).toHaveBeenLastCalledWith(
        'repo-1',
        expect.objectContaining({ overwrite: true }),
      );
    });
    // Dialog dismissed after a successful overwrite.
    await waitFor(() => expect(screen.queryByTestId('spec-conflict-dialog')).toBeNull());
  });

  it('renders the concurrent-edit banner when another user is editing the spec', async () => {
    const { api } = await import('../lib/api.js');
    api.workspacePresence.mockResolvedValueOnce([
      { session_id: 's-maria', user_id: 'maria', editing_entity: 'spec:specs/auth.md' },
    ]);
    const wsStore = { sessionId: 's-self', onMessage: () => () => {}, send: vi.fn() };
    render(EditorSplit, {
      props: {
        content: '',
        repoId: 'repo-1',
        specPath: 'specs/auth.md',
        workspaceId: 'ws1',
        wsStore,
      },
    });
    await waitFor(() => {
      expect(screen.getByTestId('concurrent-edit-banner')).toBeTruthy();
    });
  });

  // ── Preview state machine (ui-layout.md §2/§9) ─────────────────────────────

  it('shows Preview button and starts thorough preview on click (spec context)', async () => {
    const { api } = await import('../lib/api.js');
    api.thoroughPreview.mockResolvedValueOnce({
      predictions: [
        { node_id: 'RetryPolicy', change_type: 'new' },
        { node_id: 'PaymentPort', change_type: 'modified' },
      ],
      code_diff: [
        { path: 'src/retry.rs', diff: [{ op: 'context', text: 'fn main() {}' }] },
      ],
    });
    render(EditorSplit, {
      props: { content: '# Draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    const previewBtn = await screen.findByTestId('preview-btn');
    await fireEvent.click(previewBtn);
    await waitFor(() => {
      expect(api.thoroughPreview).toHaveBeenCalledWith('repo-1', {
        spec_path: 'specs/auth.md',
        draft_content: '# Draft',
      });
    });
    // Immediate result → complete state with Iterate available
    await waitFor(() => {
      expect(screen.getByTestId('iterate-btn')).toBeTruthy();
    });
  });

  it('renders Architecture (default) and Code Diff tabs in the right panel', async () => {
    const { api } = await import('../lib/api.js');
    api.thoroughPreview.mockResolvedValueOnce({
      predictions: [{ node_id: 'RetryPolicy', change_type: 'new' }],
      code_diff: [{ path: 'src/retry.rs', diff: [{ op: 'context', text: 'fn main() {}' }] }],
    });
    render(EditorSplit, {
      props: { content: '# Draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    await fireEvent.click(await screen.findByTestId('preview-btn'));
    // Architecture tab is selected by default
    const archTab = await screen.findByTestId('tab-architecture');
    expect(archTab.getAttribute('aria-selected')).toBe('true');
    expect(screen.getByTestId('tab-code-diff').getAttribute('aria-selected')).toBe('false');

    // Switch to Code Diff tab — line-level diff from the preview result
    await fireEvent.click(screen.getByTestId('tab-code-diff'));
    const panel = await screen.findByTestId('code-diff-panel');
    expect(panel.textContent).toContain('src/retry.rs');
    expect(screen.getByTestId('tab-code-diff').getAttribute('aria-selected')).toBe('true');
  });

  it('iterating returns to the editing state with results still visible', async () => {
    const { api } = await import('../lib/api.js');
    api.thoroughPreview.mockResolvedValueOnce({
      predictions: [{ node_id: 'RetryPolicy', change_type: 'new' }],
      code_diff: [{ path: 'src/retry.rs', diff: [{ op: 'context', text: 'x' }] }],
    });
    render(EditorSplit, {
      props: { content: '# Draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    await fireEvent.click(await screen.findByTestId('preview-btn'));
    await screen.findByTestId('iterate-btn');
    await fireEvent.click(screen.getByTestId('iterate-btn'));
    // Back in editing state: textarea active again, Preview button available,
    // code diff still rendered when switching tabs (results retained).
    await waitFor(() => expect(screen.getByTestId('editor-split-textarea')).toBeTruthy());
    expect(screen.getByTestId('preview-btn')).toBeTruthy();
    await fireEvent.click(screen.getByTestId('tab-code-diff'));
    expect(screen.getByTestId('code-diff-panel').textContent).toContain('src/retry.rs');
  });

  it('polls taskStatus while the thorough preview runs and completes', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-9' });
      api.taskStatus
        .mockResolvedValueOnce({ status: 'in_progress' })
        .mockResolvedValueOnce({
          status: 'review',
          predictions: [{ node_id: 'ErrorHandler', change_type: 'new' }],
          code_diff: [{ path: 'src/error.rs', diff: [{ op: 'context', text: 'y' }] }],
        });
      render(EditorSplit, {
        props: { content: '# Draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
      });
      await fireEvent.click(await screen.findByTestId('preview-btn'));
      // Running state: editor locked (§9 State 2), cancel available
      await vi.waitFor(() => expect(screen.getByTestId('editor-locked')).toBeTruthy());
      expect(screen.getByTestId('cancel-preview')).toBeTruthy();
      // Poll fires at 10s intervals — advance through both statuses
      await vi.advanceTimersByTimeAsync(10000);
      await vi.advanceTimersByTimeAsync(10000);
      await vi.waitFor(() => expect(screen.getByTestId('iterate-btn')).toBeTruthy());
      expect(api.taskStatus).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('cancel during a running preview returns to the editing state', async () => {
    const { api } = await import('../lib/api.js');
    api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-10' });
    api.taskStatus.mockResolvedValue({ status: 'in_progress' });
    render(EditorSplit, {
      props: { content: '# Draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    await fireEvent.click(await screen.findByTestId('preview-btn'));
    await waitFor(() => expect(screen.getByTestId('editor-locked')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('cancel-preview'));
    await waitFor(() => expect(screen.getByTestId('editor-split-textarea')).toBeTruthy());
    expect(screen.queryByTestId('editor-locked')).toBeNull();
  });

  // ── Meta-spec context (§9 preview loop via EditorSplit) ───────────────────

  it('shows target spec selector in editing state for meta-spec context', async () => {
    render(EditorSplit, {
      props: {
        content: 'You are a backend dev...',
        context: 'meta-spec',
        repoId: null,
        workspaceId: 'ws-1',
        targetSpecs: [
          { path: 'specs/system/payment-retry.md', repo_id: 'repo-1' },
          { path: 'specs/system/charge-processing.md', repo_id: 'repo-1' },
          { path: 'specs/system/identity-security.md', repo_id: 'repo-2' },
        ],
      },
    });
    const selector = await screen.findByTestId('spec-selector');
    expect(selector.textContent).toContain('specs/system/payment-retry.md');
    expect(selector.textContent).toContain('specs/system/identity-security.md');
    // Preview disabled until a target is selected (§9 State 1)
    expect(screen.getByTestId('preview-btn').disabled).toBe(true);
  });

  it('selecting specs enables Preview and runs previewMetaSpec against them (real §5 body)', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      // Real §5 PreviewResponse (meta_specs.rs): { preview_id, agents: [{
      // agent_id, repo_id, spec_path, branch }] } — nothing else.
      api.previewMetaSpec.mockResolvedValueOnce({
        preview_id: 'pv-1',
        agents: [
          { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/system/payment-retry.md', branch: 'preview/pv-1/payment-retry' },
          { agent_id: 'ag-2', repo_id: 'repo-1', spec_path: 'specs/system/charge-processing.md', branch: 'preview/pv-1/charge-processing' },
        ],
      });
      api.previewMetaSpecStatus.mockResolvedValue({
        preview_id: 'pv-1',
        state: 'complete',
        agents: [
          { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/system/payment-retry.md', branch: 'preview/pv-1/payment-retry', status: 'complete', diff: null },
          { agent_id: 'ag-2', repo_id: 'repo-1', spec_path: 'specs/system/charge-processing.md', branch: 'preview/pv-1/charge-processing', status: 'complete', diff: null },
        ],
      });
      render(EditorSplit, {
        props: {
          content: 'You are a backend dev...',
          context: 'meta-spec',
          workspaceId: 'ws-1',
          targetSpecs: [
            { path: 'specs/system/payment-retry.md', repo_id: 'repo-1' },
            { path: 'specs/system/charge-processing.md', repo_id: 'repo-1' },
          ],
        },
      });
      // Select all targets
      await fireEvent.click(await screen.findByText('Select All'));
      const previewBtn = screen.getByTestId('preview-btn');
      expect(previewBtn.disabled).toBe(false);
      await fireEvent.click(previewBtn);
      await vi.waitFor(() => {
        expect(api.previewMetaSpec).toHaveBeenCalledWith({
          draft: { kind: 'meta:persona', content: 'You are a backend dev...' },
          targets: [
            { repo_id: 'repo-1', spec_path: 'specs/system/payment-retry.md' },
            { repo_id: 'repo-1', spec_path: 'specs/system/charge-processing.md' },
          ],
        });
      });
      // The status poll completes at 2s intervals
      await vi.advanceTimersByTimeAsync(2100);
      await vi.waitFor(() => expect(screen.getByTestId('iterate-btn')).toBeTruthy());
      // Architecture tab (default) renders the per-agent run summary — the
      // spec paths and preview branches the real §5 status reports.
      const impact = screen.getByTestId('meta-impact');
      expect(impact.textContent).toContain('specs/system/payment-retry.md');
      expect(impact.textContent).toContain('preview/pv-1/payment-retry');
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('renders the produced per-agent diff in the Code Diff tab (real §5 status shape)', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      // Exact wire shapes from crates/gyre-server/src/api/meta_specs.rs —
      // PreviewStatusResponse carries per-agent { spec_path, branch, status,
      // diff } where diff.patches hold the unified-diff text the preview
      // branch produced. Feeding this (not synthetic mocks) pins that the
      // right panel renders what production actually returns.
      api.previewMetaSpec.mockResolvedValueOnce({
        preview_id: 'pv-real-1',
        agents: [
          { agent_id: 'ag-9', repo_id: 'repo-9', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-real-1/retry' },
        ],
      });
      api.previewMetaSpecStatus.mockResolvedValue({
        preview_id: 'pv-real-1',
        state: 'complete',
        agents: [
          {
            agent_id: 'ag-9',
            repo_id: 'repo-9',
            spec_path: 'specs/payment/retry.md',
            branch: 'preview/pv-real-1/retry',
            status: 'complete',
            diff: {
              files_changed: 1,
              insertions: 3,
              deletions: 1,
              patches: [{ patch: '@@ specs/payment/retry.md@@\n-old line\n+new line\n context' }],
            },
          },
        ],
      });
      render(EditorSplit, {
        props: {
          content: 'You are a backend dev...',
          context: 'meta-spec',
          workspaceId: 'ws-1',
          targetSpecs: [{ path: 'specs/payment/retry.md', repo_id: 'repo-9' }],
        },
      });
      await fireEvent.click(await screen.findByText('Select All'));
      await fireEvent.click(screen.getByTestId('preview-btn'));
      await vi.advanceTimersByTimeAsync(2100);
      await vi.waitFor(() => expect(screen.getByTestId('iterate-btn')).toBeTruthy());
      // Architecture tab (default) shows the per-agent run summary with the
      // diff stats the real endpoint returns.
      const impact = screen.getByTestId('meta-impact');
      expect(impact.textContent).toContain('specs/payment/retry.md');
      expect(impact.textContent).toContain('preview/pv-real-1/retry');
      expect(impact.textContent).toContain('+3/-1');
      // Code Diff tab shows the agent's produced unified diff lines.
      await fireEvent.click(screen.getByTestId('tab-code-diff'));
      const panel = await screen.findByTestId('code-diff-panel');
      expect(panel.textContent).toContain('-old line');
      expect(panel.textContent).toContain('+new line');
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('meta-spec Iterate returns to editing with the selector AND the last preview results still visible (§9)', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      // Real §5 PreviewResponse / PreviewStatusResponse shapes (meta_specs.rs):
      // { preview_id, agents: [{ agent_id, repo_id, spec_path, branch }] } and
      // the polled status adds { status, diff } per agent.
      api.previewMetaSpec.mockResolvedValueOnce({
        preview_id: 'pv-iter-1',
        agents: [
          { agent_id: 'ag-iter-1', repo_id: 'repo-9', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-iter-1/retry' },
        ],
      });
      api.previewMetaSpecStatus.mockResolvedValue({
        preview_id: 'pv-iter-1',
        state: 'complete',
        agents: [
          { agent_id: 'ag-iter-1', repo_id: 'repo-9', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-iter-1/retry', status: 'complete', diff: null },
        ],
      });
      render(EditorSplit, {
        props: {
          content: 'You are a backend dev...',
          context: 'meta-spec',
          workspaceId: 'ws-1',
          specPath: 'persona/backend-dev',
          targetSpecs: [{ path: 'specs/payment/retry.md', repo_id: 'repo-9' }],
        },
      });
      await fireEvent.click(await screen.findByText('Select All'));
      await fireEvent.click(screen.getByTestId('preview-btn'));
      await vi.advanceTimersByTimeAsync(2000);
      await vi.waitFor(() => expect(screen.getByTestId('iterate-btn')).toBeTruthy());
      // State 3: tabs render the real §5 per-agent summary; no selector yet.
      expect(screen.getByTestId('meta-impact').textContent).toContain('specs/payment/retry.md');
      expect(screen.queryByTestId('spec-selector')).toBeNull();

      await fireEvent.click(screen.getByTestId('iterate-btn'));

      // Back to State 1: editor editable, Preview button back, selector
      // visible again for the next iteration...
      await waitFor(() => expect(screen.getByTestId('editor-split-textarea')).toBeTruthy());
      expect(screen.getByTestId('preview-btn')).toBeTruthy();
      const selector = screen.getByTestId('spec-selector');
      expect(selector.textContent).toContain('specs/payment/retry.md');
      // ...AND the last preview results remain visible (§9 Iterate):
      // Architecture impact + Code Diff tab with the per-spec statuses.
      expect(screen.getByTestId('tab-architecture')).toBeTruthy();
      expect(screen.getByTestId('meta-impact').textContent).toContain('specs/payment/retry.md');
      expect(screen.getByTestId('meta-impact').textContent).toContain('preview/pv-iter-1/retry');
      await fireEvent.click(screen.getByTestId('tab-code-diff'));
      const statuses = await screen.findByTestId('meta-spec-statuses');
      expect(statuses.textContent).toContain('specs/payment/retry.md');
      expect(statuses.textContent).toContain('Complete');
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('a thoroughPreview rejection (real deployments: 404 — no server route) returns to editing with an error banner, never a fabricated complete state', async () => {
    const { api } = await import('../lib/api.js');
    // POST /repos/:id/graph/thorough-preview has no server route, so every
    // real spec-context Preview rejects with request()'s 404 error.
    api.thoroughPreview.mockRejectedValueOnce(
      new Error('API /repos/repo-1/graph/thorough-preview: 404 Not Found'),
    );
    render(EditorSplit, {
      props: { content: '# Draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    await fireEvent.click(await screen.findByTestId('preview-btn'));
    // Back to editing with the error banner — §9 State 3 is never presented
    // without a real completed result.
    await waitFor(() => expect(screen.getByTestId('preview-error')).toBeTruthy());
    expect(screen.getByTestId('preview-error').textContent).toContain('404');
    expect(screen.getByTestId('editor-split-textarea')).toBeTruthy();
    expect(screen.queryByTestId('iterate-btn')).toBeNull();
    expect(screen.queryByTestId('preview-running')).toBeNull();
  });

  it('a previewMetaSpec rejection (endpoint unavailable) returns to editing with an error banner, never a fabricated complete state', async () => {
    const { api } = await import('../lib/api.js');
    api.previewMetaSpec.mockRejectedValueOnce(
      new Error('API /meta-specs/preview: 404 Not Found'),
    );
    render(EditorSplit, {
      props: {
        content: 'You are a backend dev...',
        context: 'meta-spec',
        workspaceId: 'ws-1',
        targetSpecs: [{ path: 'specs/a.md', repo_id: 'repo-1' }],
      },
    });
    await fireEvent.click(await screen.findByText('Select All'));
    await fireEvent.click(screen.getByTestId('preview-btn'));
    await waitFor(() => expect(screen.getByTestId('preview-error')).toBeTruthy());
    expect(screen.getByTestId('preview-error').textContent).toContain('404');
    expect(screen.getByTestId('editor-split-textarea')).toBeTruthy();
    expect(screen.queryByTestId('iterate-btn')).toBeNull();
  });

  it('only sends targets with a repo_id — a spec without one can never spawn a preview agent', async () => {
    const { api } = await import('../lib/api.js');
    api.previewMetaSpec.mockResolvedValueOnce({ preview_id: 'pv-2', agents: [] });
    api.previewMetaSpecStatus.mockResolvedValue({ preview_id: 'pv-2', state: 'complete', agents: [] });
    render(EditorSplit, {
      props: {
        content: 'prompt text',
        context: 'meta-spec',
        workspaceId: 'ws-1',
        targetSpecs: [
          { path: 'specs/with-repo.md', repo_id: 'repo-1' },
          { path: 'specs/no-repo.md' },
        ],
      },
    });
    await fireEvent.click(await screen.findByText('Select All'));
    await fireEvent.click(screen.getByTestId('preview-btn'));
    await waitFor(() => {
      expect(api.previewMetaSpec).toHaveBeenCalledWith({
        draft: { kind: 'meta:persona', content: 'prompt text' },
        targets: [{ repo_id: 'repo-1', spec_path: 'specs/with-repo.md' }],
      });
    });
  });

  it('Escape cancels a running preview instead of closing the split (§9 State 2)', async () => {
    const onClose = vi.fn();
    const { api } = await import('../lib/api.js');
    api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-esc' });
    api.taskStatus.mockResolvedValue({ status: 'in_progress' });
    render(EditorSplit, {
      props: { content: '# Draft', repoId: 'repo-1', specPath: 'specs/auth.md', onClose },
    });
    await fireEvent.click(await screen.findByTestId('preview-btn'));
    await waitFor(() => expect(screen.getByTestId('editor-locked')).toBeTruthy());
    await fireEvent.keyDown(window, { key: 'Escape' });
    // Preview cancelled — back to editing — but the split itself stays open.
    await waitFor(() => expect(screen.getByTestId('editor-split-textarea')).toBeTruthy());
    expect(screen.queryByTestId('preview-running')).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('sends the caller-provided draftKind as the draft kind', async () => {
    const { api } = await import('../lib/api.js');
    api.previewMetaSpec.mockResolvedValueOnce({ preview_id: 'pv-3', agents: [] });
    api.previewMetaSpecStatus.mockResolvedValue({ preview_id: 'pv-3', state: 'complete', agents: [] });
    render(EditorSplit, {
      props: {
        content: 'principle text',
        context: 'meta-spec',
        draftKind: 'meta:principle',
        workspaceId: 'ws-1',
        targetSpecs: [{ path: 'specs/a.md', repo_id: 'repo-1' }],
      },
    });
    await fireEvent.click(await screen.findByText('Select All'));
    await fireEvent.click(screen.getByTestId('preview-btn'));
    await waitFor(() => {
      expect(api.previewMetaSpec).toHaveBeenCalledWith({
        draft: { kind: 'meta:principle', content: 'principle text' },
        targets: [{ repo_id: 'repo-1', spec_path: 'specs/a.md' }],
      });
    });
  });

  // ── Locked-editor pending-changes diff (§9 State 2 "shows diff of changes") ──

  it('locked editor shows the diff of pending changes against the baseline while a preview runs', async () => {
    const { api } = await import('../lib/api.js');
    api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-diff-1' });
    api.taskStatus.mockResolvedValue({ status: 'in_progress' });
    render(EditorSplit, {
      props: {
        content: '# Persona\nHandle all errors with Result<T, E>.\nBe terse.',
        baselineContent: '# Persona\nBe terse.',
        repoId: 'repo-1',
        specPath: 'specs/auth.md',
      },
    });
    await fireEvent.click(await screen.findByTestId('preview-btn'));
    await waitFor(() => expect(screen.getByTestId('editor-locked')).toBeTruthy());
    // Diff view — not the verbatim draft
    const diff = screen.getByTestId('locked-diff');
    expect(diff).toBeTruthy();
    // Context line unchanged, added line marked with '+' (§9 State 2 wireframe)
    expect(diff.querySelector('.locked-diff-context .locked-diff-text').textContent).toBe('# Persona');
    const addLine = diff.querySelector('.locked-diff-add .locked-diff-text');
    expect(addLine.textContent).toBe('Handle all errors with Result<T, E>.');
    expect(diff.querySelector('.locked-diff-add .locked-diff-marker').textContent).toBe('+');
    // Summary counts the pending additions
    expect(screen.getByTestId('locked-diff-summary').textContent).toBe('+1 -0');
    // Locked: no textarea while the preview runs
    expect(screen.queryByTestId('editor-split-textarea')).toBeNull();
  });

  it('locked editor marks removed baseline lines with "-"', async () => {
    const { api } = await import('../lib/api.js');
    api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-diff-2' });
    api.taskStatus.mockResolvedValue({ status: 'in_progress' });
    render(EditorSplit, {
      props: {
        content: 'New body.',
        baselineContent: 'Old body.',
        repoId: 'repo-1',
        specPath: 'specs/auth.md',
      },
    });
    await fireEvent.click(await screen.findByTestId('preview-btn'));
    await waitFor(() => expect(screen.getByTestId('locked-diff')).toBeTruthy());
    const diff = screen.getByTestId('locked-diff');
    expect(diff.querySelector('.locked-diff-remove .locked-diff-text').textContent).toBe('Old body.');
    expect(diff.querySelector('.locked-diff-remove .locked-diff-marker').textContent).toBe('-');
    expect(diff.querySelector('.locked-diff-add .locked-diff-text').textContent).toBe('New body.');
    expect(screen.getByTestId('locked-diff-summary').textContent).toBe('+1 -1');
  });

  it('locked editor falls back to the verbatim draft when no baseline is provided', async () => {
    const { api } = await import('../lib/api.js');
    api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-diff-3' });
    api.taskStatus.mockResolvedValue({ status: 'in_progress' });
    render(EditorSplit, {
      props: { content: 'Just a draft.', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    await fireEvent.click(await screen.findByTestId('preview-btn'));
    await waitFor(() => expect(screen.getByTestId('editor-locked')).toBeTruthy());
    // No baseline → verbatim draft, no diff markers
    expect(screen.queryByTestId('locked-diff')).toBeNull();
    expect(screen.getByTestId('editor-locked').textContent).toContain('Just a draft.');
  });

  it('locked editor shows no diff when the draft equals the baseline', async () => {
    const { api } = await import('../lib/api.js');
    api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-diff-4' });
    api.taskStatus.mockResolvedValue({ status: 'in_progress' });
    render(EditorSplit, {
      props: {
        content: 'Unchanged draft.',
        baselineContent: 'Unchanged draft.',
        repoId: 'repo-1',
        specPath: 'specs/auth.md',
      },
    });
    await fireEvent.click(await screen.findByTestId('preview-btn'));
    await waitFor(() => expect(screen.getByTestId('editor-locked')).toBeTruthy());
    expect(screen.queryByTestId('locked-diff')).toBeNull();
    expect(screen.getByTestId('editor-locked').textContent).toContain('Unchanged draft.');
  });

  // ── LLM suggestion Accept / Edit / Dismiss (§3 steps 4-6) ──────────────────

  /** Build an SSE response body from (event, data) pairs, server format. */
  function sseResponse(events) {
    const text = events.map(([evt, data]) => `event: ${evt}\ndata: ${JSON.stringify(data)}\n\n`).join('');
    return {
      ok: true,
      status: 200,
      body: {
        getReader: () => {
          let sent = false;
          return {
            read: () => {
              if (sent) return Promise.resolve({ value: undefined, done: true });
              sent = true;
              return Promise.resolve({ value: new TextEncoder().encode(text), done: false });
            },
          };
        },
      },
    };
  }

  async function getLlmSuggestion(diff) {
    const { api } = await import('../lib/api.js');
    api.specsAssist.mockResolvedValueOnce(
      sseResponse([
        ['partial', { text: 'Adding error handling section.' }],
        ['complete', { diff, explanation: 'Adding error handling section.' }],
      ]),
    );
    render(EditorSplit, {
      props: { content: '# Auth\n\nExisting body.\n', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add error handling' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    return screen.findByTestId('llm-suggestion');
  }

  it('Accept applies the LLM diff to the editor content in-memory', async () => {
    const block = await getLlmSuggestion([
      { op: 'add', path: '## Existing', content: '## Error Handling\n\nReturn structured errors.' },
    ]);
    expect(block).toBeTruthy();
    await fireEvent.click(screen.getByTestId('accept-suggestion'));
    // Suggestion dismissed; content updated with the applied diff (not saved)
    await waitFor(() => expect(screen.queryByTestId('llm-suggestion')).toBeNull());
    const ta = screen.getByTestId('editor-split-textarea');
    expect(ta.value).toContain('## Error Handling');
    const { api } = await import('../lib/api.js');
    expect(api.specsSave).not.toHaveBeenCalled();
  });

  it('Edit copies the suggested text into the editor for manual refinement', async () => {
    const block = await getLlmSuggestion([
      { op: 'add', path: '## Existing', content: '## Error Handling\n\nReturn structured errors.' },
    ]);
    expect(block).toBeTruthy();
    await fireEvent.click(screen.getByTestId('edit-suggestion'));
    await waitFor(() => expect(screen.queryByTestId('llm-suggestion')).toBeNull());
    const ta = screen.getByTestId('editor-split-textarea');
    // Suggested text appended verbatim for the human to refine (§3 step 5)
    expect(ta.value).toContain('## Error Handling');
    expect(ta.value).toContain('# Auth');
  });

  it('Dismiss removes the suggestion without touching the editor content', async () => {
    const before = '# Auth\n\nExisting body.\n';
    const block = await getLlmSuggestion([
      { op: 'add', path: '## Existing', content: '## Error Handling\n\nReturn structured errors.' },
    ]);
    expect(block).toBeTruthy();
    await fireEvent.click(screen.getByTestId('dismiss-suggestion'));
    await waitFor(() => expect(screen.queryByTestId('llm-suggestion')).toBeNull());
    expect(screen.getByTestId('editor-split-textarea').value).toBe(before);
  });

  it('renders the partial explanation progressively while the SSE stream is open (§3 step 2)', async () => {
    const { api } = await import('../lib/api.js');
    const enc = (s) => new TextEncoder().encode(s);
    // Gate the second read so the intermediate streaming state is
    // observable: the partial event is delivered, the complete event is
    // held back until the test releases it.
    let releaseComplete;
    const gate = new Promise((resolve) => { releaseComplete = resolve; });
    let reads = 0;
    api.specsAssist.mockResolvedValueOnce({
      ok: true,
      status: 200,
      body: {
        getReader: () => ({
          read: () => {
            reads += 1;
            if (reads === 1) {
              return Promise.resolve({
                value: enc('event: partial\ndata: {"text":"Adding error handling."}\n\n'),
                done: false,
              });
            }
            if (reads === 2) {
              return gate.then(() => ({
                value: enc('event: complete\ndata: {"diff":[{"op":"add","path":"## Existing","content":"## Error Handling"}],"explanation":"Adding error handling."}\n\n'),
                done: false,
              }));
            }
            return Promise.resolve({ value: undefined, done: true });
          },
        }),
      },
    });
    render(EditorSplit, {
      props: { content: '# Auth\n\n## Existing\nbody\n', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add error handling' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    // While the stream is open the partial explanation renders progressively
    // and the input is in its loading (disabled) state.
    await waitFor(() => {
      expect(screen.getByTestId('llm-streaming').textContent).toContain('Adding error handling.');
    });
    expect(screen.getByTestId('llm-input').disabled).toBe(true);
    releaseComplete();
    // The complete event turns the stream into a suggestion block.
    await screen.findByTestId('llm-suggestion');
    expect(screen.queryByTestId('llm-streaming')).toBeNull();
    expect(screen.getByTestId('llm-input').disabled).toBe(false);
  });

  it('multiple suggestions coexist as separate blocks; dismissing one keeps the others (§3)', async () => {
    const { api } = await import('../lib/api.js');
    api.specsAssist
      .mockResolvedValueOnce(sseResponse([
        ['complete', {
          diff: [{ op: 'add', path: '## Existing', content: '## Error Handling' }],
          explanation: 'First suggestion.',
        }],
      ]))
      .mockResolvedValueOnce(sseResponse([
        ['complete', {
          diff: [{ op: 'add', path: '## Existing', content: '## Timeouts' }],
          explanation: 'Second suggestion.',
        }],
      ]));
    render(EditorSplit, {
      props: { content: '# Auth\n\n## Existing\nbody\n', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add error handling' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    await screen.findByTestId('llm-suggestion');
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add timeouts' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    // Two requests → two coexisting inline diff blocks, not one replacing
    // the other (§3 "Multiple suggestions... Each appears as a separate
    // inline diff block").
    await waitFor(() => expect(screen.getAllByTestId('llm-suggestion').length).toBe(2));
    // Dismiss only the first block — the second survives untouched.
    const blocks = screen.getAllByTestId('llm-suggestion');
    await fireEvent.click(within(blocks[0]).getByTestId('dismiss-suggestion'));
    await waitFor(() => expect(screen.getAllByTestId('llm-suggestion').length).toBe(1));
    expect(screen.getByTestId('llm-suggestion').textContent).toContain('Timeouts');
    // Dismiss never touches the editor content.
    expect(screen.getByTestId('editor-split-textarea').value).toBe('# Auth\n\n## Existing\nbody\n');
  });

  it('accepting one of several suggestions leaves the others curatable (§3)', async () => {
    const { api } = await import('../lib/api.js');
    api.specsAssist
      .mockResolvedValueOnce(sseResponse([
        ['complete', {
          diff: [{ op: 'add', path: '## Existing', content: '## Error Handling' }],
          explanation: 'First suggestion.',
        }],
      ]))
      .mockResolvedValueOnce(sseResponse([
        ['complete', {
          diff: [{ op: 'add', path: '## Existing', content: '## Timeouts' }],
          explanation: 'Second suggestion.',
        }],
      ]));
    render(EditorSplit, {
      props: { content: '# Auth\n\n## Existing\nbody\n', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add error handling' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    await screen.findByTestId('llm-suggestion');
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add timeouts' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    await waitFor(() => expect(screen.getAllByTestId('llm-suggestion').length).toBe(2));
    // Accept the first block: its diff is applied in-memory and only that
    // block is consumed.
    const blocks = screen.getAllByTestId('llm-suggestion');
    await fireEvent.click(within(blocks[0]).getByTestId('accept-suggestion'));
    await waitFor(() => expect(screen.getAllByTestId('llm-suggestion').length).toBe(1));
    const ta = screen.getByTestId('editor-split-textarea');
    expect(ta.value).toContain('## Error Handling');
    expect(ta.value).not.toContain('## Timeouts');
    expect(api.specsSave).not.toHaveBeenCalled();
  });
});

// ── §3 Progressive Preview: instant tier, debounced fast tier, ghost modes ──

describe('EditorSplit — system-explorer.md §3 progressive preview', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('instant tier: impact bar shows connected specs, repos implementing, governed nodes', async () => {
    const { api } = await import('../lib/api.js');
    api.specLinks.mockResolvedValueOnce([
      // One inbound implements link from another repo, one outbound link.
      { source_path: 'specs/other.md', target_path: 'specs/auth.md', link_type: 'implements', source_repo_id: 'repo-9' },
      { source_path: 'specs/auth.md', target_path: 'specs/dependency-graph.md', link_type: 'references' },
    ]);
    api.repoGraph.mockResolvedValue({
      nodes: [
        { id: 'n1', name: 'Auth', spec_path: 'specs/auth.md' },
        { id: 'n2', name: 'Session', spec_path: 'specs/auth.md' },
        { id: 'n3', name: 'Unrelated', spec_path: null },
      ],
      edges: [],
    });
    render(EditorSplit, {
      props: { content: '# auth', repoId: 'repo-1', specPath: 'specs/auth.md' },
    });
    const bar = await screen.findByTestId('impact-bar');
    await waitFor(() => {
      expect(screen.getByTestId('impact-connected-specs').textContent).toContain('2 connected');
      expect(screen.getByTestId('impact-repos').textContent).toContain('2 repos');
      expect(screen.getByTestId('impact-governed-nodes').textContent).toContain('2 governed');
    });
    expect(api.specLinks).toHaveBeenCalledWith('specs/auth.md', 'repo-1');
    expect(bar).toBeTruthy();
  });

  it('fast tier: prediction is debounced — one call after the edit pause, not per keystroke', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      api.graphPredict.mockResolvedValue({ predictions: [] });
      render(EditorSplit, {
        props: { content: '', repoId: 'repo-1', specPath: 'specs/auth.md' },
      });
      const ta = screen.getByTestId('editor-split-textarea');
      // Three keystrokes inside the debounce window…
      await fireEvent.input(ta, { target: { value: 'a' } });
      await fireEvent.input(ta, { target: { value: 'ab' } });
      await fireEvent.input(ta, { target: { value: 'abc' } });
      await vi.advanceTimersByTimeAsync(500);
      expect(api.graphPredict).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(800); // past the 1.2s debounce
      await vi.waitFor(() => expect(api.graphPredict).toHaveBeenCalledTimes(1));
      expect(api.graphPredict).toHaveBeenCalledWith('repo-1', {
        spec_path: 'specs/auth.md',
        draft_content: 'abc',
      });
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('fast tier: shows the Predicting… indicator while the call is in flight', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      let resolvePredict;
      api.graphPredict.mockImplementationOnce(
        () => new Promise((resolve) => { resolvePredict = resolve; }),
      );
      render(EditorSplit, {
        props: { content: 'draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
      });
      await vi.advanceTimersByTimeAsync(1300);
      await vi.waitFor(() => expect(screen.getByTestId('predicting-chip')).toBeTruthy());
      resolvePredict({ predictions: [] });
      await vi.waitFor(() =>
        expect(screen.queryByTestId('predicting-chip')).toBeNull(),
      );
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('fast tier: prediction response renders ghosts with the §3 visual language', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      api.repoGraph.mockResolvedValue({
        nodes: [
          { id: 'n1', name: 'SearchQuery', spec_path: 'specs/auth.md', node_type: 'type' },
        ],
        edges: [],
      });
      api.graphPredict.mockResolvedValueOnce({
        predictions: [
          { action: 'add', node_type: 'Type', name: 'VectorIndex', confidence: 0.85, reason: 'vector search', parent_trait: 'SearchQuery' },
          { action: 'modify', node_id: 'n1', name: 'SearchQuery', confidence: 0.9, reason: 'needs modes' },
          { action: 'remove', node_id: 'n1', name: 'SearchQuery', confidence: 0.7, reason: 'superseded' },
        ],
      });
      render(EditorSplit, {
        props: { content: 'draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
      });
      await vi.advanceTimersByTimeAsync(1300);
      await vi.waitFor(() =>
        expect(document.querySelectorAll('[data-testid="ghost-node"]').length).toBe(3),
      );
      const add = document.querySelector('[data-action="add"] .ghost-border');
      expect(add.getAttribute('stroke')).toBe('#22c55e');
      expect(Number(add.getAttribute('opacity'))).toBeGreaterThan(0.5);
      const remove = document.querySelector('[data-action="remove"]');
      expect(remove.querySelector('[data-testid="ghost-strikethrough"]')).toBeTruthy();
      // New relationship (add → parent) renders a dotted green edge.
      const edge = document.querySelector('[data-testid="ghost-edge"]');
      expect(edge).toBeTruthy();
      expect(edge.getAttribute('stroke')).toBe('#22c55e');
      // Ghost tooltip carries the prediction reason.
      await fireEvent.mouseEnter(document.querySelector('[data-action="modify"]'));
      expect(document.querySelector('[data-testid="ghost-tooltip"]').textContent).toContain('needs modes');
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('ghost overlay clears when the spec is saved (and does not re-predict the saved draft)', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      api.repoGraph.mockResolvedValue({
        nodes: [{ id: 'n1', name: 'SearchQuery', spec_path: 'specs/auth.md' }],
        edges: [],
      });
      api.graphPredict.mockResolvedValue({
        predictions: [{ action: 'add', name: 'VectorIndex', confidence: 0.9, reason: 'r' }],
      });
      api.specsSave.mockResolvedValueOnce({ mr_id: 9 });
      render(EditorSplit, {
        props: { content: 'draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
      });
      await vi.advanceTimersByTimeAsync(1300);
      await vi.waitFor(() =>
        expect(document.querySelectorAll('[data-testid="ghost-node"]').length).toBe(1),
      );
      await fireEvent.click(screen.getByRole('button', { name: /save/i }));
      await waitFor(() => expect(api.specsSave).toHaveBeenCalled());
      // Overlay cleared after the save…
      await vi.waitFor(() =>
        expect(document.querySelectorAll('[data-testid="ghost-node"]').length).toBe(0),
      );
      // …and the unchanged saved draft is not re-predicted.
      const calls = api.graphPredict.mock.calls.length;
      await vi.advanceTimersByTimeAsync(3000);
      expect(api.graphPredict.mock.calls.length).toBe(calls);
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('thorough tier: ghosts pulse while the preview agent runs and turn solid on completion', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      api.repoGraph.mockResolvedValue({
        nodes: [{ id: 'n1', name: 'SearchQuery', spec_path: 'specs/auth.md' }],
        edges: [],
      });
      api.graphPredict.mockResolvedValue({
        predictions: [{ action: 'modify', node_id: 'n1', name: 'SearchQuery', confidence: 0.9, reason: 'r' }],
      });
      api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-pulse', agent_id: 'agent-1' });
      api.taskStatus
        .mockResolvedValueOnce({ status: 'in_progress' })
        .mockResolvedValueOnce({ status: 'done' });
      render(EditorSplit, {
        props: { content: 'draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
      });
      await vi.advanceTimersByTimeAsync(1300);
      await vi.waitFor(() =>
        expect(document.querySelectorAll('[data-testid="ghost-node"]').length).toBe(1),
      );
      await fireEvent.click(screen.getByTestId('preview-btn'));
      // While the agent runs: the ghost pulses (§3 "Pulsing border").
      await vi.waitFor(() => {
        const ghost = document.querySelector('[data-testid="ghost-node"]');
        expect(ghost).toBeTruthy();
        expect(ghost.classList.contains('ghost-previewing')).toBe(true);
      });
      await vi.advanceTimersByTimeAsync(10000);
      await vi.advanceTimersByTimeAsync(10000);
      // On completion the ghost becomes solid (§3 "Confirmed by actual
      // implementation").
      await vi.waitFor(() => {
        const ghost = document.querySelector('[data-testid="ghost-node"]');
        expect(ghost).toBeTruthy();
        expect(ghost.classList.contains('ghost-confirmed')).toBe(true);
        expect(ghost.classList.contains('ghost-previewing')).toBe(false);
      });
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('thorough tier: completion resolves the preview MR and renders its real diff', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-mr', agent_id: 'agent-7' });
      api.taskStatus.mockResolvedValueOnce({ status: 'review' });
      api.mergeRequests.mockResolvedValueOnce([
        { id: 'mr-old', task_id: 'other-task', author_agent_id: 'someone-else', repository_id: 'repo-1' },
        { id: 'mr-1', task_id: 'task-mr', author_agent_id: 'agent-7', repository_id: 'repo-1' },
      ]);
      api.mrDiff.mockResolvedValueOnce({
        files_changed: 1,
        insertions: 2,
        deletions: 1,
        files: [
          {
            path: 'src/search.rs',
            status: 'Modified',
            hunks: [
              {
                header: '@@ -1,2 +1,3 @@',
                lines: [
                  { line_type: 'context', content: 'fn search() {' },
                  { line_type: 'delete', content: '    old();' },
                  { line_type: 'add', content: '    new();' },
                ],
              },
            ],
          },
        ],
      });
      render(EditorSplit, {
        props: { content: 'draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
      });
      await fireEvent.click(await screen.findByTestId('preview-btn'));
      await vi.advanceTimersByTimeAsync(10000);
      await vi.waitFor(() => expect(screen.getByTestId('iterate-btn')).toBeTruthy());
      // The MR was resolved via the spawned agent / task linkage…
      expect(api.mergeRequests).toHaveBeenCalledWith({ repository_id: 'repo-1' });
      expect(api.mrDiff).toHaveBeenCalledWith('mr-1');
      // …and its real hunks render in the Code Diff tab.
      await fireEvent.click(screen.getByTestId('tab-code-diff'));
      const panel = await screen.findByTestId('code-diff-panel');
      expect(panel.textContent).toContain('src/search.rs');
      expect(panel.textContent).toContain('new();');
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  // Real wire vocabulary only: GET /api/v1/tasks/{id} serializes status as
  // backlog/in_progress/review/done/blocked/cancelled (task_status_str) —
  // there is no 'failed' status, so the failure path is 'blocked'/'cancelled'.
  it.each(['blocked', 'cancelled'])(
    'thorough tier: a %s task fails the preview and returns to editing',
    async (terminal) => {
      vi.useFakeTimers();
      try {
        const { api } = await import('../lib/api.js');
        api.thoroughPreview.mockResolvedValueOnce({ task_id: 'task-fail', agent_id: 'agent-9' });
        api.taskStatus
          .mockResolvedValueOnce({ status: 'in_progress' })
          .mockResolvedValueOnce({ status: terminal });
        render(EditorSplit, {
          props: { content: 'draft', repoId: 'repo-1', specPath: 'specs/auth.md' },
        });
        await fireEvent.click(await screen.findByTestId('preview-btn'));
        // Running state first (§9 State 2)…
        await vi.waitFor(() => expect(screen.getByTestId('editor-locked')).toBeTruthy());
        await vi.advanceTimersByTimeAsync(10000);
        await vi.advanceTimersByTimeAsync(10000);
        // …then the terminal status fails the preview: error banner shown,
        // editor unlocked, no completion artifacts.
        await vi.waitFor(() => expect(screen.getByTestId('preview-error')).toBeTruthy());
        expect(screen.getByTestId('preview-error').textContent).toContain(terminal);
        await vi.waitFor(() => expect(screen.getByTestId('editor-split-textarea')).toBeTruthy());
        expect(screen.queryByTestId('iterate-btn')).toBeNull();
        expect(api.mergeRequests).not.toHaveBeenCalled();
      } finally {
        vi.useRealTimers();
      }
    },
    20000,
  );
});

// ─── Task-206 review repair: the diff viewer updates mid-run (§5 step 4) ──────
describe('EditorSplit -- meta-spec preview diffs update mid-run', () => {
  it('shows a finished agent\'s diff in the Code Diff tab while the run is still going', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      api.previewMetaSpec.mockResolvedValueOnce({
        preview_id: 'pv-mid',
        agents: [
          { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-mid/retry' },
          { agent_id: 'ag-2', repo_id: 'repo-1', spec_path: 'specs/system/design.md', branch: 'preview/pv-mid/design' },
        ],
      });
      // state stays 'running' (second agent still going), but the FIRST
      // agent already finished and carries its diff — the Code Diff tab
      // must render it now, not only after the whole run completes.
      api.previewMetaSpecStatus.mockResolvedValue({
        preview_id: 'pv-mid',
        state: 'running',
        agents: [
          {
            agent_id: 'ag-1',
            repo_id: 'repo-1',
            spec_path: 'specs/payment/retry.md',
            branch: 'preview/pv-mid/retry',
            status: 'complete',
            diff: {
              files_changed: 1,
              insertions: 1,
              deletions: 1,
              patches: [{ patch: '@@ specs/payment/retry.md@@\n-old line\n+mid-run line' }],
            },
          },
          { agent_id: 'ag-2', repo_id: 'repo-1', spec_path: 'specs/system/design.md', branch: 'preview/pv-mid/design', status: 'running', diff: null },
        ],
      });
      render(EditorSplit, {
        props: {
          content: 'You are a backend dev...',
          context: 'meta-spec',
          workspaceId: 'ws-1',
          targetSpecs: [
            { path: 'specs/payment/retry.md', repo_id: 'repo-1' },
            { path: 'specs/system/design.md', repo_id: 'repo-1' },
          ],
        },
      });
      await fireEvent.click(await screen.findByText('Select All'));
      await fireEvent.click(screen.getByTestId('preview-btn'));
      await vi.advanceTimersByTimeAsync(2100);

      // Still running — no iterate button yet...
      expect(screen.getByTestId('preview-running')).toBeTruthy();
      expect(screen.queryByTestId('iterate-btn')).toBeNull();
      // ...but the finished agent's diff is already on the Code Diff tab.
      await fireEvent.click(screen.getByTestId('tab-code-diff'));
      const panel = await screen.findByTestId('code-diff-panel');
      expect(panel.textContent).toContain('+mid-run line');
      expect(panel.textContent).toContain('-old line');
    } finally {
      vi.useRealTimers();
    }
  }, 20000);
});
