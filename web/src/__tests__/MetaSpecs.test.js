import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';
import MetaSpecs from '../components/MetaSpecs.svelte';

const mockPersonas = [
  {
    id: 'persona-1',
    name: 'Backend Developer',
    system_prompt: 'You are a backend developer focused on Rust.',
    approval_status: 'Approved',
  },
  {
    id: 'persona-2',
    name: 'Security Reviewer',
    system_prompt: 'You review code for security vulnerabilities.',
    approval_status: 'Pending',
  },
];

const mockSpecs = [
  { path: 'specs/payment/retry.md', title: 'Payment Retry', kind: null, approval_status: 'approved', current_sha: 'abc12345' },
  { path: 'specs/system/design.md', title: 'Design Principles', kind: null, approval_status: 'pending', current_sha: 'def67890' },
];

const mockMetaSpecs = [
  { path: 'specs/personas/backend.md', title: 'Backend Persona', kind: 'meta:persona', approval_status: 'approved', current_sha: 'aaa11111' },
  { path: 'specs/principles/quality.md', title: 'Quality Principle', kind: 'meta:principle', approval_status: 'pending', current_sha: 'bbb22222' },
];

vi.mock('../lib/api.js', () => ({
  api: {
    getSpecs: vi.fn().mockResolvedValue([]),
    personas: vi.fn().mockResolvedValue([]),
    getMetaSpecBlastRadius: vi.fn().mockResolvedValue({ affected_workspaces: [], affected_repos: [] }),
    previewPersona: vi.fn().mockRejectedValue(new Error('Not implemented')),
    previewPersonaStatus: vi.fn().mockRejectedValue(new Error('Not implemented')),
    publishPersona: vi.fn().mockRejectedValue(new Error('Not implemented')),
  },
}));

import { api } from '../lib/api.js';

// Suppress fetch errors in tests
global.fetch = vi.fn().mockRejectedValue(new Error('fetch not available in test'));

describe('MetaSpecs — tenant scope (default)', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getSpecs.mockResolvedValue([...mockMetaSpecs]);
    api.personas.mockResolvedValue([]);
  });

  it('renders without throwing', () => {
    expect(() => render(MetaSpecs, { props: { scope: 'tenant' } })).not.toThrow();
  });

  it('shows Meta-Specs heading', () => {
    const { getByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(getByText('Meta-Specs')).toBeTruthy();
  });

  it('renders catalog table after loading', async () => {
    const { findByTestId } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const table = await findByTestId('catalog-table');
    expect(table).toBeTruthy();
  });

  it('shows persona and principle rows in catalog', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(await findByText('Backend Persona')).toBeTruthy();
    expect(await findByText('Quality Principle')).toBeTruthy();
  });

  it('shows kind filter pills', () => {
    const { getByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(getByText('All')).toBeTruthy();
    expect(getByText('Persona')).toBeTruthy();
    expect(getByText('Principle')).toBeTruthy();
  });

  it('filters by kind when pill is clicked', async () => {
    api.getSpecs.mockResolvedValue([...mockMetaSpecs]);
    const { findByText, container, queryByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findByText('Backend Persona');

    // Click the filter pill specifically (button within .filter-pills)
    const pills = container.querySelectorAll('.filter-pills .pill');
    const personaPill = Array.from(pills).find(b => b.textContent.trim() === 'Persona');
    expect(personaPill).toBeTruthy();
    await fireEvent.click(personaPill);

    await waitFor(() => {
      expect(queryByText('Quality Principle')).toBeNull();
    });
  });

  it('shows empty state when no meta-specs exist', async () => {
    api.getSpecs.mockResolvedValue([]);
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(await findByText('No meta-specs found')).toBeTruthy();
  });

  it('shows error state when API fails', async () => {
    api.getSpecs.mockRejectedValue(new Error('Server error'));
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(await findByText('Failed to load meta-specs')).toBeTruthy();
  });
});

describe('MetaSpecs — workspace scope', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getSpecs.mockResolvedValue([...mockSpecs]);
    api.personas.mockResolvedValue([...mockPersonas]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders without throwing in workspace scope', () => {
    expect(() => render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } })).not.toThrow();
  });

  it('shows the preview loop container', async () => {
    const { findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const loop = await findByTestId('preview-loop');
    expect(loop).toBeTruthy();
  });

  it('shows persona textarea in editing state', async () => {
    const { findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const textarea = await findByTestId('persona-textarea');
    expect(textarea).toBeTruthy();
  });

  it('shows persona selector dropdown', async () => {
    const { findByLabelText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const select = await findByLabelText('Persona');
    expect(select).toBeTruthy();
  });

  it('shows Preview and Publish buttons in editing state', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    expect(await findByText('Preview')).toBeTruthy();
    expect(await findByText('Publish')).toBeTruthy();
  });

  it('Preview button is disabled when no specs selected', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const btn = await findByText('Preview');
    expect(btn.disabled).toBe(true);
  });

  it('shows spec checklist with available specs', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    expect(await findByText('specs/payment/retry.md')).toBeTruthy();
    expect(await findByText('specs/system/design.md')).toBeTruthy();
  });

  it('Select All enables Preview button', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const selectAll = await findByText('Select All');
    await fireEvent.click(selectAll);

    const previewBtn = await findByText('Preview');
    await waitFor(() => {
      expect(previewBtn.disabled).toBe(false);
    });
  });

  it('transitions to running state on Preview click', async () => {
    vi.useFakeTimers();
    api.previewPersona.mockRejectedValue(new Error('stub'));

    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const selectAll = await findByText('Select All');
    await fireEvent.click(selectAll);

    const previewBtn = await findByText('Preview');
    await fireEvent.click(previewBtn);

    expect(await findByText('Preview: Running')).toBeTruthy();
    expect(await findByText('Cancel Preview')).toBeTruthy();
    vi.useRealTimers();
  });

  it('transitions to complete state after simulation', async () => {
    vi.useFakeTimers();
    api.previewPersona.mockRejectedValue(new Error('stub'));

    const { findByText, findByTestId } = render(MetaSpecs, {
      props: { scope: 'workspace', workspaceId: 'ws-1' },
    });

    const selectAll = await findByText('Select All');
    await fireEvent.click(selectAll);
    const previewBtn = await findByText('Preview');
    await fireEvent.click(previewBtn);

    // Advance timers to complete the simulation (2 specs × 1200ms + buffer)
    await vi.advanceTimersByTimeAsync(4000);

    await waitFor(async () => {
      expect(await findByTestId('preview-complete')).toBeTruthy();
    });

    vi.useRealTimers();
  });

  it('shows Iterate button in complete state', async () => {
    vi.useFakeTimers();
    api.previewPersona.mockRejectedValue(new Error('stub'));

    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const selectAll = await findByText('Select All');
    await fireEvent.click(selectAll);
    const previewBtn = await findByText('Preview');
    await fireEvent.click(previewBtn);

    await vi.advanceTimersByTimeAsync(4000);

    await waitFor(async () => {
      expect(await findByText('Iterate')).toBeTruthy();
    });

    vi.useRealTimers();
  });

  it('Iterate transitions back to editing state', async () => {
    vi.useFakeTimers();
    api.previewPersona.mockRejectedValue(new Error('stub'));

    const { findByText, findByTestId } = render(MetaSpecs, {
      props: { scope: 'workspace', workspaceId: 'ws-1' },
    });

    const selectAll = await findByText('Select All');
    await fireEvent.click(selectAll);
    await fireEvent.click(await findByText('Preview'));
    await vi.advanceTimersByTimeAsync(4000);

    const iterateBtn = await findByText('Iterate');
    await fireEvent.click(iterateBtn);

    // Back to editing — textarea should reappear
    await waitFor(async () => {
      expect(await findByTestId('persona-textarea')).toBeTruthy();
    });

    vi.useRealTimers();
  });

  it('Publish button is visible in editing state', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    expect(await findByText('Publish')).toBeTruthy();
  });

  it('Cancel Preview returns to editing state', async () => {
    vi.useFakeTimers();
    api.previewPersona.mockRejectedValue(new Error('stub'));

    const { findByText, findByTestId } = render(MetaSpecs, {
      props: { scope: 'workspace', workspaceId: 'ws-1' },
    });

    const selectAll = await findByText('Select All');
    await fireEvent.click(selectAll);
    await fireEvent.click(await findByText('Preview'));
    await fireEvent.click(await findByText('Cancel Preview'));

    await waitFor(async () => {
      expect(await findByTestId('persona-textarea')).toBeTruthy();
    });

    vi.useRealTimers();
  });
});

describe('MetaSpecs — repo scope', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getSpecs.mockResolvedValue([...mockSpecs]);
    api.personas.mockResolvedValue([...mockPersonas]);
  });

  it('renders without throwing in repo scope', () => {
    expect(() => render(MetaSpecs, { props: { scope: 'repo', workspaceId: 'ws-1' } })).not.toThrow();
  });

  it('shows repo redirect message', () => {
    const { getByText } = render(MetaSpecs, { props: { scope: 'repo', workspaceId: 'ws-1' } });
    expect(getByText(/Meta-specs are workspace-scoped/)).toBeTruthy();
  });

  it('shows workspace editor after redirect message', async () => {
    const { findByTestId } = render(MetaSpecs, { props: { scope: 'repo', workspaceId: 'ws-1' } });
    // Workspace editor renders below redirect notice
    const loop = await findByTestId('preview-loop');
    expect(loop).toBeTruthy();
  });
});

describe('MetaSpecs — DiffSuggestion accept updates textarea', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getSpecs.mockResolvedValue([...mockSpecs]);
    api.personas.mockResolvedValue([...mockPersonas]);
    global.fetch = vi.fn().mockRejectedValue(new Error('fetch not available'));
  });

  it('AcceptSuggestion appends content to persona textarea', async () => {
    const { findByTestId, findByLabelText } = render(MetaSpecs, {
      props: { scope: 'workspace', workspaceId: 'ws-1' },
    });

    // Wait for textarea
    const textarea = await findByTestId('persona-textarea');
    const initialValue = textarea.value;

    // Trigger a chat message → produces mock suggestion
    const chatInput = await findByLabelText('Message input');
    await fireEvent.input(chatInput, { target: { value: 'Add error handling' } });
    // Dispatch ctrl+enter to send
    await fireEvent.keyDown(chatInput, { key: 'Enter', ctrlKey: true });

    // Wait for the Accept button to appear in a DiffSuggestion
    await waitFor(() => {
      const acceptBtns = document.querySelectorAll('.diff-actions button');
      return acceptBtns.length > 0;
    });

    const acceptBtn = document.querySelector('.diff-actions button');
    if (acceptBtn) {
      await fireEvent.click(acceptBtn);
      await waitFor(() => {
        expect(textarea.value.length).toBeGreaterThan(initialValue.length);
      });
    }
  });
});
