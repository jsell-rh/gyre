import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    composeApply: vi.fn(),
    composeStatus: vi.fn(),
    composeTeardown: vi.fn(),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

import { api } from '../lib/api.js';
import ComposeView from '../components/ComposeView.svelte';

describe('ComposeView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders without throwing', () => {
    expect(() => render(ComposeView)).not.toThrow();
  });

  it('shows Agent Compose heading', () => {
    const { getByText } = render(ComposeView);
    expect(getByText('Agent Compose')).toBeTruthy();
  });

  it('shows Apply Compose Spec section', () => {
    const { getByText } = render(ComposeView);
    expect(getByText('Apply Compose Spec')).toBeTruthy();
  });

  it('shows Compose Status section', () => {
    const { getByText } = render(ComposeView);
    expect(getByText('Compose Status')).toBeTruthy();
  });

  it('renders spec editor textarea', () => {
    const { getByLabelText } = render(ComposeView);
    expect(getByLabelText('Spec content editor')).toBeTruthy();
  });

  it('Apply button is disabled when textarea is empty', () => {
    const { getByText } = render(ComposeView);
    const btn = getByText('Apply Compose');
    expect(btn.disabled).toBe(true);
  });

  it('Apply button becomes enabled when valid JSON is entered', async () => {
    const { getByLabelText, getByText } = render(ComposeView);
    const textarea = getByLabelText('Spec content editor');
    await fireEvent.input(textarea, { target: { value: '{"version":"1"}' } });

    // Wait for the JSON validation debounce (400ms)
    await waitFor(() => {
      expect(getByText('Valid JSON')).toBeTruthy();
    }, { timeout: 1000 });

    const btn = getByText('Apply Compose');
    expect(btn.disabled).toBe(false);
  });

  it('shows invalid JSON hint for bad input', async () => {
    const { getByLabelText, findByText } = render(ComposeView);
    const textarea = getByLabelText('Spec content editor');
    await fireEvent.input(textarea, { target: { value: '{bad json' } });

    // Wait for the JSON validation debounce
    const errorMsg = await findByText(/Unexpected token|Expected/i);
    expect(errorMsg).toBeTruthy();
  });

  it('shows Refresh and Teardown buttons in status section', () => {
    const { getByText } = render(ComposeView);
    expect(getByText('Refresh')).toBeTruthy();
    expect(getByText('Teardown')).toBeTruthy();
  });

  it('Refresh button is disabled when no session ID is entered', () => {
    const { getByText } = render(ComposeView);
    const btn = getByText('Refresh');
    expect(btn.disabled).toBe(true);
  });

  it('Teardown button is disabled when no session ID is entered', () => {
    const { getByText } = render(ComposeView);
    const btn = getByText('Teardown');
    expect(btn.disabled).toBe(true);
  });

  it('shows success banner after successful apply', async () => {
    api.composeApply.mockResolvedValue({ compose_id: 'test-session-123' });
    api.composeStatus.mockResolvedValue({ agents: [] });

    const { getByLabelText, getByText, findByText } = render(ComposeView);
    const textarea = getByLabelText('Spec content editor');
    await fireEvent.input(textarea, { target: { value: '{"version":"1"}' } });

    // Wait for JSON validation
    await waitFor(() => {
      expect(getByText('Valid JSON')).toBeTruthy();
    }, { timeout: 1000 });

    const applyBtn = getByText('Apply Compose');
    await fireEvent.click(applyBtn);

    expect(await findByText('Compose applied successfully')).toBeTruthy();
    expect(await findByText('test-session-123')).toBeTruthy();
  });

  it('shows agent list when status returns agents', async () => {
    api.composeApply.mockResolvedValue({ compose_id: 'sess-abc' });
    api.composeStatus.mockResolvedValue({
      agents: [
        { agent_id: 'agent-001-full-id', name: 'worker-1', status: 'active' },
        { agent_id: 'agent-002-full-id', name: 'worker-2', status: 'idle' },
      ],
    });

    const { getByLabelText, getByText, findByText } = render(ComposeView);
    const textarea = getByLabelText('Spec content editor');
    await fireEvent.input(textarea, { target: { value: '{"version":"1"}' } });

    await waitFor(() => {
      expect(getByText('Valid JSON')).toBeTruthy();
    }, { timeout: 1000 });

    await fireEvent.click(getByText('Apply Compose'));

    expect(await findByText('worker-1')).toBeTruthy();
    expect(await findByText('worker-2')).toBeTruthy();
    expect(await findByText('2 agents')).toBeTruthy();
  });

  it('shows error when apply fails', async () => {
    api.composeApply.mockRejectedValue(new Error('Server unreachable'));

    const { getByLabelText, getByText, findByText } = render(ComposeView);
    const textarea = getByLabelText('Spec content editor');
    await fireEvent.input(textarea, { target: { value: '{"version":"1"}' } });

    await waitFor(() => {
      expect(getByText('Valid JSON')).toBeTruthy();
    }, { timeout: 1000 });

    await fireEvent.click(getByText('Apply Compose'));

    expect(await findByText('Server unreachable')).toBeTruthy();
  });

  it('session ID input exists and accepts text', async () => {
    const { getByPlaceholderText } = render(ComposeView);
    const input = getByPlaceholderText('Compose session ID…');
    expect(input).toBeTruthy();
    await fireEvent.input(input, { target: { value: 'my-session' } });
    expect(input.value).toBe('my-session');
  });
});
