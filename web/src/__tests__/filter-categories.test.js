/**
 * filter-categories.test.js — unit tests for the in-view filter panel's
 * classification and concept discovery (task-178, ui-navigation.md §3
 * "In-view filter panel": Boundaries, Interfaces, Data, Specs, Concepts).
 */
import { describe, it, expect } from 'vitest';
import {
  nodeCategory,
  discoverConcepts,
  nodeMatchesConcept,
  hiddenNodeIds,
  categoryCounts,
} from '../lib/filter-categories.js';

const NODES = [
  { id: 'pkg', node_type: 'package', name: 'payment-domain', qualified_name: 'payment-domain' },
  { id: 'mod', node_type: 'module', name: 'auth', qualified_name: 'payment-domain::auth' },
  { id: 'trait', node_type: 'trait', name: 'PaymentRetry', qualified_name: 'payment-domain::PaymentRetry' },
  { id: 'ep', node_type: 'endpoint', name: 'POST /payments', qualified_name: 'api::POST /payments' },
  { id: 'struct', node_type: 'struct', name: 'PaymentRequest', qualified_name: 'payment-domain::PaymentRequest' },
  { id: 'fn', node_type: 'function', name: 'create_payment', qualified_name: 'payment-domain::create_payment' },
  { id: 'spec', node_type: 'spec', name: 'payment-retry.md', qualified_name: 'specs/system/payment-retry.md' },
];

describe('nodeCategory', () => {
  it('classifies boundary, interface, data, and spec node types', () => {
    expect(nodeCategory(NODES[0])).toBe('boundaries'); // package
    expect(nodeCategory(NODES[1])).toBe('boundaries'); // module
    expect(nodeCategory(NODES[2])).toBe('interfaces'); // trait
    expect(nodeCategory(NODES[3])).toBe('interfaces'); // endpoint
    expect(nodeCategory(NODES[4])).toBe('data'); // struct
    expect(nodeCategory(NODES[6])).toBe('specs'); // spec
  });

  it('leaf implementation types belong to no category (stay visible regardless)', () => {
    expect(nodeCategory(NODES[5])).toBeNull(); // function
    expect(nodeCategory({ node_type: 'method' })).toBeNull();
    expect(nodeCategory(null)).toBeNull();
    expect(nodeCategory({})).toBeNull();
  });

  it('is case-insensitive on node_type', () => {
    expect(nodeCategory({ node_type: 'Package' })).toBe('boundaries');
    expect(nodeCategory({ node_type: 'TRAIT' })).toBe('interfaces');
  });
});

describe('discoverConcepts', () => {
  it('discovers cross-cutting name tokens shared by multiple nodes', () => {
    const concepts = discoverConcepts(NODES);
    const names = concepts.map((c) => c.name);
    // "payment" appears in package, trait, struct, function, spec names.
    const payment = concepts.find((c) => c.name === 'payment');
    expect(payment).toBeTruthy();
    expect(payment.count).toBeGreaterThanOrEqual(4);
    expect(names).not.toContain('create'); // verb, not a domain concept
  });

  it('excludes stopwords and single-occurrence tokens', () => {
    const concepts = discoverConcepts([
      { id: 'a', node_type: 'function', name: 'get_manager', qualified_name: 'x::get_manager' },
      { id: 'b', node_type: 'function', name: 'set_manager', qualified_name: 'x::set_manager' },
    ]);
    // "manager" is a stopword; "get"/"set" are stopwords; nothing remains.
    expect(concepts.map((c) => c.name)).toEqual([]);
  });

  it('caps the concept list and sorts by prevalence', () => {
    const many = Array.from({ length: 30 }, (_, i) => ({
      id: `n${i}`,
      node_type: 'function',
      name: `thing${i} shared${i % 2 ? '' : 'x'}`,
      qualified_name: `x::thing${i}`,
    }));
    const concepts = discoverConcepts(many, { maxConcepts: 5 });
    expect(concepts.length).toBeLessThanOrEqual(5);
    for (let i = 1; i < concepts.length; i++) {
      expect(concepts[i - 1].count).toBeGreaterThanOrEqual(concepts[i].count);
    }
  });
});

describe('nodeMatchesConcept', () => {
  it('matches case-insensitive substrings on name and qualified_name', () => {
    expect(nodeMatchesConcept(NODES[4], 'payment')).toBe(true); // name
    expect(nodeMatchesConcept(NODES[1], 'payment-domain')).toBe(true); // qualified_name
    expect(nodeMatchesConcept(NODES[3], 'ledger')).toBe(false);
    expect(nodeMatchesConcept(NODES[0], '')).toBe(false);
  });
});

describe('hiddenNodeIds', () => {
  it('hides nodes whose category checkbox is unchecked', () => {
    const hidden = hiddenNodeIds(NODES, { hiddenCategories: ['boundaries'] });
    expect(hidden.has('pkg')).toBe(true);
    expect(hidden.has('mod')).toBe(true);
    expect(hidden.has('struct')).toBe(false); // data still visible
    expect(hidden.has('fn')).toBe(false); // uncategorized stays visible
  });

  it('hides nodes matching no selected concept', () => {
    const hidden = hiddenNodeIds(NODES, { concepts: ['auth'] });
    expect(hidden.has('mod')).toBe(false); // matches "auth"
    expect(hidden.has('pkg')).toBe(true); // no "auth" in name
  });

  it('keeps a node visible when it matches any selected concept', () => {
    const hidden = hiddenNodeIds(NODES, { concepts: ['auth', 'ledger'] });
    expect(hidden.has('mod')).toBe(false); // matches "auth"
  });

  it('combines category and concept filters (intersection)', () => {
    // Concept-only: nodes matching no selected concept are hidden.
    const byConcept = hiddenNodeIds(NODES, { concepts: ['auth'] });
    expect(byConcept.has('mod')).toBe(false); // name "auth" matches
    expect(byConcept.has('pkg')).toBe(true); // "payment-domain" matches nothing

    // Combined: a node that matches the concept is still hidden when its
    // category checkbox is unchecked; a matching node in a checked category
    // stays visible.
    const combined = hiddenNodeIds(NODES, { hiddenCategories: ['data'], concepts: ['payment'] });
    expect(combined.has('struct')).toBe(true); // matches "payment" but data unchecked
    expect(combined.has('pkg')).toBe(false); // matches, boundaries checked
    expect(combined.has('trait')).toBe(false); // matches, interfaces checked
  });

  it('returns an empty set when nothing is filtered', () => {
    expect(hiddenNodeIds(NODES, {}).size).toBe(0);
    expect(hiddenNodeIds(NODES, { hiddenCategories: [], concepts: [] }).size).toBe(0);
    expect(hiddenNodeIds(null, { hiddenCategories: ['data'] }).size).toBe(0);
  });

  it('skips deleted nodes', () => {
    const nodes = [{ id: 'x', node_type: 'module', name: 'a', deleted_at: 123 }];
    expect(hiddenNodeIds(nodes, { hiddenCategories: ['boundaries'] }).size).toBe(0);
  });
});

describe('categoryCounts', () => {
  it('counts nodes per category', () => {
    expect(categoryCounts(NODES)).toEqual({
      boundaries: 2,
      interfaces: 2,
      data: 1,
      specs: 1,
    });
  });
});
