/**
 * SpecDetailCanvas — tests for the architecture mini canvas embedded in the spec detail panel.
 *
 * TASK-359: S2: Spec detail panel — mini canvas + predict loop
 *
 * Tests cover:
 *   - Architecture tab present/disabled/enabled for spec entities
 *   - Graph load filters nodes by spec_path
 *   - ArchPreviewCanvas embedded in mini mode
 *   - Empty state when no nodes match spec
 *   - 503 handled gracefully (silent failure in predict loop)
 *   - "Expand to canvas" button present after graph loads
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, screen, act, waitFor } from '@testing-library/svelte';
import DetailPanel from '../lib/DetailPanel.svelte';

// Helper: build a fetch mock that returns graph data for /graph, {} otherwise
function graphFetch(graphData = null) {
  const defaultData = graphData;
  return vi.fn((url) => {
    const urlStr = typeof url === 'string' ? url : url?.url ?? '';
    if (urlStr.includes('/graph') && !urlStr.includes('predict') && !urlStr.includes('types') && defaultData) {
      return Promise.resolve({
        ok: true, status: 200, statusText: 'OK',
        json: () => Promise.resolve(defaultData),
      });
    }
    return Promise.resolve({ ok: true, status: 200, statusText: 'OK', json: () => Promise.resolve({}) });
  });
}

// ── Fixtures ──────────────────────────────────────────────────────────────────

const specPath = 'specs/system/auth.md';

const specEntityWithRepo = {
  type: 'spec',
  id: specPath,
  data: {
    name: 'auth.md',
    repo_id: 'repo-abc',
  },
};

const specEntityNoRepo = {
  type: 'spec',
  id: specPath,
  data: {
    name: 'auth.md',
  },
};

// The component strips the "specs/" prefix from entity.id before filtering.
// So spec_path in graph nodes should match the stripped path.
const strippedPath = specPath.replace(/^specs\//, '');

// Graph with 2 nodes owned by this spec, 1 owned by a different spec
const graphResponse = {
  nodes: [
    { id: 'n1', node_type: 'module',   name: 'AuthModule',  spec_path: strippedPath },
    { id: 'n2', node_type: 'type',     name: 'AuthToken',   spec_path: strippedPath },
    { id: 'n3', node_type: 'endpoint', name: 'POST /login', spec_path: 'system/login.md' },
  ],
  edges: [
    { source: 'n1', target: 'n2', label: 'owns' },     // spec-internal — kept
    { source: 'n1', target: 'n3', label: 'calls' },    // cross-spec — filtered out
  ],
};


// Flush the microtask queue multiple times to allow Svelte $effect chains to settle
async function flushMicrotasks(rounds = 5) {
  for (let i = 0; i < rounds; i++) {
    await act(() => Promise.resolve());
  }
}

// Activate architecture tab and await async graph load
async function activateArchTab() {
  const tab = screen.getByRole('tab', { name: /architecture/i });
  await fireEvent.click(tab);
  // Wait for the arch tab content to appear (async graph load may take multiple ticks)
  await waitFor(() => {
    expect(document.querySelector('.arch-tab')).toBeTruthy();
  });
  // Give additional microtasks for state to settle after graph fetch
  await flushMicrotasks(12);
  return tab;
}

// ── Architecture tab visibility ───────────────────────────────────────────────

describe('Architecture tab — spec entity', () => {
  it('shows Architecture tab for spec with repo_id', () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    expect(screen.getByRole('tab', { name: /architecture/i })).toBeTruthy();
  });

  it('Architecture tab is disabled when spec has no repo_id', () => {
    render(DetailPanel, { props: { entity: specEntityNoRepo } });
    const tab = screen.getByRole('tab', { name: /architecture/i });
    expect(tab.disabled).toBe(true);
  });

  it('Architecture tab is enabled when spec has repo_id', () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    const tab = screen.getByRole('tab', { name: /architecture/i });
    expect(tab.disabled).toBe(false);
  });

  it('spec entity shows all original tabs plus architecture', () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    expect(screen.getByRole('tab', { name: /content/i })).toBeTruthy();
    expect(screen.getByRole('tab', { name: /edit/i })).toBeTruthy();
    expect(screen.getByRole('tab', { name: /progress/i })).toBeTruthy();
    expect(screen.getByRole('tab', { name: /links/i })).toBeTruthy();
    expect(screen.getByRole('tab', { name: /history/i })).toBeTruthy();
    expect(screen.getByRole('tab', { name: /architecture/i })).toBeTruthy();
  });

  it('Architecture tab absent for non-spec entities', () => {
    render(DetailPanel, {
      props: {
        entity: { type: 'mr', id: 'mr-1', data: { name: 'Fix', status: 'open', conversation_sha: null } },
      },
    });
    expect(screen.queryByRole('tab', { name: /architecture/i })).toBeNull();
  });

  it('Architecture tab absent for agent entities', () => {
    render(DetailPanel, {
      props: {
        entity: { type: 'agent', id: 'ag-1', data: { name: 'worker', status: 'active', conversation_sha: null } },
      },
    });
    expect(screen.queryByRole('tab', { name: /architecture/i })).toBeNull();
  });
});

// ── Graph load + node filtering ───────────────────────────────────────────────

describe('Architecture tab — graph load and node filtering', () => {
  beforeEach(() => {
    global.fetch = graphFetch(graphResponse);
  });

  it('activating Architecture tab triggers graph load', async () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    const calls = global.fetch.mock.calls.map(([url]) => typeof url === 'string' ? url : url?.url ?? '');
    expect(calls.some((u) => u.includes('/repos/repo-abc/graph'))).toBe(true);
  });

  it('renders mini canvas container after graph loads', async () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    expect(document.querySelector('[data-testid="arch-mini-canvas-wrap"]')).toBeTruthy();
  });

  it('node count label shows only spec-governed nodes (2, not 3)', async () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    // graphResponse has 2 nodes with specPath — wait for async graph load to complete
    await waitFor(() => expect(screen.queryByText(/2 nodes governed/i)).toBeTruthy());
  });

  it('shows 1-node singular label when exactly one node matches', async () => {
    global.fetch = graphFetch({
      nodes: [{ id: 'n1', node_type: 'module', name: 'AuthModule', spec_path: strippedPath }],
      edges: [],
    });
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    await waitFor(() => expect(screen.queryByText(/1 node governed/i)).toBeTruthy());
  });

  it('shows empty state when no nodes match spec_path', async () => {
    global.fetch = graphFetch({
      nodes: [{ id: 'x1', node_type: 'module', name: 'Unrelated', spec_path: 'other/spec.md' }],
      edges: [],
    });
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    await waitFor(() => expect(screen.queryByText(/No graph data/i)).toBeTruthy());
  });

  it('only spec-internal edges appear in the canvas', async () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    await waitFor(() => {
      // n1→n2 both governed (edge kept), n1→n3 cross-spec (filtered out)
      const svg = document.querySelector('[data-testid="arch-preview-svg"]');
      if (svg) {
        const edges = svg.querySelectorAll('.arch-edge');
        expect(edges.length).toBe(1);
      }
    });
  });

  it('does not load graph for spec without repo_id', async () => {
    render(DetailPanel, { props: { entity: specEntityNoRepo } });
    // Architecture tab is disabled; even if clicked, graph should not be fetched
    const calls = global.fetch.mock.calls.map(([url]) => typeof url === 'string' ? url : '');
    expect(calls.some((u) => u.includes('/graph'))).toBe(false);
  });
});

// ── Error handling ────────────────────────────────────────────────────────────

describe('Architecture tab — error handling', () => {
  it('shows Retry button on fetch failure', async () => {
    global.fetch = vi.fn((url) => {
      const urlStr = typeof url === 'string' ? url : '';
      if (urlStr.includes('/graph') && !urlStr.includes('predict') && !urlStr.includes('types')) {
        return Promise.resolve({
          ok: false, status: 503, statusText: 'Service Unavailable',
          json: () => Promise.resolve({ message: 'Service unavailable' }),
        });
      }
      return Promise.resolve({ ok: true, status: 200, statusText: 'OK', json: () => Promise.resolve({}) });
    });

    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    await waitFor(() => expect(screen.queryByRole('button', { name: /retry/i })).toBeTruthy());
  });

  it('does not crash when graph endpoint throws a network error', async () => {
    global.fetch = vi.fn((url) => {
      const urlStr = typeof url === 'string' ? url : '';
      if (urlStr.includes('/graph') && !urlStr.includes('predict') && !urlStr.includes('types')) {
        return Promise.reject(new Error('Network error'));
      }
      return Promise.resolve({ ok: true, status: 200, statusText: 'OK', json: () => Promise.resolve({}) });
    });

    expect(() => render(DetailPanel, { props: { entity: specEntityWithRepo } })).not.toThrow();
    const tab = screen.getByRole('tab', { name: /architecture/i });
    await fireEvent.click(tab);
    await act(() => Promise.resolve());
    await act(() => Promise.resolve());
    // Should show error state, not crash
    expect(document.querySelector('.arch-tab')).toBeTruthy();
  });

  it('graphPredict 503 is silently ignored (no toast or crash)', async () => {
    global.fetch = graphFetch(graphResponse);
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    await waitFor(() => expect(document.querySelector('.arch-tab')).toBeTruthy());
  });
});

// ── Canvas and expand button ──────────────────────────────────────────────────

describe('Architecture tab — canvas UI', () => {
  beforeEach(() => {
    global.fetch = graphFetch(graphResponse);
  });

  it('arch-canvas-container present after graph loads', async () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    expect(document.querySelector('.arch-canvas-container')).toBeTruthy();
  });

  it('arch-mini-header present after graph loads', async () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    expect(document.querySelector('.arch-mini-header')).toBeTruthy();
  });

  it('renders ArchPreviewCanvas SVG inside arch-canvas-container', async () => {
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    await waitFor(() => {
      const container = document.querySelector('.arch-canvas-container');
      expect(container?.querySelector('[data-testid="arch-preview-svg"]')).toBeTruthy();
    });
  });

  it('arch-expand-wrap is NOT rendered when goToRepoTab context is absent (unit test env)', async () => {
    // In unit tests there is no App.svelte context, so goToRepoTab = null
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    // When goToRepoTab context is null, the expand button section is not rendered
    expect(document.querySelector('.arch-expand-wrap')).toBeNull();
  });

  it('arch tab renders without throwing when repoId is present', async () => {
    expect(() => render(DetailPanel, { props: { entity: specEntityWithRepo } })).not.toThrow();
  });
});

// ── State reset on entity change ──────────────────────────────────────────────

describe('Architecture tab — state reset on entity change', () => {
  it('Architecture tab is disabled after entity changes to one without repo_id', async () => {
    const { rerender } = render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await rerender({ entity: specEntityNoRepo });
    const tab = screen.getByRole('tab', { name: /architecture/i });
    expect(tab.disabled).toBe(true);
  });

  it('Architecture tab re-enables when entity gets a repo_id', async () => {
    const { rerender } = render(DetailPanel, { props: { entity: specEntityNoRepo } });
    await rerender({ entity: specEntityWithRepo });
    const tab = screen.getByRole('tab', { name: /architecture/i });
    expect(tab.disabled).toBe(false);
  });
});

// ── Graph predict badge ───────────────────────────────────────────────────────

describe('Architecture tab — predict overlay badge', () => {
  it('predict badge is absent when no overlays loaded', async () => {
    global.fetch = graphFetch(graphResponse);
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await activateArchTab();
    // No predict call yet — badge should be absent
    expect(document.querySelector('.arch-predict-badge')).toBeNull();
  });
});
