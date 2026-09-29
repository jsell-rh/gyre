/**
 * DetailPanelChat.test.js — HSI §4 Scoped Inline Chat + Hard Interrupt
 *
 * Covers the agent detail panel (steering chat + Pause/Stop/Message hard
 * interrupt) and the MR detail panel (feedback chat targeting the author agent).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, screen, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => {
  const fn = (val) => vi.fn().mockResolvedValue(val);
  return {
    api: {
      // Agent info-tab loaders
      agent: fn({ id: 'worker-12', name: 'worker-12', status: 'active', workspace_id: 'ws-1' }),
      agentLogs: fn([]),
      agentContainer: fn(null),
      agentWorkload: fn(null),
      agentTouchedPaths: fn(null),
      agentCard: fn(null),
      costsByAgent: fn([]),
      task: fn(null),
      mrTrace: fn(null),
      agentMessages: fn([]),
      agentLogStreamUrl: () => 'http://localhost/stream',
      // MR loaders
      mergeRequest: fn({ id: 'mr-1', name: 'Fix retry', status: 'open', author_agent_id: 'worker-12' }),
      mrDependencies: fn(null),
      mrGates: fn([]),
      mrTimeline: fn([]),
      mrDiff: fn(null),
      repoGates: fn([]),
      // Actions under test
      sendAgentMessage: vi.fn().mockResolvedValue({}),
      adminKillAgent: vi.fn().mockResolvedValue({}),
    },
  };
});

vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

import { api } from '../lib/api.js';
import DetailPanel from '../lib/DetailPanel.svelte';

const agentEntity = {
  type: 'agent',
  id: 'worker-12',
  data: { name: 'worker-12', status: 'active', conversation_sha: 'abc123' },
};

const mrEntity = {
  type: 'mr',
  id: 'mr-1',
  data: { name: 'Fix retry', status: 'open' },
};

beforeEach(() => {
  vi.clearAllMocks();
  api.agent.mockResolvedValue({ id: 'worker-12', name: 'worker-12', status: 'active', workspace_id: 'ws-1' });
  api.mergeRequest.mockResolvedValue({ id: 'mr-1', name: 'Fix retry', status: 'open', author_agent_id: 'worker-12' });
  api.sendAgentMessage.mockResolvedValue({});
  api.adminKillAgent.mockResolvedValue({});
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('Agent detail panel — scoped chat', () => {
  it('chat tab shows the InlineChat recipient indicator', async () => {
    render(DetailPanel, { props: { entity: agentEntity } });
    await fireEvent.click(screen.getByRole('tab', { name: /chat/i }));
    expect(await screen.findByText('Message to worker-12 ▸')).toBeTruthy();
  });

  it('sends a Directed-tier steering message via the scoped chat', async () => {
    render(DetailPanel, { props: { entity: agentEntity } });
    await fireEvent.click(screen.getByRole('tab', { name: /chat/i }));
    await screen.findByText('Message to worker-12 ▸');
    const textarea = screen.getByRole('textbox');
    await fireEvent.input(textarea, { target: { value: 'try a different approach' } });
    await fireEvent.click(screen.getByRole('button', { name: /send/i }));
    await waitFor(() => expect(api.sendAgentMessage).toHaveBeenCalled());
    expect(api.sendAgentMessage).toHaveBeenCalledWith('ws-1', 'worker-12', {
      kind: 'user_message',
      tier: 'directed',
      payload: { content: 'try a different approach' },
    });
  });
});

describe('Agent detail panel — hard interrupt', () => {
  it('Pause sends a Directed StatusUpdate with pause_requested', async () => {
    render(DetailPanel, { props: { entity: agentEntity } });
    const pauseBtn = await screen.findByRole('button', { name: /^Pause$/ });
    await fireEvent.click(pauseBtn);
    await waitFor(() => expect(api.sendAgentMessage).toHaveBeenCalled());
    expect(api.sendAgentMessage).toHaveBeenCalledWith('ws-1', 'worker-12', {
      kind: 'status_update',
      payload: { status: 'pause_requested', summary: 'Human requested pause' },
    });
  });

  it('Stop confirms then calls the kill endpoint', async () => {
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    render(DetailPanel, { props: { entity: agentEntity } });
    const stopBtn = await screen.findByRole('button', { name: /^Stop$/ });
    await fireEvent.click(stopBtn);
    expect(confirmSpy).toHaveBeenCalledWith('Stop agent worker-12? Work will be preserved in the worktree.');
    await waitFor(() => expect(api.adminKillAgent).toHaveBeenCalledWith('worker-12'));
  });

  it('Stop does nothing when the confirmation is dismissed', async () => {
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    render(DetailPanel, { props: { entity: agentEntity } });
    const stopBtn = await screen.findByRole('button', { name: /^Stop$/ });
    await fireEvent.click(stopBtn);
    expect(api.adminKillAgent).not.toHaveBeenCalled();
  });

  it('Message opens the inline chat', async () => {
    render(DetailPanel, { props: { entity: agentEntity } });
    const msgBtn = await screen.findByRole('button', { name: /^Message$/ });
    await fireEvent.click(msgBtn);
    // Switching to the chat tab surfaces the recipient indicator.
    expect(await screen.findByText('Message to worker-12 ▸')).toBeTruthy();
  });
});

describe('MR detail panel — feedback chat', () => {
  it('has a Chat tab targeting the author agent', async () => {
    render(DetailPanel, { props: { entity: mrEntity } });
    expect(screen.getByRole('tab', { name: /chat/i })).toBeTruthy();
  });

  it('sends a Directed-tier feedback message to the active author agent', async () => {
    render(DetailPanel, { props: { entity: mrEntity } });
    await fireEvent.click(screen.getByRole('tab', { name: /chat/i }));
    expect(await screen.findByText('Message to worker-12 ▸')).toBeTruthy();
    const textarea = screen.getByRole('textbox');
    await fireEvent.input(textarea, { target: { value: 'naming convention is wrong' } });
    await fireEvent.click(screen.getByRole('button', { name: /send/i }));
    await waitFor(() => expect(api.sendAgentMessage).toHaveBeenCalled());
    expect(api.sendAgentMessage).toHaveBeenCalledWith('ws-1', 'worker-12', {
      kind: 'user_message',
      tier: 'directed',
      payload: { content: 'naming convention is wrong' },
    });
  });

  it('shows the completed-agent state with Ask Why when the author is no longer active', async () => {
    api.agent.mockResolvedValue({ id: 'worker-12', name: 'worker-12', status: 'dead', workspace_id: 'ws-1' });
    render(DetailPanel, { props: { entity: mrEntity } });
    await fireEvent.click(screen.getByRole('tab', { name: /chat/i }));
    expect(await screen.findByText('Agent completed')).toBeTruthy();
    expect(screen.getByRole('button', { name: /ask why/i })).toBeTruthy();
    // No chat input for a completed author.
    expect(screen.queryByText('Message to worker-12 ▸')).toBeNull();
  });
});
