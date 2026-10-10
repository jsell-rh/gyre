// Final probe: DEFAULT side-panel wiring — onback undefined (empty history),
// onclose wired (App.svelte closeDetailPanel). Realistic Esc from the editor.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, screen } from '@testing-library/svelte';
import DetailPanel from '../lib/DetailPanel.svelte';
import fs from 'node:fs';

const specEntityWithRepo = {
  type: 'spec',
  id: 'specs/system/auth.md',
  data: { name: 'auth.md', repo_id: 'repo-abc', title: 'Auth Spec' },
};

describe('Esc from editor textarea, default side-panel wiring (no onback)', () => {
  beforeEach(() => {
    global.fetch.mockResolvedValue({
      ok: true, status: 200, json: async () => ({ content: '# Auth' }),
    });
  });

  it('what the user sees after Esc', async () => {
    const onclose = vi.fn();
    const { container } = render(DetailPanel, { props: { entity: specEntityWithRepo, onclose } });
    await fireEvent.click(screen.getByRole('tab', { name: /edit/i }));
    await screen.findByRole('button', { name: /preview/i });
    await fireEvent.click(screen.getByRole('button', { name: /preview/i }));
    const ta = await screen.findByTestId('editor-split-textarea');
    expect(ta).toBeTruthy();

    await fireEvent.keyDown(ta, { key: 'Escape' });
    await new Promise(r => setTimeout(r, 200));

    const results = {
      onclose_calls: onclose.mock.calls.length,
      panel_present: Boolean(container.querySelector('.detail-panel')),
      normal_detail_view: Boolean(container.querySelector('.panel-header')),
      editor_split_present: Boolean(container.querySelector('.editor-split')),
    };
    fs.writeFileSync('/tmp/stage/review-evidence/esc-probe3-result.json', JSON.stringify(results, null, 2));
    expect(true).toBe(true);
  });
});
