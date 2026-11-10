/**
 * generated-view.test.js — HSI §3 Generated Views (task-089).
 *
 * Pins the two pure pieces of the generated-view flow:
 *   1. viewSpecToViewQuery — the ViewSpec → ViewQuery translation the
 *      ExplorerCanvas renders (concept/filter scopes, callouts, and the
 *      LLM explanation as the query annotation / sidebar annotation).
 *   2. parseGenerateSse — parsing the generate endpoint's SSE body
 *      (partial + complete events, malformed frames skipped).
 */
import { describe, it, expect } from 'vitest';
import {
  normalizeNodeType,
  normalizeEdgeType,
  viewSpecToViewQuery,
  parseGenerateSse,
} from '../lib/generated-view.js';

describe('normalizeNodeType', () => {
  it('lowercases TitleCase ViewSpec node types', () => {
    expect(normalizeNodeType('Module')).toBe('module');
    expect(normalizeNodeType('Endpoint')).toBe('endpoint');
  });

  it('passes already-normalized types through', () => {
    expect(normalizeNodeType('function')).toBe('function');
  });

  it('tolerates null/undefined', () => {
    expect(normalizeNodeType(null)).toBe('');
    expect(normalizeNodeType(undefined)).toBe('');
  });
});

describe('normalizeEdgeType', () => {
  it('snake_cases CamelCase ViewSpec edge types', () => {
    expect(normalizeEdgeType('RoutesTo')).toBe('routes_to');
    expect(normalizeEdgeType('DependsOn')).toBe('depends_on');
    expect(normalizeEdgeType('GovernedBy')).toBe('governed_by');
  });

  it('passes snake_case through unchanged', () => {
    expect(normalizeEdgeType('routes_to')).toBe('routes_to');
    expect(normalizeEdgeType('calls')).toBe('calls');
  });
});

describe('viewSpecToViewQuery', () => {
  it('rejects non-object specs', () => {
    expect(viewSpecToViewQuery(null)).toBeNull();
    expect(viewSpecToViewQuery('spec')).toBeNull();
    expect(viewSpecToViewQuery([1, 2])).toBeNull();
  });

  it('maps data.concept to a concept scope with normalized edges and depth', () => {
    const query = viewSpecToViewQuery({
      name: 'How auth works',
      data: {
        concept: 'auth',
        node_types: ['Module', 'Function'],
        edge_types: ['Contains', 'RoutesTo'],
        depth: 2,
      },
      layout: 'hierarchical',
    });
    expect(query.scope).toEqual({
      type: 'concept',
      seed_nodes: ['auth'],
      expand_direction: 'both',
      expand_edges: ['contains', 'routes_to'],
      expand_depth: 2,
    });
  });

  it('maps node_types to a filter scope when there is no concept', () => {
    const query = viewSpecToViewQuery({
      name: 'Endpoints',
      data: { node_types: ['Endpoint'], edge_types: [], depth: 1 },
      layout: 'list',
    });
    expect(query.scope).toEqual({ type: 'filter', node_types: ['endpoint'] });
  });

  it('maps filter.spec_path to a governed_by computed filter', () => {
    const query = viewSpecToViewQuery({
      name: 'Spec governed',
      data: { node_types: [], edge_types: [], depth: 1, filter: { spec_path: 'specs/system/identity-security.md' } },
      layout: 'graph',
    });
    expect(query.scope).toEqual({
      type: 'filter',
      computed: "$governed_by('specs/system/identity-security.md')",
    });
  });

  it('escapes quotes in spec_path so it cannot break the computed expression', () => {
    const query = viewSpecToViewQuery({
      name: 'x',
      data: { node_types: [], edge_types: [], depth: 1, filter: { spec_path: "specs/o'brien.md" } },
      layout: 'graph',
    });
    // The literal is escaped, not truncated or injected.
    expect(query.scope.computed).toBe("$governed_by('specs/o\\'brien.md')");
  });

  it('falls back to an all scope when the data layer has no selectors', () => {
    const query = viewSpecToViewQuery({
      name: 'Everything',
      data: { node_types: [], edge_types: [], depth: 1 },
      layout: 'graph',
    });
    expect(query.scope).toEqual({ type: 'all' });
  });

  it('maps annotations to callouts', () => {
    const query = viewSpecToViewQuery({
      name: 'Annotated',
      data: { node_types: ['Type'], edge_types: [], depth: 1 },
      layout: 'graph',
      annotations: [
        { node_name: 'require_auth_middleware', text: 'Entry point' },
        { node_name: '', text: 'dropped — no node' },
        'not-an-annotation',
      ],
    });
    expect(query.callouts).toEqual([{ node: 'require_auth_middleware', text: 'Entry point' }]);
  });

  it('puts the LLM explanation into the query annotation (sidebar annotation)', () => {
    const query = viewSpecToViewQuery({
      name: 'How auth works',
      data: { concept: 'auth', node_types: [], edge_types: [], depth: 2 },
      layout: 'hierarchical',
      explanation: 'Authentication flows through require_auth_middleware.',
    });
    expect(query.annotation).toEqual({
      title: 'How auth works',
      description: 'Authentication flows through require_auth_middleware.',
    });
    expect(query.zoom).toBe('fit');
  });

  it('omits the annotation when neither name nor explanation exists', () => {
    const query = viewSpecToViewQuery({
      data: { node_types: ['Type'], edge_types: [], depth: 1 },
      layout: 'graph',
    });
    expect(query.annotation).toBeUndefined();
  });
});

describe('parseGenerateSse', () => {
  it('parses partial and complete events', () => {
    const text = [
      'event: partial',
      'data: {"explanation":"Generating view..."}',
      '',
      'event: complete',
      'data: {"view_spec":{"name":"n","layout":"graph"},"explanation":"done"}',
      '',
    ].join('\n');
    const events = parseGenerateSse(text);
    expect(events.partial).toEqual({ explanation: 'Generating view...' });
    expect(events.complete).toEqual({ view_spec: { name: 'n', layout: 'graph' }, explanation: 'done' });
  });

  it('skips frames with malformed JSON data', () => {
    const text = 'event: complete\ndata: {not json}\n\nevent: complete\ndata: {"view_spec":null,"explanation":"invalid"}\n\n';
    const events = parseGenerateSse(text);
    expect(events.complete).toEqual({ view_spec: null, explanation: 'invalid' });
  });

  it('returns nulls for empty or non-string input', () => {
    expect(parseGenerateSse('')).toEqual({ partial: null, complete: null });
    expect(parseGenerateSse(null)).toEqual({ partial: null, complete: null });
  });
});
