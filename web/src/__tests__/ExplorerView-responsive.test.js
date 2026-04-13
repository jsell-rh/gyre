import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

// ── API mock ─────────────────────────────────────────────────────────────
// Must be before the ExplorerView import so vi.mock runs first.
vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([{ id: 'test-repo-1', name: 'test-repo' }]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([{ id: 'test-repo-1', name: 'test-repo' }]),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    generateExplorerView: vi.fn().mockResolvedValue({ ok: false }),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';

// ── Mocks ────────────────────────────────────────────────────────────────

let mockCtx;
let matchMediaListeners;
let matchMediaMatches;

beforeEach(() => {
  mockCtx = {
    clearRect: vi.fn(),
    fillRect: vi.fn(),
    strokeRect: vi.fn(),
    beginPath: vi.fn(),
    closePath: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    quadraticCurveTo: vi.fn(),
    fillText: vi.fn(),
    measureText: vi.fn(() => ({ width: 40 })),
    scale: vi.fn(),
    setTransform: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    translate: vi.fn(),
    rotate: vi.fn(),
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 0,
    globalAlpha: 1,
    font: '',
    textAlign: '',
    textBaseline: '',
    shadowColor: '',
    shadowBlur: 0,
    setLineDash: vi.fn(),
    getLineDash: vi.fn(() => []),
  };
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    disconnect() {}
    unobserve() {}
  };
  global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
  global.cancelAnimationFrame = vi.fn();

  // Mock WebSocket for ExplorerChat
  global.WebSocket = class MockWebSocket {
    constructor() {
      this.readyState = 1;
      this.send = vi.fn();
      this.close = vi.fn();
      this.onopen = null;
      this.onmessage = null;
      this.onerror = null;
      this.onclose = null;
    }
  };

  // Mock matchMedia to test viewport-resize behavior
  matchMediaListeners = [];
  matchMediaMatches = false; // Start as medium viewport (< 1025px)
  global.window.matchMedia = vi.fn((query) => ({
    matches: matchMediaMatches,
    media: query,
    addEventListener: vi.fn((event, handler) => {
      matchMediaListeners.push({ event, handler });
    }),
    removeEventListener: vi.fn(),
  }));
});

afterEach(() => {
  vi.restoreAllMocks();
  matchMediaListeners = [];
});

// Provide minimal context and repo-scope props so graph loads
const REPO_SCOPE = { type: 'repo', repoId: 'test-repo-1', workspaceId: 'ws1' };

function renderExplorerView(props = {}) {
  return render(ExplorerView, {
    props: { scope: REPO_SCOPE, ...props },
    context: new Map([
      ['navigate', vi.fn()],
      ['goToWorkspaceSettings', vi.fn()],
    ]),
  });
}

// ── Responsive layout tests ──────────────────────────────────────────────

describe('ExplorerView — responsive layout', () => {
  it('renders explorer view with chat area visible by default', async () => {
    const { container } = renderExplorerView();
    // Wait for async graph load (mocked to resolve immediately)
    await waitFor(() => {
      expect(container.querySelector('.explorer-chat-area')).toBeTruthy();
    });
  });

  it('chat collapse button exists in the chat area', async () => {
    const { container } = renderExplorerView();
    await waitFor(() => {
      expect(container.querySelector('.chat-collapse-btn')).toBeTruthy();
    });
    // Expand button should NOT be visible when chat is expanded
    expect(container.querySelector('.chat-expand-btn')).toBeFalsy();
  });

  it('clicking collapse button removes chat area and shows expand button', async () => {
    const { container } = renderExplorerView();
    await waitFor(() => {
      expect(container.querySelector('.explorer-chat-area')).toBeTruthy();
    });

    const collapseBtn = container.querySelector('.chat-collapse-btn');
    expect(collapseBtn).toBeTruthy();
    await fireEvent.click(collapseBtn);

    // After collapse: chat area removed, expand button appears
    expect(container.querySelector('.explorer-chat-area')).toBeFalsy();
    const expandBtn = container.querySelector('.chat-expand-btn');
    expect(expandBtn).toBeTruthy();
  });

  it('clicking expand button restores chat area', async () => {
    const { container } = renderExplorerView();
    await waitFor(() => {
      expect(container.querySelector('.explorer-chat-area')).toBeTruthy();
    });

    // Collapse first
    const collapseBtn = container.querySelector('.chat-collapse-btn');
    expect(collapseBtn).toBeTruthy();
    await fireEvent.click(collapseBtn);
    expect(container.querySelector('.explorer-chat-area')).toBeFalsy();

    // Click expand
    const expandBtn = container.querySelector('.chat-expand-btn');
    expect(expandBtn).toBeTruthy();
    await fireEvent.click(expandBtn);

    // Chat area should be restored
    expect(container.querySelector('.explorer-chat-area')).toBeTruthy();
    expect(container.querySelector('.chat-expand-btn')).toBeFalsy();
  });
});

describe('ExplorerView — viewport resize chat state sync', () => {
  it('matchMedia listener is registered for wide viewport breakpoint', async () => {
    renderExplorerView();
    await waitFor(() => {
      expect(window.matchMedia).toHaveBeenCalledWith('(min-width: 1025px)');
    });
    expect(matchMediaListeners.length).toBeGreaterThan(0);
    expect(matchMediaListeners[0].event).toBe('change');
  });

  it('chatCollapsed resets to false when viewport widens past 1025px', async () => {
    const { container } = renderExplorerView();
    await waitFor(() => {
      expect(container.querySelector('.explorer-chat-area')).toBeTruthy();
    });

    // Collapse chat
    const collapseBtn = container.querySelector('.chat-collapse-btn');
    expect(collapseBtn).toBeTruthy();
    await fireEvent.click(collapseBtn);
    expect(container.querySelector('.explorer-chat-area')).toBeFalsy();

    // Simulate viewport widening past 1025px
    const changeHandler = matchMediaListeners.find(l => l.event === 'change');
    expect(changeHandler).toBeTruthy();
    changeHandler.handler({ matches: true });

    // Wait for Svelte reactivity to process the state change
    await waitFor(() => {
      expect(container.querySelector('.explorer-chat-area')).toBeTruthy();
    });
  });
});

describe('ExplorerView — chat panel DOM structure', () => {
  it('chat collapse bar contains a button with correct aria-label', async () => {
    const { container } = renderExplorerView();
    await waitFor(() => {
      expect(container.querySelector('.chat-collapse-btn')).toBeTruthy();
    });
    const collapseBtn = container.querySelector('.chat-collapse-btn');
    expect(collapseBtn.getAttribute('aria-label')).toBe('Collapse chat panel');
    expect(collapseBtn.getAttribute('type')).toBe('button');
  });

  it('expand button has correct aria-label and label text', async () => {
    const { container } = renderExplorerView();
    await waitFor(() => {
      expect(container.querySelector('.chat-collapse-btn')).toBeTruthy();
    });

    // Collapse to show expand button
    const collapseBtn = container.querySelector('.chat-collapse-btn');
    await fireEvent.click(collapseBtn);

    const expandBtn = container.querySelector('.chat-expand-btn');
    expect(expandBtn).toBeTruthy();
    expect(expandBtn.getAttribute('aria-label')).toBe('Open chat panel');
    const label = expandBtn.querySelector('.chat-expand-label');
    expect(label).toBeTruthy();
    expect(label.textContent).toBe('Chat');
  });
});
