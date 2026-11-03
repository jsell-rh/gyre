import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent, screen, waitFor } from '@testing-library/svelte';
import DetailPanel from '../lib/DetailPanel.svelte';

const specEntityWithRepo = {
  type: 'spec',
  id: 'specs/system/auth.md',
  data: { name: 'auth.md', repo_id: 'repo-abc', title: 'Auth Spec' },
};

describe('debug esc', () => {
  it('what happens on Escape', async () => {
    global.fetch.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({ content: '# Auth' }),
    });
    const origAdd = window.addEventListener.bind(window);
    const regLog = [];
    window.addEventListener = (type, fn, opts) => {
      regLog.push(type + ':' + (fn.name || 'anon'));
      return origAdd(type, fn, opts);
    };
    const onback = vi.fn();
    const onclose = vi.fn();
    const { container } = render(DetailPanel, {
      props: { entity: specEntityWithRepo, onback, onclose },
    });
    const editTab = screen.getByRole('tab', { name: /edit/i });
    await fireEvent.click(editTab);
    const previewBtn = await screen.findByRole('button', { name: /preview/i });
    await fireEvent.click(previewBtn);
    await screen.findByRole('button', { name: /close editor split/i });

    // Instrument window listeners
    const log = [];
    window.addEventListener('keydown', (e) => {
      log.push('outer-listener defaultPrevented=' + e.defaultPrevented + ' escConsumed=' + Boolean(e._escConsumed));
    });

    const evt = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    window.dispatchEvent(evt);
    log.push('dispatched');
    await new Promise((r) => setTimeout(r, 50));
    console.log('LOG:', JSON.stringify(log));
    console.log('REGLIST:', JSON.stringify(regLog));
    console.log('WINDOW-ESC onback calls:', onback.mock.calls.length, 'onclose calls:', onclose.mock.calls.length);
    console.log('expanded after:', Boolean(container.querySelector('.detail-panel.expanded')));
    console.log('editor split still there:', Boolean(screen.queryByRole('button', { name: /close editor split/i })));

    // Second: dispatch bubbling from the textarea
    const ta = screen.queryByTestId('editor-split-textarea');
    if (ta) {
      await fireEvent.keyDown(ta, { key: 'Escape', bubbles: true, cancelable: true });
      await new Promise((r) => setTimeout(r, 50));
      console.log('TEXTAREA-ESC onback calls:', onback.mock.calls.length, 'onclose calls:', onclose.mock.calls.length);
      console.log('after textarea esc — expanded:', Boolean(container.querySelector('.detail-panel.expanded')));
      console.log('after textarea esc — editor split still there:', Boolean(screen.queryByRole('button', { name: /close editor split/i })));
    }
  });
});
