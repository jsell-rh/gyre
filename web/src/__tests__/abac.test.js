/**
 * abac.test.js — unit tests for the shared ABAC UI helpers
 * (abac-policy-engine.md §UI, task-132).
 *
 * The conflict detector is the load-bearing piece: it must flag pairs of
 * enabled policies whose scope + resource_type + action coverage overlaps
 * with conflicting effects (the spec's "unexpected interactions"), and
 * must NOT flag pairs that cannot match the same request.
 */

import { describe, it, expect } from 'vitest';
import {
  ALL_ATTRIBUTES,
  CONDITION_OPERATORS,
  attributeSuggestions,
  detectPolicyConflicts,
} from '../lib/abac.js';

function policy(overrides = {}) {
  return {
    id: 'p-' + Math.random().toString(36).slice(2),
    name: 'policy',
    scope: 'tenant',
    scope_id: null,
    priority: 100,
    effect: 'allow',
    conditions: [],
    actions: ['*'],
    resource_types: ['*'],
    enabled: true,
    built_in: false,
    immutable: false,
    ...overrides,
  };
}

describe('attribute catalog', () => {
  it('covers Subject, Resource, and Environment categories from §Attributes', () => {
    const categories = new Set(ALL_ATTRIBUTES.map((a) => a.category));
    expect([...categories].sort()).toEqual(['Environment', 'Resource', 'Subject']);
  });

  it('includes the spec-named attributes', () => {
    const names = ALL_ATTRIBUTES.map((a) => a.name);
    for (const expected of [
      'subject.type',
      'subject.workspace_role',
      'subject.team_ids',
      'subject.repo_scope',
      'subject.chain_depth',
      'resource.type',
      'resource.workspace_id',
      'resource.approval_status',
      'env.budget_remaining',
      'env.main_health',
    ]) {
      expect(names).toContain(expected);
    }
  });

  it('lists all 8 spec condition operators', () => {
    expect(CONDITION_OPERATORS.sort()).toEqual([
      'contains', 'equals', 'exists', 'greater_than', 'in', 'less_than', 'not_equals', 'not_in',
    ]);
  });
});

describe('attributeSuggestions', () => {
  it('prefix match ranks first and substring matches are included', () => {
    const suggestions = attributeSuggestions('workspace');
    expect(suggestions.length).toBeGreaterThan(0);
    expect(suggestions[0].name.startsWith('subject.workspace')).toBe(true);
    expect(suggestions.some((a) => a.name === 'resource.workspace_id')).toBe(true);
  });

  it('matches on description text', () => {
    const suggestions = attributeSuggestions('merge-queue');
    expect(suggestions.some((a) => a.name === 'env.main_health')).toBe(true);
  });

  it('empty query returns the whole catalog', () => {
    expect(attributeSuggestions('').length).toBe(ALL_ATTRIBUTES.length);
    expect(attributeSuggestions(null).length).toBe(ALL_ATTRIBUTES.length);
  });
});

describe('detectPolicyConflicts', () => {
  it('flags overlapping scope+resource+action with conflicting effects', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', name: 'allow-push', effect: 'allow', actions: ['push'], resource_types: ['repo'], scope: 'workspace', scope_id: 'ws-1' }),
      policy({ id: 'b', name: 'deny-push', effect: 'deny', actions: ['push', 'read'], resource_types: ['repo', 'agent'], scope: 'workspace', scope_id: 'ws-1' }),
    ]);
    expect(conflicts.length).toBe(1);
    expect(conflicts[0].policyIds.sort()).toEqual(['a', 'b']);
    expect(conflicts[0].actions).toEqual(['push']);
    expect(conflicts[0].resourceTypes).toEqual(['repo']);
    expect(conflicts[0].explanation).toContain('allow-push');
    expect(conflicts[0].explanation).toContain('deny-push');
    expect(conflicts[0].explanation).toContain('workspace:ws-1');
  });

  it('does not flag pairs with the same effect', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', effect: 'allow' }),
      policy({ id: 'b', effect: 'allow' }),
    ]);
    expect(conflicts).toEqual([]);
  });

  it('does not flag non-overlapping actions', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', effect: 'allow', actions: ['push'] }),
      policy({ id: 'b', effect: 'deny', actions: ['merge'] }),
    ]);
    expect(conflicts).toEqual([]);
  });

  it('does not flag non-overlapping resource types', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', effect: 'allow', resource_types: ['repo'] }),
      policy({ id: 'b', effect: 'deny', resource_types: ['spec'] }),
    ]);
    expect(conflicts).toEqual([]);
  });

  it('wildcard actions/resource types overlap everything', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', effect: 'allow', actions: ['*'], resource_types: ['repo'] }),
      policy({ id: 'b', effect: 'deny', actions: ['read'], resource_types: ['*'] }),
    ]);
    expect(conflicts.length).toBe(1);
    expect(conflicts[0].actions).toEqual(['read']);
    expect(conflicts[0].resourceTypes).toEqual(['repo']);
  });

  it('same scope with different scope_ids does not overlap', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', effect: 'allow', scope: 'workspace', scope_id: 'ws-1' }),
      policy({ id: 'b', effect: 'deny', scope: 'workspace', scope_id: 'ws-2' }),
    ]);
    expect(conflicts).toEqual([]);
  });

  it('same scope, one unscoped (null scope_id), overlaps', () => {
    // A null scope_id at a level applies to every entity at that level
    // (policy_engine::scope_applies), so it overlaps the scoped sibling.
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', effect: 'allow', scope: 'workspace', scope_id: null }),
      policy({ id: 'b', effect: 'deny', scope: 'workspace', scope_id: 'ws-2' }),
    ]);
    expect(conflicts.length).toBe(1);
  });

  it('cross-scope: unscoped broader policy overlaps narrower scope', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', effect: 'allow', scope: 'tenant', scope_id: null }),
      policy({ id: 'b', effect: 'deny', scope: 'workspace', scope_id: 'ws-2' }),
    ]);
    expect(conflicts.length).toBe(1);
    expect(conflicts[0].explanation).toContain('tenant');
  });

  it('cross-scope: scoped tenant policy is not flagged against other scopes (client cannot resolve ownership)', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', effect: 'allow', scope: 'tenant', scope_id: 't-1' }),
      policy({ id: 'b', effect: 'deny', scope: 'workspace', scope_id: 'ws-2' }),
    ]);
    expect(conflicts).toEqual([]);
  });

  it('disabled policies are excluded', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', effect: 'allow', enabled: false }),
      policy({ id: 'b', effect: 'deny' }),
    ]);
    expect(conflicts).toEqual([]);
  });

  it('explanation reports the expected winner by priority (§Policy Composition)', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', name: 'low-allow', effect: 'allow', priority: 100 }),
      policy({ id: 'b', name: 'high-deny', effect: 'deny', priority: 200 }),
    ]);
    expect(conflicts[0].explanation).toContain('"high-deny" (deny) wins');
  });

  it('equal priority: deny wins (§Policy Composition rule 3)', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', name: 'same-allow', effect: 'allow', priority: 100 }),
      policy({ id: 'b', name: 'same-deny', effect: 'deny', priority: 100 }),
    ]);
    expect(conflicts[0].explanation).toContain('"same-deny" (deny) wins');
  });

  it('equal priority, different scopes: more specific scope wins (repo > workspace > tenant)', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', name: 'ws-allow', effect: 'allow', priority: 100, scope: 'workspace', scope_id: null }),
      policy({ id: 'b', name: 'repo-deny', effect: 'deny', priority: 100, scope: 'repo', scope_id: 'r-1' }),
    ]);
    expect(conflicts[0].explanation).toContain('"repo-deny" (deny) wins');
  });

  it('immutable deny wins over a higher-priority allow', () => {
    const conflicts = detectPolicyConflicts([
      policy({ id: 'a', name: 'big-allow', effect: 'allow', priority: 999 }),
      policy({ id: 'b', name: 'imm-deny', effect: 'deny', priority: 1, immutable: true }),
    ]);
    expect(conflicts[0].explanation).toContain('"imm-deny" (deny) wins');
    expect(conflicts[0].explanation).toContain('immutable');
  });

  it('tolerates missing / non-array input', () => {
    expect(detectPolicyConflicts(null)).toEqual([]);
    expect(detectPolicyConflicts(undefined)).toEqual([]);
    expect(detectPolicyConflicts('nope')).toEqual([]);
  });
});
