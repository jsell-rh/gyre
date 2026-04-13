import { describe, it, expect } from 'vitest';
import { validateViewQuery } from '../lib/view-query-validator.js';

// ---------------------------------------------------------------------------
// Valid queries
// ---------------------------------------------------------------------------
describe('validateViewQuery — valid queries', () => {
  it('accepts a minimal scope:all query', () => {
    const result = validateViewQuery({ scope: { type: 'all' } });
    expect(result.valid).toBe(true);
    expect(result.errors).toEqual([]);
  });

  it('accepts a focus scope with required fields', () => {
    const result = validateViewQuery({
      scope: { type: 'focus', node: 'MyClass', depth: 3, direction: 'incoming' },
    });
    expect(result.valid).toBe(true);
    expect(result.errors).toEqual([]);
  });

  it('accepts a focus scope with $clicked interactive binding', () => {
    const result = validateViewQuery({
      scope: { type: 'focus', node: '$clicked' },
    });
    expect(result.valid).toBe(true);
  });

  it('accepts a filter scope with node_types', () => {
    const result = validateViewQuery({
      scope: { type: 'filter', node_types: ['function', 'type'] },
    });
    expect(result.valid).toBe(true);
  });

  it('accepts test_gaps scope', () => {
    const result = validateViewQuery({ scope: { type: 'test_gaps' } });
    expect(result.valid).toBe(true);
  });

  it('accepts a diff scope', () => {
    const result = validateViewQuery({
      scope: { type: 'diff', from_commit: 'abc1234', to_commit: 'def5678' },
    });
    expect(result.valid).toBe(true);
  });

  it('accepts a concept scope', () => {
    const result = validateViewQuery({
      scope: {
        type: 'concept',
        seed_nodes: ['Auth'],
        expand_edges: ['calls', 'contains'],
        expand_depth: 3,
        expand_direction: 'both',
      },
    });
    expect(result.valid).toBe(true);
  });

  it('accepts a full query with all optional sections', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      emphasis: {
        highlight: { matched: { color: '#ff0000', label: 'hit' } },
        dim_unmatched: 0.3,
        tiered_colors: ['red', 'orange', 'yellow'],
        heat: { metric: 'complexity', palette: 'blue-red' },
        badges: { template: '{{count}} calls', metric: 'incoming_calls' },
      },
      edges: { filter: ['calls', 'contains'], exclude: ['field_of'] },
      zoom: 'fit',
      annotation: { title: 'Test', description: 'A test view' },
      groups: [{ name: 'core', nodes: ['Auth', 'User'], color: 'blue', label: 'Core' }],
      callouts: [{ node: 'Auth', text: 'Entry point', color: 'green' }],
      narrative: [
        { node: 'Auth', text: 'Start here', order: 1 },
        { node: 'User', text: 'Then here', order: 2 },
      ],
    });
    expect(result.valid).toBe(true);
    expect(result.errors).toEqual([]);
  });

  it('accepts zoom as a level object', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      zoom: { level: 2.5 },
    });
    expect(result.valid).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// Invalid queries — structural
// ---------------------------------------------------------------------------
describe('validateViewQuery — invalid queries (structural)', () => {
  it('rejects null', () => {
    const result = validateViewQuery(null);
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('non-null object');
  });

  it('rejects a string', () => {
    const result = validateViewQuery('not a query');
    expect(result.valid).toBe(false);
  });

  it('rejects an array', () => {
    const result = validateViewQuery([1, 2, 3]);
    expect(result.valid).toBe(false);
  });

  it('rejects a query with no scope', () => {
    const result = validateViewQuery({ emphasis: {} });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('scope');
  });

  it('rejects an unknown scope type', () => {
    const result = validateViewQuery({ scope: { type: 'unknown_scope' } });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('unknown_scope');
  });
});

// ---------------------------------------------------------------------------
// Invalid queries — scope-specific
// ---------------------------------------------------------------------------
describe('validateViewQuery — scope-specific errors', () => {
  it('rejects focus scope with empty node', () => {
    const result = validateViewQuery({ scope: { type: 'focus', node: '' } });
    expect(result.valid).toBe(false);
    expect(result.errors).toEqual(
      expect.arrayContaining([expect.stringContaining('node')])
    );
  });

  it('rejects focus scope with depth > 100', () => {
    const result = validateViewQuery({
      scope: { type: 'focus', node: 'Foo', depth: 200 },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('200');
    expect(result.errors[0]).toContain('100');
  });

  it('rejects focus scope with invalid direction', () => {
    const result = validateViewQuery({
      scope: { type: 'focus', node: 'Foo', direction: 'sideways' },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('sideways');
  });

  it('rejects concept scope with empty seed_nodes', () => {
    const result = validateViewQuery({
      scope: { type: 'concept', seed_nodes: [] },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('seed_nodes');
  });

  it('rejects concept scope with expand_depth > 100', () => {
    const result = validateViewQuery({
      scope: { type: 'concept', seed_nodes: ['A'], expand_depth: 150 },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('150');
  });

  it('rejects concept scope with invalid expand_direction', () => {
    const result = validateViewQuery({
      scope: { type: 'concept', seed_nodes: ['A'], expand_direction: 'upward' },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('upward');
  });

  it('rejects diff scope with empty from_commit', () => {
    const result = validateViewQuery({
      scope: { type: 'diff', from_commit: '', to_commit: 'abc1234' },
    });
    expect(result.valid).toBe(false);
    expect(result.errors).toEqual(
      expect.arrayContaining([expect.stringContaining('from_commit')])
    );
  });

  it('rejects diff scope with empty to_commit', () => {
    const result = validateViewQuery({
      scope: { type: 'diff', from_commit: 'abc1234', to_commit: '' },
    });
    expect(result.valid).toBe(false);
    expect(result.errors).toEqual(
      expect.arrayContaining([expect.stringContaining('to_commit')])
    );
  });

  it('rejects diff scope with identical commits', () => {
    const result = validateViewQuery({
      scope: { type: 'diff', from_commit: 'abc1234', to_commit: 'abc1234' },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('identical');
  });
});

// ---------------------------------------------------------------------------
// Invalid queries — edge types
// ---------------------------------------------------------------------------
describe('validateViewQuery — edge type validation', () => {
  it('rejects unknown edge types in edges.filter', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      edges: { filter: ['calls', 'invented_edge'] },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('invented_edge');
  });

  it('rejects unknown edge types in edges.exclude', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      edges: { exclude: ['bad_type'] },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('bad_type');
  });

  it('rejects unknown edge types in focus scope edges', () => {
    const result = validateViewQuery({
      scope: { type: 'focus', node: 'Foo', edges: ['not_a_real_edge'] },
    });
    expect(result.valid).toBe(false);
    expect(result.errors).toEqual(
      expect.arrayContaining([expect.stringContaining('not_a_real_edge')])
    );
  });

  it('accepts edge types case-insensitively', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      edges: { filter: ['Calls', 'CONTAINS', 'Depends_On'] },
    });
    expect(result.valid).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// Invalid queries — emphasis
// ---------------------------------------------------------------------------
describe('validateViewQuery — emphasis validation', () => {
  it('rejects dim_unmatched below 0', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      emphasis: { dim_unmatched: -0.5 },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('dim_unmatched');
  });

  it('rejects dim_unmatched above 1', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      emphasis: { dim_unmatched: 1.5 },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('dim_unmatched');
  });

  it('accepts dim_unmatched at boundaries', () => {
    expect(validateViewQuery({
      scope: { type: 'all' },
      emphasis: { dim_unmatched: 0.0 },
    }).valid).toBe(true);

    expect(validateViewQuery({
      scope: { type: 'all' },
      emphasis: { dim_unmatched: 1.0 },
    }).valid).toBe(true);
  });

  it('rejects empty tiered_colors array', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      emphasis: { tiered_colors: [] },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('tiered_colors');
  });

  it('rejects invalid colors in tiered_colors', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      emphasis: { tiered_colors: ['red', 'not-a-color-at-all'] },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('not-a-color-at-all');
  });

  it('rejects unknown heat metric', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      emphasis: { heat: { metric: 'invented_metric' } },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('invented_metric');
  });

  it('accepts known heat metrics', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      emphasis: { heat: { metric: 'complexity' } },
    });
    expect(result.valid).toBe(true);
  });

  it('rejects invalid highlight.matched.color', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      emphasis: { highlight: { matched: { color: 'not_a_color' } } },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('not_a_color');
  });

  it('rejects badge template exceeding 500 chars', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      emphasis: { badges: { template: 'x'.repeat(501) } },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('501');
  });
});

// ---------------------------------------------------------------------------
// Invalid queries — zoom
// ---------------------------------------------------------------------------
describe('validateViewQuery — zoom validation', () => {
  it('rejects unknown zoom name', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      zoom: 'unknown_zoom',
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('unknown_zoom');
  });

  it('rejects number-as-string zoom', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      zoom: '2.5',
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('looks like a number');
  });

  it('rejects zoom level below 0.05', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      zoom: { level: 0.01 },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('out of range');
  });

  it('rejects zoom level above 20', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      zoom: { level: 25.0 },
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('out of range');
  });

  it('accepts zoom level at boundaries', () => {
    expect(validateViewQuery({
      scope: { type: 'all' },
      zoom: { level: 0.05 },
    }).valid).toBe(true);

    expect(validateViewQuery({
      scope: { type: 'all' },
      zoom: { level: 20.0 },
    }).valid).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// Color validation (via callouts and groups)
// ---------------------------------------------------------------------------
describe('validateViewQuery — color validation', () => {
  it('accepts hex colors', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      callouts: [{ node: 'A', text: 'test', color: '#ff0000' }],
    });
    expect(result.valid).toBe(true);
  });

  it('accepts short hex colors', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      callouts: [{ node: 'A', text: 'test', color: '#f00' }],
    });
    expect(result.valid).toBe(true);
  });

  it('accepts rgba hex colors', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      callouts: [{ node: 'A', text: 'test', color: '#ff000080' }],
    });
    expect(result.valid).toBe(true);
  });

  it('accepts named CSS colors', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      groups: [{ name: 'g', color: 'cornflowerblue' }],
    });
    expect(result.valid).toBe(true);
  });

  it('accepts rgb() function', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      callouts: [{ node: 'A', text: 'test', color: 'rgb(255, 0, 0)' }],
    });
    expect(result.valid).toBe(true);
  });

  it('accepts hsl() function', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      callouts: [{ node: 'A', text: 'test', color: 'hsl(120, 50%, 50%)' }],
    });
    expect(result.valid).toBe(true);
  });

  it('rejects invalid color strings in callouts', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      callouts: [{ node: 'A', text: 'test', color: 'notacolor' }],
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('notacolor');
  });

  it('rejects invalid color strings in groups', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      groups: [{ name: 'g', color: 'rgb(abc)' }],
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('rgb(abc)');
  });
});

// ---------------------------------------------------------------------------
// Narrative validation
// ---------------------------------------------------------------------------
describe('validateViewQuery — narrative validation', () => {
  it('rejects duplicate narrative step orders', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      narrative: [
        { node: 'A', text: 'First', order: 1 },
        { node: 'B', text: 'Second', order: 1 },
      ],
    });
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toContain('Duplicate');
    expect(result.errors[0]).toContain('order');
  });

  it('accepts narrative steps with unique orders', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      narrative: [
        { node: 'A', text: 'First', order: 1 },
        { node: 'B', text: 'Second', order: 2 },
      ],
    });
    expect(result.valid).toBe(true);
  });

  it('accepts narrative steps without order (unordered)', () => {
    const result = validateViewQuery({
      scope: { type: 'all' },
      narrative: [
        { node: 'A', text: 'First' },
        { node: 'B', text: 'Second' },
      ],
    });
    expect(result.valid).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// Multiple errors reported
// ---------------------------------------------------------------------------
describe('validateViewQuery — multiple errors', () => {
  it('reports multiple errors in a single query', () => {
    const result = validateViewQuery({
      scope: { type: 'focus', node: '', depth: 200, direction: 'sideways' },
      emphasis: { dim_unmatched: 5.0 },
      edges: { filter: ['bad_edge'] },
    });
    expect(result.valid).toBe(false);
    // Should have errors for: empty node, depth > 100, bad direction, dim_unmatched, bad edge
    expect(result.errors.length).toBeGreaterThanOrEqual(4);
  });
});
