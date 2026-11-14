/**
 * AnalyticsView.test.js — tests for the product analytics dashboard
 * (analytics.md §Analytics Dashboard (UI), task-149)
 *
 * Covers:
 *   - All five sections render from real API calls (bar chart, events
 *     list, decision panel, funnel, top agents)
 *   - The bar chart zero-fills all 30 calendar days
 *   - Events list is filterable (event name filter reaches the API) and
 *     paginated (Load more advances the timestamp cursor)
 *   - Clicking an event row expands the full properties JSON
 *   - The decision panel evaluates the selected decision with the
 *     edited JSON context and renders the outcome
 *   - The funnel computes drop-off rates between steps and applies
 *     custom funnel definitions
 *   - Top agents render completion rate / avg duration (— when unknown)
 *   - Retention days surface read-only from the admin retention config
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    analyticsDaily: vi.fn(),
    analyticsEvents: vi.fn(),
    analyticsTop: vi.fn(),
    analyticsCount: vi.fn(),
    analyticsDecide: vi.fn(),
    adminRetention: vi.fn(),
  },
  setAuthToken: vi.fn(),
}));

import AnalyticsView from '../components/AnalyticsView.svelte';
import { api } from '../lib/api.js';

const today = () => new Date().toISOString().slice(0, 10);

function mockEvent(id, eventName, agentId, ts, properties) {
  return { id, event_name: eventName, agent_id: agentId, timestamp: ts, properties };
}

function setupDefaultMocks() {
  api.analyticsDaily.mockResolvedValue([{ date: today(), count: 12 }]);
  api.analyticsEvents.mockResolvedValue([
    mockEvent('e-1', 'mr.merged', 'agent-1', Math.floor(Date.now() / 1000) - 60, { mr_id: 'mr-1' }),
    mockEvent('e-2', 'agent.completed', 'agent-1', Math.floor(Date.now() / 1000) - 120, { duration_secs: 90 }),
    mockEvent('e-3', 'agent.spawned', 'agent-2', Math.floor(Date.now() / 1000) - 180, { persona: 'implementer' }),
  ]);
  api.analyticsTop.mockImplementation((params = {}) => {
    if (params.dimension === 'agent') {
      return Promise.resolve([
        { agent_id: 'agent-1', event_count: 10, completed: 8, failed: 2, completion_rate: 0.8, avg_duration_secs: 120 },
        { agent_id: 'agent-2', event_count: 1, completed: 0, failed: 0, completion_rate: null, avg_duration_secs: null },
      ]);
    }
    return Promise.resolve([
      { event_name: 'mr.merged', count: 5 },
      { event_name: 'agent.completed', count: 3 },
    ]);
  });
  api.analyticsCount.mockImplementation((params = {}) => {
    const counts = {
      'agent.spawned': 100,
      'task.status_changed': 50,
      'agent.completed': 25,
      'mr.merged': 10,
      'custom.step': 7,
    };
    return Promise.resolve({ event_name: params.event_name, count: counts[params.event_name] ?? 0 });
  });
  api.analyticsDecide.mockResolvedValue({
    decision: 'feature.promote',
    promote: true,
    confidence: 0.9,
    reason: 'adoption above threshold',
    adoption_rate: 0.62,
  });
  api.adminRetention.mockResolvedValue([
    { data_type: 'analytics_events', max_age_days: 365 },
    { data_type: 'audit_events', max_age_days: 365 },
  ]);
}

function renderView() {
  return render(AnalyticsView, { props: { workspace: { id: 'ws-1', name: 'Payments' } } });
}

describe('AnalyticsView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setupDefaultMocks();
  });

  it('renders all five sections with data loaded from the analytics API', async () => {
    const { container } = renderView();

    await waitFor(() => {
      expect(api.analyticsDaily).toHaveBeenCalledWith({ days: 30 });
      expect(api.analyticsEvents).toHaveBeenCalledWith({ limit: 50 });
      expect(api.analyticsCount).toHaveBeenCalled();
      expect(api.analyticsTop).toHaveBeenCalledWith(
        expect.objectContaining({ dimension: 'agent', limit: 10, workspace_id: 'ws-1' }),
      );
    });

    // All five sections present.
    for (const section of [
      'analytics-daily-section',
      'analytics-events-section',
      'analytics-decision-section',
      'analytics-funnel-section',
      'analytics-top-section',
    ]) {
      expect(container.querySelector(`[data-testid="${section}"]`)).toBeTruthy();
    }
  });

  it('renders one zero-filled bar per day for the last 30 days', async () => {
    const { container } = renderView();
    // One day has 12 events, the other 29 have none — all 30 render.
    await waitFor(() => {
      expect(container.querySelectorAll('.daily-bar').length).toBe(30);
    });
    const heights = [...container.querySelectorAll('.daily-bar')].map((r) => Number(r.getAttribute('height')));
    expect(Math.max(...heights)).toBeGreaterThan(0);
    expect(heights.filter((h) => h === 0).length).toBe(29);
  });

  it('shows the events table with a properties preview per row', async () => {
    const { container } = renderView();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-events-table"]')).toBeTruthy();
    });
    const rows = container.querySelectorAll('[data-testid="analytics-events-table"] tbody tr');
    expect(rows.length).toBe(3);
    expect(container.textContent).toContain('mr.merged');
  });

  it('expands the full properties JSON when an event row is clicked', async () => {
    const { container } = renderView();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-events-table"]')).toBeTruthy();
    });
    expect(container.querySelector('[data-testid="analytics-event-props"]')).toBeNull();

    fireEvent.click(container.querySelector('[data-testid="analytics-events-table"] tbody tr'));
    await waitFor(() => {
      const pre = container.querySelector('[data-testid="analytics-event-props"]');
      expect(pre).toBeTruthy();
      expect(pre.textContent).toContain('"mr_id"');
      expect(pre.textContent).toContain('"mr-1"');
    });
  });

  it('applies the event-name filter to the events query', async () => {
    const { container } = renderView();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-events-table"]')).toBeTruthy();
    });

    const select = container.querySelector('[data-testid="analytics-filter-event"]');
    fireEvent.change(select, { target: { value: 'mr.merged' } });

    await waitFor(() => {
      expect(api.analyticsEvents).toHaveBeenCalledWith(
        expect.objectContaining({ event_name: 'mr.merged', limit: 50 }),
      );
    });
  });

  it('paginates with a timestamp cursor when a full page is returned', async () => {
    const now = Math.floor(Date.now() / 1000);
    const pageOne = Array.from({ length: 50 }, (_, i) =>
      mockEvent(`p1-${i}`, 'agent.spawned', 'agent-1', now - i, { i }),
    );
    api.analyticsEvents.mockResolvedValue(pageOne);
    const { container } = renderView();

    const more = await waitFor(() => {
      const btn = container.querySelector('[data-testid="analytics-events-more"]');
      expect(btn).toBeTruthy();
      return btn;
    });
    expect(container.querySelector('[data-testid="analytics-events-count"]').textContent).toContain('50 shown');

    api.analyticsEvents.mockClear();
    api.analyticsEvents.mockResolvedValue([]);
    fireEvent.click(more);

    await waitFor(() => {
      expect(api.analyticsEvents).toHaveBeenCalledWith(
        expect.objectContaining({ limit: 50, until: String(now - 49) }),
      );
    });
  });

  it('evaluates the selected decision with the edited JSON context', async () => {
    const { container } = renderView();

    fireEvent.change(container.querySelector('[data-testid="analytics-decision-select"]'), {
      target: { value: 'feature.promote' },
    });
    fireEvent.input(container.querySelector('[data-testid="analytics-decision-context"]'), {
      target: { value: '{"flag": "new-checkout"}' },
    });
    fireEvent.click(container.querySelector('[data-testid="analytics-decision-evaluate"]'));

    await waitFor(() => {
      expect(api.analyticsDecide).toHaveBeenCalledWith('feature.promote', { flag: 'new-checkout' });
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-decision-result"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="analytics-decision-promote"]').textContent).toContain('✓');
      expect(container.querySelector('[data-testid="analytics-decision-confidence"]').textContent).toContain('0.9');
      // Metrics (non-outcome fields) render too.
      expect(container.querySelector('[data-testid="analytics-decision-result"]').textContent).toContain('adoption_rate');
    });
  });

  it('rejects invalid decision context JSON without calling the API', async () => {
    const { container } = renderView();
    fireEvent.input(container.querySelector('[data-testid="analytics-decision-context"]'), {
      target: { value: '{not json' },
    });
    fireEvent.click(container.querySelector('[data-testid="analytics-decision-evaluate"]'));

    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-decision-error"]')).toBeTruthy();
    });
    expect(api.analyticsDecide).not.toHaveBeenCalled();
  });

  it('computes funnel drop-off rates between steps', async () => {
    const { container } = renderView();
    // Default funnel counts: 100 → 50 → 25 → 10.
    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-funnel-step-3"]')).toBeTruthy();
    });
    // Step 2 counts only InProgress status changes (property filter).
    expect(api.analyticsCount).toHaveBeenCalledWith(
      expect.objectContaining({ event_name: 'task.status_changed', property: 'new_status:InProgress' }),
    );
    expect(container.querySelector('[data-testid="analytics-funnel-dropoff-1"]').textContent).toContain('50% continue (50% drop-off)');
    expect(container.querySelector('[data-testid="analytics-funnel-dropoff-2"]').textContent).toContain('50% continue (50% drop-off)');
    expect(container.querySelector('[data-testid="analytics-funnel-dropoff-3"]').textContent).toContain('40% continue (60% drop-off)');
  });

  it('applies a custom funnel definition', async () => {
    const { container } = renderView();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-funnel-step-3"]')).toBeTruthy();
    });

    fireEvent.click(container.querySelector('[data-testid="analytics-funnel-custom-toggle"]'));
    const input = container.querySelector('[data-testid="analytics-funnel-custom-input"]');
    fireEvent.input(input, {
      target: { value: 'agent.spawned\ncustom.step key:value\nmr.merged' },
    });
    fireEvent.click(container.querySelector('[data-testid="analytics-funnel-custom-apply"]'));

    await waitFor(() => {
      expect(api.analyticsCount).toHaveBeenCalledWith(
        expect.objectContaining({ event_name: 'custom.step', property: 'key:value' }),
      );
    });
    await waitFor(() => {
      // 3 custom steps replace the default 4.
      expect(container.querySelectorAll('[data-testid^="analytics-funnel-step-"]').length).toBe(3);
    });
  });

  it('rejects a malformed custom funnel step', async () => {
    const { container } = renderView();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-funnel-step-3"]')).toBeTruthy();
    });
    fireEvent.click(container.querySelector('[data-testid="analytics-funnel-custom-toggle"]'));
    fireEvent.input(container.querySelector('[data-testid="analytics-funnel-custom-input"]'), {
      target: { value: 'agent.spawned\ncustom.step novalue\nmr.merged' },
    });
    fireEvent.click(container.querySelector('[data-testid="analytics-funnel-custom-apply"]'));

    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-funnel-custom-error"]')).toBeTruthy();
    });
    // The default funnel is unchanged.
    expect(container.querySelectorAll('[data-testid^="analytics-funnel-step-"]').length).toBe(4);
  });

  it('ranks top agents with completion rate and avg duration', async () => {
    const { container } = renderView();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-top-table"]')).toBeTruthy();
    });
    const rates = container.querySelectorAll('[data-testid="analytics-top-rate"]');
    expect(rates.length).toBe(2);
    expect(rates[0].textContent).toBe('80%');
    expect(rates[1].textContent).toBe('—');
    expect(container.querySelector('[data-testid="analytics-top-table"]').textContent).toContain('2.0m');
  });

  it('surfaces the configured analytics retention window', async () => {
    const { container } = renderView();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="analytics-retention"]')).toBeTruthy();
    });
    expect(container.querySelector('[data-testid="analytics-retention"]').textContent).toContain('365');
  });
});
