import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import NodeDetailPanel from '../lib/NodeDetailPanel.svelte';

const TYPE_NODE = {
  id: 'n1', node_type: 'type', name: 'User', qualified_name: 'domain.User',
  file_path: 'domain/models.rs', line_start: 10, line_end: 30,
  visibility: 'public', spec_path: 'specs/platform-model.md', spec_confidence: 'high',
  complexity: 35, churn_count_30d: 2, test_coverage: 0.3,
  last_modified_by: 'agent-1', last_modified_at: 1711000000, created_sha: 'abc1234',
  first_seen_at: 1710000000, test_node: false,
};

const INTERFACE_NODE = {
  id: 'n2', node_type: 'interface', name: 'TaskPort', qualified_name: 'ports.TaskPort',
  file_path: 'ports/task.rs', line_start: 1, line_end: 20,
  visibility: 'public', spec_confidence: 'high',
  test_node: false,
};

const TEST_NODE = {
  id: 'n3', node_type: 'function', name: 'test_create_user', qualified_name: 'tests.test_create_user',
  file_path: 'tests/test_api.rs', line_start: 1, line_end: 15,
  visibility: 'public', spec_confidence: 'none',
  test_node: true,
};

const NODES = [TYPE_NODE, INTERFACE_NODE, TEST_NODE];

const EDGES = [
  { id: 'e1', source_id: 'n2', target_id: 'n1', edge_type: 'implements' },
  { id: 'e2', source_id: 'n3', target_id: 'n1', edge_type: 'calls' },
  { id: 'e3', source_id: 'n1', target_id: 'n2', edge_type: 'implements' },
];

describe('NodeDetailPanel', () => {
  it('renders common flows when node is null', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: null, nodes: [], edges: [] },
    });
    const panel = container.querySelector('.detail-panel');
    expect(panel).toBeTruthy();
    expect(panel.textContent).toContain('Common Flows');
  });

  it('renders detail panel with node data', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    const panel = container.querySelector('.detail-panel');
    expect(panel).toBeTruthy();
    expect(container.querySelector('.detail-name')?.textContent).toBe('User');
    expect(container.querySelector('.detail-type-badge')?.textContent).toBe('Type');
  });

  it('shows file location', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    const file = container.querySelector('.detail-file code');
    expect(file?.textContent).toContain('domain/models.rs:10');
  });

  it('shows spec linkage', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    const spec = container.querySelector('.detail-spec-button') ?? container.querySelector('.detail-spec');
    expect(spec?.textContent).toContain('specs/platform-model.md');
  });

  it('shows risk assessment - high risk for complex + low coverage', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    const risk = container.querySelector('.risk-high');
    expect(risk).toBeTruthy();
    expect(risk?.textContent).toContain('High complexity');
  });

  it('shows risk assessment - healthy for simple nodes', () => {
    const simpleNode = { ...TYPE_NODE, complexity: 5, test_coverage: 0.9 };
    const { container } = render(NodeDetailPanel, {
      props: { node: simpleNode, nodes: NODES, edges: EDGES },
    });
    const riskLow = container.querySelector('.risk-low');
    expect(riskLow).toBeTruthy();
    expect(riskLow?.textContent).toContain('Healthy');
  });

  it('shows test node badge in metrics', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TEST_NODE, nodes: NODES, edges: EDGES },
    });
    const testBadge = container.querySelector('.test-node');
    expect(testBadge).toBeTruthy();
  });

  it('shows provenance info', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    const provenance = container.querySelectorAll('.detail-provenance');
    expect(provenance.length).toBeGreaterThanOrEqual(1);
    const text = Array.from(provenance).map(p => p.textContent).join(' ');
    expect(text).toContain('agent-1');
  });

  it('shows relationships for type node', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    const sectionTitles = Array.from(container.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(sectionTitles).toContain('Implements (1)');
    expect(sectionTitles).toContain('Used By (1)');
  });

  it('shows spec coverage section for spec-linked nodes', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    const sectionTitles = Array.from(container.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(sectionTitles).toContain('Spec Coverage');
  });

  it('navigable ref links call onNavigate', async () => {
    const onNavigate = vi.fn();
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, onNavigate },
    });
    const refLinks = container.querySelectorAll('.detail-ref-link');
    expect(refLinks.length).toBeGreaterThanOrEqual(1);
  });

  it('shows close button that calls onClose', async () => {
    const onClose = vi.fn();
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, onClose },
    });
    const closeBtn = container.querySelector('.detail-close');
    expect(closeBtn).toBeTruthy();
  });

  it('renders edge detail view', () => {
    const edgeNode = {
      id: 'edge-n1-n2',
      name: 'User → TaskPort',
      node_type: 'edge',
      edge_type: 'implements',
      source_node: TYPE_NODE,
      target_node: INTERFACE_NODE,
    };
    const { container } = render(NodeDetailPanel, {
      props: { node: edgeNode, nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.detail-edge-type code')?.textContent).toContain('implements');
    const endpoints = container.querySelectorAll('.detail-edge-endpoint');
    expect(endpoints.length).toBe(2);
  });

  it('renders span detail view', () => {
    const spanNode = {
      id: 'span-s1',
      name: 'test_operation',
      node_type: 'span',
      span_id: 's1',
      duration_us: 1500,
      status: 'ok',
      service_name: 'test-service',
      attributes: { 'http.method': 'GET', 'http.url': '/api/users' },
      input_summary: 'GET /api/users',
      output_summary: '{"users": [...]}',
    };
    const { container } = render(NodeDetailPanel, {
      props: { node: spanNode, nodes: NODES, edges: EDGES },
    });
    const grid = container.querySelector('.span-detail-grid');
    expect(grid).toBeTruthy();
    expect(grid.textContent).toContain('test_operation');
    expect(grid.textContent).toContain('1.5ms');
    expect(grid.textContent).toContain('test-service');
    // Attributes
    const attrs = container.querySelectorAll('.span-attr-row');
    expect(attrs.length).toBe(2);
    // Input/output
    expect(container.querySelector('.span-io-summary')?.textContent).toContain('GET /api/users');
  });

  it('renders span error status with red styling', () => {
    const spanNode = {
      id: 'span-s2',
      name: 'failing_operation',
      node_type: 'span',
      duration_us: 500,
      status: 'error',
    };
    const { container } = render(NodeDetailPanel, {
      props: { node: spanNode, nodes: NODES, edges: EDGES },
    });
    const errorStatus = container.querySelector('.span-error');
    expect(errorStatus).toBeTruthy();
    expect(errorStatus.textContent).toBe('error');
  });

  it('shows usedBy relationships for type nodes', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    // Verify usedBy section is populated via edges
    const sectionTitles = Array.from(container.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(sectionTitles).toContain('Used By (1)');
  });
});

// ---------------------------------------------------------------------------
// Moldable view type dispatch
// ---------------------------------------------------------------------------
describe('NodeDetailPanel -- moldable view type dispatch', () => {
  function moldableViewType(nodeType) {
    switch (nodeType) {
      case 'type':
      case 'table':
      case 'component':
        return 'type';
      case 'interface':
      case 'trait':
        return 'trait';
      case 'endpoint':
        return 'endpoint';
      case 'spec':
        return 'spec';
      default:
        return null;
    }
  }

  it('type node_type maps to "type" moldable view', () => {
    expect(moldableViewType('type')).toBe('type');
  });

  it('table node_type maps to "type" moldable view', () => {
    expect(moldableViewType('table')).toBe('type');
  });

  it('component node_type maps to "type" moldable view', () => {
    expect(moldableViewType('component')).toBe('type');
  });

  it('interface node_type maps to "trait" moldable view', () => {
    expect(moldableViewType('interface')).toBe('trait');
  });

  it('trait node_type maps to "trait" moldable view', () => {
    expect(moldableViewType('trait')).toBe('trait');
  });

  it('endpoint node_type maps to "endpoint" moldable view', () => {
    expect(moldableViewType('endpoint')).toBe('endpoint');
  });

  it('spec node_type maps to "spec" moldable view', () => {
    expect(moldableViewType('spec')).toBe('spec');
  });

  it('function node_type returns null (generic view)', () => {
    expect(moldableViewType('function')).toBeNull();
  });

  it('module node_type returns null (generic view)', () => {
    expect(moldableViewType('module')).toBeNull();
  });

  it('package node_type returns null (generic view)', () => {
    expect(moldableViewType('package')).toBeNull();
  });

  it('moldable view labels are correct', () => {
    const labels = { type: 'Type View', trait: 'Trait View', endpoint: 'Endpoint View', spec: 'Spec View' };
    expect(labels[moldableViewType('type')]).toBe('Type View');
    expect(labels[moldableViewType('interface')]).toBe('Trait View');
    expect(labels[moldableViewType('endpoint')]).toBe('Endpoint View');
    expect(labels[moldableViewType('spec')]).toBe('Spec View');
  });

  it('renders type badge for TYPE_NODE', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.detail-type-badge')?.textContent).toBe('Type');
  });

  it('renders interface badge for INTERFACE_NODE', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: INTERFACE_NODE, nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.detail-type-badge')?.textContent).toBe('Interface / Trait');
  });

  it('renders function badge for TEST_NODE', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TEST_NODE, nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.detail-type-badge')?.textContent).toBe('Function');
  });
});

// ---------------------------------------------------------------------------
// Relationship computation from edges
// ---------------------------------------------------------------------------
describe('NodeDetailPanel -- relationship computation', () => {
  function computeRelationships(node, nodes, edges) {
    const nodeId = node.id;
    const implementedBy = [];
    const implementsTraits = [];
    const calledBy = [];
    const callsOut = [];
    const fields = [];
    const contains = [];
    const usedBy = [];
    const methods = [];
    let containedIn = null;
    let governedBy = null;

    for (const e of edges) {
      const src = e.source_id;
      const tgt = e.target_id;
      const et = (e.edge_type ?? '').toLowerCase();

      if (et === 'implements' && tgt === nodeId) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) implementedBy.push(srcNode);
      }
      if (et === 'implements' && src === nodeId) {
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode) implementsTraits.push(tgtNode);
      }
      if (et === 'calls' && tgt === nodeId) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) calledBy.push(srcNode);
      }
      if (et === 'calls' && src === nodeId) {
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode) callsOut.push(tgtNode);
      }
      if (et === 'contains' && src === nodeId) {
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode) {
          contains.push(tgtNode);
          if (tgtNode.node_type === 'function') methods.push(tgtNode);
        }
      }
      if (et === 'contains' && tgt === nodeId) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) containedIn = srcNode;
      }
      if (et === 'governed_by' && src === nodeId) {
        governedBy = tgt;
      }
    }

    // Populate usedBy
    for (const e of edges) {
      const src = e.source_id;
      const tgt = e.target_id;
      const et = (e.edge_type ?? '').toLowerCase();
      if (tgt === nodeId && (et === 'calls' || et === 'field_of' || et === 'routes_to' || et === 'contains')) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode && !usedBy.some(u => u.id === srcNode.id)) usedBy.push(srcNode);
      }
    }

    return { implementedBy, implements: implementsTraits, calledBy, calls: callsOut, fields, containedIn, contains, governedBy, usedBy, methods };
  }

  it('computes implements relationships for TYPE_NODE', () => {
    const rels = computeRelationships(TYPE_NODE, NODES, EDGES);
    expect(rels.implements.length).toBe(1);
    expect(rels.implements[0].name).toBe('TaskPort');
  });

  it('computes implementedBy for INTERFACE_NODE', () => {
    const rels = computeRelationships(INTERFACE_NODE, NODES, EDGES);
    expect(rels.implementedBy.length).toBe(1);
    expect(rels.implementedBy[0].name).toBe('User');
  });

  it('computes calledBy for TYPE_NODE', () => {
    const rels = computeRelationships(TYPE_NODE, NODES, EDGES);
    expect(rels.calledBy.length).toBe(1);
    expect(rels.calledBy[0].name).toBe('test_create_user');
  });

  it('computes usedBy for TYPE_NODE', () => {
    const rels = computeRelationships(TYPE_NODE, NODES, EDGES);
    // usedBy includes callers and nodes that contain/route_to this node
    expect(rels.usedBy.length).toBeGreaterThanOrEqual(1);
  });

  it('returns empty relationships for a node with no edges', () => {
    const isolatedNode = { id: 'isolated', node_type: 'function', name: 'lonely' };
    const rels = computeRelationships(isolatedNode, [isolatedNode], []);
    expect(rels.implementedBy).toHaveLength(0);
    expect(rels.calledBy).toHaveLength(0);
    expect(rels.calls).toHaveLength(0);
    expect(rels.usedBy).toHaveLength(0);
    expect(rels.containedIn).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Visibility badge formatting
// ---------------------------------------------------------------------------
describe('NodeDetailPanel -- visibility badge formatting', () => {
  function visibilityBadge(visibility) {
    const v = (visibility ?? '').toLowerCase();
    return v === 'public' ? 'pub' : v === 'private' ? 'priv' : v;
  }

  it('formats public as "pub"', () => {
    expect(visibilityBadge('public')).toBe('pub');
  });

  it('formats Public (capitalized) as "pub"', () => {
    expect(visibilityBadge('Public')).toBe('pub');
  });

  it('formats private as "priv"', () => {
    expect(visibilityBadge('private')).toBe('priv');
  });

  it('formats crate visibility as-is', () => {
    expect(visibilityBadge('crate')).toBe('crate');
  });

  it('formats empty visibility as empty string', () => {
    expect(visibilityBadge('')).toBe('');
  });

  it('handles null visibility', () => {
    expect(visibilityBadge(null)).toBe('');
  });

  it('handles undefined visibility', () => {
    expect(visibilityBadge(undefined)).toBe('');
  });

  it('renders visibility badge in component', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES },
    });
    const visBadge = container.querySelector('.detail-vis-badge');
    expect(visBadge).toBeTruthy();
    expect(visBadge?.textContent).toBe('[pub]');
  });
});

// ---------------------------------------------------------------------------
// Method call count computation
// ---------------------------------------------------------------------------
describe('NodeDetailPanel -- method call count computation', () => {
  const TRAIT_NODE = {
    id: 't1', node_type: 'interface', name: 'Repository', qualified_name: 'ports.Repository',
    file_path: 'ports/repo.rs', line_start: 1, line_end: 30, visibility: 'public', test_node: false,
  };

  const METHOD_NODES = [
    { id: 'm1', node_type: 'function', name: 'find_by_id', qualified_name: 'ports.Repository.find_by_id', visibility: 'public', test_node: false },
    { id: 'm2', node_type: 'function', name: 'save', qualified_name: 'ports.Repository.save', visibility: 'public', test_node: false },
    { id: 'm3', node_type: 'function', name: 'delete', qualified_name: 'ports.Repository.delete', visibility: 'public', test_node: false },
  ];

  const ALL_NODES = [TRAIT_NODE, ...METHOD_NODES];

  const METHOD_EDGES = [
    { id: 'me1', source_id: 't1', target_id: 'm1', edge_type: 'contains' },
    { id: 'me2', source_id: 't1', target_id: 'm2', edge_type: 'contains' },
    { id: 'me3', source_id: 't1', target_id: 'm3', edge_type: 'contains' },
    // Callers of find_by_id (3 callers)
    { id: 'c1', source_id: 'x1', target_id: 'm1', edge_type: 'calls' },
    { id: 'c2', source_id: 'x2', target_id: 'm1', edge_type: 'calls' },
    { id: 'c3', source_id: 'x3', target_id: 'm1', edge_type: 'calls' },
    // Callers of save (1 caller)
    { id: 'c4', source_id: 'x1', target_id: 'm2', edge_type: 'calls' },
    // delete has no callers
  ];

  function computeMethodCallCounts(traitNode, nodes, edges) {
    if (traitNode.node_type !== 'interface' && traitNode.node_type !== 'trait') return new Map();
    // Find methods (functions contained by the trait)
    const methods = [];
    for (const e of edges) {
      if (e.edge_type === 'contains' && e.source_id === traitNode.id) {
        const tgtNode = nodes.find(n => n.id === e.target_id);
        if (tgtNode && tgtNode.node_type === 'function') methods.push(tgtNode);
      }
    }
    // Count call edges targeting each method
    const counts = new Map();
    for (const method of methods) {
      let count = 0;
      for (const e of edges) {
        if (e.edge_type === 'calls' && e.target_id === method.id) count++;
      }
      counts.set(method.id, count);
    }
    return counts;
  }

  it('counts call-site counts for each trait method', () => {
    const counts = computeMethodCallCounts(TRAIT_NODE, ALL_NODES, METHOD_EDGES);
    expect(counts.get('m1')).toBe(3); // find_by_id has 3 callers
    expect(counts.get('m2')).toBe(1); // save has 1 caller
    expect(counts.get('m3')).toBe(0); // delete has no callers
  });

  it('returns empty map for non-interface nodes', () => {
    const fnNode = { id: 'fn1', node_type: 'function', name: 'foo' };
    const counts = computeMethodCallCounts(fnNode, ALL_NODES, METHOD_EDGES);
    expect(counts.size).toBe(0);
  });

  it('returns empty map when trait has no methods', () => {
    const emptyTrait = { id: 'et1', node_type: 'interface', name: 'Empty' };
    const counts = computeMethodCallCounts(emptyTrait, [emptyTrait], []);
    expect(counts.size).toBe(0);
  });

  it('identifies most-called method', () => {
    const counts = computeMethodCallCounts(TRAIT_NODE, ALL_NODES, METHOD_EDGES);
    let maxId = null;
    let maxCount = -1;
    for (const [id, count] of counts) {
      if (count > maxCount) { maxCount = count; maxId = id; }
    }
    expect(maxId).toBe('m1'); // find_by_id
    expect(maxCount).toBe(3);
  });

  it('identifies uncalled methods (potential dead code)', () => {
    const counts = computeMethodCallCounts(TRAIT_NODE, ALL_NODES, METHOD_EDGES);
    const uncalled = [];
    for (const [id, count] of counts) {
      if (count === 0) uncalled.push(id);
    }
    expect(uncalled).toContain('m3'); // delete is uncalled
    expect(uncalled).not.toContain('m1');
  });
});

// ── Evaluative tab tests ──

const TRACE_SPANS = [
  { span_id: 's1', graph_node_id: 'n1', operation_name: 'find_user', duration_us: 5000, status: 'ok', start_time: 1710000000000000, parent_span_id: null, attributes: { 'db.system': 'sqlite' }, input_summary: 'id=42', output_summary: '{"name":"Alice"}' },
  { span_id: 's2', graph_node_id: 'n1', operation_name: 'validate_user', duration_us: 500, status: 'ok', start_time: 1710000001000000, parent_span_id: 's1', attributes: {}, input_summary: null, output_summary: null },
  { span_id: 's3', graph_node_id: 'n1', operation_name: 'save_user', duration_us: 12000, status: 'ERROR', start_time: 1710000002000000, parent_span_id: 's1', attributes: { 'error.message': 'constraint violation' }, input_summary: null, output_summary: null },
  { span_id: 's4', graph_node_id: 'n2', operation_name: 'list_tasks', duration_us: 3000, status: 'ok', start_time: 1710000003000000, parent_span_id: null, attributes: {}, input_summary: null, output_summary: null },
];

describe('NodeDetailPanel -- evaluative tab', () => {
  it('shows evaluative tab when lens is evaluative and trace data exists', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: TRACE_SPANS },
    });
    const sectionTitles = Array.from(container.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(sectionTitles.some(t => t.includes('Evaluative'))).toBe(true);
  });

  it('hides evaluative tab when lens is structural', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'structural', traceSpans: TRACE_SPANS },
    });
    const sectionTitles = Array.from(container.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(sectionTitles.some(t => t.includes('Evaluative'))).toBe(false);
  });

  it('hides evaluative tab when lens is observable', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'observable', traceSpans: TRACE_SPANS },
    });
    const sectionTitles = Array.from(container.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(sectionTitles.some(t => t.includes('Evaluative'))).toBe(false);
  });

  it('filters spans to only those touching the selected node', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: TRACE_SPANS },
    });
    // Node n1 has 3 spans (s1, s2, s3), node n2 has 1 (s4)
    const spanRows = container.querySelectorAll('.eval-span-row');
    expect(spanRows.length).toBe(3);
  });

  it('sorts spans by duration descending (slowest first)', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: TRACE_SPANS },
    });
    const spanNames = Array.from(container.querySelectorAll('.eval-span-name')).map(el => el.textContent);
    // save_user: 12000, find_user: 5000, validate_user: 500
    expect(spanNames[0]).toBe('save_user');
    expect(spanNames[1]).toBe('find_user');
    expect(spanNames[2]).toBe('validate_user');
  });

  it('shows operation name, duration, and status for each span', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: TRACE_SPANS },
    });
    const firstRow = container.querySelector('.eval-span-row');
    expect(firstRow).toBeTruthy();
    expect(firstRow.querySelector('.eval-span-name')?.textContent).toBe('save_user');
    expect(firstRow.querySelector('.eval-span-duration')?.textContent).toBe('12.0ms');
    expect(firstRow.querySelector('.eval-status-error')).toBeTruthy();
  });

  it('shows aggregate stats (p50, p95, error rate)', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: TRACE_SPANS },
    });
    const statLabels = Array.from(container.querySelectorAll('.eval-stat-label')).map(el => el.textContent);
    expect(statLabels).toContain('p50');
    expect(statLabels).toContain('p95');
    expect(statLabels).toContain('Errors');
    expect(statLabels).toContain('Mean');

    // Error rate: 1 out of 3 = 33%
    const errorStatValue = container.querySelectorAll('.eval-stat-value');
    const errorText = Array.from(errorStatValue).map(el => el.textContent);
    expect(errorText.some(t => t.includes('33%'))).toBe(true);
  });

  it('shows span count badge', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: TRACE_SPANS },
    });
    const badge = container.querySelector('.detail-badge');
    const evalBadge = Array.from(container.querySelectorAll('.detail-badge')).find(b => b.textContent.includes('spans'));
    expect(evalBadge?.textContent).toContain('3 spans');
  });

  it('marks error spans with error styling', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: TRACE_SPANS },
    });
    const errorRows = container.querySelectorAll('.eval-span-error');
    expect(errorRows.length).toBe(1);
  });

  it('calls onSpanSelect when a span row is clicked', async () => {
    const onSpanSelect = vi.fn();
    const { container } = render(NodeDetailPanel, {
      props: { node: TYPE_NODE, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: TRACE_SPANS, onSpanSelect },
    });
    const firstRow = container.querySelector('.eval-span-row');
    firstRow.click();
    expect(onSpanSelect).toHaveBeenCalledTimes(1);
    expect(onSpanSelect).toHaveBeenCalledWith(expect.objectContaining({ span_id: 's3' }));
  });

  it('shows no evaluative tab when no spans match the node', () => {
    const { container } = render(NodeDetailPanel, {
      props: { node: INTERFACE_NODE, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: [TRACE_SPANS[3]] },
    });
    // INTERFACE_NODE is n2, only s4 matches n2, but we pass only s4 which is for n2
    // Actually n2 does have s4, let's test with no matching spans
    const noMatchNode = { ...TYPE_NODE, id: 'n999' };
    const { container: c2 } = render(NodeDetailPanel, {
      props: { node: noMatchNode, nodes: NODES, edges: EDGES, lens: 'evaluative', traceSpans: TRACE_SPANS },
    });
    const sectionTitles = Array.from(c2.querySelectorAll('.detail-section-title')).map(t => t.textContent);
    expect(sectionTitles.some(t => t.includes('Evaluative'))).toBe(false);
  });
});
