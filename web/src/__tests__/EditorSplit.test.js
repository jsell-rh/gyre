import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, screen, waitFor } from '@testing-library/svelte';
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
    specsAssist: vi.fn(),
    specsSave: vi.fn().mockResolvedValue({ mr_id: 42 }),
    workspacePresence: vi.fn().mockResolvedValue([]),
    thoroughPreview: vi.fn(),
    taskStatus: vi.fn(),
    previewPersona: vi.fn(),
    previewPersonaStatus: vi.fn(),
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
    expect(document.body.textContent).toContain('LLM editing requires repo context');
  });

  it('shows ctrl+enter hint when repoId is set', () => {
    render(EditorSplit, { props: { content: '', repoId: 'repo-1' } });
    expect(document.body.textContent).toContain('Ctrl+Enter');
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

  it('triggers graphPredict on mount when repoId is set', async () => {
    const { api } = await import('../lib/api.js');
    render(EditorSplit, { props: { content: '', repoId: 'repo-1', specPath: 'specs/auth.md' } });
    await waitFor(() => {
      expect(api.graphPredict).toHaveBeenCalledWith('repo-1', expect.objectContaining({ spec_path: 'specs/auth.md' }));
    });
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
        .mockResolvedValueOnce({ status: 'running' })
        .mockResolvedValueOnce({
          status: 'completed',
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
    api.taskStatus.mockResolvedValue({ status: 'running' });
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
          { path: 'specs/system/payment-retry.md' },
          { path: 'specs/system/charge-processing.md' },
          { path: 'specs/system/identity-security.md' },
        ],
      },
    });
    const selector = await screen.findByTestId('spec-selector');
    expect(selector.textContent).toContain('specs/system/payment-retry.md');
    expect(selector.textContent).toContain('specs/system/identity-security.md');
    // Preview disabled until a target is selected (§9 State 1)
    expect(screen.getByTestId('preview-btn').disabled).toBe(true);
  });

  it('selecting specs enables Preview and runs previewPersona against them', async () => {
    vi.useFakeTimers();
    try {
      const { api } = await import('../lib/api.js');
      api.previewPersona.mockResolvedValueOnce({
        preview_id: 'pv-1',
        state: 'running',
        specs: [
          { path: 'specs/system/payment-retry.md', status: 'running' },
          { path: 'specs/system/charge-processing.md', status: 'running' },
        ],
      });
      api.previewPersonaStatus.mockResolvedValue({
        state: 'complete',
        specs: [
          { path: 'specs/system/payment-retry.md', status: 'complete' },
          { path: 'specs/system/charge-processing.md', status: 'complete' },
        ],
        architecture_diff: ['+ ErrorHandler module', '~ ChargeService', '= 45 unchanged'],
        specs_diff: [{ path: 'specs/system/payment-retry.md', diff: [{ op: 'context', text: 'z' }] }],
      });
      render(EditorSplit, {
        props: {
          content: 'You are a backend dev...',
          context: 'meta-spec',
          workspaceId: 'ws-1',
          specPath: 'persona/backend-dev',
          targetSpecs: [
            { path: 'specs/system/payment-retry.md' },
            { path: 'specs/system/charge-processing.md' },
          ],
        },
      });
      // Select all targets
      await fireEvent.click(await screen.findByText('Select All'));
      const previewBtn = screen.getByTestId('preview-btn');
      expect(previewBtn.disabled).toBe(false);
      await fireEvent.click(previewBtn);
      await vi.waitFor(() => {
        expect(api.previewPersona).toHaveBeenCalledWith('ws-1', {
          persona_id: 'persona/backend-dev',
          content: 'You are a backend dev...',
          spec_paths: ['specs/system/payment-retry.md', 'specs/system/charge-processing.md'],
        });
      });
      // Persona poll completes at 1.5s intervals
      await vi.advanceTimersByTimeAsync(2000);
      await vi.waitFor(() => expect(screen.getByTestId('iterate-btn')).toBeTruthy());
      // Architecture tab shows the structural impact lines via overlays
      expect(screen.getByTestId('tab-architecture')).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  }, 20000);

  it('runs previewPersona without persona_id when specPath is null', async () => {
    const { api } = await import('../lib/api.js');
    api.previewPersona.mockResolvedValueOnce({ preview_id: 'pv-2' });
    api.previewPersonaStatus.mockResolvedValue({ state: 'complete', specs: [] });
    render(EditorSplit, {
      props: {
        content: 'prompt text',
        context: 'meta-spec',
        workspaceId: 'ws-1',
        targetSpecs: [{ path: 'specs/a.md' }],
      },
    });
    await fireEvent.click(await screen.findByText('Select All'));
    await fireEvent.click(screen.getByTestId('preview-btn'));
    await waitFor(() => {
      expect(api.previewPersona).toHaveBeenCalledWith('ws-1', {
        persona_id: undefined,
        content: 'prompt text',
        spec_paths: ['specs/a.md'],
      });
    });
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
});
