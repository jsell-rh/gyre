/**
 * WorkspaceHomeArchitecture.test.js
 *
 * Tests for the collapsible Architecture section in WorkspaceHome
 * (ui-navigation.md §2 + ui-layout.md §5 Workspace Scope — Realized
 * Architecture).
 *
 * Spec requirements:
 *   - Collapsed by default; "Show workspace graph" / "Hide workspace graph" toggle
 *   - When expanded, renders the workspace-scoped Explorer: repos as nodes,
 *     cross-repo dependencies as edges (C4 Container level)
 *   - Repo drill from the workspace graph routes through the host shell
 *     (onSelectRepo(repo, 'architecture')) so the URL/breadcrumb update —
 *     ui-layout.md §5 "Double-click → drill down to crate level (scope
 *     changes to repo)"
 *   - A repo from another workspace is NOT opened (scope containment)
 *
 * The ExplorerView itself (graph rendering, empty state, single-click detail
 * panel, fallback drill) is covered by ExplorerViewWorkspaceGraph.test.js.
 * Here it is stubbed so these tests assert the section wiring only.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// ── Mocks ──────────────────────────────────────────────────────────────────────

vi.mock('../lib/api.js', () => ({
  api: {
    myNotifications: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    approveSpec: vi.fn(),
    revokeSpec: vi.fn(),
    enqueue: vi.fn(),
    markNotificationRead: vi.fn(),
    getWorkspaceBriefing: vi.fn().mockResolvedValue({ narrative: '' }),
    briefingAsk: vi.fn(),
    tasks: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    mrGates: vi.fn().mockResolvedValue([]),
    mrDiff: vi.fn().mockResolvedValue({ files_changed: 0, insertions: 0, deletions: 0 }),
    updateTaskStatus: vi.fn().mockResolvedValue({}),
    agents: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    costSummary: vi.fn().mockResolvedValue([]),
    agent: vi.fn().mockResolvedValue({ name: 'test-agent' }),
    task: vi.fn().mockResolvedValue({ title: 'test-task' }),
    mergeRequest: vi.fn().mockResolvedValue({ title: 'test-mr' }),
    mergeQueue: vi.fn().mockResolvedValue([]),
    mergeQueueGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    workspaceDependencyGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    staleDependencies: vi.fn().mockResolvedValue([]),
    breakingChanges: vi.fn().mockResolvedValue([]),
    sendAgentMessage: vi.fn().mockResolvedValue({}),
    createWorkspace: vi.fn().mockResolvedValue({ id: 'ws-new', name: 'New WS', slug: 'new-ws' }),
    repoBlastRadius: vi.fn().mockResolvedValue({ direct: [], transitive: [], total: 0 }),
    repoDependents: vi.fn().mockResolvedValue([]),
    workspaceDependencyPolicy: vi.fn().mockResolvedValue(null),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastInfo: vi.fn(),
  toastError: vi.fn(),
}));

// Stub ExplorerView (Svelte 5 component convention: (anchor, props)).
// Renders a marker plus a button that invokes the captured onEnterRepo prop,
// so tests can drive the repo-drill wiring without mounting the real canvas.
const lastStubProps = { current: null };
vi.mock('../components/ExplorerView.svelte', () => ({
  default: function ExplorerViewStub(anchor, props) {
    lastStubProps.current = props;
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'explorer-view-stub');
    el.setAttribute('data-scope-type', props?.scope?.type ?? 'tenant');
    el.setAttribute('data-scope-workspace-id', props?.scope?.workspaceId ?? '');
    const btn = document.createElement('button');
    btn.setAttribute('data-testid', 'stub-enter-repo');
    btn.textContent = 'drill';
    btn.addEventListener('click', () => {
      // Simulated repo-node double-click from the workspace graph
      props?.onEnterRepo?.({ id: 'repo-1', name: 'frontend' });
    });
    el.appendChild(btn);
    if (anchor instanceof Node) anchor.before(el);
    else if (props?.target) props.target.appendChild(el);
  },
}));

import { api } from '../lib/api.js';
import { toastError } from '../lib/toast.svelte.js';
import WorkspaceHome from '../components/WorkspaceHome.svelte';

// ── Fixtures ──────────────────────────────────────────────────────────────────

const WORKSPACE = { id: 'ws-1', name: 'Payments', slug: 'payments', trust_level: 'Guided' };

async function expandArch(container) {
  await fireEvent.click(container.querySelector('[data-testid="arch-toggle"]'));
  await waitFor(() => expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy());
}

describe('WorkspaceHome — Architecture section (ui-navigation.md §2, ui-layout.md §5)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    lastStubProps.current = null;
    api.workspaceRepos.mockResolvedValue([
      { id: 'repo-1', name: 'frontend', description: 'SPA', default_branch: 'main' },
    ]);
  });

  it('renders the Architecture section when workspace is provided', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    expect(container.querySelector('[data-testid="section-architecture"]')).toBeTruthy();
  });

  it('is collapsed by default (ui-navigation.md §2 — architecture collapsed unless expanded)', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    expect(toggle).toBeTruthy();
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(container.querySelector('[data-testid="arch-body"]')).toBeNull();
  });

  it('shows "Show workspace graph" label when collapsed (default)', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-toggle"]').textContent).toContain('Show workspace graph');
    });
  });

  it('shows "Hide workspace graph" label when expanded', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    await fireEvent.click(toggle);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-toggle"]').textContent).toContain('Hide workspace graph');
    });
  });

  it('expands when toggle is clicked from collapsed', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    await fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    await waitFor(() => expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy());
  });

  it('re-collapses when toggle is clicked a second time', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    await fireEvent.click(toggle); // expand
    await waitFor(() => expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy());
    await fireEvent.click(toggle); // collapse
    expect(container.querySelector('[data-testid="arch-body"]')).toBeNull();
  });

  it('renders the workspace-scoped Explorer when expanded (ui-layout.md §5)', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await expandArch(container);
    await waitFor(() => {
      const stub = container.querySelector('[data-testid="explorer-view-stub"]');
      expect(stub).toBeTruthy();
      expect(stub.getAttribute('data-scope-type')).toBe('workspace');
      expect(stub.getAttribute('data-scope-workspace-id')).toBe('ws-1');
    });
    // embedded: the Explorer suppresses its own page header inside the section
    expect(lastStubProps.current?.embedded).toBe(true);
  });

  it('repo drill from the workspace graph routes through onSelectRepo with the architecture tab (scope change to repo)', async () => {
    const onSelectRepo = vi.fn();
    const { container } = render(WorkspaceHome, {
      props: { workspace: WORKSPACE, onSelectRepo },
    });
    await expandArch(container);
    await waitFor(() => expect(container.querySelector('[data-testid="stub-enter-repo"]')).toBeTruthy());
    await fireEvent.click(container.querySelector('[data-testid="stub-enter-repo"]'));
    await waitFor(() => {
      expect(onSelectRepo).toHaveBeenCalledTimes(1);
      const [repo, tab] = onSelectRepo.mock.calls[0];
      expect(repo.id).toBe('repo-1');
      expect(repo.name).toBe('frontend');
      expect(tab).toBe('architecture');
    });
  });

  it('a repo from another workspace is not opened from the workspace graph (scope containment)', async () => {
    const onSelectRepo = vi.fn();
    const { container } = render(WorkspaceHome, {
      props: { workspace: WORKSPACE, onSelectRepo },
    });
    await expandArch(container);
    await waitFor(() => expect(lastStubProps.current?.onEnterRepo).toBeTruthy());
    // External repo (referenced by a dependency edge but not in this workspace)
    lastStubProps.current.onEnterRepo({ id: 'foreign-repo', name: 'other-ws-repo' });
    await waitFor(() => {
      expect(onSelectRepo).not.toHaveBeenCalled();
      expect(toastError).toHaveBeenCalled();
    });
  });

  it('Architecture section has correct aria-labelledby', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const section = container.querySelector('[data-testid="section-architecture"]');
    const labelId = section.getAttribute('aria-labelledby');
    expect(labelId).toBe('section-architecture');
    expect(container.querySelector(`#${labelId}`)).toBeTruthy();
  });

  it('toggle button has aria-controls pointing to arch-body id', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    expect(toggle.getAttribute('aria-controls')).toBe('arch-body');
  });

  it('section is not rendered when workspace is null', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: null } });
    expect(container.querySelector('[data-testid="section-architecture"]')).toBeNull();
  });

  it('shows the workspace home sections when workspace is provided', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="section-decisions"]')).toBeTruthy();
    });
    expect(container.querySelector('[data-testid="section-repos"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-briefing"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-specs"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-architecture"]')).toBeTruthy();
  });

  it('Architecture section appears after Specs (per ui-navigation.md §2 section order)', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const sections = [...container.querySelectorAll('[data-testid^="section-"]')];
    const ids = sections.map(s => s.getAttribute('data-testid'));
    const specsIdx = ids.indexOf('section-specs');
    const archIdx = ids.indexOf('section-architecture');
    expect(archIdx).toBeGreaterThan(specsIdx);
  });
});
