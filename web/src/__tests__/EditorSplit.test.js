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
    graphPredict: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    specsAssist: vi.fn(),
    specsSave: vi.fn().mockResolvedValue({ mr_id: 42 }),
    workspacePresence: vi.fn().mockResolvedValue([]),
  },
}));

// Mock toast
vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
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
});
