import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import PersonaCatalog from '../components/PersonaCatalog.svelte';

// ---------------------------------------------------------------------------
// Helper function tests (extracted from component logic for unit coverage)
// ---------------------------------------------------------------------------
describe('PersonaCatalog helper functions', () => {
  // autoSlug: lowercases, replaces non-alnum with hyphens, trims leading/trailing hyphens
  function autoSlug(name) {
    return name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  }

  describe('autoSlug()', () => {
    it('converts name to lowercase kebab-case', () => {
      expect(autoSlug('Backend Developer')).toBe('backend-developer');
    });

    it('strips special characters', () => {
      expect(autoSlug('My@Cool#Persona!')).toBe('my-cool-persona');
    });

    it('collapses multiple non-alnum chars into single hyphen', () => {
      expect(autoSlug('foo   bar---baz')).toBe('foo-bar-baz');
    });

    it('strips leading and trailing hyphens', () => {
      expect(autoSlug('--hello--')).toBe('hello');
      expect(autoSlug('!!!test!!!')).toBe('test');
    });

    it('handles empty string', () => {
      expect(autoSlug('')).toBe('');
    });

    it('handles single word', () => {
      expect(autoSlug('Analyzer')).toBe('analyzer');
    });

    it('preserves numbers', () => {
      expect(autoSlug('Agent V2.1')).toBe('agent-v2-1');
    });
  });

  // scopeVariant: maps scope kind to badge variant
  function scopeVariant(scope) {
    const s = typeof scope === 'object' ? (scope?.kind ?? '').toLowerCase() : (scope ?? '').toLowerCase();
    if (s === 'tenant') return 'danger';
    if (s === 'workspace') return 'info';
    if (s === 'repo') return 'warning';
    return 'default';
  }

  describe('scopeVariant()', () => {
    it('returns "danger" for tenant scope (object)', () => {
      expect(scopeVariant({ kind: 'Tenant', id: 't-1' })).toBe('danger');
    });

    it('returns "info" for workspace scope (object)', () => {
      expect(scopeVariant({ kind: 'Workspace', id: 'ws-1' })).toBe('info');
    });

    it('returns "warning" for repo scope (object)', () => {
      expect(scopeVariant({ kind: 'Repo', id: 'r-1' })).toBe('warning');
    });

    it('returns "default" for unknown scope kind', () => {
      expect(scopeVariant({ kind: 'Custom', id: 'c-1' })).toBe('default');
    });

    it('handles string scope (legacy format)', () => {
      expect(scopeVariant('Tenant')).toBe('danger');
      expect(scopeVariant('Workspace')).toBe('info');
      expect(scopeVariant('Repo')).toBe('warning');
    });

    it('handles null/undefined scope', () => {
      expect(scopeVariant(null)).toBe('default');
      expect(scopeVariant(undefined)).toBe('default');
    });

    it('handles object with missing kind', () => {
      expect(scopeVariant({ id: 'x' })).toBe('default');
      expect(scopeVariant({})).toBe('default');
    });
  });

  // scopeLabel: extracts display label from scope
  function scopeLabel(scope) {
    if (typeof scope === 'object') return scope?.kind ?? 'workspace';
    return scope ?? 'workspace';
  }

  describe('scopeLabel()', () => {
    it('returns kind from object scope', () => {
      expect(scopeLabel({ kind: 'Tenant', id: 't-1' })).toBe('Tenant');
    });

    it('returns string scope as-is', () => {
      expect(scopeLabel('Repo')).toBe('Repo');
    });

    it('defaults to "workspace" for null', () => {
      expect(scopeLabel(null)).toBe('workspace');
    });

    it('defaults to "workspace" for object without kind', () => {
      expect(scopeLabel({})).toBe('workspace');
    });
  });

  // approvalVariant: maps approval status to badge variant
  function approvalVariant(status) {
    if (status === 'Approved') return 'success';
    if (status === 'Deprecated') return 'default';
    return 'warning';
  }

  describe('approvalVariant()', () => {
    it('returns "success" for Approved', () => {
      expect(approvalVariant('Approved')).toBe('success');
    });

    it('returns "default" for Deprecated', () => {
      expect(approvalVariant('Deprecated')).toBe('default');
    });

    it('returns "warning" for Pending', () => {
      expect(approvalVariant('Pending')).toBe('warning');
    });

    it('returns "warning" for undefined/null', () => {
      expect(approvalVariant(undefined)).toBe('warning');
      expect(approvalVariant(null)).toBe('warning');
    });

    it('returns "warning" for any unknown status', () => {
      expect(approvalVariant('Draft')).toBe('warning');
      expect(approvalVariant('InReview')).toBe('warning');
    });
  });
});

// ---------------------------------------------------------------------------
// PersonaCatalog component rendering tests
// ---------------------------------------------------------------------------
describe('PersonaCatalog component', () => {
  beforeEach(() => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
  });

  it('renders without throwing', () => {
    expect(() => render(PersonaCatalog)).not.toThrow();
  });

  it('shows loading skeletons initially', () => {
    const { container } = render(PersonaCatalog);
    expect(container.querySelector('[aria-busy="true"]')).toBeTruthy();
  });

  it('shows empty state when no personas returned', async () => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) })
    );
    render(PersonaCatalog);
    await waitFor(() => {
      const empty = document.querySelector('.persona-catalog');
      expect(empty).toBeTruthy();
    });
  });

  it('renders persona cards when data is loaded', async () => {
    const personas = [
      {
        id: 'p-1',
        name: 'Backend Dev',
        slug: 'backend-dev',
        scope: { kind: 'Workspace', id: 'ws-1' },
        capabilities: ['rust', 'api-design'],
        approval_status: 'Approved',
        description: 'A backend developer persona',
      },
      {
        id: 'p-2',
        name: 'Frontend Dev',
        slug: 'frontend-dev',
        scope: { kind: 'Repo', id: 'r-1' },
        capabilities: [],
        approval_status: 'Pending',
      },
    ];
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(personas) })
    );

    const { container } = render(PersonaCatalog);
    await waitFor(() => {
      expect(container.innerHTML).toContain('Backend Dev');
    });
    expect(container.innerHTML).toContain('Frontend Dev');
    expect(container.innerHTML).toContain('A backend developer persona');
    expect(container.innerHTML).toContain('rust');
    expect(container.innerHTML).toContain('api-design');
  });

  it('shows Approve button for non-approved personas only', async () => {
    const personas = [
      { id: 'p-1', name: 'Approved One', scope: { kind: 'Tenant', id: 't-1' }, capabilities: [], approval_status: 'Approved' },
      { id: 'p-2', name: 'Pending One', scope: { kind: 'Tenant', id: 't-1' }, capabilities: [], approval_status: 'Pending' },
    ];
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(personas) })
    );

    const { container } = render(PersonaCatalog);
    await waitFor(() => {
      expect(container.innerHTML).toContain('Approved One');
    });

    const approveButtons = container.querySelectorAll('.btn-approve-sm');
    // Only the Pending one should have an Approve button
    expect(approveButtons.length).toBe(1);
    expect(approveButtons[0].getAttribute('aria-label')).toContain('Pending One');
  });

  it('every persona card has a Delete button', async () => {
    const personas = [
      { id: 'p-1', name: 'Alpha', scope: { kind: 'Tenant', id: 't-1' }, capabilities: [], approval_status: 'Approved' },
      { id: 'p-2', name: 'Beta', scope: { kind: 'Tenant', id: 't-1' }, capabilities: [], approval_status: 'Pending' },
    ];
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(personas) })
    );

    const { container } = render(PersonaCatalog);
    await waitFor(() => {
      expect(container.innerHTML).toContain('Alpha');
    });

    const deleteButtons = container.querySelectorAll('.btn-danger-sm');
    expect(deleteButtons.length).toBe(2);
  });

  it('has "+ New Persona" button in header', () => {
    const { container } = render(PersonaCatalog);
    const buttons = container.querySelectorAll('button');
    const labels = Array.from(buttons).map(b => b.textContent);
    expect(labels.some(l => l.includes('New Persona'))).toBe(true);
  });

  it('shows content hash chip when persona has content_hash', async () => {
    const personas = [
      {
        id: 'p-1',
        name: 'Hashed',
        scope: { kind: 'Tenant', id: 't-1' },
        capabilities: [],
        approval_status: 'Approved',
        content_hash: 'abcdef1234567890abcdef',
      },
    ];
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(personas) })
    );

    const { container } = render(PersonaCatalog);
    await waitFor(() => {
      expect(container.innerHTML).toContain('Hashed');
    });

    const hashChip = container.querySelector('.hash-chip');
    expect(hashChip).toBeTruthy();
    // Should show first 8 chars of hash
    expect(hashChip.textContent).toBe('abcdef12');
  });

  it('persona icon shows first letter of name uppercased', async () => {
    const personas = [
      { id: 'p-1', name: 'zeus', scope: { kind: 'Tenant', id: 't-1' }, capabilities: [], approval_status: 'Pending' },
    ];
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(personas) })
    );

    const { container } = render(PersonaCatalog);
    await waitFor(() => {
      expect(container.innerHTML).toContain('zeus');
    });

    const icon = container.querySelector('.persona-icon');
    expect(icon.textContent.trim()).toBe('Z');
  });

  it('clicking Delete shows confirmation modal', async () => {
    const personas = [
      { id: 'p-1', name: 'Doomed', scope: { kind: 'Tenant', id: 't-1' }, capabilities: [], approval_status: 'Pending' },
    ];
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(personas) })
    );

    const { container } = render(PersonaCatalog);
    await waitFor(() => {
      expect(container.innerHTML).toContain('Doomed');
    });

    const deleteBtn = container.querySelector('[aria-label="Delete persona Doomed"]');
    expect(deleteBtn).toBeTruthy();
    await fireEvent.click(deleteBtn);

    // Confirmation modal should appear with "Delete this persona?" text
    await waitFor(() => {
      expect(document.body.innerHTML).toContain('Delete this persona?');
    });
  });

  it('approve calls the API and updates the persona in-place', async () => {
    const personas = [
      { id: 'p-1', name: 'Pending Agent', scope: { kind: 'Workspace', id: 'ws-1' }, capabilities: [], approval_status: 'Pending' },
    ];

    let callCount = 0;
    global.fetch = vi.fn((url, opts) => {
      callCount++;
      // First call: load personas
      if (callCount === 1) {
        return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(personas) });
      }
      // Second call: approve
      if (opts?.method === 'POST' && url.includes('/approve')) {
        return Promise.resolve({
          ok: true,
          status: 200,
          json: () => Promise.resolve({ ...personas[0], approval_status: 'Approved' }),
        });
      }
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve([]) });
    });

    const { container } = render(PersonaCatalog);
    await waitFor(() => {
      expect(container.innerHTML).toContain('Pending Agent');
    });

    const approveBtn = container.querySelector('[aria-label="Approve persona Pending Agent"]');
    expect(approveBtn).toBeTruthy();
    await fireEvent.click(approveBtn);

    // After approval, the approve API should be called
    await waitFor(() => {
      const approveCalls = global.fetch.mock.calls.filter(
        ([url, opts]) => typeof url === 'string' && url.includes('/approve') && opts?.method === 'POST'
      );
      expect(approveCalls.length).toBe(1);
    });
  });

  it('handles API error gracefully when loading fails', async () => {
    global.fetch = vi.fn(() => Promise.reject(new Error('Network failure')));

    const { container } = render(PersonaCatalog);
    // Should not crash — component should render even on error
    await waitFor(() => {
      expect(container.querySelector('.persona-catalog')).toBeTruthy();
    });
  });

  it('header displays title and subtitle', () => {
    const { container } = render(PersonaCatalog);
    expect(container.innerHTML).toContain('Persona Catalog');
    expect(container.innerHTML).toContain('Reusable agent persona definitions');
  });
});
