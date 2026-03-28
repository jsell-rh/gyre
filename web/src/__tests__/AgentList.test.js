import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import AgentList from '../components/AgentList.svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    agents: vi.fn().mockResolvedValue([
      {
        id: 'agent-aaa-111',
        name: 'worker-1',
        status: 'Active',
        current_task_id: 'task-001',
        last_heartbeat: Math.floor(Date.now() / 1000) - 45,
        spawned_at: Math.floor(Date.now() / 1000) - 3600,
        parent_id: null,
        lifetime_budget_secs: 7200,
      },
      {
        id: 'agent-bbb-222',
        name: 'worker-2',
        status: 'Idle',
        current_task_id: null,
        last_heartbeat: Math.floor(Date.now() / 1000) - 120,
        spawned_at: Math.floor(Date.now() / 1000) - 86400,
        parent_id: 'agent-aaa-111',
        lifetime_budget_secs: null,
      },
      {
        id: 'agent-ccc-333',
        name: 'worker-3',
        status: 'Error',
        current_task_id: 'task-002',
        last_heartbeat: Math.floor(Date.now() / 1000) - 600,
        spawned_at: Math.floor(Date.now() / 1000) - 7200,
        parent_id: null,
        lifetime_budget_secs: null,
      },
    ]),
    allRepos: vi.fn().mockResolvedValue([
      { id: 'repo-1', name: 'my-service' },
      { id: 'repo-2', name: 'other-service' },
    ]),
    tasks: vi.fn().mockResolvedValue([
      { id: 'task-001', title: 'Implement login' },
      { id: 'task-002', title: 'Fix dashboard bug' },
    ]),
    computeList: vi.fn().mockResolvedValue([
      { id: 'ct-1', name: 'gyre-agent-default', target_type: 'container' },
    ]),
    repoBranches: vi.fn().mockResolvedValue([
      { name: 'main' },
      { name: 'develop' },
    ]),
    spawnAgent: vi.fn().mockResolvedValue({
      agent: { id: 'agent-new-999' },
      token: 'tok-secret-123',
      clone_url: 'http://localhost:3000/git/repo-1/my-service.git',
      worktree_path: '/tmp/worktrees/worker-new',
      branch: 'feat/new-work',
    }),
    agentLogs: vi.fn().mockResolvedValue(['[INFO] started', '[INFO] running task']),
    agentContainer: vi.fn().mockResolvedValue({
      container_id: 'ctr-abc123',
      image: 'gyre-agent:latest',
      image_hash: 'sha256:deadbeef',
      runtime: 'podman',
      started_at: Math.floor(Date.now() / 1000) - 3600,
      stopped_at: null,
      exit_code: null,
    }),
    agentTtyUrl: vi.fn().mockReturnValue('ws://localhost:3000/agents/agent-aaa-111/tty'),
    agentCard: vi.fn().mockResolvedValue(null),
    getAgentCard: vi.fn().mockResolvedValue(null),
    setAgentCard: vi.fn().mockResolvedValue(null),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  toastInfo: vi.fn(),
}));


describe('AgentList', () => {
  const defaultProps = { workspaceId: 'ws-1' };

  describe('rendering', () => {
    it('renders without throwing', () => {
      expect(() => render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      })).not.toThrow();
    });

    it('shows Agents heading', async () => {
      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => {
        expect(getByText('Agents')).toBeTruthy();
      });
    });

    it('shows agent count after loading', async () => {
      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => {
        expect(getByText('3 agents registered')).toBeTruthy();
      });
    });

    it('shows all agents in grid view by default', async () => {
      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => {
        expect(getByText('worker-1')).toBeTruthy();
        expect(getByText('worker-2')).toBeTruthy();
        expect(getByText('worker-3')).toBeTruthy();
      });
    });

    it('shows task labels resolved from task list', async () => {
      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => {
        expect(getByText('Implement login')).toBeTruthy();
        expect(getByText('Fix dashboard bug')).toBeTruthy();
      });
    });
  });

  describe('status filtering', () => {
    it('shows filter bar with status pills', async () => {
      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      expect(getByText('All')).toBeTruthy();
      expect(getByText('Active')).toBeTruthy();
      expect(getByText('Idle')).toBeTruthy();
      expect(getByText('Blocked')).toBeTruthy();
      expect(getByText('Error')).toBeTruthy();
      expect(getByText('Dead')).toBeTruthy();
    });

    it('filters agents by clicking a status pill', async () => {
      const { container, getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => expect(getByText('worker-1')).toBeTruthy());

      // Click "Idle" filter pill (inside filter-bar)
      const idlePill = container.querySelector('.filter-bar .pill:nth-child(3)');
      await fireEvent.click(idlePill);

      await waitFor(() => {
        // Only worker-2 (Idle) should be visible
        expect(getByText('worker-2')).toBeTruthy();
        const agentNames = Array.from(container.querySelectorAll('.agent-name')).map(e => e.textContent);
        expect(agentNames).not.toContain('worker-1');
        expect(agentNames).not.toContain('worker-3');
      });
    });

    it('clears filter when clicking the same status pill again', async () => {
      const { container, getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => expect(getByText('worker-1')).toBeTruthy());

      // Click "Active" pill
      const activePill = container.querySelector('.filter-bar .pill:nth-child(2)');
      await fireEvent.click(activePill);
      await waitFor(() => expect(getByText('worker-1')).toBeTruthy());

      // Click Active again to clear
      await fireEvent.click(activePill);

      await waitFor(() => {
        expect(getByText('worker-1')).toBeTruthy();
        expect(getByText('worker-2')).toBeTruthy();
        expect(getByText('worker-3')).toBeTruthy();
      });
    });

    it('shows empty state when filter matches no agents', async () => {
      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => expect(getByText('worker-1')).toBeTruthy());

      // Click "Dead" which has no agents
      await fireEvent.click(getByText('Dead'));

      await waitFor(() => {
        expect(getByText('No agents found')).toBeTruthy();
      });
    });
  });

  describe('view mode toggle', () => {
    it('shows grid/table view toggle buttons', () => {
      const { getByLabelText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      expect(getByLabelText('Grid view')).toBeTruthy();
      expect(getByLabelText('Table view')).toBeTruthy();
    });

    it('switches to table view when clicking table toggle', async () => {
      const { getByLabelText, container, getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => expect(getByText('worker-1')).toBeTruthy());

      await fireEvent.click(getByLabelText('Table view'));

      await waitFor(() => {
        // Table view should have table element
        expect(container.querySelector('table')).toBeTruthy();
      });
    });
  });

  describe('spawn modal', () => {
    it('opens spawn modal when clicking Spawn Agent button', async () => {
      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await fireEvent.click(getByText('+ Spawn Agent'));

      await waitFor(() => {
        expect(getByText('Spawn Agent')).toBeTruthy();
      });
    });

    it('shows form fields in spawn modal', async () => {
      const { container, getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await fireEvent.click(getByText('+ Spawn Agent'));

      await waitFor(() => {
        const modal = container.querySelector('.modal');
        expect(modal).toBeTruthy();
        // Check that form fields exist inside the modal
        const labels = Array.from(modal.querySelectorAll('label')).map(l => l.textContent.trim());
        expect(labels.some(l => l.startsWith('Name'))).toBe(true);
        expect(labels.some(l => l.startsWith('Repository'))).toBe(true);
        expect(labels.some(l => l.startsWith('Task'))).toBe(true);
        expect(labels.some(l => l.startsWith('Branch'))).toBe(true);
      });
    });

    it('shows validation error when spawning with empty fields', async () => {
      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await fireEvent.click(getByText('+ Spawn Agent'));
      await waitFor(() => expect(getByText('Spawn')).toBeTruthy());

      // Click Spawn without filling fields
      await fireEvent.click(getByText('Spawn'));

      await waitFor(() => {
        expect(getByText('All fields are required.')).toBeTruthy();
      });
    });

    it('closes spawn modal with Cancel button', async () => {
      const { getByText, queryByRole } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await fireEvent.click(getByText('+ Spawn Agent'));
      await waitFor(() => expect(getByText('Spawn Agent')).toBeTruthy());

      await fireEvent.click(getByText('Cancel'));

      await waitFor(() => {
        expect(queryByRole('dialog')).toBeFalsy();
      });
    });

    it('shows compute target selector when targets are available', async () => {
      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await fireEvent.click(getByText('+ Spawn Agent'));

      await waitFor(() => {
        expect(getByText('Compute Target')).toBeTruthy();
      });
    });
  });

  describe('agent selection and detail panel', () => {
    it('shows detail panel when clicking an agent card', async () => {
      const { container, getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => expect(getByText('worker-1')).toBeTruthy());

      const cards = container.querySelectorAll('.agent-card:not(.skeleton-card)');
      await fireEvent.click(cards[0]);

      await waitFor(() => {
        expect(container.querySelector('.detail-panel')).toBeTruthy();
        expect(container.querySelector('.detail-header').textContent).toContain('worker-1');
      });
    });

    it('shows info/logs/terminal tabs in detail panel', async () => {
      const { container, getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => expect(getByText('worker-1')).toBeTruthy());
      const cards = container.querySelectorAll('.agent-card:not(.skeleton-card)');
      await fireEvent.click(cards[0]);

      await waitFor(() => {
        const tabs = container.querySelectorAll('.dtab');
        const tabNames = Array.from(tabs).map(t => t.textContent.trim());
        expect(tabNames).toContain('Info');
        expect(tabNames).toContain('Logs');
        expect(tabNames).toContain('Terminal');
      });
    });

    it('deselects agent when clicking the same card again', async () => {
      const { container, getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => expect(getByText('worker-1')).toBeTruthy());

      const cards = container.querySelectorAll('.agent-card:not(.skeleton-card)');
      await fireEvent.click(cards[0]);
      await waitFor(() => expect(container.querySelector('.detail-panel')).toBeTruthy());

      await fireEvent.click(cards[0]);
      await waitFor(() => {
        expect(container.querySelector('.detail-panel')).toBeFalsy();
      });
    });

    it('closes detail panel with close button', async () => {
      const { container, getByText, getByLabelText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => expect(getByText('worker-1')).toBeTruthy());
      const cards = container.querySelectorAll('.agent-card:not(.skeleton-card)');
      await fireEvent.click(cards[0]);
      await waitFor(() => expect(container.querySelector('.detail-panel')).toBeTruthy());

      await fireEvent.click(getByLabelText('Close agent detail'));

      await waitFor(() => {
        expect(container.querySelector('.detail-panel')).toBeFalsy();
      });
    });
  });

  describe('error handling', () => {
    it('shows error message when agents fail to load', async () => {
      const { api } = await import('../lib/api.js');
      api.agents.mockRejectedValueOnce(new Error('Server is down'));

      const { getByText } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      await waitFor(() => {
        expect(getByText('Error: Server is down')).toBeTruthy();
      });
    });
  });

  describe('loading state', () => {
    it('shows skeleton cards during initial load', () => {
      const { container } = render(AgentList, {
        props: defaultProps,
        context: new Map([['navigate', vi.fn()]]),
      });

      expect(container.querySelectorAll('.skeleton-card').length).toBeGreaterThan(0);
    });
  });
});
