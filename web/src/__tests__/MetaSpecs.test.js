import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, fireEvent, screen } from '@testing-library/svelte';
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
  { path: 'specs/payment/retry.md', title: 'Payment Retry', kind: null, approval_status: 'approved', current_sha: 'abc12345', repo_id: 'repo-1' },
  { path: 'specs/system/design.md', title: 'Design Principles', kind: null, approval_status: 'pending', current_sha: 'def67890', repo_id: 'repo-2' },
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
    created_by: 'user-1',
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
    created_by: 'user-2',
  },
  {
    id: 'ms-3',
    kind: 'meta:standard',
    name: 'Code Style Standard',
    scope: 'Global',
    prompt: 'Use consistent formatting.',
    version: 1,
    required: false,
    approval_status: 'Rejected',
    created_by: 'user-3',
  },
  {
    id: 'ms-4',
    kind: 'meta:process',
    name: 'Review Process',
    scope: 'Workspace',
    scope_id: 'ws-abc',
    prompt: 'All code must be reviewed.',
    version: 3,
    required: false,
    approval_status: 'Approved',
    approved_by: 'admin-1',
    created_by: 'user-1',
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
    previewMetaSpec: vi.fn().mockRejectedValue(new Error('Not implemented')),
    previewMetaSpecStatus: vi.fn().mockRejectedValue(new Error('Not implemented')),
    deleteMetaSpecPreview: vi.fn().mockResolvedValue(null),
    publishPersona: vi.fn().mockRejectedValue(new Error('Not implemented')),
    specsAssist: vi.fn().mockRejectedValue(new Error('Not available')),
    metaSpecsAssist: vi.fn().mockRejectedValue(new Error('Not available')),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    graphPredict: vi.fn().mockResolvedValue({ predictions: [] }),
    specsSave: vi.fn().mockResolvedValue({ mr_id: 1 }),
    workspacePresence: vi.fn().mockResolvedValue([]),
  },
}));

import { api } from '../lib/api.js';

global.fetch = vi.fn().mockRejectedValue(new Error('fetch not available in test'));

// ─── Tenant scope ─────────────────────────────────────────────────────────────

describe('MetaSpecs -- tenant scope (default)', () => {
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
    expect((await findAllByText('Backend Persona')).length).toBeGreaterThan(0);
    expect((await findAllByText('Quality Principle')).length).toBeGreaterThan(0);
  });

  it('shows kind filter pills', () => {
    const { getByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(getByText('All')).toBeTruthy();
    expect(getByText('Persona')).toBeTruthy();
    expect(getByText('Principle')).toBeTruthy();
    expect(getByText('Standard')).toBeTruthy();
    expect(getByText('Process')).toBeTruthy();
  });

  it('filters sidebar by kind when Persona pill is clicked', async () => {
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

  it('filters sidebar by Standard kind', async () => {
    const { findAllByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Backend Persona');

    const pills = container.querySelectorAll('.filter-pills .pill');
    const standardPill = Array.from(pills).find(b => b.textContent.trim() === 'Standard');
    await fireEvent.click(standardPill);

    await waitFor(() => {
      const sidebarItems = container.querySelectorAll('.sidebar-item');
      // Only 1 standard spec: "Code Style Standard"
      expect(sidebarItems.length).toBe(1);
      expect(sidebarItems[0].textContent).toContain('Code Style Standard');
    });
  });

  it('All pill shows all specs again after filtering', async () => {
    const { findAllByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Backend Persona');

    const pills = container.querySelectorAll('.filter-pills .pill');
    const personaPill = Array.from(pills).find(b => b.textContent.trim() === 'Persona');
    await fireEvent.click(personaPill);

    const allPill = Array.from(pills).find(b => b.textContent.trim() === 'All');
    await fireEvent.click(allPill);

    await waitFor(() => {
      const sidebar = container.querySelectorAll('.sidebar-item');
      expect(sidebar.length).toBe(4);
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

  it('shows retry button on error', async () => {
    api.getMetaSpecs.mockRejectedValue(new Error('Server error'));
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(await findByText('Retry')).toBeTruthy();
  });

  it('retry reloads specs', async () => {
    api.getMetaSpecs.mockRejectedValueOnce(new Error('Server error'));
    api.getMetaSpecs.mockResolvedValueOnce([...mockMetaSpecs]);
    const { findByText, findAllByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const retryBtn = await findByText('Retry');
    await fireEvent.click(retryBtn);
    expect((await findAllByText('Backend Persona')).length).toBeGreaterThan(0);
  });

  it('shows + New Meta-spec button', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    expect(await findByText('+ New Meta-spec')).toBeTruthy();
  });

  it('shows create panel with kind cards on + New Meta-spec click', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const btn = await findByText('+ New Meta-spec');
    await fireEvent.click(btn);
    expect(await findByText('New Meta-spec')).toBeTruthy();
  });

  it('auto-selects first spec and shows editor tabs', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
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

    await waitFor(() => {
      expect(secondItem.classList.contains('active')).toBe(true);
    });
  });

  it('shows Approve in Approval tab for Pending items', async () => {
    const { findAllByText, findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Quality Principle');
    const sidebarItems = container.querySelectorAll('.sidebar-item');
    if (sidebarItems.length > 1) {
      await fireEvent.click(sidebarItems[1]);
    }
    const approvalTab = await findByText('Approval');
    await fireEvent.click(approvalTab);
    expect(await findByText('Approve')).toBeTruthy();
  });

  it('shows sidebar status dots', async () => {
    const { container, findAllByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Backend Persona');
    const dots = container.querySelectorAll('.sidebar-status-dot');
    expect(dots.length).toBeGreaterThan(0);
  });

  it('shows Required chip on required specs', async () => {
    const { container, findAllByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Quality Principle');
    const chips = container.querySelectorAll('.required-chip');
    expect(chips.length).toBeGreaterThan(0);
  });

  it('shows version chips in sidebar', async () => {
    const { container, findAllByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Backend Persona');
    const verChips = container.querySelectorAll('.ver-chip');
    expect(verChips.length).toBeGreaterThan(0);
  });
});

// ─── Create flow ──────────────────────────────────────────────────────────────

describe('MetaSpecs -- create flow', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getMetaSpecs.mockResolvedValue([...mockMetaSpecs]);
    api.getSpecs.mockResolvedValue([]);
  });

  it('create panel shows all four kind cards', async () => {
    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));
    await findByText('New Meta-spec');
    const kindCards = container.querySelectorAll('.kind-card');
    expect(kindCards.length).toBe(4);
    // Check kind labels are present
    const labels = Array.from(container.querySelectorAll('.kind-card-label')).map(el => el.textContent);
    expect(labels).toContain('Persona');
    expect(labels).toContain('Principle');
    expect(labels).toContain('Standard');
    expect(labels).toContain('Process');
  });

  it('create panel shows Name and Scope fields', async () => {
    const { findByText, findByLabelText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));
    expect(await findByLabelText('Name')).toBeTruthy();
    expect(await findByLabelText('Scope')).toBeTruthy();
  });

  it('create panel shows Content/Prompt textarea', async () => {
    const { findByText, findByLabelText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));
    expect(await findByLabelText('Content / Prompt')).toBeTruthy();
  });

  it('create panel shows Required checkbox', async () => {
    const { findByText, findByLabelText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));
    expect(await findByLabelText(/Required/)).toBeTruthy();
  });

  it('Cancel button returns to editor view', async () => {
    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));
    await findByText('New Meta-spec');
    // Click the Cancel button inside create-actions
    const cancelBtn = container.querySelector('.create-actions button');
    await fireEvent.click(cancelBtn);
    // Should show sidebar items again (create panel hidden)
    await waitFor(() => {
      const sidebarItems = container.querySelectorAll('.sidebar-item');
      expect(sidebarItems.length).toBeGreaterThan(0);
    });
  });

  it('Create Meta-spec button calls API with form data', async () => {
    const created = { id: 'ms-new', kind: 'meta:persona', name: 'Test Persona', scope: 'Global', prompt: 'test prompt', version: 1, required: false, approval_status: 'Pending' };
    api.createMetaSpec.mockResolvedValue(created);

    const { findByText, findByLabelText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));

    const nameInput = await findByLabelText('Name');
    await fireEvent.input(nameInput, { target: { value: 'Test Persona' } });

    const promptArea = await findByLabelText('Content / Prompt');
    await fireEvent.input(promptArea, { target: { value: 'test prompt' } });

    await fireEvent.click(await findByText('Create Meta-spec'));

    await waitFor(() => {
      expect(api.createMetaSpec).toHaveBeenCalledWith(expect.objectContaining({
        kind: 'meta:persona',
        name: 'Test Persona',
        prompt: 'test prompt',
      }));
    });
  });

  it('selecting a kind card updates the form', async () => {
    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));

    const kindCards = container.querySelectorAll('.kind-card');
    // Click "Standard" (third card)
    await fireEvent.click(kindCards[2]);

    await waitFor(() => {
      expect(kindCards[2].classList.contains('selected')).toBe(true);
    });
  });

  it('shows Workspace ID field when scope is Workspace', async () => {
    const { findByText, findByLabelText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));

    const scopeSelect = await findByLabelText('Scope');
    await fireEvent.change(scopeSelect, { target: { value: 'Workspace' } });

    expect(await findByLabelText('Workspace ID')).toBeTruthy();
  });

  it('Escape key closes create panel', async () => {
    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));
    await findByText('New Meta-spec');

    await fireEvent.keyDown(window, { key: 'Escape' });

    await waitFor(() => {
      const createPanel = container.querySelector('.create-panel');
      expect(createPanel).toBeNull();
    });
  });
});

// ─── Editor tabs ──────────────────────────────────────────────────────────────

describe('MetaSpecs -- editor tabs', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getMetaSpecs.mockResolvedValue([...mockMetaSpecs]);
    api.getSpecs.mockResolvedValue([]);
  });

  it('edit tab shows word count', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // First spec prompt is "You are a backend engineer." = 5 words
    await waitFor(async () => {
      expect(await findByText(/\d+ words/)).toBeTruthy();
    });
  });

  it('edit tab shows Save button (disabled when clean)', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const saveBtn = await findByText('Saved');
    expect(saveBtn.closest('button').disabled).toBe(true);
  });

  it('typing in textarea enables Save button', async () => {
    const { findByTestId, findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const textarea = await findByTestId('spec-textarea');
    await fireEvent.input(textarea, { target: { value: 'Modified content' } });

    await waitFor(async () => {
      const saveBtn = await findByText(/Save \(creates v/);
      expect(saveBtn.closest('button').disabled).toBe(false);
    });
  });

  it('Save calls updateMetaSpec API', async () => {
    const updated = { ...mockMetaSpecs[0], prompt: 'Modified content', version: 3 };
    api.updateMetaSpec.mockResolvedValue(updated);

    const { findByTestId, findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const textarea = await findByTestId('spec-textarea');
    await fireEvent.input(textarea, { target: { value: 'Modified content' } });

    const saveBtn = await findByText(/Save \(creates v/);
    await fireEvent.click(saveBtn);

    await waitFor(() => {
      expect(api.updateMetaSpec).toHaveBeenCalledWith('ms-1', { prompt: 'Modified content' });
    });
  });

  it('Impact tab loads blast radius', async () => {
    api.getMetaSpecBlastRadius.mockResolvedValue({
      affected_workspaces: [{ id: 'ws-1' }],
      affected_repos: [{ id: 'repo-1', reason: 'direct', workspace_id: 'ws-1' }],
    });

    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const impactTab = await findByText('Impact');
    await fireEvent.click(impactTab);

    await waitFor(() => {
      expect(api.getMetaSpecBlastRadius).toHaveBeenCalledWith('ms-1');
    });
  });

  it('Impact tab shows metric cards', async () => {
    api.getMetaSpecBlastRadius.mockResolvedValue({
      affected_workspaces: [{ id: 'ws-1' }],
      affected_repos: [],
    });

    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('Impact'));

    expect(await findByText('Bound workspaces')).toBeTruthy();
    expect(await findByText('Affected repos')).toBeTruthy();
    expect(await findByText('Agent runs')).toBeTruthy();
    expect(await findByText('Gate failures')).toBeTruthy();
  });

  it('Impact tab shows blast radius error', async () => {
    api.getMetaSpecBlastRadius.mockResolvedValue({ error: 'Service unavailable' });

    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('Impact'));

    expect(await findByText('Service unavailable')).toBeTruthy();
  });

  it('History tab loads version history', async () => {
    api.getMetaSpecVersions.mockResolvedValue([
      { version: 2, content_hash: 'abc123def456', prompt: 'v2 content' },
      { version: 1, content_hash: 'xyz789000111', prompt: 'v1 content' },
    ]);

    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('History'));

    await waitFor(() => {
      expect(api.getMetaSpecVersions).toHaveBeenCalledWith('ms-1');
    });
    // Check version badges in the timeline
    await waitFor(() => {
      const verBadges = container.querySelectorAll('.ver-badge');
      expect(verBadges.length).toBeGreaterThanOrEqual(2);
    });
  });

  it('History tab shows empty state when no versions', async () => {
    api.getMetaSpecVersions.mockResolvedValue([]);
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('History'));
    expect(await findByText('No version history')).toBeTruthy();
  });

  it('History tab shows diff when version node is clicked', async () => {
    api.getMetaSpecVersions.mockResolvedValue([
      { version: 2, content_hash: 'abc123def456', prompt: 'line one changed' },
      { version: 1, content_hash: 'xyz789000111', prompt: 'line one original' },
    ]);

    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('History'));

    // Wait for version timeline to render
    await waitFor(() => {
      const versionNodes = container.querySelectorAll('.version-node');
      expect(versionNodes.length).toBeGreaterThan(0);
    });

    const versionNodes = container.querySelectorAll('.version-node');
    await fireEvent.click(versionNodes[0]);

    await waitFor(() => {
      const diffPanel = container.querySelector('.version-diff-panel');
      expect(diffPanel).toBeTruthy();
    });
  });

  it('Approval tab shows current approval status', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // First spec is Approved
    await fireEvent.click(await findByText('Approval'));
    expect(await findByText(/Approved by/)).toBeTruthy();
  });

  it('Approval tab shows Revoke Approval for approved spec', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('Approval'));
    expect(await findByText('Revoke Approval')).toBeTruthy();
  });

  it('Approval tab shows Approve and Reject for Pending spec', async () => {
    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // Select the Pending spec (second item)
    const sidebarItems = await waitFor(() => {
      const items = container.querySelectorAll('.sidebar-item');
      expect(items.length).toBeGreaterThan(1);
      return items;
    });
    await fireEvent.click(sidebarItems[1]);
    await fireEvent.click(await findByText('Approval'));
    expect(await findByText('Approve')).toBeTruthy();
    expect(await findByText('Reject')).toBeTruthy();
  });

  it('Approval tab shows Re-approve for Rejected spec', async () => {
    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // Select the Rejected spec (third item: Code Style Standard)
    const sidebarItems = await waitFor(() => {
      const items = container.querySelectorAll('.sidebar-item');
      expect(items.length).toBeGreaterThan(2);
      return items;
    });
    await fireEvent.click(sidebarItems[2]);
    await fireEvent.click(await findByText('Approval'));
    expect(await findByText('Re-approve')).toBeTruthy();
  });

  it('Approve button calls updateMetaSpec', async () => {
    const updated = { ...mockMetaSpecs[1], approval_status: 'Approved' };
    api.updateMetaSpec.mockResolvedValue(updated);

    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const sidebarItems = await waitFor(() => {
      const items = container.querySelectorAll('.sidebar-item');
      expect(items.length).toBeGreaterThan(1);
      return items;
    });
    await fireEvent.click(sidebarItems[1]);
    await fireEvent.click(await findByText('Approval'));
    await fireEvent.click(await findByText('Approve'));

    await waitFor(() => {
      expect(api.updateMetaSpec).toHaveBeenCalledWith('ms-2', { approval_status: 'Approved' });
    });
  });

  it('Reject button calls updateMetaSpec', async () => {
    const updated = { ...mockMetaSpecs[1], approval_status: 'Rejected' };
    api.updateMetaSpec.mockResolvedValue(updated);

    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const sidebarItems = await waitFor(() => {
      const items = container.querySelectorAll('.sidebar-item');
      expect(items.length).toBeGreaterThan(1);
      return items;
    });
    await fireEvent.click(sidebarItems[1]);
    await fireEvent.click(await findByText('Approval'));
    await fireEvent.click(await findByText('Reject'));

    await waitFor(() => {
      expect(api.updateMetaSpec).toHaveBeenCalledWith('ms-2', { approval_status: 'Rejected' });
    });
  });

  it('Approval tab shows approval meta details', async () => {
    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // Wait for sidebar to load
    await waitFor(() => {
      const items = container.querySelectorAll('.sidebar-item');
      expect(items.length).toBe(4);
    });
    // Select spec with approved_by (4th item: Review Process)
    const sidebarItems = container.querySelectorAll('.sidebar-item');
    await fireEvent.click(sidebarItems[3]);
    await fireEvent.click(await findByText('Approval'));

    // Check approval meta section renders scope and created_by
    await waitFor(() => {
      const metaRows = container.querySelectorAll('.approval-meta-row');
      expect(metaRows.length).toBeGreaterThanOrEqual(3);
    });
  });
});

// ─── Delete flow ──────────────────────────────────────────────────────────────

describe('MetaSpecs -- delete flow', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getMetaSpecs.mockResolvedValue([...mockMetaSpecs]);
    api.getSpecs.mockResolvedValue([]);
  });

  it('Delete button shows confirmation modal', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // Wait for first spec to auto-select, then click Delete
    await findByText('Edit');
    await fireEvent.click(await findByText('Delete'));

    expect(await findByText('Delete Meta-spec')).toBeTruthy();
    expect(await findByText(/Are you sure you want to delete/)).toBeTruthy();
  });

  it('Cancel in delete modal closes it', async () => {
    const { findByText, queryByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findByText('Edit');
    await fireEvent.click(await findByText('Delete'));
    await findByText('Delete Meta-spec');

    // Click Cancel
    const cancelBtn = await findByText('Cancel');
    await fireEvent.click(cancelBtn);

    await waitFor(() => {
      expect(queryByText('Delete Meta-spec')).toBeNull();
    });
  });

  it('Confirm delete calls API and removes spec', async () => {
    api.deleteMetaSpec.mockResolvedValue(undefined);

    const { findByText, findAllByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Backend Persona');
    await fireEvent.click(await findByText('Delete'));

    // Modal shows the delete button
    const modalDeleteBtns = container.querySelectorAll('.form-actions button');
    const confirmBtn = Array.from(modalDeleteBtns).find(b => b.textContent.includes('Delete'));
    await fireEvent.click(confirmBtn);

    await waitFor(() => {
      expect(api.deleteMetaSpec).toHaveBeenCalledWith('ms-1');
    });
  });
});

// ─── Dirty state and discard modal ────────────────────────────────────────────

describe('MetaSpecs -- dirty state tracking', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getMetaSpecs.mockResolvedValue([...mockMetaSpecs]);
    api.getSpecs.mockResolvedValue([]);
  });

  it('editing textarea sets dirty state', async () => {
    const { findByTestId, findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const textarea = await findByTestId('spec-textarea');
    await fireEvent.input(textarea, { target: { value: 'changed' } });

    // Save button should show version increment text
    expect(await findByText(/Save \(creates v/)).toBeTruthy();
  });

  it('switching specs with dirty state shows discard modal', async () => {
    const { findByTestId, findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const textarea = await findByTestId('spec-textarea');
    await fireEvent.input(textarea, { target: { value: 'changed' } });

    // Click second sidebar item
    const sidebarItems = await waitFor(() => {
      const items = container.querySelectorAll('.sidebar-item');
      expect(items.length).toBeGreaterThan(1);
      return items;
    });
    await fireEvent.click(sidebarItems[1]);

    expect(await findByText('Unsaved Changes')).toBeTruthy();
    expect(await findByText('Keep Editing')).toBeTruthy();
    expect(await findByText('Discard')).toBeTruthy();
  });

  it('Keep Editing closes the discard modal without switching', async () => {
    const { findByTestId, findByText, container, queryByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const textarea = await findByTestId('spec-textarea');
    await fireEvent.input(textarea, { target: { value: 'changed' } });

    const sidebarItems = container.querySelectorAll('.sidebar-item');
    await fireEvent.click(sidebarItems[1]);

    await fireEvent.click(await findByText('Keep Editing'));

    await waitFor(() => {
      expect(queryByText('Unsaved Changes')).toBeNull();
    });
  });

  it('Discard closes modal and switches to new spec', async () => {
    const { findByTestId, findByText, container, queryByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const textarea = await findByTestId('spec-textarea');
    await fireEvent.input(textarea, { target: { value: 'changed' } });

    const sidebarItems = container.querySelectorAll('.sidebar-item');
    await fireEvent.click(sidebarItems[1]);

    await fireEvent.click(await findByText('Discard'));

    await waitFor(() => {
      expect(queryByText('Unsaved Changes')).toBeNull();
      expect(sidebarItems[1].classList.contains('active')).toBe(true);
    });
  });

  it('+ New Meta-spec with dirty state shows discard modal', async () => {
    const { findByTestId, findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const textarea = await findByTestId('spec-textarea');
    await fireEvent.input(textarea, { target: { value: 'changed' } });

    await fireEvent.click(await findByText('+ New Meta-spec'));

    expect(await findByText('Unsaved Changes')).toBeTruthy();
  });
});

// ─── Required toggle ─────────────────────────────────────────────────────────

describe('MetaSpecs -- required toggle', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getMetaSpecs.mockResolvedValue([...mockMetaSpecs]);
    api.getSpecs.mockResolvedValue([]);
  });

  it('shows Optional/Required toggle button', async () => {
    const { findByLabelText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // First spec is not required
    expect(await findByLabelText(/Optional/)).toBeTruthy();
  });

  it('clicking toggle calls updateMetaSpec', async () => {
    const updated = { ...mockMetaSpecs[0], required: true };
    api.updateMetaSpec.mockResolvedValue(updated);

    const { findByLabelText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const toggleBtn = await findByLabelText(/Optional/);
    await fireEvent.click(toggleBtn);

    await waitFor(() => {
      expect(api.updateMetaSpec).toHaveBeenCalledWith('ms-1', { required: true });
    });
  });
});

// ─── Workspace scope ──────────────────────────────────────────────────────────
describe('MetaSpecs -- workspace scope', () => {
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

  it('excludes specs without a repo from the target checklist', async () => {
    const orphan = { path: 'specs/system/orphan.md', title: 'Orphan', kind: null, repo_id: null };
    api.getSpecs.mockResolvedValue([...mockSpecs, orphan]);
    const { queryByText, findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    expect(await findByText('specs/payment/retry.md')).toBeTruthy();
    expect(await waitFor(() => queryByText('specs/system/orphan.md'))).toBeNull();
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

  it('Clear deselects all specs', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Clear'));

    const previewBtn = await findByText('Preview');
    await waitFor(() => {
      expect(previewBtn.disabled).toBe(true);
    });
  });

  it('toggling individual spec checkbox works', async () => {
    const { findByText, container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await findByText('specs/payment/retry.md');

    const checkboxes = container.querySelectorAll('.spec-check-item input[type="checkbox"]');
    expect(checkboxes.length).toBe(2);

    await fireEvent.click(checkboxes[0]);

    await waitFor(() => {
      expect(checkboxes[0].checked).toBe(true);
    });
  });

  it('transitions to running state on Preview click', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({ preview_id: 'pv-1', agents: [] });

    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const selectAll = await findByText('Select All');
    await fireEvent.click(selectAll);

    const previewBtn = await findByText('Preview');
    await fireEvent.click(previewBtn);

    expect(await findByText('Preview: Running')).toBeTruthy();
    expect(await findByText('Cancel Preview')).toBeTruthy();
    vi.useRealTimers();
  });

  it('Preview POST sends the spec-conformant draft and targets', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({ preview_id: 'pv-1', agents: [] });

    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));

    await waitFor(() => {
      expect(api.previewMetaSpec).toHaveBeenCalledWith({
        draft: { kind: 'meta:persona', content: 'You are a backend developer focused on Rust.' },
        targets: [
          { repo_id: 'repo-1', spec_path: 'specs/payment/retry.md' },
          { repo_id: 'repo-2', spec_path: 'specs/system/design.md' },
        ],
      });
    });
    vi.useRealTimers();
  });

  it('shows one progress item per spawned agent', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-1',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry' },
        { agent_id: 'ag-2', repo_id: 'repo-2', spec_path: 'specs/system/design.md', branch: 'preview/pv-1/design' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-1',
      state: 'running',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry', status: 'running', diff: null },
        { agent_id: 'ag-2', repo_id: 'repo-2', spec_path: 'specs/system/design.md', branch: 'preview/pv-1/design', status: 'running', diff: null },
      ],
    });

    const { findByText, container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));

    await vi.advanceTimersByTimeAsync(2100);

    await waitFor(() => {
      const progressItems = container.querySelectorAll('.progress-item');
      expect(progressItems.length).toBe(2);
    });
    vi.useRealTimers();
  });

  it('transitions to complete when the server reports all agents done', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-1',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-1',
      state: 'complete',
      agents: [
        {
          agent_id: 'ag-1',
          repo_id: 'repo-1',
          spec_path: 'specs/payment/retry.md',
          branch: 'preview/pv-1/retry',
          status: 'complete',
          diff: { files_changed: 1, insertions: 2, deletions: 1, patches: [{ path: 'src/lib.rs', status: 'modified', patch: '+fn new() {}\n-fn old() {}' }] },
        },
      ],
    });

    const { findByText, findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));

    await vi.advanceTimersByTimeAsync(2100);

    await waitFor(async () => {
      expect(await findByTestId('preview-complete')).toBeTruthy();
    });
    expect(await findByText('Iterate')).toBeTruthy();
    vi.useRealTimers();
  });

  it('renders the produced diff patch lines after completion', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-1',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-1',
      state: 'complete',
      agents: [
        {
          agent_id: 'ag-1',
          repo_id: 'repo-1',
          spec_path: 'specs/payment/retry.md',
          branch: 'preview/pv-1/retry',
          status: 'complete',
          diff: {
            files_changed: 1,
            insertions: 2,
            deletions: 1,
            patches: [{ path: 'src/lib.rs', status: 'modified', patch: '+fn new() {}\n-fn old() {}\n ctx' }],
          },
        },
      ],
    });

    const { findByText, findByTestId, container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));
    await vi.advanceTimersByTimeAsync(2100);
    await findByTestId('preview-complete');

    await waitFor(() => {
      const lines = container.querySelectorAll('.code-diff-line');
      expect(lines.length).toBe(3);
      expect(lines[0].classList.contains('add')).toBe(true);
      expect(lines[1].classList.contains('remove')).toBe(true);
      expect(lines[2].classList.contains('ctx')).toBe(true);
    });
    vi.useRealTimers();
  });

  it('classifies unified-diff file headers as context, not add/remove', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-hdr-1',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-hdr-1/retry' },
      ],
    });
    // Real git-adapter patch text carries ---/+++/@@ headers; the viewer
    // must not color them as removed/added lines (parity with the EditorSplit
    // renderer's header filter).
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-hdr-1',
      state: 'complete',
      agents: [
        {
          agent_id: 'ag-1',
          repo_id: 'repo-1',
          spec_path: 'specs/payment/retry.md',
          branch: 'preview/pv-hdr-1/retry',
          status: 'complete',
          diff: {
            files_changed: 1,
            insertions: 1,
            deletions: 0,
            patches: [{
              path: 'src/lib.rs',
              status: 'modified',
              patch: '--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,1 +1,2 @@\n fn old() {}\n+fn new() {}',
            }],
          },
        },
      ],
    });

    const { findByText, findByTestId, container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));
    await vi.advanceTimersByTimeAsync(2100);
    await findByTestId('preview-complete');

    await waitFor(() => {
      const lines = Array.from(container.querySelectorAll('.code-diff-line'));
      const plusHeader = lines.find((l) => l.textContent === '+++ b/src/lib.rs');
      expect(plusHeader).toBeTruthy();
      expect(plusHeader.classList.contains('add')).toBe(false);
      expect(plusHeader.classList.contains('ctx')).toBe(true);
      const minusHeader = lines.find((l) => l.textContent === '--- a/src/lib.rs');
      expect(minusHeader).toBeTruthy();
      expect(minusHeader.classList.contains('remove')).toBe(false);
      expect(minusHeader.classList.contains('ctx')).toBe(true);
      const added = lines.find((l) => l.textContent === '+fn new() {}');
      expect(added.classList.contains('add')).toBe(true);
    });
    vi.useRealTimers();
  });

  it('shows per-agent tabs for multi-target runs', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-1',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry' },
        { agent_id: 'ag-2', repo_id: 'repo-2', spec_path: 'specs/system/design.md', branch: 'preview/pv-1/design' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-1',
      state: 'complete',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry', status: 'complete', diff: null },
        { agent_id: 'ag-2', repo_id: 'repo-2', spec_path: 'specs/system/design.md', branch: 'preview/pv-1/design', status: 'failed', diff: null },
      ],
    });

    const { findByText, findByTestId, container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));
    await vi.advanceTimersByTimeAsync(2100);
    await findByTestId('preview-complete');

    await waitFor(() => {
      const tabs = container.querySelectorAll('.impact-tab');
      expect(tabs.length).toBe(2);
      expect(tabs[0].textContent).toBe('retry');
      expect(tabs[1].textContent).toBe('design');
    });
    // Switch to the failed agent and see its state rendered
    await fireEvent.click(container.querySelectorAll('.impact-tab')[1]);
    await waitFor(() => {
      expect(container.querySelector('#preview-agent-panel-1 [role="tabpanel"], #preview-agent-panel-1')).toBeTruthy();
    });
    expect(await findByText('The preview agent failed before producing a diff.')).toBeTruthy();
    vi.useRealTimers();
  });

  it('shows Iterate button in complete state', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-1',
      agents: [{ agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry' }],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-1',
      state: 'complete',
      agents: [{ agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry', status: 'complete', diff: null }],
    });

    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const selectAll = await findByText('Select All');
    await fireEvent.click(selectAll);
    const previewBtn = await findByText('Preview');
    await fireEvent.click(previewBtn);

    await vi.advanceTimersByTimeAsync(2100);

    await waitFor(async () => {
      expect(await findByText('Iterate')).toBeTruthy();
    });
    vi.useRealTimers();
  });

  it('Iterate transitions back to editing state', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-1',
      agents: [{ agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry' }],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-1',
      state: 'complete',
      agents: [{ agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-1/retry', status: 'complete', diff: null }],
    });

    const { findByText, findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });

    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));
    await vi.advanceTimersByTimeAsync(2100);

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
    api.previewMetaSpec.mockResolvedValue({ preview_id: 'pv-1', agents: [] });

    const { findByText, findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });

    const selectAll = await findByText('Select All');
    await fireEvent.click(selectAll);
    await fireEvent.click(await findByText('Preview'));
    await fireEvent.click(await findByText('Cancel Preview'));

    await waitFor(async () => {
      expect(await findByTestId('persona-textarea')).toBeTruthy();
    });
    vi.useRealTimers();
  });

  it('a stale in-flight status tick cannot stop or hijack a re-started preview (cancel → re-Preview)', async () => {
    vi.useFakeTimers();
    // Run 1's first status tick hangs until the test resolves it — by then
    // the user has cancelled and started run 2.
    let resolveRun1Status;
    const run1Status = new Promise((resolve) => { resolveRun1Status = resolve; });
    let postCall = 0;
    api.previewMetaSpec.mockImplementation(async () => {
      postCall += 1;
      const id = postCall === 1 ? 'pv-stale-1' : 'pv-stale-2';
      const agent = postCall === 1 ? 'ag-stale-1' : 'ag-stale-2';
      return {
        preview_id: id,
        agents: [
          { agent_id: agent, repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: `preview/${id}/retry` },
        ],
      };
    });
    let statusCall = 0;
    api.previewMetaSpecStatus.mockImplementation(() => {
      statusCall += 1;
      if (statusCall === 1) return run1Status;
      if (statusCall === 2) {
        return Promise.resolve({
          preview_id: 'pv-stale-2',
          state: 'running',
          agents: [
            { agent_id: 'ag-stale-2', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-stale-2/retry', status: 'running', diff: null },
          ],
        });
      }
      return Promise.resolve({
        preview_id: 'pv-stale-2',
        state: 'complete',
        agents: [
          {
            agent_id: 'ag-stale-2',
            repo_id: 'repo-1',
            spec_path: 'specs/payment/retry.md',
            branch: 'preview/pv-stale-2/retry',
            status: 'complete',
            diff: { files_changed: 1, insertions: 1, deletions: 0, patches: [{ path: 'src/lib.rs', status: 'modified', patch: '+fn new() {}' }] },
          },
        ],
      });
    });

    const { findByText, findByTestId, queryByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));
    // Run 1's first tick fires and hangs on the unresolved status promise.
    await vi.advanceTimersByTimeAsync(2100);

    // Cancel, then immediately re-Preview: run 2 gets a fresh poll interval.
    await fireEvent.click(await findByText('Cancel Preview'));
    await fireEvent.click(await findByText('Preview'));
    // Run 2's first tick reports running.
    await vi.advanceTimersByTimeAsync(2100);
    expect(queryByTestId('preview-running')).toBeTruthy();

    // The stale run-1 tick finally resolves as complete: it must neither
    // flip the panel to complete nor stop run 2's poller (regression: the
    // post-await guard used to stopPreview() whatever interval was live,
    // clearing the NEW run's poller and leaving the UI stuck in running
    // with no polling and no timeout).
    resolveRun1Status({
      preview_id: 'pv-stale-1',
      state: 'complete',
      agents: [
        { agent_id: 'ag-stale-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-stale-1/retry', status: 'complete', diff: null },
      ],
    });
    await vi.advanceTimersByTimeAsync(0);
    // Still running run 2 — the stale complete response changed nothing.
    expect(queryByTestId('preview-running')).toBeTruthy();
    expect(queryByTestId('preview-complete')).toBeNull();

    // Run 2's own poller is still alive: its next tick completes the run.
    await vi.advanceTimersByTimeAsync(2100);
    await waitFor(() => {
      expect(queryByTestId('preview-complete')).toBeTruthy();
    });
    vi.useRealTimers();
  });

  it('failed POST shows error and returns to editing', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockRejectedValue(new Error('API /meta-specs/preview: 403 Forbidden'));

    const { findByText, findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));

    await waitFor(async () => {
      expect(await findByTestId('persona-textarea')).toBeTruthy();
    });
    vi.useRealTimers();
  });

  it('Clean Up calls the DELETE endpoint and returns to editing', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-9',
      agents: [{ agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-9/retry' }],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-9',
      state: 'complete',
      agents: [{ agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-9/retry', status: 'complete', diff: null }],
    });

    const { findByText, findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));
    await vi.advanceTimersByTimeAsync(2100);
    await findByTestId('preview-complete');

    await fireEvent.click(await findByText('Clean Up'));

    await waitFor(() => {
      expect(api.deleteMetaSpecPreview).toHaveBeenCalledWith('pv-9');
    });
    await waitFor(async () => {
      expect(await findByTestId('persona-textarea')).toBeTruthy();
    });
    vi.useRealTimers();
  });

  it('Publish calls API', async () => {
    api.publishPersona.mockResolvedValue({});

    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Publish'));

    await waitFor(() => {
      expect(api.publishPersona).toHaveBeenCalledWith(
        'ws-1',
        'persona-1',
        expect.objectContaining({ content: expect.any(String) }),
      );
    });
  });

  it('shows workspace subtitle', async () => {
    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    expect(await findByText(/Preview how persona and principle changes/)).toBeTruthy();
  });

  it('changing meta-spec dropdown updates textarea', async () => {
    const { findByLabelText, findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const select = await findByLabelText('Persona');
    await fireEvent.change(select, { target: { value: 'persona-2' } });

    const textarea = await findByTestId('persona-textarea');
    await waitFor(() => {
      expect(textarea.value).toBe('You review code for security vulnerabilities.');
    });
  });

  it('handles graceful fallback when both APIs fail with catches', async () => {
    // loadWorkspaceData has .catch(() => []) on each call, so errors degrade to empty arrays
    api.getMetaSpecs.mockRejectedValue(new Error('Network error'));
    api.getSpecs.mockRejectedValue(new Error('Network error'));

    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    // Should still render with empty state
    expect(await findByText('No specs available in this workspace.')).toBeTruthy();
  });

  it('loads data correctly when APIs succeed', async () => {
    api.getMetaSpecs.mockResolvedValue([...mockPersonas]);
    api.getSpecs.mockResolvedValue([...mockSpecs]);

    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    expect(await findByText('specs/payment/retry.md')).toBeTruthy();
  });

  it('shows empty specs message when no non-meta specs', async () => {
    api.getSpecs.mockResolvedValue([]);
    api.getMetaSpecs.mockResolvedValue([...mockPersonas]);

    const { findByText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    expect(await findByText('No specs available in this workspace.')).toBeTruthy();
  });

  it('disables meta-spec selector during running state', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({ preview_id: 'pv-1', agents: [] });

    const { findByText, findByLabelText } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));

    const select = await findByLabelText('Persona');
    expect(select.disabled).toBe(true);

    vi.useRealTimers();
  });

  it('locked editor shows the diff of pending changes vs the published prompt (§9 State 2)', async () => {
    vi.useFakeTimers();
    // Real §5 PreviewResponse (meta_specs.rs): { preview_id, agents } — the
    // POST starts the run; the locked pane appears while it is running.
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-lock-1',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-lock-1/retry' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-lock-1',
      state: 'running',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-lock-1/retry', status: 'running', diff: null },
      ],
    });

    const { findByText, findByTestId, queryByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });

    // Edit the draft beyond the published registry prompt (persona-1).
    const textarea = await findByTestId('persona-textarea');
    await fireEvent.input(textarea, {
      target: { value: 'You are a backend developer focused on Rust.\nHandle all errors with Result<T, E>.' },
    });

    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByTestId('preview-btn'));

    // Editor locked, showing the pending change marked with '+' — the §9
    // State 2 wireframe's "+ Handle all errors..." line.
    const locked = await findByTestId('editor-locked');
    expect(locked).toBeTruthy();
    expect(queryByTestId('persona-textarea')).toBeNull();
    const diff = await findByTestId('locked-diff');
    const addLine = diff.querySelector('.locked-diff-add .locked-diff-text');
    expect(addLine.textContent).toBe('Handle all errors with Result<T, E>.');
    expect(diff.querySelector('.locked-diff-add .locked-diff-marker').textContent).toBe('+');
    // The unchanged published prompt stays as context
    const ctxLine = diff.querySelector('.locked-diff-context .locked-diff-text');
    expect(ctxLine.textContent).toBe('You are a backend developer focused on Rust.');
    expect(screen.getByTestId('locked-diff-summary').textContent).toBe('+1 -0');
    vi.useRealTimers();
  });

  it('locked editor falls back to the verbatim draft when the meta-spec has no stored prompt', async () => {
    vi.useFakeTimers();
    api.getMetaSpecs.mockResolvedValue([
      { ...mockPersonas[0], prompt: '' },
    ]);
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-lock-2',
      agents: [
        { agent_id: 'ag-2', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-lock-2/retry' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-lock-2',
      state: 'running',
      agents: [
        { agent_id: 'ag-2', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-lock-2/retry', status: 'running', diff: null },
      ],
    });

    const { findByText, findByTestId, queryByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.input(await findByTestId('persona-textarea'), {
      target: { value: 'Draft with no baseline.' },
    });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByTestId('preview-btn'));

    const locked = await findByTestId('editor-locked');
    expect(locked).toBeTruthy();
    // Empty baseline → no diff markers, verbatim draft
    expect(queryByTestId('locked-diff')).toBeNull();
    expect(locked.textContent).toContain('Draft with no baseline.');
    vi.useRealTimers();
  });

  it('refreshes the diff baseline after an in-session publish — no pending diff without further edits (§9 publish → preview)', async () => {
    vi.useFakeTimers();
    api.publishPersona.mockResolvedValue({});
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-lock-3',
      agents: [
        { agent_id: 'ag-3', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-lock-3/retry' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-lock-3',
      state: 'running',
      agents: [
        { agent_id: 'ag-3', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-lock-3/retry', status: 'running', diff: null },
      ],
    });

    const { findByText, findByTestId, queryByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });

    // Edit the draft beyond the published registry prompt (persona-1).
    const textarea = await findByTestId('persona-textarea');
    await fireEvent.input(textarea, {
      target: { value: 'You are a backend developer focused on Rust.\nHandle all errors with Result<T, E>.' },
    });

    // Publish the draft — it becomes the new published registry prompt.
    await fireEvent.click(await findByText('Publish'));
    await waitFor(() => {
      expect(api.publishPersona).toHaveBeenCalledWith(
        'ws-1',
        'persona-1',
        { content: 'You are a backend developer focused on Rust.\nHandle all errors with Result<T, E>.' },
      );
    });

    // Preview with no further edits: the draft equals the just-published
    // baseline, so the locked pane must show the verbatim draft with NO
    // pending-changes markers (regression: stale pre-publish baseline
    // rendered the already-published line as pending '+1 -0').
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByTestId('preview-btn'));

    const locked = await findByTestId('editor-locked');
    expect(locked).toBeTruthy();
    expect(queryByTestId('locked-diff')).toBeNull();
    expect(locked.textContent).toContain('Handle all errors with Result<T, E>.');
    vi.useRealTimers();
  });

  it('diffs post-publish edits against the new baseline — only the newly added line is pending', async () => {
    vi.useFakeTimers();
    api.publishPersona.mockResolvedValue({});
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-lock-4',
      agents: [
        { agent_id: 'ag-4', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-lock-4/retry' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-lock-4',
      state: 'running',
      agents: [
        { agent_id: 'ag-4', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-lock-4/retry', status: 'running', diff: null },
      ],
    });

    const { findByText, findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });

    const textarea = await findByTestId('persona-textarea');
    await fireEvent.input(textarea, {
      target: { value: 'You are a backend developer focused on Rust.\nHandle all errors with Result<T, E>.' },
    });

    // Publish, then keep editing beyond the just-published version.
    await fireEvent.click(await findByText('Publish'));
    await waitFor(() => expect(api.publishPersona).toHaveBeenCalled());
    await fireEvent.input(await findByTestId('persona-textarea'), {
      target: { value: 'You are a backend developer focused on Rust.\nHandle all errors with Result<T, E>.\nPrefer explicit types in public APIs.' },
    });

    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByTestId('preview-btn'));

    // Only the post-publish line is pending; both just-published lines are
    // context (regression: stale baseline marked them pending too).
    const diff = await findByTestId('locked-diff');
    expect(screen.getByTestId('locked-diff-summary').textContent).toBe('+1 -0');
    const addLine = diff.querySelector('.locked-diff-add .locked-diff-text');
    expect(addLine.textContent).toBe('Prefer explicit types in public APIs.');
    expect(diff.querySelectorAll('.locked-diff-add')).toHaveLength(1);
    expect(diff.querySelectorAll('.locked-diff-remove')).toHaveLength(0);
    vi.useRealTimers();
  });

  it('switching meta-specs refreshes the locked-diff baseline — an unmodified draft shows no pending diff (§9 switch → preview)', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-switch-1',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-switch-1/retry' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-switch-1',
      state: 'running',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-switch-1/retry', status: 'running', diff: null },
      ],
    });

    const { findByText, findByLabelText, findByTestId, queryByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });

    // Switch persona-1 → persona-2 in the dropdown WITHOUT editing the
    // draft (the exact §9 iteration sequence the locked-diff tests above
    // never exercised — they only edited the auto-selected first meta-spec).
    const select = await findByLabelText('Persona');
    await fireEvent.change(select, { target: { value: 'persona-2' } });
    const textarea = await findByTestId('persona-textarea');
    await waitFor(() => {
      expect(textarea.value).toBe('You review code for security vulnerabilities.');
    });

    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByTestId('preview-btn'));

    // Regression: the baseline used to stay persona-1's published prompt, so
    // persona-2's unmodified prompt line rendered as a pending '+' line
    // fabricated against persona-1. The locked pane must show the verbatim
    // draft with NO pending-changes diff.
    const locked = await findByTestId('editor-locked');
    expect(locked).toBeTruthy();
    expect(queryByTestId('locked-diff')).toBeNull();
    expect(locked.textContent).toContain('You review code for security vulnerabilities.');
    vi.useRealTimers();
  });

  it('switching meta-specs after a completed preview clears the old run\'s panel (generation reset)', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-switch-2',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-switch-2/retry' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-switch-2',
      state: 'complete',
      agents: [
        {
          agent_id: 'ag-1',
          repo_id: 'repo-1',
          spec_path: 'specs/payment/retry.md',
          branch: 'preview/pv-switch-2/retry',
          status: 'complete',
          diff: { files_changed: 1, insertions: 1, deletions: 0, patches: [{ path: 'src/lib.rs', status: 'modified', patch: '+fn new() {}' }] },
        },
      ],
    });

    const { findByText, findByLabelText, findByTestId, queryByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByText('Preview'));
    await vi.advanceTimersByTimeAsync(2100);
    await findByTestId('preview-complete');

    // Switching meta-specs must reset the preview state (bump the run
    // generation and clear the old run's panel) — the old behavior only
    // stopped the poller, leaving the completed run's panel mounted over
    // the editor while the user edits a different meta-spec.
    const select = await findByLabelText('Persona');
    await fireEvent.change(select, { target: { value: 'persona-2' } });

    await waitFor(() => {
      expect(queryByTestId('preview-complete')).toBeNull();
    });
    const textarea = await findByTestId('persona-textarea');
    await waitFor(() => {
      expect(textarea.value).toBe('You review code for security vulnerabilities.');
    });
    vi.useRealTimers();
  });
});

// ─── Repo scope ───────────────────────────────────────────────────────────────

describe('MetaSpecs -- repo scope', () => {
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

  it('shows workspace link when workspaceId is provided', () => {
    const { getByText } = render(MetaSpecs, { props: { scope: 'repo', workspaceId: 'ws-1' } });
    expect(getByText('View workspace editor')).toBeTruthy();
  });

  it('shows fallback text when workspaceId is not provided', () => {
    const { getByText } = render(MetaSpecs, { props: { scope: 'repo' } });
    expect(getByText(/Select a workspace to edit meta-specs/)).toBeTruthy();
  });
});

// ─── §9 State 1 chat: workspace-scoped LLM assist ────────────────────────────

describe('MetaSpecs -- workspace chat routes to the workspace assist endpoint', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getSpecs.mockResolvedValue([...mockSpecs]);
    api.getMetaSpecs.mockResolvedValue([...mockPersonas]);
  });

  it('sends the in-editor draft to metaSpecsAssist and streams the answer', async () => {
    // Real Response instance: InlineChat streams only genuine Response
    // objects (`result instanceof Response`), so a structural mock would
    // silently exercise nothing.
    api.metaSpecsAssist.mockResolvedValueOnce(
      new Response(
        'data: ' + JSON.stringify({ type: 'partial', text: 'Adding defensive error handling.' }) + '\n\n' +
        'data: ' + JSON.stringify({ type: 'complete', answer: 'Suggested: harden retry paths.' }) + '\n\n',
        { headers: { 'Content-Type': 'text/event-stream' } }
      )
    );

    const { findByLabelText, findByText, queryByText } = render(MetaSpecs, {
      props: { scope: 'workspace', workspaceId: 'ws-1' },
    });

    // loadWorkspaceData selects the first meta-spec as the draft.
    const chatInput = await findByLabelText('Message input');
    await fireEvent.input(chatInput, { target: { value: 'Add error handling' } });
    await fireEvent.keyDown(chatInput, { key: 'Enter', ctrlKey: true });

    // The §9 wire call: the workspace-scoped assist with the in-editor draft
    // and the registry id. Regression guard: this pane previously called a
    // nonexistent api.specsAssistGlobal, so the TypeError was swallowed and
    // the chat always fell back to the canned local suggestion.
    await waitFor(() => {
      expect(api.metaSpecsAssist).toHaveBeenCalledWith('ws-1', {
        instruction: 'Add error handling',
        draft_content: 'You are a backend developer focused on Rust.',
        meta_spec_id: 'persona-1',
      });
    });
    expect(api.specsAssist).not.toHaveBeenCalled();

    // The streamed answer reached the chat history — the assist response was
    // really consumed, and the canned fallback suggestion never fired.
    await findByText('Suggested: harden retry paths.');
    expect(queryByText('+ Add error handling')).toBeNull();
  });
});

// ─── DiffSuggestion integration ───────────────────────────────────────────────

describe('MetaSpecs -- DiffSuggestion accept updates textarea', () => {
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
    expect(acceptBtn).toBeTruthy();
    await fireEvent.click(acceptBtn);
    await waitFor(() => {
      expect(textarea.value.length).toBeGreaterThan(initialValue.length);
    });
  });
});

// ─── Accessibility ────────────────────────────────────────────────────────────

describe('MetaSpecs -- accessibility', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getMetaSpecs.mockResolvedValue([...mockMetaSpecs]);
    api.getSpecs.mockResolvedValue([]);
  });

  it('sidebar has navigation role', async () => {
    const { container, findAllByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Backend Persona');
    const nav = container.querySelector('nav.spec-sidebar');
    expect(nav).toBeTruthy();
    expect(nav.getAttribute('aria-label')).toBe('Meta-specs list');
  });

  it('editor tabs have tablist role', async () => {
    const { container, findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findByText('Edit');
    const tablist = container.querySelector('[role="tablist"]');
    expect(tablist).toBeTruthy();
  });

  it('tab panels have tabpanel role', async () => {
    const { container, findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findByText('Edit');
    const panel = container.querySelector('[role="tabpanel"]');
    expect(panel).toBeTruthy();
  });

  it('filter pills have aria-pressed', async () => {
    const { container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const allPill = container.querySelector('.pill');
    expect(allPill.getAttribute('aria-pressed')).toBe('true');
  });

  it('selected sidebar item has aria-current', async () => {
    const { container, findAllByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await findAllByText('Backend Persona');
    const activeItem = container.querySelector('.sidebar-item.active');
    expect(activeItem.getAttribute('aria-current')).toBe('true');
  });

  it('spec textarea has aria-label', async () => {
    const { findByTestId } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const textarea = await findByTestId('spec-textarea');
    expect(textarea.getAttribute('aria-label')).toBe('Meta-spec content');
  });

  it('workspace view sets aria-busy during loading', () => {
    const { container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    const view = container.querySelector('.workspace-view');
    expect(view.getAttribute('aria-busy')).toBe('true');
  });

  it('kind cards in create panel have aria-pressed', async () => {
    const { findByText, container } = render(MetaSpecs, { props: { scope: 'tenant' } });
    await fireEvent.click(await findByText('+ New Meta-spec'));
    const cards = container.querySelectorAll('.kind-card');
    // First card (Persona) should be selected by default
    expect(cards[0].getAttribute('aria-pressed')).toBe('true');
    expect(cards[1].getAttribute('aria-pressed')).toBe('false');
  });
});

// ─── Agent Rules → Architecture navigation ────────────────────────────────────

describe('MetaSpecs -- Agent Rules → Architecture navigation', () => {
  const metaSpecWithBlast = {
    id: 'ms-arch-1',
    kind: 'meta:principle',
    name: 'No Mocking Principle',
    prompt: 'Never mock databases in tests.',
    approval_status: 'Approved',
    required: false,
    version: 2,
    scope: 'Global',
    created_by: 'human',
  };

  const blastWithRepos = {
    affected_workspaces: [{ id: 'ws-alpha' }],
    affected_repos: [
      { id: 'repo-a', reason: 'direct', workspace_id: 'ws-alpha' },
      { id: 'repo-b', reason: 'transitive', workspace_id: 'ws-alpha' },
    ],
  };

  beforeEach(() => {
    vi.clearAllMocks();
    api.getMetaSpecs.mockResolvedValue([metaSpecWithBlast]);
    api.getMetaSpecBlastRadius.mockResolvedValue(blastWithRepos);
  });

  it('renders arch-nav-links for each affected repo in Impact tab', async () => {
    const { findByRole, findAllByTestId } = render(MetaSpecs, { props: { scope: 'tenant' } });
    // Click Impact tab
    const impactTab = await findByRole('tab', { name: /impact/i });
    await fireEvent.click(impactTab);
    // Wait for blast radius to load
    const links = await findAllByTestId('arch-nav-link');
    expect(links).toHaveLength(2);
  });

  it('arch-nav-link href contains workspace_id and repo id', async () => {
    const { findByRole, findAllByTestId } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const impactTab = await findByRole('tab', { name: /impact/i });
    await fireEvent.click(impactTab);
    const links = await findAllByTestId('arch-nav-link');
    const href = links[0].getAttribute('href');
    expect(href).toContain('/workspaces/ws-alpha/r/repo-a/architecture');
  });

  it('arch-nav-link href includes show_overlays param with metaspec id', async () => {
    const { findByRole, findAllByTestId } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const impactTab = await findByRole('tab', { name: /impact/i });
    await fireEvent.click(impactTab);
    const links = await findAllByTestId('arch-nav-link');
    const href = links[0].getAttribute('href');
    expect(href).toContain('show_overlays=metaspec:ms-arch-1');
  });

  it('arch-nav-link has accessible aria-label', async () => {
    const { findByRole, findAllByTestId } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const impactTab = await findByRole('tab', { name: /impact/i });
    await fireEvent.click(impactTab);
    const links = await findAllByTestId('arch-nav-link');
    expect(links[0].getAttribute('aria-label')).toContain('Architecture');
  });

  it('does not render arch-nav-links when no affected_repos', async () => {
    api.getMetaSpecBlastRadius.mockResolvedValue({ affected_workspaces: [], affected_repos: [] });
    const { findByRole, queryAllByTestId } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const impactTab = await findByRole('tab', { name: /impact/i });
    await fireEvent.click(impactTab);
    // Wait for blast radius to resolve
    await waitFor(() => expect(api.getMetaSpecBlastRadius).toHaveBeenCalled());
    const links = queryAllByTestId('arch-nav-link');
    expect(links).toHaveLength(0);
  });

  it('shows "View in Architecture" section heading when repos exist', async () => {
    const { findByRole, findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const impactTab = await findByRole('tab', { name: /impact/i });
    await fireEvent.click(impactTab);
    const heading = await findByText('View in Architecture');
    expect(heading).toBeTruthy();
  });

  it('shows explanatory subtitle for arch navigation', async () => {
    const { findByRole, findByText } = render(MetaSpecs, { props: { scope: 'tenant' } });
    const impactTab = await findByRole('tab', { name: /impact/i });
    await fireEvent.click(impactTab);
    const text = await findByText(/Click a repo to see governed nodes/i);
    expect(text).toBeTruthy();
  });
});

// ─── Task-206 review repairs: mid-run diffs and shared-path targets ───────────
describe('MetaSpecs -- preview diff viewer updates mid-run (§5 step 4)', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.getSpecs.mockResolvedValue([...mockSpecs]);
    api.getMetaSpecs.mockResolvedValue([...mockPersonas]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders a finished agent\'s diff while the run is still running', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-mid',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-mid/retry' },
        { agent_id: 'ag-2', repo_id: 'repo-2', spec_path: 'specs/system/design.md', branch: 'preview/pv-mid/design' },
      ],
    });
    // state stays 'running': the SECOND agent is still going. The first
    // agent finished and its diff is already present — the viewer must show
    // it NOW, not after the whole run completes.
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-mid',
      state: 'running',
      agents: [
        {
          agent_id: 'ag-1',
          repo_id: 'repo-1',
          spec_path: 'specs/payment/retry.md',
          branch: 'preview/pv-mid/retry',
          status: 'complete',
          diff: {
            files_changed: 1,
            insertions: 1,
            deletions: 0,
            patches: [{ path: 'src/lib.rs', status: 'modified', patch: '+fn mid_run() {}' }],
          },
        },
        { agent_id: 'ag-2', repo_id: 'repo-2', spec_path: 'specs/system/design.md', branch: 'preview/pv-mid/design', status: 'running', diff: null },
      ],
    });

    const { findByText, findByTestId, container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByTestId('preview-btn'));
    await vi.advanceTimersByTimeAsync(2100);

    // Still in the running state...
    expect(await findByTestId('preview-running')).toBeTruthy();
    // ...and the finished agent's diff lines are already rendered, with the
    // running agent still listed in the progress strip.
    await waitFor(() => {
      const lines = container.querySelectorAll('.code-diff-line');
      expect(lines.length).toBeGreaterThan(0);
      expect(lines[0].textContent).toContain('+fn mid_run() {}');
    });
    const progressItems = container.querySelectorAll('.progress-item');
    expect(progressItems.length).toBe(2);
    vi.useRealTimers();
  });

  it('keeps the mid-run viewer hidden while no agent has produced a diff', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-mid-none',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-mid-none/retry' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-mid-none',
      state: 'running',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-1', spec_path: 'specs/payment/retry.md', branch: 'preview/pv-mid-none/retry', status: 'running', diff: null },
      ],
    });

    const { findByText, findByTestId, container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByTestId('preview-btn'));
    await vi.advanceTimersByTimeAsync(2100);

    expect(await findByTestId('preview-running')).toBeTruthy();
    expect(container.querySelectorAll('.code-diff-line').length).toBe(0);
    vi.useRealTimers();
  });
});

describe('MetaSpecs -- shared spec paths across repos are distinct targets', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    // Two workspace repos exposing the SAME spec path: keyed-each blocks
    // that key on the path alone would collide (Svelte keyed-each error)
    // and the two targets would be indistinguishable.
    api.getSpecs.mockResolvedValue([
      { path: 'specs/system/design.md', title: 'Design (repo A)', kind: null, approval_status: 'approved', current_sha: 'a1', repo_id: 'repo-a' },
      { path: 'specs/system/design.md', title: 'Design (repo B)', kind: null, approval_status: 'pending', current_sha: 'b2', repo_id: 'repo-b' },
    ]);
    api.getMetaSpecs.mockResolvedValue([...mockPersonas]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders two distinguishable checklist rows for the shared path', async () => {
    const { container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await waitFor(() => {
      const rows = container.querySelectorAll('.spec-check-item');
      expect(rows.length).toBe(2);
    });
    // Both rows carry the shared path plus a repo discriminator, so the
    // labels are not identical.
    const labels = Array.from(container.querySelectorAll('.spec-check-path')).map((el) => el.textContent);
    expect(labels[0]).not.toBe(labels[1]);
    expect(labels.every((l) => l.includes('specs/system/design.md'))).toBe(true);
    expect(labels.some((l) => l.includes('repo-a'))).toBe(true);
    expect(labels.some((l) => l.includes('repo-b'))).toBe(true);
  });

  it('selects targets independently and spawns only the checked repo', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({ preview_id: 'pv-dup', agents: [] });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-dup',
      state: 'complete',
      agents: [],
    });

    const { container, findByText, findByTestId } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await waitFor(() => {
      expect(container.querySelectorAll('.spec-check-item').length).toBe(2);
    });
    // Check ONLY repo-b's row: the shared path must not select both repos.
    const checkboxes = container.querySelectorAll('.spec-check-item input[type="checkbox"]');
    await fireEvent.click(checkboxes[1]);
    expect(checkboxes[0].checked).toBe(false);
    expect(checkboxes[1].checked).toBe(true);

    await fireEvent.click(await findByTestId('preview-btn'));
    await waitFor(() => {
      expect(api.previewMetaSpec).toHaveBeenCalledTimes(1);
      expect(api.previewMetaSpec).toHaveBeenCalledWith({
        draft: expect.anything(),
        targets: [{ repo_id: 'repo-b', spec_path: 'specs/system/design.md' }],
      });
    });
    vi.useRealTimers();
  });

  it('disambiguates tabs for agents that share a spec stem', async () => {
    vi.useFakeTimers();
    api.previewMetaSpec.mockResolvedValue({
      preview_id: 'pv-tabs',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-a', spec_path: 'specs/system/design.md', branch: 'preview/pv-tabs/system-design' },
        { agent_id: 'ag-2', repo_id: 'repo-b', spec_path: 'docs/design.md', branch: 'preview/pv-tabs/design' },
      ],
    });
    api.previewMetaSpecStatus.mockResolvedValue({
      preview_id: 'pv-tabs',
      state: 'complete',
      agents: [
        { agent_id: 'ag-1', repo_id: 'repo-a', spec_path: 'specs/system/design.md', branch: 'preview/pv-tabs/system-design', status: 'complete', diff: null },
        { agent_id: 'ag-2', repo_id: 'repo-b', spec_path: 'docs/design.md', branch: 'preview/pv-tabs/design', status: 'complete', diff: null },
      ],
    });

    const { findByText, findByTestId, container } = render(MetaSpecs, { props: { scope: 'workspace', workspaceId: 'ws-1' } });
    await fireEvent.click(await findByText('Select All'));
    await fireEvent.click(await findByTestId('preview-btn'));
    await vi.advanceTimersByTimeAsync(2100);
    await findByTestId('preview-complete');

    await waitFor(() => {
      const tabs = Array.from(container.querySelectorAll('.impact-tab')).map((t) => t.textContent);
      expect(tabs.length).toBe(2);
      // Same stem 'design' disambiguated with the parent directory,
      // mirroring the server's preview_branch_name — never two identical
      // labels for two different targets.
      expect(tabs[0]).not.toBe(tabs[1]);
      expect(new Set(tabs).size).toBe(2);
      expect(tabs.some((t) => t.includes('design'))).toBe(true);
    });
    vi.useRealTimers();
  });
});
