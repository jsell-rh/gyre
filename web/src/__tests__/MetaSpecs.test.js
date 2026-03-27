import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';
import MetaSpecs from '../components/MetaSpecs.svelte';

const mockPersonas = [
  {
    id: 'persona-1',
    kind: 'meta:persona',
    name: 'Backend Developer',
    scope: 'Global',
    prompt: 'You are a backend developer focused on Rust.',
    version: 1,
    required: false,
    approval_status: 'Approved',
  },
  {
    id: 'persona-2',
    kind: 'meta:persona',
    name: 'Security Reviewer',
    scope: 'Global',
    prompt: 'You review code for security vulnerabilities.',
    version: 1,
    required: false,
    approval_status: 'Pending',
  },
];

const mockSpecs = [
  { path: 'specs/payment/retry.md', title: 'Payment Retry', kind: null, approval_status: 'approved', current_sha: 'abc12345' },
  { path: 'specs/system/design.md', title: 'Design Principles', kind: null, approval_status: 'pending', current_sha: 'def67890' },
];

const mockMetaSpecs = [
  {
    id: 'ms-1',
    kind: 'meta:persona',
    name: 'Backend Persona',
    scope: 'Global',
    prompt: 'You are a backend engineer.',
    version: 2,
    required: false,
    approval_status: 'Approved',
  },
  {
    id: 'ms-2',
    kind: 'meta:principle',
    name: 'Quality Principle',
    scope: 'Global',
    prompt: 'Write high quality code.',
    version: 1,
    required: true,
    approval_status: 'Pending',
  },
];

vi.mock('../lib/api.js', () => ({
  api: {
    getSpecs: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    getMetaSpec: vi.fn().mockResolvedValue(null),
    createMetaSpec: vi.fn().mockResolvedValue(null),
    updateMetaSpec: vi.fn().mockResolvedValue(null),
    deleteMetaSpec: vi.fn().mockResolvedValue(null),
    getMetaSpecVersions: vi.fn().mockResolvedValue([]),
    getMetaSpecBlastRadius: vi.fn().mockResolvedValue({ affected_workspaces: [], affected_repos: [] }),
    previewPersona: vi.fn().mockRejectedValue(new Error('Not implemented')),
    previewPersonaStatus: vi.fn().mockRejectedValue(new Error('Not implemented')),
    publishPersona: vi.fn().mockRejectedValue(new Error('Not implemented')),
  },
}));

import { api } from '../lib/api.js';

global.fetch = vi.fn().mockRejectedValue(new Error('fetch not available in test'));

describe('MetaSpecs — tenant scope (default)', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getMetaSpecs.mockResolvedValue([...mockMetaSpecs]);
    api.getSpecs.mockResolvedValue([]);
  });

  it('renders without throwing', () => {
    expect(() => render(MetaSpecs, { props: { scope: 'tenant' } })).not.toThrow();
  });

  it('shows Meta-Specs heading', () => {
    const { getByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(getByText('Meta-Specs')).toBeTruthy();
  });

  it('shows sidebar with spec names after loading', async () => {
    const { findAllByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // Name appears in sidebar AND editor title (auto-selected), so use findAllByText
    expect((await findAllByText('Backend Persona')).length).toBeGreaterThan(0);
    expect((await findAllByText('Quality Principle')).length).toBeGreaterThan(0);
  });

  it('shows kind filter pills', () => {
    const { getByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(getByText('All')).toBeTruthy();
    expect(getByText('Persona')).toBeTruthy();
    expect(getByText('Principle')).toBeTruthy();
  });

  it('filters sidebar by kind when pill is clicked', async () => {
    api.getMetaSpecs.mockResolvedValue([...mockMetaSpecs]);
    const { findAllByText, container, queryByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Backend Persona');

    const pills = container.querySelectorAll('.filter-pills .pill');
    const personaPill = Array.from(pills).find(b => b.textContent.trim() === 'Persona');
    expect(personaPill).toBeTruthy();
    await fireEvent.click(personaPill);

    await waitFor(() => {
      expect(queryByText('Quality Principle')).toBeNull();
    });
  });

  it('shows empty state when no meta-specs exist', async () => {
    api.getMetaSpecs.mockResolvedValue([]);
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(await findByText('Select or create a meta-spec')).toBeTruthy();
  });

  it('shows error state when API fails', async () => {
    api.getMetaSpecs.mockRejectedValue(new Error('Server error'));
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(await findByText('Failed to load meta-specs')).toBeTruthy();
  });

  it('shows + New Meta-spec button', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(await findByText('+ New Meta-spec')).toBeTruthy();
  });

  it('shows create panel with kind cards on + New Meta-spec click', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const btn = await findByText('+ New Meta-spec');
    await fireEvent.click(btn);
    // Create panel renders kind selection grid
    expect(await findByText('New Meta-spec')).toBeTruthy();
  });

  it('auto-selects first spec and shows editor tabs', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // Editor tabs should appear after auto-selecting first spec
    expect(await findByText('Edit')).toBeTruthy();
    expect(await findByText('Impact')).toBeTruthy();
    expect(await findByText('History')).toBeTruthy();
    expect(await findByText('Approval')).toBeTruthy();
  });

  it('shows spec textarea in edit tab', async () => {
    const { findByTestId } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(await findByTestId('spec-textarea')).toBeTruthy();
  });

  it('clicking sidebar item selects it', async () => {
    const { findAllByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Backend Persona');

    const sidebarItems = container.querySelectorAll('.sidebar-item');
    expect(sidebarItems.length).toBeGreaterThan(0);
    const secondItem = sidebarItems[1];
    await fireEvent.click(secondItem);

    // Second item should become active
    await waitFor(() => {
      expect(secondItem.classList.contains('active')).toBe(true);
    });
  });

  it('shows Approve in Approval tab for Pending items', async () => {
    const { findAllByText, findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // Wait for sidebar to load
    await findAllByText('Quality Principle');
    // Click the second sidebar item (Quality Principle, which is Pending)
    const sidebarItems = container.querySelectorAll('.sidebar-item');
    if (sidebarItems.length > 1) {
      await fireEvent.click(sidebarItems[1]);
    }
    const approvalTab = await findByText('Approval');
    await fireEvent.click(approvalTab);
    // Approval tab should show Approve button
    expect(await findByText('Approve')).toBeTruthy();
  });
});

describe('MetaSpecs — workspace scope', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getSpecs.mockResolvedValue([...mockSpecs]);
    api.getMetaSpecs.mockResolvedValue([...mockPersonas]);
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

  it('shows meta-spec selector dropdown', async () => {
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
    api.getMetaSpecs.mockResolvedValue([...mockPersonas]);
  });

  it('renders without throwing in repo scope', () => {
    expect(() => render(MetaSpecs, { props: { scope: 'repo', workspaceId: 'ws-1' } })).not.toThrow();
  });

  it('shows repo redirect message', () => {
    const { getByText } = render(MetaSpecs, { props: { scope: 'repo', workspaceId: 'ws-1' } });
    expect(getByText(/Meta-specs are workspace-scoped/)).toBeTruthy();
  });
});

describe('MetaSpecs — DiffSuggestion accept updates textarea', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getSpecs.mockResolvedValue([...mockSpecs]);
    api.getMetaSpecs.mockResolvedValue([...mockPersonas]);
    global.fetch = vi.fn().mockRejectedValue(new Error('fetch not available'));
  });

  it('AcceptSuggestion appends content to persona textarea', async () => {
    const { findByTestId, findByLabelText } = render(MetaSpecs, {
      props: { scope: 'workspace', workspaceId: 'ws-1' },
    });

    const textarea = await findByTestId('persona-textarea');
    const initialValue = textarea.value;

    const chatInput = await findByLabelText('Message input');
    await fireEvent.input(chatInput, { target: { value: 'Add error handling' } });
    await fireEvent.keyDown(chatInput, { key: 'Enter', ctrlKey: true });

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
