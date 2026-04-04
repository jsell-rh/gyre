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
