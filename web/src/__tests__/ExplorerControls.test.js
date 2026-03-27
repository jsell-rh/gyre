import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import ExplorerControls from '../components/ExplorerControls.svelte';
import ExplorerFilterPanel from '../components/ExplorerFilterPanel.svelte';
import ExplorerCodeTab from '../components/ExplorerCodeTab.svelte';

// Top-level mock so vitest hoisting works correctly
vi.mock('../lib/api.js', () => ({
  api: {
    explorerViews: vi.fn().mockResolvedValue([
      { id: 'sv-1', name: 'Auth Flow', layout: 'graph', data: {} },
    ]),
    saveExplorerView: vi.fn().mockResolvedValue({ id: 'sv-new', name: 'Generated view' }),
    deleteExplorerView: vi.fn().mockResolvedValue(null),
    generateExplorerView: vi.fn().mockResolvedValue({
      ok: true,
      body: {
        getReader: () => ({
          read: vi.fn()
            .mockResolvedValueOnce({
              done: false,
              value: new TextEncoder().encode(
                'event: partial\ndata: Loading\n\nevent: complete\ndata: {"view_spec":{"name":"Test","layout":"graph"},"explanation":"Here is the auth flow"}\n\n'
              ),
            })
            .mockResolvedValueOnce({ done: true }),
        }),
      },
    }),
    repoBranches: vi.fn().mockResolvedValue([
      { id: 'b-1', name: 'main', last_commit: 'abc1234def', author: 'alice', status: 'active' },
      { id: 'b-2', name: 'feat/auth', last_commit: 'def5678abc', author: 'bob', status: 'active' },
    ]),
    mergeRequests: vi.fn().mockResolvedValue([
      { id: 'mr-1', title: 'Add auth', status: 'open', author_id: 'alice', updated_at: Math.floor(Date.now() / 1000) - 3600 },
    ]),
    mergeQueue: vi.fn().mockResolvedValue([
      { id: 'q-1', merge_request_id: 'mr-1', repository_id: 'repo-1', priority: 50, status: 'queued' },
    ]),
  },
}));

import { api } from '../lib/api.js';

// Stub getContext to return openDetailPanel stub
import { getContext } from 'svelte';
vi.mock('svelte', async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    getContext: vi.fn().mockReturnValue(vi.fn()),
  };
});

describe('ExplorerControls', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.explorerViews.mockResolvedValue([
      { id: 'sv-1', name: 'Auth Flow', layout: 'graph', data: {} },
    ]);
  });

  it('renders without throwing', () => {
    expect(() => render(ExplorerControls)).not.toThrow();
  });

  it('renders lens selector with 3 options', () => {
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1' },
    });
    const lensSelect = container.querySelector('#lens-select');
    expect(lensSelect).toBeTruthy();
    const options = lensSelect.querySelectorAll('option');
    expect(options.length).toBe(3);
  });

  it('lens option Observable is disabled', () => {
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1' },
    });
    const lensSelect = container.querySelector('#lens-select');
    const options = [...lensSelect.querySelectorAll('option')];
    const observable = options.find(o => o.textContent.includes('Observable'));
    expect(observable).toBeTruthy();
    expect(observable.disabled).toBe(true);
  });

  it('renders view selector with built-in options', () => {
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1' },
    });
    const viewSelect = container.querySelector('#view-select');
    expect(viewSelect).toBeTruthy();
    const opts = [...viewSelect.querySelectorAll('option')].map(o => o.textContent);
    expect(opts.some(o => o.includes('Boundary'))).toBe(true);
    expect(opts.some(o => o.includes('Spec Realization'))).toBe(true);
    expect(opts.some(o => o.includes('Change'))).toBe(true);
  });

  it('shows saved views in view selector after load', async () => {
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1' },
    });
    await waitFor(() => {
      const opts = [...container.querySelectorAll('#view-select option')].map(o => o.textContent);
      expect(opts.some(o => o.includes('Auth Flow'))).toBe(true);
    });
  });

  it('calls onLensChange when lens is selected', async () => {
    const onLensChange = vi.fn();
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', onLensChange },
    });
    const lensSelect = container.querySelector('#lens-select');
    await fireEvent.change(lensSelect, { target: { value: 'evaluative' } });
    expect(onLensChange).toHaveBeenCalledWith('evaluative');
  });

  it('calls onViewChange when view is selected', async () => {
    const onViewChange = vi.fn();
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', onViewChange },
    });
    const viewSelect = container.querySelector('#view-select');
    await fireEvent.change(viewSelect, { target: { value: 'change' } });
    expect(onViewChange).toHaveBeenCalledWith(expect.objectContaining({ layout: 'timeline' }));
  });

  it('calls onSearch when search input changes', async () => {
    const onSearch = vi.fn();
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', onSearch },
    });
    const searchInput = container.querySelector('.ctrl-search');
    expect(searchInput).toBeTruthy();
    await fireEvent.input(searchInput, { target: { value: 'auth' } });
    expect(onSearch).toHaveBeenCalledWith('auth');
  });

  it('calls onFilterToggle when filter button clicked', async () => {
    const onFilterToggle = vi.fn();
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', onFilterToggle },
    });
    const filterBtn = container.querySelector('.icon-btn');
    expect(filterBtn).toBeTruthy();
    await fireEvent.click(filterBtn);
    expect(onFilterToggle).toHaveBeenCalled();
  });

  it('renders LLM Ask input', () => {
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1' },
    });
    const askInput = container.querySelector('.ctrl-ask');
    expect(askInput).toBeTruthy();
  });

  it('shows LLM explanation after ask', async () => {
    const onViewChange = vi.fn();
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', onViewChange },
    });
    const askInput = container.querySelector('.ctrl-ask');
    await fireEvent.input(askInput, { target: { value: 'How does auth work?' } });
    // Simulate binding update
    askInput.value = 'How does auth work?';
    const askBtn = container.querySelector('.ask-btn');
    await fireEvent.click(askBtn);
    await waitFor(() => {
      expect(api.generateExplorerView).toHaveBeenCalledWith('ws-1', expect.objectContaining({ question: expect.any(String) }));
    });
  });

  it('does NOT render Architecture/Code tab switcher at workspace scope', () => {
    const { container } = render(ExplorerControls, {
      props: { scope: 'workspace', workspaceId: 'ws-1' },
    });
    const tabSwitcher = container.querySelector('.tab-switcher');
    expect(tabSwitcher).toBeNull();
  });

  it('renders Architecture/Code tab switcher at repo scope', () => {
    const { container } = render(ExplorerControls, {
      props: { scope: 'repo', repoId: 'repo-1', workspaceId: 'ws-1' },
    });
    const tabSwitcher = container.querySelector('.tab-switcher');
    expect(tabSwitcher).toBeTruthy();
  });

  it('Code tab switcher has Architecture and Code buttons', () => {
    const { container } = render(ExplorerControls, {
      props: { scope: 'repo', repoId: 'repo-1', workspaceId: 'ws-1' },
    });
    const tabs = [...container.querySelectorAll('.tab-btn')].map(b => b.textContent.trim());
    expect(tabs).toContain('Architecture');
    expect(tabs).toContain('Code');
  });

  it('hides canvas controls when Code tab is active', async () => {
    const { container } = render(ExplorerControls, {
      props: { scope: 'repo', activeTab: 'code', repoId: 'repo-1', workspaceId: 'ws-1' },
    });
    const lensSelect = container.querySelector('#lens-select');
    expect(lensSelect).toBeNull();
  });

  it('shows canvas controls when Architecture tab is active', () => {
    const { container } = render(ExplorerControls, {
      props: { scope: 'repo', activeTab: 'architecture', repoId: 'repo-1', workspaceId: 'ws-1' },
    });
    const lensSelect = container.querySelector('#lens-select');
    expect(lensSelect).toBeTruthy();
  });

  it('does NOT render playback controls by default', () => {
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', showPlayback: false },
    });
    const playback = container.querySelector('.playback-controls');
    expect(playback).toBeNull();
  });

  it('renders playback controls when showPlayback=true', () => {
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', showPlayback: true },
    });
    const playback = container.querySelector('.playback-controls');
    expect(playback).toBeTruthy();
  });

  it('playback controls include play, pause, step buttons', () => {
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', showPlayback: true },
    });
    const html = container.innerHTML;
    expect(html).toContain('▶');
    expect(html).toContain('⏸');
    expect(html).toContain('⏭');
  });

  it('playback speed dropdown has correct options', () => {
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', showPlayback: true },
    });
    const speedSelect = container.querySelector('#speed-select');
    expect(speedSelect).toBeTruthy();
    const opts = [...speedSelect.querySelectorAll('option')].map(o => o.value);
    expect(opts).toContain('1x');
    expect(opts).toContain('0.25x');
    expect(opts).toContain('2x');
  });

  it('calls onPlaybackChange when play clicked', async () => {
    const onPlaybackChange = vi.fn();
    const { container } = render(ExplorerControls, {
      props: { workspaceId: 'ws-1', showPlayback: true, onPlaybackChange },
    });
    const playBtn = [...container.querySelectorAll('.ctrl-btn.icon-btn')]
      .find(b => b.textContent.includes('▶'));
    await fireEvent.click(playBtn);
    expect(onPlaybackChange).toHaveBeenCalledWith({ cmd: 'play', value: undefined });
  });

  it('renders scrub bar when traceTimeline provided', () => {
    const { container } = render(ExplorerControls, {
      props: {
        workspaceId: 'ws-1',
        showPlayback: true,
        traceTimeline: { min: 0, max: 1000, current: 500 },
      },
    });
    const scrub = container.querySelector('.scrub-bar');
    expect(scrub).toBeTruthy();
  });
});

describe('ExplorerFilterPanel', () => {
  it('renders nothing when visible=false', () => {
    const { container } = render(ExplorerFilterPanel, { props: { visible: false } });
    expect(container.querySelector('.filter-panel')).toBeNull();
  });

  it('renders filter panel when visible=true', () => {
    const { container } = render(ExplorerFilterPanel, { props: { visible: true } });
    expect(container.querySelector('.filter-panel')).toBeTruthy();
  });

  it('shows 4 category checkboxes', () => {
    const { container } = render(ExplorerFilterPanel, { props: { visible: true } });
    const checkboxes = container.querySelectorAll('input[type="checkbox"]');
    expect(checkboxes.length).toBe(4);
  });

  it('shows visibility radio options', () => {
    const { container } = render(ExplorerFilterPanel, { props: { visible: true } });
    const radios = container.querySelectorAll('input[type="radio"]');
    expect(radios.length).toBeGreaterThanOrEqual(3);
  });

  it('calls onfilterchange when category toggled', async () => {
    const onfilterchange = vi.fn();
    const { container } = render(ExplorerFilterPanel, { props: { visible: true, onfilterchange } });
    const firstCheckbox = container.querySelector('input[type="checkbox"]');
    await fireEvent.change(firstCheckbox);
    expect(onfilterchange).toHaveBeenCalled();
  });

  it('renders churn slider', () => {
    const { container } = render(ExplorerFilterPanel, { props: { visible: true } });
    const slider = container.querySelector('.churn-slider');
    expect(slider).toBeTruthy();
  });
});

describe('ExplorerCodeTab', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.repoBranches.mockResolvedValue([
      { id: 'b-1', name: 'main', last_commit: 'abc1234', author: 'alice', status: 'active' },
    ]);
    api.mergeRequests.mockResolvedValue([
      { id: 'mr-1', title: 'Add auth', status: 'open', author_id: 'alice', updated_at: Date.now() / 1000 - 3600 },
    ]);
    api.mergeQueue.mockResolvedValue([]);
  });

  it('renders without throwing', () => {
    expect(() => render(ExplorerCodeTab)).not.toThrow();
  });

  it('shows 3 sub-tabs: Branches, Merge Requests, Merge Queue', () => {
    const { container } = render(ExplorerCodeTab, { props: { repoId: 'repo-1' } });
    const tabs = [...container.querySelectorAll('.subtab-btn')].map(b => b.textContent.trim());
    expect(tabs).toContain('Branches');
    expect(tabs).toContain('Merge Requests');
    expect(tabs).toContain('Merge Queue');
  });

  it('loads and shows branches by default', async () => {
    const { container } = render(ExplorerCodeTab, { props: { repoId: 'repo-1' } });
    await waitFor(() => {
      expect(container.innerHTML).toContain('main');
    });
  });

  it('filter input is present', () => {
    const { container } = render(ExplorerCodeTab, { props: { repoId: 'repo-1' } });
    expect(container.querySelector('.filter-input')).toBeTruthy();
  });

  it('clicking Merge Requests tab loads MRs', async () => {
    const { container } = render(ExplorerCodeTab, { props: { repoId: 'repo-1' } });
    const mrTab = [...container.querySelectorAll('.subtab-btn')].find(b => b.textContent.includes('Merge Requests'));
    await fireEvent.click(mrTab);
    await waitFor(() => {
      expect(api.mergeRequests).toHaveBeenCalledWith({ repository_id: 'repo-1' });
    });
  });

  it('clicking Merge Queue tab loads queue', async () => {
    const { container } = render(ExplorerCodeTab, { props: { repoId: 'repo-1' } });
    const qTab = [...container.querySelectorAll('.subtab-btn')].find(b => b.textContent.includes('Merge Queue'));
    await fireEvent.click(qTab);
    await waitFor(() => {
      expect(api.mergeQueue).toHaveBeenCalled();
    });
  });

  it('sortable columns in branches tab', async () => {
    const { container } = render(ExplorerCodeTab, { props: { repoId: 'repo-1' } });
    await waitFor(() => {
      const sortBtns = container.querySelectorAll('.sort-btn');
      expect(sortBtns.length).toBeGreaterThan(0);
    });
  });
});
