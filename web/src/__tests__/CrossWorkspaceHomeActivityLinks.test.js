/**
 * CrossWorkspaceHomeActivityLinks.test.js — activity-dashboard.md
 * §"Each activity entry links to the relevant artifact" on the
 * cross-workspace dashboard.
 *
 * Regression (review finding, task-295 repair round): the cross-workspace
 * activity timeline deep-linked gate-failure/test-result entries to the MR
 * detail's DEFAULT tab via nav(event.entity_type, event.entity_id, event)
 * — only the WorkspaceHome sidebar carried the _openTab: 'gates' hint, so
 * AC #7 ("gate-failure/test-result entries landing on the MR's gate
 * output") was met on one surface only. These tests fail under that
 * revert: the entry must carry _openTab: 'gates' into the entity-detail
 * navigation, while non-gate MR entries must NOT (they land on the MR's
 * default tab).
 *
 * The entries fed here are the exact GET /api/v1/activity wire shape
 * (crates/gyre-server/src/api/activity.rs): event_type 'gate_failed',
 * entity_type 'mr', gate_name/commit_sha/branch refs surfaced from the
 * notification body.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// The component resolves its navigation through svelte contexts — spy on
// goToEntityDetail (the full-page entity-detail path nav() prefers).
const navCtx = vi.hoisted(() => ({ goToEntityDetail: null }));
vi.mock('svelte', async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    getContext: (key, ...rest) =>
      key === 'goToEntityDetail' ? navCtx.goToEntityDetail : actual.getContext(key, ...rest),
  };
});

vi.mock('../lib/api.js', () => ({
  api: {
    myNotifications: vi.fn().mockResolvedValue([]),
    workspaces: vi.fn().mockResolvedValue([]),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    getWorkspaceBriefing: vi.fn().mockResolvedValue(null),
    activity: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    agents: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
  },
}));

import CrossWorkspaceHome from '../components/CrossWorkspaceHome.svelte';
import {
  mergeFetchedActivity,
  resetActivityStream,
} from '../lib/activityStream.svelte.js';

/** A gate-failure entry in the /api/v1/activity wire shape. */
function gateFailureEntry(overrides = {}) {
  return {
    id: 'notif-gate-1',
    event_type: 'gate_failed',
    title: 'Gate failed',
    entity_type: 'mr',
    entity_id: 'mr-9',
    entity_name: 'Add payments',
    mr_id: 'mr-9',
    gate_name: 'tests',
    commit_sha: '0123456789abcdef0123456789abcdef01234567',
    branch: 'feature/payments',
    repo_id: 'repo-1',
    workspace_id: 'ws-1',
    timestamp: 1_700_000_100,
    ...overrides,
  };
}

describe('CrossWorkspaceHome activity timeline artifact deep links', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetActivityStream();
    navCtx.goToEntityDetail = vi.fn();
  });

  it('a gate-failure entry deep-links to the MR gate output (_openTab: gates)', async () => {
    mergeFetchedActivity('ws-1', [gateFailureEntry()]);
    const { container } = render(CrossWorkspaceHome);
    const link = await waitFor(() => {
      const el = container.querySelector('button[title="View mr: mr-9"]');
      expect(el).toBeTruthy();
      return el;
    }, { timeout: 3000 });
    await fireEvent.click(link);

    expect(navCtx.goToEntityDetail).toHaveBeenCalledTimes(1);
    const [type, id, data] = navCtx.goToEntityDetail.mock.calls[0];
    expect(type).toBe('mr');
    expect(id).toBe('mr-9');
    expect(data._openTab).toBe('gates');
    // The artifact refs the entry carries ride along for the detail view.
    expect(data.repo_id).toBe('repo-1');
  });

  it('a non-gate MR entry lands on the MR default tab (no _openTab hint)', async () => {
    mergeFetchedActivity('ws-1', [
      gateFailureEntry({
        id: 'notif-merged-1',
        event_type: 'merged',
        title: 'MR merged',
        gate_name: null,
      }),
    ]);
    const { container } = render(CrossWorkspaceHome);
    const link = await waitFor(() => {
      const el = container.querySelector('button[title="View mr: mr-9"]');
      expect(el).toBeTruthy();
      return el;
    }, { timeout: 3000 });
    await fireEvent.click(link);

    expect(navCtx.goToEntityDetail).toHaveBeenCalledTimes(1);
    const [type, id, data] = navCtx.goToEntityDetail.mock.calls[0];
    expect(type).toBe('mr');
    expect(id).toBe('mr-9');
    expect(data._openTab).toBeUndefined();
  });

  it('an entry carrying only mr_id + gate_name (no entity_type) also lands on gates', async () => {
    // The fallback artifact button path: a gate-failure entry whose
    // entity_type/entity_id are absent but whose body carries the MR ref.
    mergeFetchedActivity('ws-1', [
      gateFailureEntry({ entity_type: null, entity_id: null, entity_name: null }),
    ]);
    const { container } = render(CrossWorkspaceHome);
    const link = await waitFor(() => {
      const el = container.querySelector('button[title="View MR: mr-9"]');
      expect(el).toBeTruthy();
      return el;
    }, { timeout: 3000 });
    await fireEvent.click(link);

    expect(navCtx.goToEntityDetail).toHaveBeenCalledTimes(1);
    const [type, id, data] = navCtx.goToEntityDetail.mock.calls[0];
    expect(type).toBe('mr');
    expect(id).toBe('mr-9');
    expect(data._openTab).toBe('gates');
  });
});
