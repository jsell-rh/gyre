import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import SpecConflictDialog from '../lib/SpecConflictDialog.svelte';

const conflict = {
  spec_path: 'specs/system/auth.md',
  base_sha: 'old',
  current_sha: 'new',
  current_content: 'server version',
  submitted_content: 'my version',
  diff: [
    { op: 'context', text: 'unchanged line' },
    { op: 'remove', text: 'server-only line' },
    { op: 'add', text: 'my-only line' },
  ],
};

describe('SpecConflictDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders nothing when there is no conflict', () => {
    render(SpecConflictDialog, { props: { conflict: null } });
    expect(screen.queryByTestId('spec-conflict-dialog')).toBeNull();
  });

  it('renders a side-by-side diff with per-op left/right alignment', () => {
    const { container } = render(SpecConflictDialog, { props: { conflict } });
    expect(screen.getByTestId('spec-conflict-dialog')).toBeTruthy();

    const rows = container.querySelectorAll('.diff-row');
    expect(rows.length).toBe(3);

    // context → both sides show the line
    const ctxCells = rows[0].querySelectorAll('.diff-cell');
    expect(ctxCells[0].textContent).toBe('unchanged line');
    expect(ctxCells[1].textContent).toBe('unchanged line');

    // remove → left (server) only, right empty
    const remCells = rows[1].querySelectorAll('.diff-cell');
    expect(remCells[0].textContent).toBe('server-only line');
    expect(remCells[1].textContent).toBe('');

    // add → right (yours) only, left empty
    const addCells = rows[2].querySelectorAll('.diff-cell');
    expect(addCells[0].textContent).toBe('');
    expect(addCells[1].textContent).toBe('my-only line');
  });

  it('invokes onOverwrite when Overwrite is clicked', async () => {
    const onOverwrite = vi.fn();
    render(SpecConflictDialog, { props: { conflict, onOverwrite } });
    await fireEvent.click(screen.getByTestId('conflict-overwrite-btn'));
    expect(onOverwrite).toHaveBeenCalledOnce();
  });

  it('invokes onDiscard when Discard is clicked', async () => {
    const onDiscard = vi.fn();
    render(SpecConflictDialog, { props: { conflict, onDiscard } });
    await fireEvent.click(screen.getByTestId('conflict-discard-btn'));
    expect(onDiscard).toHaveBeenCalledOnce();
  });

  it('invokes onClose when the close button is clicked', async () => {
    const onClose = vi.fn();
    render(SpecConflictDialog, { props: { conflict, onClose } });
    await fireEvent.click(screen.getByRole('button', { name: /close conflict dialog/i }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
