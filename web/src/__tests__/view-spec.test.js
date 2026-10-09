/**
 * Tests for the ViewSpec grammar (ui-layout.md §4).
 *
 * The validation here is the client-side mirror of the server-side
 * `validate_view_spec` in crates/gyre-common/src/view_spec.rs — the same
 * cases must fail both places (belt and suspenders).
 */
import { describe, it, expect } from 'vitest';
import { validateViewSpec, isViewSpec, LAYOUT_TYPES } from '../lib/types/view-spec.ts';

// The complete example from ui-layout.md §4 "Structure".
const SPEC_EXAMPLE = {
  name: 'How authentication works',
  description: 'Authentication flow from request to identity resolution',
  data: {
    concept: 'auth',
    node_types: ['Module', 'Function', 'Type', 'Endpoint'],
    edge_types: ['Contains', 'Implements', 'RoutesTo'],
    depth: 2,
    filter: { min_churn: 0, spec_path: null, visibility: null },
    repo_id: null,
  },
  layout: 'hierarchical',
  encoding: {
    color: { field: 'node_type', scale: 'categorical' },
    size: { field: 'churn_count_30d', scale: 'linear', range: [24, 64] },
    border: {
      field: 'spec_confidence',
      scale: { high: '#22c55e', medium: '#eab308', low: '#f97316', none: '#ef4444' },
    },
    opacity: { field: 'visibility', scale: { public: 1.0, private: 0.4 } },
    label: 'qualified_name',
    group_by: 'file_path',
  },
  annotations: [
    { node_name: 'require_auth_middleware', text: 'Entry point — validates all tokens' },
    { node_name: 'AuthenticatedAgent', text: 'Resolves caller identity from JWT' },
  ],
  highlight: { spec_path: 'specs/system/identity-security.md' },
  explanation: 'Authentication flows through require_auth_middleware which validates...',
};

/** The side-by-side composition example from ui-layout.md §4. */
const SIDE_BY_SIDE_EXAMPLE = {
  name: 'Spec realization',
  data: { node_types: [], edge_types: [], depth: 1 },
  layout: 'side-by-side',
  left: {
    data: { repo_id: 'repo-1', filter: { spec_path: 'system/payment-retry.md' } },
    layout: 'list',
    encoding: { label: 'name', color: { field: 'node_type' } },
  },
  right: {
    data: { repo_id: 'repo-1', node_types: ['Type', 'Function'] },
    layout: 'hierarchical',
  },
};

describe('ViewSpec grammar — spec examples', () => {
  it('accepts the full §4 Structure example', () => {
    const result = validateViewSpec(SPEC_EXAMPLE);
    expect(result.errors).toEqual([]);
    expect(result.valid).toBe(true);
    expect(isViewSpec(SPEC_EXAMPLE)).toBe(true);
  });

  it('accepts the §4 side-by-side composition example', () => {
    const result = validateViewSpec(SIDE_BY_SIDE_EXAMPLE);
    expect(result.errors).toEqual([]);
    expect(result.valid).toBe(true);
  });

  it('exposes the eight specced layout names in kebab-case', () => {
    expect([...LAYOUT_TYPES]).toEqual([
      'graph',
      'hierarchical',
      'layered',
      'list',
      'timeline',
      'side-by-side',
      'diff',
      'flow',
    ]);
  });
});

describe('ViewSpec grammar — rejection cases', () => {
  it('rejects a non-object spec', () => {
    const result = validateViewSpec('not a spec');
    expect(result.valid).toBe(false);
    expect(result.errors[0]).toMatch(/object/);
  });

  it('rejects a missing name', () => {
    const result = validateViewSpec({ ...SPEC_EXAMPLE, name: '' });
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/name/);
  });

  it('rejects an unknown layout name', () => {
    const result = validateViewSpec({ ...SPEC_EXAMPLE, layout: 'sankey' });
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/unknown layout/);
  });

  it('rejects flow layout without trace_source', () => {
    const result = validateViewSpec({ ...SPEC_EXAMPLE, layout: 'flow' });
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/flow.*trace_source/);
  });

  it('accepts flow layout with trace_source', () => {
    const result = validateViewSpec({
      ...SPEC_EXAMPLE,
      layout: 'flow',
      data: { ...SPEC_EXAMPLE.data, trace_source: { mr_id: 'mr-47' } },
    });
    expect(result.errors).toEqual([]);
  });

  it('rejects filter.spec_path without repo_id', () => {
    const result = validateViewSpec({
      ...SPEC_EXAMPLE,
      data: { ...SPEC_EXAMPLE.data, filter: { spec_path: 'system/payment-retry.md' } },
    });
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/spec_path requires data\.repo_id/);
  });

  it('rejects side-by-side missing the right sub-view', () => {
    const { right, ...leftOnly } = SIDE_BY_SIDE_EXAMPLE;
    void right;
    const result = validateViewSpec(leftOnly);
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/left.*right|right.*left/);
  });

  it('rejects side-by-side sub-view that is itself side-by-side (nesting depth > 1)', () => {
    const result = validateViewSpec({
      ...SIDE_BY_SIDE_EXAMPLE,
      left: { ...SIDE_BY_SIDE_EXAMPLE.left, layout: 'side-by-side' },
    });
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/cannot contain side-by-side/);
  });

  it('rejects top-level-only fields on a sub-view', () => {
    const result = validateViewSpec({
      ...SIDE_BY_SIDE_EXAMPLE,
      left: {
        ...SIDE_BY_SIDE_EXAMPLE.left,
        name: 'sub-view name',
        annotations: [{ node_name: 'a', text: 'b' }],
        explanation: 'leaked field',
      },
    });
    expect(result.valid).toBe(false);
    const joined = result.errors.join();
    expect(joined).toMatch(/name/);
    expect(joined).toMatch(/annotations/);
    expect(joined).toMatch(/explanation/);
  });

  it('does not inherit parent repo_id into sub-views (no field inheritance)', () => {
    // Parent declares repo_id, but the LEFT sub-view's filter.spec_path must
    // still fail — sub-views don't inherit the parent's data fields.
    const result = validateViewSpec({
      ...SIDE_BY_SIDE_EXAMPLE,
      data: { node_types: [], edge_types: [], depth: 1, repo_id: 'repo-1' },
      left: {
        data: { filter: { spec_path: 'system/payment-retry.md' } },
        layout: 'list',
      },
    });
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/spec_path requires data\.repo_id/);
  });

  it('rejects a flow sub-view without trace_source', () => {
    const result = validateViewSpec({
      ...SIDE_BY_SIDE_EXAMPLE,
      left: { data: { node_types: [] }, layout: 'flow' },
    });
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/flow.*trace_source/);
  });

  it('rejects a sub-view with a missing data layer', () => {
    const result = validateViewSpec({
      ...SIDE_BY_SIDE_EXAMPLE,
      left: { layout: 'list' },
    });
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/own data layer/);
  });

  it("rejects orphan 'left'/'right' on a non-side-by-side layout", () => {
    // `left`/`right` are meaningful only for 'side-by-side'; on any other
    // layout they smuggle nested content no renderer consumes (mirror of
    // the Rust validate_view_spec rule).
    const result = validateViewSpec({
      ...SIDE_BY_SIDE_EXAMPLE,
      layout: 'list',
    });
    expect(result.valid).toBe(false);
    expect(result.errors.join()).toMatch(/only allowed with layout 'side-by-side'/);
  });
});
