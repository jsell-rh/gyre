import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, screen, waitFor } from '@testing-library/svelte';
import Briefing from '../components/Briefing.svelte';

// Structured briefing data matching S4.3 API shape
const MOCK_STRUCTURED = {
  completed: [
    {
      id: 'c1',
      title: 'Payment retry logic',
      spec_ref: 'payment-retry.md',
      mrs_merged: 3,
      decision: 'exponential backoff',
      confidence: 'high',
    },
  ],
  in_progress: [
    {
      id: 'p1',
      title: 'Auth refactor',
      spec_ref: 'identity-security.md',
      sub_specs_done: 3,
      sub_specs_total: 5,
      active_agents: 2,
      uncertainties: [{ agent_id: 'worker-8', text: 'token refresh for offline...' }],
    },
  ],
  cross_workspace: [
    {
      id: 'x1',
      source_workspace: 'platform-core',
      spec_ref: 'idempotent-api.md',
      description: 'platform-core updated idempotent-api.md',
    },
  ],
  exceptions: [
    {
      id: 'e1',
      type: 'gate_failure',
      description: 'cargo test failed (3 tests).',
      mr_id: '47',
      repo: 'billing-service',
    },
  ],
  metrics: {
    mrs_count: 12,
    runs_count: 47,
    cost_usd: 23.40,
    budget_pct: 67,
  },
};

vi.mock('../lib/api.js', () => ({
  api: {
    getWorkspaceBriefing: vi.fn(),
    workspaces: vi.fn(),
    briefingAsk: vi.fn(),
  },
}));

describe('Briefing S4.3', () => {
  beforeEach(async () => {
    localStorage.clear();
    vi.clearAllMocks();
    const { api } = await import('../lib/api.js');
    api.getWorkspaceBriefing.mockResolvedValue(MOCK_STRUCTURED);
    api.workspaces.mockResolvedValue([]);
  });

  it('renders without throwing', () => {
    expect(() => render(Briefing)).not.toThrow();
  });

  it('shows the briefing title', () => {
    const { getByText } = render(Briefing);
    expect(getByText('Briefing')).toBeTruthy();
  });

  it('renders time range selector with all options', () => {
    render(Briefing);
    const select = screen.getByRole('combobox', { name: /time range/i });
    expect(select).toBeTruthy();
    expect(select.options.length).toBeGreaterThanOrEqual(5);
  });

  describe('Sections from API data', () => {
    it('renders COMPLETED section with data', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => {
        expect(screen.getByTestId('section-completed')).toBeTruthy();
      });
      expect(screen.getByText('Payment retry logic')).toBeTruthy();
    });

    it('renders IN PROGRESS section with data', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => {
        expect(screen.getByTestId('section-in-progress')).toBeTruthy();
      });
      expect(screen.getByText('Auth refactor')).toBeTruthy();
    });

    it('renders CROSS-WORKSPACE section with data', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => {
        expect(screen.getByTestId('section-cross-workspace')).toBeTruthy();
      });
      expect(screen.getByText('platform-core updated idempotent-api.md')).toBeTruthy();
    });

    it('renders EXCEPTIONS section with data', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => {
        expect(screen.getByTestId('section-exceptions')).toBeTruthy();
      });
      expect(screen.getByText(/cargo test failed/i)).toBeTruthy();
    });

    it('renders METRICS row with all cells', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => {
        expect(screen.getByTestId('section-metrics')).toBeTruthy();
      });
      expect(screen.getByTestId('metric-mrs')).toBeTruthy();
      expect(screen.getByTestId('metric-runs')).toBeTruthy();
      expect(screen.getByTestId('metric-cost')).toBeTruthy();
      expect(screen.getByTestId('metric-budget')).toBeTruthy();
    });

    it('shows spec reference links', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('section-completed'));
      const specLinks = screen.getAllByTestId('spec-ref-link');
      expect(specLinks.length).toBeGreaterThan(0);
    });

    it('shows uncertainty warning for in-progress items', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('section-in-progress'));
      expect(screen.getByText(/token refresh for offline/i)).toBeTruthy();
    });

    it('shows action buttons for in-progress items', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('section-in-progress'));
      expect(screen.getByTestId('respond-to-agent-btn')).toBeTruthy();
      expect(screen.getByTestId('view-spec-btn')).toBeTruthy();
    });

    it('shows action buttons for exceptions', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('section-exceptions'));
      expect(screen.getByTestId('view-diff-btn')).toBeTruthy();
      expect(screen.getByTestId('view-output-btn')).toBeTruthy();
      expect(screen.getByTestId('override-btn')).toBeTruthy();
      expect(screen.getByTestId('close-mr-btn')).toBeTruthy();
    });

    it('shows review changes and dismiss buttons for cross-workspace items', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('section-cross-workspace'));
      expect(screen.getByTestId('review-changes-btn')).toBeTruthy();
      expect(screen.getByTestId('dismiss-btn')).toBeTruthy();
    });
  });

  describe('Time range selector', () => {
    it('defaults to "Since last visit" (calls API with no since param)', async () => {
      const { api } = await import('../lib/api.js');
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => expect(api.getWorkspaceBriefing).toHaveBeenCalled());
      expect(api.getWorkspaceBriefing).toHaveBeenCalledWith('ws-1', null);
    });

    it('passes since epoch when 24h is selected', async () => {
      const { api } = await import('../lib/api.js');
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByRole('combobox', { name: /time range/i }));

      const select = screen.getByRole('combobox', { name: /time range/i });
      await fireEvent.change(select, { target: { value: '24h' } });

      await waitFor(() => {
        const calls = api.getWorkspaceBriefing.mock.calls;
        const last = calls[calls.length - 1];
        expect(typeof last[1]).toBe('number');
        expect(last[1]).toBeGreaterThan(0);
      });
    });

    it('shows custom date input when "Custom range" selected', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByRole('combobox', { name: /time range/i }));
      const select = screen.getByRole('combobox', { name: /time range/i });
      await fireEvent.change(select, { target: { value: 'custom' } });
      expect(screen.getByTestId('custom-date-input')).toBeTruthy();
    });
  });

  describe('Entity reference → detail panel', () => {
    it('clicking spec-ref-link renders shell container', async () => {
      const { container } = render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('section-completed'));
      const specLinks = screen.getAllByTestId('spec-ref-link');
      await fireEvent.click(specLinks[0]);
      expect(container.querySelector('.shell')).toBeTruthy();
    });

    it('clicking agent-ref-link renders shell container', async () => {
      const { container } = render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('section-in-progress'));
      const agentLink = screen.getByTestId('agent-ref-link');
      await fireEvent.click(agentLink);
      expect(container.querySelector('.shell')).toBeTruthy();
    });

    it('clicking mr-ref-link renders shell container', async () => {
      const { container } = render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('section-exceptions'));
      const mrLink = screen.getByTestId('mr-ref-link');
      await fireEvent.click(mrLink);
      expect(container.querySelector('.shell')).toBeTruthy();
    });
  });

  describe('Dismiss cross-workspace item', () => {
    it('removes cross-workspace item on dismiss click', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('section-cross-workspace'));
      expect(screen.getByText('platform-core updated idempotent-api.md')).toBeTruthy();
      await fireEvent.click(screen.getByTestId('dismiss-btn'));
      await waitFor(() => {
        expect(screen.queryByText('platform-core updated idempotent-api.md')).toBeNull();
      });
    });
  });

  describe('Q&A Chat', () => {
    it('renders InlineChat with briefing recipient', async () => {
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('briefing-chat'));
      expect(screen.getByText('Ask about this briefing ▸')).toBeTruthy();
    });

    it('calls briefingAsk when user submits question', async () => {
      const { api } = await import('../lib/api.js');
      const mockResponse = new Response('data: {"type":"complete","text":"42"}\n\n', {
        headers: { 'Content-Type': 'text/event-stream' },
      });
      api.briefingAsk.mockResolvedValue(mockResponse);

      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => screen.getByTestId('briefing-chat'));

      const input = screen.getByRole('textbox');
      await fireEvent.input(input, { target: { value: 'What happened today?' } });
      await fireEvent.click(screen.getByRole('button', { name: /send/i }));

      await waitFor(() => {
        expect(api.briefingAsk).toHaveBeenCalledWith(
          'ws-1',
          expect.objectContaining({ question: 'What happened today?' })
        );
      });
    });
  });

  describe('Mock/fallback data', () => {
    it('shows mock briefing when API returns empty data', async () => {
      const { api } = await import('../lib/api.js');
      api.getWorkspaceBriefing.mockResolvedValue({
        completed: [], in_progress: [], cross_workspace: [], exceptions: [], metrics: null,
      });
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => {
        expect(screen.getByText('Payment retry logic')).toBeTruthy();
      });
    });

    it('shows mock briefing on API 404 error', async () => {
      const { api } = await import('../lib/api.js');
      api.getWorkspaceBriefing.mockRejectedValue(
        new Error('API /workspaces/ws-1/briefing: 404 Not Found')
      );
      render(Briefing, { props: { workspaceId: 'ws-1', scope: 'workspace' } });
      await waitFor(() => {
        expect(screen.getByText('Payment retry logic')).toBeTruthy();
      });
    });
  });
});
