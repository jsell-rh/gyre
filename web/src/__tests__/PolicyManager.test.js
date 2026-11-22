/**
 * PolicyManager.test.js — component tests for the ABAC policy management
 * UI (abac-policy-engine.md §UI, task-132).
 *
 * Covers the manager (scope-filtered CRUD + conflict detection on the
 * GLOBAL /api/v1/policies endpoints — never workspace-prefixed URLs), the
 * policy editor's visual condition builder, the dry-run simulator, the
 * decision audit log filters, and the effective permissions view.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    policies: vi.fn().mockResolvedValue([]),
    createPolicy: vi.fn().mockResolvedValue({ id: 'p-new' }),
    updatePolicy: vi.fn().mockResolvedValue({}),
    deletePolicy: vi.fn().mockResolvedValue(null),
    policyDecisions: vi.fn().mockResolvedValue([]),
    evaluatePolicy: vi.fn().mockResolvedValue({}),
    effectivePermissions: vi.fn().mockResolvedValue([]),
  },
  setAuthToken: vi.fn(),
}));

import { api } from '../lib/api.js';
import PolicyManager from '../components/abac/PolicyManager.svelte';

const mockPolicies = [
  {
    id: 'p-allow',
    name: 'allow-developer-push',
    description: 'Developers may push',
    scope: 'workspace',
    scope_id: 'ws-1',
    priority: 100,
    effect: 'allow',
    conditions: [{ attribute: 'subject.workspace_role', operator: 'equals', value: 'Developer' }],
    actions: ['push'],
    resource_types: ['repo'],
    enabled: true,
    built_in: false,
    immutable: false,
  },
  {
    id: 'p-deny',
    name: 'deny-guest-push',
    description: 'Guests may not push',
    scope: 'workspace',
    scope_id: 'ws-1',
    priority: 200,
    effect: 'deny',
    conditions: [],
    actions: ['push', 'read'],
    resource_types: ['repo'],
    enabled: true,
    built_in: false,
    immutable: false,
  },
  {
    id: 'p-builtin',
    name: 'builtin:require-human-spec-approval',
    scope: 'tenant',
    scope_id: null,
    priority: 999,
    effect: 'deny',
    conditions: [],
    actions: ['approve'],
    resource_types: ['spec'],
    enabled: true,
    built_in: true,
    immutable: true,
  },
];

describe('PolicyManager', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.policies.mockResolvedValue([]);
  });

  it('renders the scope filter and policy table shell', async () => {
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-manager"]')).toBeTruthy();
    });
    expect(container.querySelector('[data-testid="policy-scope-filter"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="policy-scope-select"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="policy-scope-id-input"]')).toBeTruthy();
    // Empty scope → empty state (the table renders once policies load).
    expect(container.querySelector('[data-testid="policy-list-empty"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="create-policy-btn"]')).toBeTruthy();
  });

  it('loads policies from the GLOBAL /policies endpoint with scope query params', async () => {
    render(PolicyManager, { props: { initialScope: 'workspace', initialScopeId: 'ws-1', lockScope: true } });
    await waitFor(() => {
      expect(api.policies).toHaveBeenCalledWith({ scope: 'Workspace', scope_id: 'ws-1' });
    });
  });

  it('omits scope_id when blank (all entities at the scope)', async () => {
    render(PolicyManager, { props: { initialScope: 'tenant' } });
    await waitFor(() => {
      expect(api.policies).toHaveBeenCalledWith({ scope: 'Tenant' });
    });
  });

  it('renders policy rows with name, scope, effect, priority, enabled', async () => {
    api.policies.mockResolvedValue(mockPolicies);
    const { container } = render(PolicyManager, { props: { initialScope: 'workspace', initialScopeId: 'ws-1' } });
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="policy-row"]').length).toBe(3);
    });
    const text = container.querySelector('[data-testid="policy-table"]').textContent;
    expect(text).toContain('allow-developer-push');
    expect(text).toContain('deny-guest-push');
    expect(text).toContain('workspace:ws-1');
    expect(text).toContain('200');
  });

  it('changing the scope select reloads with the new scope filter', async () => {
    const { container } = render(PolicyManager, { props: { initialScope: 'tenant' } });
    await waitFor(() => {
      expect(api.policies).toHaveBeenCalledWith({ scope: 'Tenant' });
    });
    const select = container.querySelector('[data-testid="policy-scope-select"]');
    await fireEvent.change(select, { target: { value: 'repo' } });
    await waitFor(() => {
      expect(api.policies).toHaveBeenCalledWith({ scope: 'Repo' });
    });
  });

  it('locks the scope controls when lockScope is set', async () => {
    const { container } = render(PolicyManager, { props: { initialScope: 'workspace', initialScopeId: 'ws-1', lockScope: true } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-scope-select"]').disabled).toBe(true);
    });
    expect(container.querySelector('[data-testid="policy-scope-id-input"]').disabled).toBe(true);
  });

  it('disables edit/delete on server-managed policies (built_in / immutable / reserved prefixes)', async () => {
    api.policies.mockResolvedValue(mockPolicies);
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="policy-row"]').length).toBe(3);
    });
    const rows = container.querySelectorAll('[data-testid="policy-row"]');
    const builtinRow = rows[2];
    expect(builtinRow.querySelector('[data-testid="edit-policy-btn"]').disabled).toBe(true);
    expect(builtinRow.querySelector('[data-testid="delete-policy-btn"]').disabled).toBe(true);
    const userRow = rows[0];
    expect(userRow.querySelector('[data-testid="edit-policy-btn"]').disabled).toBe(false);
    expect(userRow.querySelector('[data-testid="delete-policy-btn"]').disabled).toBe(false);
  });

  it('delete calls the global DELETE /policies/{id} endpoint and reloads', async () => {
    api.policies.mockResolvedValue([mockPolicies[0]]);
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="policy-row"]').length).toBe(1);
    });
    await fireEvent.click(container.querySelector('[data-testid="delete-policy-btn"]'));
    await waitFor(() => {
      expect(api.deletePolicy).toHaveBeenCalledWith('p-allow');
    });
    // Reload after delete.
    await waitFor(() => {
      expect(api.policies.mock.calls.length).toBeGreaterThanOrEqual(2);
    });
  });

  it('highlights conflicting policies with an explanation (§UI conflict detector)', async () => {
    // allow vs deny, both push on repo at workspace:ws-1 — overlapping.
    api.policies.mockResolvedValue([mockPolicies[0], mockPolicies[1]]);
    const { container } = render(PolicyManager, { props: { initialScope: 'workspace', initialScopeId: 'ws-1' } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-conflicts"]')).toBeTruthy();
    });
    const item = container.querySelector('[data-testid="policy-conflict-item"]');
    expect(item.textContent).toContain('allow-developer-push');
    expect(item.textContent).toContain('deny-guest-push');
    expect(item.textContent).toContain('push');
    expect(item.textContent).toContain('repo');
    // Both involved rows carry the conflict highlight class.
    const rows = container.querySelectorAll('[data-testid="policy-row"]');
    expect(rows[0].classList.contains('conflict-row')).toBe(true);
    expect(rows[1].classList.contains('conflict-row')).toBe(true);
  });

  it('shows no conflict panel when policies do not overlap', async () => {
    api.policies.mockResolvedValue([mockPolicies[0], mockPolicies[2]]);
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="policy-row"]').length).toBe(2);
    });
    expect(container.querySelector('[data-testid="policy-conflicts"]')).toBeNull();
  });

  it('delete failure surfaces an error', async () => {
    api.policies.mockResolvedValue([mockPolicies[0]]);
    api.deletePolicy.mockRejectedValue(new Error('API /policies/p-allow: 400 Bad Request'));
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="policy-row"]').length).toBe(1);
    });
    await fireEvent.click(container.querySelector('[data-testid="delete-policy-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-delete-error"]').textContent)
        .toContain('400');
    });
  });
});

describe('PolicyManager — policy editor (condition builder)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.policies.mockResolvedValue([]);
  });

  it('create saves via POST /policies with the built conditions', async () => {
    api.policies.mockResolvedValue([]);
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="create-policy-btn"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="create-policy-btn"]'));

    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-editor"]')).toBeTruthy();
    });

    await fireEvent.input(container.querySelector('[data-testid="policy-name-input"]'), {
      target: { value: 'allow-viewer-read' },
    });
    await fireEvent.input(container.querySelector('[data-testid="policy-priority-input"]'), {
      target: { value: '150' },
    });
    await fireEvent.change(container.querySelector('[data-testid="policy-effect-select"]'), {
      target: { value: 'allow' },
    });
    await fireEvent.input(container.querySelector('[data-testid="policy-actions-input"]'), {
      target: { value: 'read' },
    });
    await fireEvent.input(container.querySelector('[data-testid="policy-resource-types-input"]'), {
      target: { value: 'spec, task' },
    });

    // Visual condition builder: add a row, pick attribute + operator + value.
    await fireEvent.click(container.querySelector('[data-testid="add-condition-btn"]'));
    const rows = container.querySelectorAll('[data-testid="condition-row"]');
    expect(rows.length).toBe(1);
    await fireEvent.input(rows[0].querySelector('[data-testid="condition-attribute-input"]'), {
      target: { value: 'subject.workspace_role' },
    });
    await fireEvent.change(rows[0].querySelector('[data-testid="condition-operator-select"]'), {
      target: { value: 'equals' },
    });
    await fireEvent.input(rows[0].querySelector('[data-testid="condition-value-input"]'), {
      target: { value: 'Viewer' },
    });

    // Attribute autocomplete: the catalog datalist must exist and offer
    // the spec's attribute names as option values.
    const datalist = container.querySelector('datalist');
    expect(datalist).toBeTruthy();
    expect(datalist.querySelector('option[value="subject.workspace_role"]')).toBeTruthy();
    expect(datalist.querySelector('option[value="resource.type"]')).toBeTruthy();
    expect(datalist.querySelector('option[value="env.budget_remaining"]')).toBeTruthy();

    await fireEvent.click(container.querySelector('[data-testid="policy-editor-save"]'));

    await waitFor(() => {
      expect(api.createPolicy).toHaveBeenCalledTimes(1);
    });
    const payload = api.createPolicy.mock.calls[0][0];
    expect(payload.name).toBe('allow-viewer-read');
    expect(payload.priority).toBe(150);
    expect(payload.effect).toBe('allow');
    expect(payload.actions).toEqual(['read']);
    expect(payload.resource_types).toEqual(['spec', 'task']);
    expect(payload.conditions).toEqual([
      { attribute: 'subject.workspace_role', operator: 'equals', value: 'Viewer' },
    ]);
  });

  it('condition rows can be added and removed', async () => {
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="create-policy-btn"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="create-policy-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-editor"]')).toBeTruthy();
    });

    await fireEvent.click(container.querySelector('[data-testid="add-condition-btn"]'));
    await fireEvent.click(container.querySelector('[data-testid="add-condition-btn"]'));
    expect(container.querySelectorAll('[data-testid="condition-row"]').length).toBe(2);

    await fireEvent.click(container.querySelectorAll('[data-testid="remove-condition-btn"]')[0]);
    expect(container.querySelectorAll('[data-testid="condition-row"]').length).toBe(1);
  });

  it('in-list conditions serialize to a string array; numeric comparisons to numbers', async () => {
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="create-policy-btn"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="create-policy-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-editor"]')).toBeTruthy();
    });
    await fireEvent.input(container.querySelector('[data-testid="policy-name-input"]'), {
      target: { value: 'roles-in' },
    });

    await fireEvent.click(container.querySelector('[data-testid="add-condition-btn"]'));
    let row = container.querySelector('[data-testid="condition-row"]');
    await fireEvent.input(row.querySelector('[data-testid="condition-attribute-input"]'), {
      target: { value: 'subject.workspace_role' },
    });
    await fireEvent.change(row.querySelector('[data-testid="condition-operator-select"]'), {
      target: { value: 'in' },
    });
    await fireEvent.input(row.querySelector('[data-testid="condition-value-input"]'), {
      target: { value: 'Developer, Admin' },
    });

    await fireEvent.click(container.querySelector('[data-testid="add-condition-btn"]'));
    const rows = container.querySelectorAll('[data-testid="condition-row"]');
    await fireEvent.input(rows[1].querySelector('[data-testid="condition-attribute-input"]'), {
      target: { value: 'subject.chain_depth' },
    });
    await fireEvent.change(rows[1].querySelector('[data-testid="condition-operator-select"]'), {
      target: { value: 'greater_than' },
    });
    await fireEvent.input(rows[1].querySelector('[data-testid="condition-value-input"]'), {
      target: { value: '5' },
    });

    await fireEvent.click(container.querySelector('[data-testid="policy-editor-save"]'));
    await waitFor(() => {
      expect(api.createPolicy).toHaveBeenCalledTimes(1);
    });
    const conditions = api.createPolicy.mock.calls[0][0].conditions;
    expect(conditions[0]).toEqual({
      attribute: 'subject.workspace_role',
      operator: 'in',
      value: ['Developer', 'Admin'],
    });
    expect(conditions[1]).toEqual({
      attribute: 'subject.chain_depth',
      operator: 'greater_than',
      value: 5,
    });
  });

  it('edit loads the policy into the form and saves via PUT /policies/{id}', async () => {
    api.policies.mockResolvedValue([mockPolicies[0]]);
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="policy-row"]').length).toBe(1);
    });
    await fireEvent.click(container.querySelector('[data-testid="edit-policy-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-editor"]')).toBeTruthy();
    });

    // Form is prefilled from the policy.
    expect(container.querySelector('[data-testid="policy-name-input"]').value).toBe('allow-developer-push');
    expect(container.querySelector('[data-testid="policy-priority-input"]').value).toBe('100');
    const condRows = container.querySelectorAll('[data-testid="condition-row"]');
    expect(condRows.length).toBe(1);
    expect(condRows[0].querySelector('[data-testid="condition-attribute-input"]').value)
      .toBe('subject.workspace_role');

    // Change the priority and save.
    await fireEvent.input(container.querySelector('[data-testid="policy-priority-input"]'), {
      target: { value: '250' },
    });
    await fireEvent.click(container.querySelector('[data-testid="policy-editor-save"]'));

    await waitFor(() => {
      expect(api.updatePolicy).toHaveBeenCalledWith('p-allow', expect.objectContaining({ priority: 250 }));
    });
    expect(api.createPolicy).not.toHaveBeenCalled();
  });
});

describe('PolicyManager — dry-run simulator', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.policies.mockResolvedValue([]);
  });

  it('submits to POST /policies/evaluate and displays the decision', async () => {
    api.evaluatePolicy.mockResolvedValue({
      decision: 'deny',
      matched_policy: 'builtin-default-deny',
      evaluated_policies: 7,
      evaluation_ms: 0.42,
    });
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-view-tab-simulator"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="policy-view-tab-simulator"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-simulator"]')).toBeTruthy();
    });

    await fireEvent.change(container.querySelector('[data-testid="simulator-subject-type"]'), {
      target: { value: 'agent' },
    });
    await fireEvent.input(container.querySelector('[data-testid="simulator-subject-id"]'), {
      target: { value: 'agent:worker-42' },
    });
    await fireEvent.change(container.querySelector('[data-testid="simulator-action"]'), {
      target: { value: 'push' },
    });
    await fireEvent.change(container.querySelector('[data-testid="simulator-resource-type"]'), {
      target: { value: 'repo' },
    });
    await fireEvent.input(container.querySelector('[data-testid="simulator-resource-workspace-id"]'), {
      target: { value: 'ws-1' },
    });

    await fireEvent.click(container.querySelector('[data-testid="simulator-run-btn"]'));

    await waitFor(() => {
      expect(api.evaluatePolicy).toHaveBeenCalledWith({
        subject: { type: 'agent', id: 'agent:worker-42' },
        action: 'push',
        resource: { type: 'repo', workspace_id: 'ws-1' },
      });
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="simulator-result"]')).toBeTruthy();
    });
    expect(container.querySelector('[data-testid="simulator-decision"]').textContent).toBe('Deny');
    expect(container.querySelector('[data-testid="simulator-matched-policy"]').textContent)
      .toContain('builtin-default-deny');
    expect(container.querySelector('[data-testid="simulator-evaluated-count"]').textContent).toBe('7');
    expect(container.querySelector('[data-testid="simulator-evaluation-ms"]').textContent).toContain('0.42');
  });
});

describe('PolicyManager — decision audit log', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.policies.mockResolvedValue([]);
  });

  it('loads GET /policies/decisions and renders the filterable table', async () => {
    api.policyDecisions.mockResolvedValue([
      {
        request_id: 'r-1',
        subject_id: 'user:alice',
        subject_type: 'user',
        action: 'push',
        resource_type: 'repo',
        resource_id: 'repo-1',
        decision: 'allow',
        matched_policy: 'allow-developer-push',
        evaluated_at: 1730000000,
      },
      {
        request_id: 'r-2',
        subject_id: 'agent:worker-42',
        subject_type: 'agent',
        action: 'push',
        resource_type: 'repo',
        resource_id: 'repo-2',
        decision: 'deny',
        matched_policy: 'builtin-default-deny',
        evaluated_at: 1730000100,
      },
    ]);
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-view-tab-decisions"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="policy-view-tab-decisions"]'));
    await waitFor(() => {
      expect(api.policyDecisions).toHaveBeenCalledWith({ limit: 200 });
    });
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="decision-row"]').length).toBe(2);
    });

    const table = container.querySelector('[data-testid="decision-log-table"]');
    expect(table.textContent).toContain('user:alice');
    expect(table.textContent).toContain('deny');
    expect(table.textContent).toContain('builtin-default-deny');

    // Decision filter (client-side allow/deny).
    await fireEvent.change(container.querySelector('[data-testid="decision-filter-decision"]'), {
      target: { value: 'deny' },
    });
    await waitFor(() => {
      expect(container.querySelectorAll('[data-testid="decision-row"]').length).toBe(1);
    });
    expect(container.querySelector('[data-testid="decision-log-table"]').textContent)
      .toContain('agent:worker-42');

    // Date range filter: from-date after both records empties the table.
    const fromInput = container.querySelector('[data-testid="decision-filter-from"]');
    await fireEvent.input(fromInput, { target: { value: '2100-01-01' } });
    await fireEvent.change(fromInput, { target: { value: '2100-01-01' } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="decision-log-empty"]')).toBeTruthy();
    });
  });

  it('passes server-side subject_id / resource_type filters on Apply', async () => {
    api.policyDecisions.mockResolvedValue([]);
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-view-tab-decisions"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="policy-view-tab-decisions"]'));
    await waitFor(() => {
      expect(api.policyDecisions).toHaveBeenCalled();
    });

    await fireEvent.input(container.querySelector('[data-testid="decision-filter-subject-id"]'), {
      target: { value: 'user:alice' },
    });
    await fireEvent.input(container.querySelector('[data-testid="decision-filter-resource-type"]'), {
      target: { value: 'repo' },
    });
    await fireEvent.click(container.querySelector('[data-testid="decision-apply-filters"]'));

    await waitFor(() => {
      expect(api.policyDecisions).toHaveBeenCalledWith({
        limit: 200,
        subject_id: 'user:alice',
        resource_type: 'repo',
      });
    });
  });
});

describe('PolicyManager — effective permissions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.policies.mockResolvedValue([]);
  });

  it('queries /policies/effective per resource type and renders the view', async () => {
    api.effectivePermissions.mockImplementation(({ resource_type }) =>
      Promise.resolve([
        { action: 'read', resource_type, decision: 'allow', matched_policy: 'allow-developer-read' },
        { action: 'push', resource_type, decision: 'deny', matched_policy: null },
      ])
    );
    const { container } = render(PolicyManager);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="policy-view-tab-effective"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="policy-view-tab-effective"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="effective-permissions"]')).toBeTruthy();
    });

    await fireEvent.input(container.querySelector('[data-testid="effective-subject-input"]'), {
      target: { value: 'user:alice' },
    });
    await fireEvent.click(container.querySelector('[data-testid="effective-query-btn"]'));

    // One request per known resource type, each with the documented
    // ?subject= spelling on the GLOBAL endpoint.
    await waitFor(() => {
      const calls = api.effectivePermissions.mock.calls;
      expect(calls.length).toBeGreaterThanOrEqual(5);
      for (const call of calls) {
        expect(call[0].subject).toBe('user:alice');
        expect(typeof call[0].resource_type).toBe('string');
      }
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="effective-permissions-table"]')).toBeTruthy();
    });
    const text = container.querySelector('[data-testid="effective-permissions-table"]').textContent;
    expect(text).toContain('allow');
    expect(text).toContain('allow-developer-read');
    expect(text).toContain('default deny');
    expect(container.querySelector('[data-testid="effective-summary"]').textContent)
      .toContain('allowed');
  });
});
