/**
 * MoldableViews.test.js — system-explorer.md §2 "Moldable Views".
 *
 * Covers the four tailored detail views (Type / Trait / Endpoint / Spec),
 * the entity-type routing in NodeDetailPanel, and the pure computation
 * layer in lib/moldable/model.js. The model functions are imported
 * directly (single implementation — no mirrored logic in tests).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import MoldableTypeView from '../lib/moldable/MoldableTypeView.svelte';
import MoldableTraitView from '../lib/moldable/MoldableTraitView.svelte';
import MoldableEndpointView from '../lib/moldable/MoldableEndpointView.svelte';
import MoldableSpecView from '../lib/moldable/MoldableSpecView.svelte';
import NodeDetailPanel from '../lib/NodeDetailPanel.svelte';
import {
  moldableViewType,
  computeTraitDependents,
  computeRelationships,
  computeEndpointMeta,
  computeSpecCompleteness,
  computeSpecGovernedNodes,
} from '../lib/moldable/model.js';

const specContent = vi.fn();
const specHistoryRepo = vi.fn();
const getSpecAssertionResults = vi.fn();

vi.mock('../lib/api.js', () => ({
  api: {
    specContent: (...args) => specContent(...args),
    specHistoryRepo: (...args) => specHistoryRepo(...args),
    getSpecAssertionResults: (...args) => getSpecAssertionResults(...args),
  },
}));

beforeEach(() => {
  specContent.mockReset();
  specHistoryRepo.mockReset();
  getSpecAssertionResults.mockReset();
});

// ── Fixtures ────────────────────────────────────────────────────────────────

const TYPE_NODE = {
  id: 'n1', node_type: 'type', name: 'User', qualified_name: 'domain::User',
  file_path: 'domain/models.rs', line_start: 10, line_end: 30, visibility: 'public',
  spec_path: 'specs/platform-model.md', spec_confidence: 'high',
  last_modified_by: 'agent-1', last_modified_at: 1711000000, created_at: 1710000000,
  created_sha: 'abc1234', last_modified_sha: 'def5678', churn_count_30d: 3,
  test_coverage: 0.94, complexity: 8, test_node: false,
};
const STATUS_FIELD = {
  // Real extractor convention (rust_extractor.rs): field qualified_name is
  // `Parent::field_name` and the TYPE annotation lives in doc_comment.
  id: 'f1', node_type: 'field', name: 'status', qualified_name: 'domain::User::status',
  doc_comment: 'TaskStatus', visibility: 'public', test_node: false,
};
const TASK_STATUS_TYPE = {
  id: 'n5', node_type: 'type', name: 'TaskStatus', qualified_name: 'domain::TaskStatus',
  visibility: 'public', test_node: false,
};
const TRAIT_NODE = {
  id: 'n2', node_type: 'interface', name: 'TaskPort', qualified_name: 'ports::TaskPort',
  visibility: 'public', test_node: false,
};
const HANDLER_FN = {
  id: 'n4', node_type: 'function', name: 'handle_create_user',
  qualified_name: 'api::handle_create_user', file_path: 'api/users.rs', line_start: 42,
  visibility: 'public', test_node: false,
};
const TEST_FN = {
  id: 'n3', node_type: 'function', name: 'test_create_user',
  qualified_name: 'tests::test_create_user', visibility: 'public', test_node: true,
};

const TYPE_NODES = [TYPE_NODE, STATUS_FIELD, TASK_STATUS_TYPE, TRAIT_NODE, HANDLER_FN, TEST_FN];
const TYPE_EDGES = [
  { id: 'e1', source_id: 'f1', target_id: 'n1', edge_type: 'field_of' },
  { id: 'e2', source_id: 'n3', target_id: 'n1', edge_type: 'calls' },
  { id: 'e3', source_id: 'n1', target_id: 'n2', edge_type: 'implements' },
  { id: 'e4', source_id: 'n4', target_id: 'n1', edge_type: 'calls' },
  // Extractor convention: field → its type node (DependsOn)
  { id: 'e5', source_id: 'f1', target_id: 'n5', edge_type: 'depends_on' },
];

const TRAIT_VIEW_NODE = {
  id: 't1', node_type: 'interface', name: 'TaskPort', qualified_name: 'ports::TaskPort',
  file_path: 'ports/task.rs', line_start: 1, line_end: 30, visibility: 'public',
  spec_path: 'specs/architecture.md', test_node: false,
};
const METHODS = [
  { id: 'm1', node_type: 'function', name: 'find_by_id', qualified_name: 'ports::TaskPort::find_by_id', doc_comment: 'fn find_by_id(&self, id: Id) -> Option<Task>', visibility: 'public', test_node: false },
  { id: 'm2', node_type: 'function', name: 'save', qualified_name: 'ports::TaskPort::save', doc_comment: 'fn save(&self, task: Task) -> Result', visibility: 'public', test_node: false },
  { id: 'm3', node_type: 'function', name: 'delete', qualified_name: 'ports::TaskPort::delete', doc_comment: 'fn delete(&self, id: Id) -> Result', visibility: 'public', test_node: false },
];
const IMPLS = [
  { id: 'i1', node_type: 'type', name: 'MemTaskRepo', qualified_name: 'adapters::mem::MemTaskRepo', visibility: 'public', test_node: false },
  { id: 'i2', node_type: 'type', name: 'SqliteTaskRepo', qualified_name: 'adapters::sqlite::SqliteTaskRepo', visibility: 'public', test_node: false },
];
const CALLER_X1 = { id: 'x1', node_type: 'function', name: 'run_import', qualified_name: 'server::run_import', visibility: 'public', test_node: false };
const CALLER_X2 = { id: 'x2', node_type: 'function', name: 'sync_board', qualified_name: 'domain::sync_board', visibility: 'public', test_node: false };
const TRAIT_NODES = [TRAIT_VIEW_NODE, ...METHODS, ...IMPLS, CALLER_X1, CALLER_X2];
const TRAIT_EDGES = [
  { id: 'te1', source_id: 't1', target_id: 'm1', edge_type: 'contains' },
  { id: 'te2', source_id: 't1', target_id: 'm2', edge_type: 'contains' },
  { id: 'te3', source_id: 't1', target_id: 'm3', edge_type: 'contains' },
  { id: 'te4', source_id: 'i1', target_id: 't1', edge_type: 'implements' },
  { id: 'te5', source_id: 'i2', target_id: 't1', edge_type: 'implements' },
  // x1 calls the trait directly once and find_by_id once → 2 call sites
  { id: 'tc1', source_id: 'x1', target_id: 't1', edge_type: 'calls' },
  { id: 'tc2', source_id: 'x1', target_id: 'm1', edge_type: 'calls' },
  // x2 calls find_by_id and save → 2 call sites
  { id: 'tc3', source_id: 'x2', target_id: 'm1', edge_type: 'calls' },
  { id: 'tc4', source_id: 'x2', target_id: 'm2', edge_type: 'calls' },
];

const ENDPOINT_NODE = {
  id: 'ep1', node_type: 'endpoint', name: 'api_v1_agents_spawn',
  qualified_name: 'api::api_v1_agents_spawn', file_path: 'api/spawn.rs', line_start: 40,
  visibility: 'public', spec_path: 'specs/platform-model.md', test_node: false,
  doc_comment: 'POST /api/v1/agents/spawn [RequireAdmin]',
};
const SPAWN_HANDLER = {
  id: 'h1', node_type: 'function', name: 'handle_spawn', qualified_name: 'api::handle_spawn',
  file_path: 'api/spawn.rs', line_start: 42, visibility: 'public', test_node: false,
};
const AUTH_MW = { id: 'h2', node_type: 'function', name: 'auth_check', qualified_name: 'api::auth_check', visibility: 'public', test_node: false };
const DOMAIN_FN = { id: 'h3', node_type: 'function', name: 'create_agent', qualified_name: 'domain::create_agent', visibility: 'public', test_node: false };
const REQ_TYPE = { id: 'rt', node_type: 'type', name: 'SpawnRequest', qualified_name: 'api::SpawnRequest', visibility: 'public', test_node: false };
const RESP_TYPE = { id: 'rst', node_type: 'type', name: 'SpawnResponse', qualified_name: 'api::SpawnResponse', visibility: 'public', test_node: false };
const EP_TEST = { id: 'et1', node_type: 'function', name: 'test_spawn', qualified_name: 'tests::test_spawn', visibility: 'public', test_node: true };
const ENDPOINT_NODES = [ENDPOINT_NODE, SPAWN_HANDLER, AUTH_MW, DOMAIN_FN, REQ_TYPE, RESP_TYPE, EP_TEST];
const ENDPOINT_EDGES = [
  // Route path + method live in the RoutesTo edge metadata (extractor convention)
  { id: 'ee1', source_id: 'ep1', target_id: 'h1', edge_type: 'routes_to', metadata: '{"path":"/api/v1/agents/spawn","method":"POST"}' },
  { id: 'ee2', source_id: 'h1', target_id: 'h2', edge_type: 'calls' },
  { id: 'ee3', source_id: 'h1', target_id: 'h3', edge_type: 'calls' },
  { id: 'ee4', source_id: 'h1', target_id: 'rt', edge_type: 'calls' },
  { id: 'ee5', source_id: 'h1', target_id: 'rst', edge_type: 'calls' },
  { id: 'ee6', source_id: 'et1', target_id: 'h1', edge_type: 'calls' },
];

const SPEC_NODE = {
  id: 's1', node_type: 'spec', name: 'specs/system/search.md',
  qualified_name: 'specs/system/search.md', spec_path: 'specs/system/search.md',
  visibility: 'public', test_node: false,
};
const GOV_SERVICE = {
  id: 'g1', node_type: 'module', name: 'SearchService', qualified_name: 'domain::search::SearchService',
  spec_path: 'specs/system/search.md', test_coverage: 0.8, visibility: 'public', test_node: false,
};
const GOV_BAR = {
  id: 'g2', node_type: 'component', name: 'SearchBar.svelte', qualified_name: 'web::SearchBar',
  visibility: 'public', test_node: false,
};
const GOV_REMOVED = {
  id: 'g3', node_type: 'type', name: 'OldIndex', qualified_name: 'domain::OldIndex',
  spec_path: 'specs/system/search.md', visibility: 'public', test_node: false, deleted_at: 1712000000,
};
const SPEC_NODES = [SPEC_NODE, GOV_SERVICE, GOV_BAR, GOV_REMOVED];
const SPEC_EDGES = [
  { id: 'se1', source_id: 'g2', target_id: 's1', edge_type: 'governed_by' },
];

// ── model.js — pure computation (imports the real implementation) ──────────

describe('moldable/model.js', () => {
  it('routes node types to moldable views', () => {
    expect(moldableViewType('type')).toBe('type');
    expect(moldableViewType('table')).toBe('type');
    expect(moldableViewType('component')).toBe('type');
    expect(moldableViewType('interface')).toBe('trait');
    expect(moldableViewType('trait')).toBe('trait');
    expect(moldableViewType('endpoint')).toBe('endpoint');
    expect(moldableViewType('spec')).toBe('spec');
    expect(moldableViewType('function')).toBeNull();
    expect(moldableViewType('module')).toBeNull();
  });

  it('computes trait dependents with call-site counts (callers of methods count)', () => {
    const rels = computeRelationships(TRAIT_VIEW_NODE, TRAIT_NODES, TRAIT_EDGES);
    const dependents = computeTraitDependents(TRAIT_VIEW_NODE, rels, TRAIT_EDGES, TRAIT_NODES);
    const byName = Object.fromEntries(dependents.map(d => [d.node.name, d.calls]));
    expect(byName['run_import']).toBe(2); // direct trait call + find_by_id
    expect(byName['sync_board']).toBe(2); // find_by_id + save
    expect(dependents.length).toBe(2);
  });

  it('extracts endpoint route method + path from RoutesTo edge metadata', () => {
    const meta = computeEndpointMeta(ENDPOINT_NODE, ENDPOINT_NODES, ENDPOINT_EDGES);
    expect(meta.method).toBe('POST');
    expect(meta.path).toBe('/api/v1/agents/spawn');
    expect(meta.routesTo.map(h => h.name)).toContain('handle_spawn');
  });

  it('falls back to doc_comment when no edge metadata carries the route', () => {
    const node = { ...ENDPOINT_NODE, doc_comment: 'GET /api/v1/health [Public]' };
    const meta = computeEndpointMeta(node, [node], []);
    expect(meta.method).toBe('GET');
    expect(meta.path).toBe('/api/v1/health [Public]');
  });

  it('computes spec completeness: realized excludes soft-deleted nodes', () => {
    const governed = computeSpecGovernedNodes(SPEC_NODE, SPEC_NODES, SPEC_EDGES);
    expect(governed.length).toBe(3); // spec_path match + governed_by edge (deleted included)
    const completeness = computeSpecCompleteness(governed, SPEC_NODES, SPEC_EDGES);
    expect(completeness.total).toBe(3);
    expect(completeness.realized).toBe(2); // OldIndex is soft-deleted → not realized
    expect(completeness.pct).toBe(67);
    expect(completeness.tested).toBe(1); // SearchService has coverage
  });
});

// ── MoldableTypeView ────────────────────────────────────────────────────────

describe('MoldableTypeView', () => {
  it('shows fields, implements, used-by, story, and risk metrics', () => {
    const { container } = render(MoldableTypeView, {
      props: { node: TYPE_NODE, nodes: TYPE_NODES, edges: TYPE_EDGES },
    });
    const titles = Array.from(container.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(titles.some(t => /^Fields \(1\)$/.test(t))).toBe(true);
    expect(titles.some(t => /^Implements \(1\)$/.test(t))).toBe(true);
    expect(titles.some(t => /^Used By \(\d+\)$/.test(t))).toBe(true);
    expect(titles).toContain('Story');
    expect(titles).toContain('Risk Summary');
    // Risk footer values
    const riskValues = Array.from(container.querySelectorAll('.risk-row-value')).map(v => v.textContent);
    expect(riskValues.some(v => v.includes('3/30d'))).toBe(true); // churn
    expect(riskValues.some(v => v.includes('94%'))).toBe(true); // test coverage
    // Story timeline data
    expect(container.textContent).toContain('Created on ');
    expect(container.textContent).toContain('abc1234'.slice(0, 7));
  });

  it('shows the spec linkage as a clickable reference that opens the spec inline', async () => {
    const onNavigate = vi.fn();
    const { container } = render(MoldableTypeView, {
      props: { node: TYPE_NODE, nodes: TYPE_NODES, edges: TYPE_EDGES, onNavigate },
    });
    const specBtn = container.querySelector('.detail-spec-button');
    expect(specBtn?.textContent).toContain('specs/platform-model.md');
    await fireEvent.click(specBtn);
    // Navigates to the spec entity — the host renders the spec view inline
    expect(onNavigate).toHaveBeenCalledWith(
      expect.objectContaining({ node_type: 'spec', id: 'specs/platform-model.md' })
    );
  });

  it('makes field types clickable → navigates to that type', async () => {
    const onNavigate = vi.fn();
    const { container } = render(MoldableTypeView, {
      props: { node: TYPE_NODE, nodes: TYPE_NODES, edges: TYPE_EDGES, onNavigate },
    });
    const clickableType = container.querySelector('.field-type-clickable');
    expect(clickableType).toBeTruthy();
    // The annotation shows the TYPE (doc_comment), not the field name that
    // qualified_name's last segment carries ('domain::User::status' → 'status').
    expect(clickableType.textContent).toBe(': TaskStatus');
    await fireEvent.click(clickableType);
    expect(onNavigate).toHaveBeenCalledWith(expect.objectContaining({ id: 'n5', name: 'TaskStatus' }));
  });

  it('resolves a field type from the doc_comment annotation when no DependsOn edge exists', async () => {
    // Real extractor shape (python/typescript_extractor.rs): qualified_name
    // `Parent.field_name`, type annotation in doc_comment, no DependsOn edge
    // when the type was not yet extracted at field-creation time.
    const agentType = {
      id: 'n6', node_type: 'type', name: 'Agent', qualified_name: 'domain::Agent',
      visibility: 'public', test_node: false,
    };
    const ownerField = {
      id: 'f2', node_type: 'field', name: 'owner', qualified_name: 'domain::User::owner',
      doc_comment: 'Option<Agent>', visibility: 'public', test_node: false,
    };
    const onNavigate = vi.fn();
    const { container } = render(MoldableTypeView, {
      props: {
        node: TYPE_NODE,
        nodes: [...TYPE_NODES, agentType, ownerField],
        edges: [...TYPE_EDGES, { id: 'e6', source_id: 'f2', target_id: 'n1', edge_type: 'field_of' }],
        onNavigate,
      },
    });
    const annotations = Array.from(container.querySelectorAll('.field-type-annotation'));
    const ownerAnnotation = annotations.find((a) => a.textContent.includes('Option<Agent>'));
    expect(ownerAnnotation).toBeTruthy();
    expect(ownerAnnotation.classList.contains('field-type-clickable')).toBe(true);
    await fireEvent.click(ownerAnnotation);
    // `Option<Agent>` → the Agent type node (wrapper skipped)
    expect(onNavigate).toHaveBeenCalledWith(expect.objectContaining({ id: 'n6', name: 'Agent' }));
  });

  it('makes the last-modified agent reference clickable → provenance', async () => {
    const onAgentProvenance = vi.fn();
    const { container } = render(MoldableTypeView, {
      props: { node: TYPE_NODE, nodes: TYPE_NODES, edges: TYPE_EDGES, onAgentProvenance },
    });
    const agentRefs = container.querySelectorAll('.agent-ref');
    expect(agentRefs.length).toBeGreaterThanOrEqual(1);
    await fireEvent.click(agentRefs[0]);
    expect(onAgentProvenance).toHaveBeenCalledWith('agent-1');
  });

  it('renders used-by consumers with crate attribution', () => {
    const { container } = render(MoldableTypeView, {
      props: { node: TYPE_NODE, nodes: TYPE_NODES, edges: TYPE_EDGES },
    });
    const text = container.textContent;
    expect(text).toContain('handle_create_user');
    expect(text).toContain('test_create_user');
  });
});

// ── MoldableTraitView ───────────────────────────────────────────────────────

describe('MoldableTraitView', () => {
  it('shows methods with signatures, implementations with crate, and dependents with counts', () => {
    const { container } = render(MoldableTraitView, {
      props: { node: TRAIT_VIEW_NODE, nodes: TRAIT_NODES, edges: TRAIT_EDGES },
    });
    const titles = Array.from(container.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(titles.some(t => /^Methods \(3\)$/.test(t))).toBe(true);
    expect(titles.some(t => /^Implementations \(2\)$/.test(t))).toBe(true);
    expect(titles.some(t => /^Dependents \(2\)$/.test(t))).toBe(true);
    expect(titles).toContain('Crate');

    // Method signature rendering
    const text = container.textContent;
    expect(text).toContain('find_by_id');
    expect(text).toContain('Option<Task>');

    // Implementations carry crate attribution
    expect(text).toContain('MemTaskRepo');
    expect(text).toContain('adapters');

    // Dependents carry call-site counts
    const counts = Array.from(container.querySelectorAll('.call-count')).map(c => c.textContent);
    expect(counts.some(c => c.includes('2 calls'))).toBe(true);
    expect(text).toContain('run_import');
    expect(text).toContain('sync_board');
  });

  it('navigates to a method when its row is clicked', async () => {
    const onNavigate = vi.fn();
    const { container } = render(MoldableTraitView, {
      props: { node: TRAIT_VIEW_NODE, nodes: TRAIT_NODES, edges: TRAIT_EDGES, onNavigate },
    });
    const methodLink = Array.from(container.querySelectorAll('.method-link'))
      .find(b => b.textContent.includes('find_by_id'));
    await fireEvent.click(methodLink);
    expect(onNavigate).toHaveBeenCalledWith(expect.objectContaining({ id: 'm1' }));
  });
});

// ── MoldableEndpointView ────────────────────────────────────────────────────

describe('MoldableEndpointView', () => {
  it('shows route (method + path), handler location, flow, shapes, gates, and tests', () => {
    const { container } = render(MoldableEndpointView, {
      props: { node: ENDPOINT_NODE, nodes: ENDPOINT_NODES, edges: ENDPOINT_EDGES },
    });
    const text = container.textContent;

    // Route from edge metadata
    expect(container.querySelector('.http-post')?.textContent).toBe('POST');
    expect(text).toContain('/api/v1/agents/spawn');

    // Handler with file:line location
    expect(text).toContain('handle_spawn');
    expect(text).toContain('api/spawn.rs:42');

    // Flow diagram: handler + auth + domain + request/response types
    const titles = Array.from(container.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(titles.some(t => /^Flow \(\d+\)$/.test(t))).toBe(true);
    expect(text).toContain('auth_check');
    expect(text).toContain('create_agent');

    // Request / response shapes resolved from handler call targets
    expect(text).toContain('api::SpawnRequest');
    expect(text).toContain('api::SpawnResponse');

    // Gates extracted from doc_comment bracket hints
    expect(text).toContain('RequireAdmin');

    // Test count (test node reaches the endpoint via its handler)
    expect(text).toMatch(/1 test reaches this endpoint/);
  });

  it('navigates to a flow step when clicked', async () => {
    const onNavigate = vi.fn();
    const { container } = render(MoldableEndpointView, {
      props: { node: ENDPOINT_NODE, nodes: ENDPOINT_NODES, edges: ENDPOINT_EDGES, onNavigate },
    });
    const step = Array.from(container.querySelectorAll('.flow-step .detail-ref-link'))
      .find(b => b.textContent.includes('auth_check'));
    await fireEvent.click(step);
    expect(onNavigate).toHaveBeenCalledWith(expect.objectContaining({ id: 'h2' }));
  });
});

// ── MoldableSpecView ────────────────────────────────────────────────────────

describe('MoldableSpecView', () => {
  beforeEach(() => {
    specContent.mockResolvedValue({ content: '# Search\n\nFull-text search across all entities.', current_sha: 'a1b2c3d4e5' });
    specHistoryRepo.mockResolvedValue([
      { event: 'synced', timestamp: '2026-01-01T00:00:00Z' },
      { event: 'approved', timestamp: '2026-01-02T00:00:00Z', sha: 'aaaa1111' },
    ]);
    getSpecAssertionResults.mockResolvedValue({
      assertions: [
        { line: 1, assertion_type: 'all_have', passed: true, details: 'ok' },
        { line: 2, assertion_type: 'exists', passed: false, details: 'missing node' },
      ],
    });
  });

  it('shows spec content alongside implementation realization with completeness', async () => {
    const { container } = render(MoldableSpecView, {
      props: { node: SPEC_NODE, nodes: SPEC_NODES, edges: SPEC_EDGES, repoId: 'repo-1' },
    });
    // Content fetched from the spec ledger and rendered as markdown
    await waitFor(() => {
      expect(container.querySelector('.spec-content-full h1')?.textContent).toBe('Search');
    });
    expect(specContent).toHaveBeenCalledWith('specs/system/search.md', 'repo-1');

    // Realization: linked nodes with ✓ (realized) / ✗ (removed)
    const checks = container.querySelectorAll('.spec-check');
    expect(checks.length).toBe(3);
    expect(container.querySelectorAll('.spec-check-failed').length).toBe(1);
    expect(container.textContent).toContain('SearchService');
    expect(container.textContent).toContain('OldIndex');

    // Completeness: 2 of 3 realized (67%)
    const text = container.textContent;
    expect(text).toContain('of 3 realized');
    expect(text).toContain('67%');

    // Stored assertion results
    expect(text).toContain('1/2 passing');
  });

  it('shows inline history when the History action is clicked', async () => {
    const { container } = render(MoldableSpecView, {
      props: { node: SPEC_NODE, nodes: SPEC_NODES, edges: SPEC_EDGES, repoId: 'repo-1' },
    });
    const historyBtn = Array.from(container.querySelectorAll('.moldable-action-btn'))
      .find(b => b.textContent.includes('History'));
    expect(historyBtn).toBeTruthy();
    await fireEvent.click(historyBtn);
    await waitFor(() => {
      expect(container.querySelectorAll('.spec-history-item').length).toBe(2);
    });
    expect(container.textContent).toContain('approved');
    expect(specHistoryRepo).toHaveBeenCalledWith('specs/system/search.md', 'repo-1');
  });

  it('Edit action opens the inline spec editor via onEditSpec', async () => {
    const onEditSpec = vi.fn();
    const { container } = render(MoldableSpecView, {
      props: { node: SPEC_NODE, nodes: SPEC_NODES, edges: SPEC_EDGES, repoId: 'repo-1', onEditSpec },
    });
    const editBtn = Array.from(container.querySelectorAll('.moldable-action-btn'))
      .find(b => b.textContent.includes('Edit'));
    await fireEvent.click(editBtn);
    expect(onEditSpec).toHaveBeenCalledWith('specs/system/search.md');
  });
});

// ── NodeDetailPanel routing ────────────────────────────────────────────────

describe('NodeDetailPanel routes entities to moldable views by type', () => {
  it('routes type/table/component nodes to the Type View', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: TYPE_NODES, edges: TYPE_EDGES },
    });
    expect(container.querySelector('[data-moldable-view="type"]')).toBeTruthy();
    expect(container.querySelector('[data-moldable-view="trait"]')).toBeFalsy();
  });

  it('routes interface/trait nodes to the Trait View', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TRAIT_VIEW_NODE, nodes: TRAIT_NODES, edges: TRAIT_EDGES },
    });
    expect(container.querySelector('[data-moldable-view="trait"]')).toBeTruthy();
  });

  it('routes endpoint nodes to the Endpoint View', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: ENDPOINT_NODE, nodes: ENDPOINT_NODES, edges: ENDPOINT_EDGES },
    });
    expect(container.querySelector('[data-moldable-view="endpoint"]')).toBeTruthy();
  });

  it('routes spec nodes to the Spec View', () => {
    specContent.mockResolvedValue({ content: '# Search' });
    getSpecAssertionResults.mockResolvedValue({ assertions: [] });
    const { container } = render(NodeDetailPanel, {
      props: { node: SPEC_NODE, nodes: SPEC_NODES, edges: SPEC_EDGES, repoId: 'repo-1' },
    });
    expect(container.querySelector('[data-moldable-view="spec"]')).toBeTruthy();
  });

  it('renders the generic view for non-moldable node types', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TEST_FN, nodes: TYPE_NODES, edges: TYPE_EDGES },
    });
    expect(container.querySelector('[data-moldable-view]')).toBeFalsy();
  });
});
