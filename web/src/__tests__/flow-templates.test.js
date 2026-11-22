/**
 * task-184 Predefined flow templates (system-explorer.md §5 Flow Traces):
 * pure matching/query-building logic for the curated flows (agent spawn,
 * MR merge, spec approval) — pattern matching against graph nodes, the
 * ViewQuery a template produces, and the MR whose captured gate trace
 * backs the template.
 */
import { describe, it, expect } from 'vitest';
import {
  FLOW_TEMPLATES,
  tokenize,
  nodeMatchesPattern,
  matchFlowStep,
  matchFlowTemplate,
  flowTemplateQuery,
  pickTraceMr,
} from '../lib/flow-templates.js';

const N = (id, name, node_type = 'function', extra = {}) => ({
  id,
  node_type,
  name,
  qualified_name: `api::${name}`,
  ...extra,
});

describe('flow-templates — pattern matching', () => {
  it('tokenizes snake_case, camelCase and path-qualified names', () => {
    expect(tokenize('spawn_agent')).toEqual(['spawn', 'agent']);
    expect(tokenize('createWorktree')).toEqual(['create', 'worktree']);
    expect(tokenize('api::merge_queue_v2')).toEqual(['api', 'merge', 'queue', 'v2']);
    expect(tokenize('')).toEqual([]);
  });

  it('matches whole tokens only — a partial substring is not a match', () => {
    expect(nodeMatchesPattern(N('n1', 'spawn_agent'), 'spawn')).toBe(true);
    expect(nodeMatchesPattern(N('n2', 'agent_spawn'), 'spawn')).toBe(true);
    expect(nodeMatchesPattern(N('n3', 'SpawnAgent'), 'spawn')).toBe(true);
    expect(nodeMatchesPattern(N('n4', 'unsawn_thing'), 'spawn')).toBe(false);
    expect(nodeMatchesPattern(N('n5', 'spawn_agent'), 'worktree')).toBe(false);
  });

  it('matches multi-word patterns as substrings', () => {
    expect(nodeMatchesPattern(N('n1', 'run_merge_queue_now'), 'merge_queue')).toBe(true);
    expect(nodeMatchesPattern(N('n1', 'run_merge_queue_now'), 'queue_merge')).toBe(false);
  });

  it('excludes spec/file/test node types from step matches', () => {
    const nodes = [
      N('s1', 'merge', 'spec'),
      N('f1', 'merge', 'file'),
      N('t1', 'merge', 'test'),
      N('m1', 'merge', 'function'),
    ];
    const matched = matchFlowStep({ patterns: ['merge'] }, nodes);
    expect(matched.map((n) => n.id)).toEqual(['m1']);
  });
});

describe('flow-templates — template matching', () => {
  it('offers exactly the three spec-mandated flows, each with a starting point and call path', () => {
    expect(FLOW_TEMPLATES.map((t) => t.id)).toEqual(['agent-spawn', 'mr-merge', 'spec-approval']);
    for (const t of FLOW_TEMPLATES) {
      expect(t.start?.patterns?.length).toBeGreaterThan(0);
      expect(t.steps?.length).toBeGreaterThan(0);
      expect(t.spec).toMatch(/^specs\/system\//);
      // Every step names what it matches — no assertionless steps.
      for (const s of [t.start, ...t.steps]) {
        expect(s.label).toBeTruthy();
        expect(s.patterns.length).toBeGreaterThan(0);
      }
    }
  });

  it('resolves every step to its graph nodes and dedupes across steps in path order', () => {
    const nodes = [
      N('spawn', 'spawn_agent'),
      N('assign', 'assign_task'),
      N('worktree', 'create_worktree'),
      N('container', 'create_container'),
      N('budget', 'allocate_budget'),
      N('runtime', 'start_runtime'),
      N('unrelated', 'format_string'),
    ];
    const tpl = {
      id: 'x', label: 'X',
      start: { label: 'Entry', patterns: ['spawn'] },
      steps: [
        { label: 'A', patterns: ['assign'] },
        { label: 'B', patterns: ['worktree'] },
        // Same node matched by two steps — deduped, not duplicated.
        { label: 'C', patterns: ['container', 'spawn'] },
      ],
    };
    const match = matchFlowTemplate(tpl, nodes);
    expect(match.steps.map((s) => s.nodes.map((n) => n.id))).toEqual([
      ['spawn'],
      ['assign'],
      ['worktree'],
      ['spawn', 'container'],
    ]);
    expect(match.matchedNodeIds).toEqual(['spawn', 'assign', 'worktree', 'container']);
    expect(match.matchedNodeIds).not.toContain('unrelated');
  });

  it('an empty match reports zero nodes — never fabricates a path', () => {
    const match = matchFlowTemplate(FLOW_TEMPLATES[0], [N('x', 'format_string')]);
    expect(match.matchedNodeIds).toEqual([]);
  });
});

describe('flow-templates — ViewQuery construction', () => {
  it('builds a concept query seeded with the matched path nodes and a spec-linked annotation', () => {
    const nodes = [N('spawn', 'spawn_agent'), N('worktree', 'create_worktree')];
    const tpl = {
      id: 'x', label: 'Agent spawn flow', spec: 'specs/system/agent-runtime.md',
      start: { label: 'Agent spawn request', patterns: ['spawn'] },
      steps: [{ label: 'Worktree creation', patterns: ['worktree'] }],
    };
    const q = flowTemplateQuery(tpl, matchFlowTemplate(tpl, nodes));
    // Concept scope: seeds are the matched node ids (resolvable by the
    // canvas's concept matcher); depth 0 — the view is the exact expected
    // call path, no neighborhood expansion.
    expect(q.scope.type).toBe('concept');
    expect(q.scope.seed_nodes).toEqual(['spawn', 'worktree']);
    expect(q.scope.expand_edges).toEqual(['calls', 'contains']);
    expect(q.scope.expand_depth).toBe(0);
    // Annotation names the flow, the expected call path, and its spec.
    expect(q.annotation.title).toBe('Agent spawn flow trace');
    expect(q.annotation.description).toContain('Agent spawn request → Worktree creation');
    expect(q.annotation.description).toContain('specs/system/agent-runtime.md');
  });
});

describe('flow-templates — trace MR selection', () => {
  const MRS = [
    { id: 'mr-newest', title: 'Update README', source_branch: 'docs', spec_ref: null },
    { id: 'mr-spec', title: 'Edit thing', source_branch: 'spec-edit/payment-retry-ab12', spec_ref: 'specs/system/payment-retry.md@sha' },
    { id: 'mr-spawn', title: 'Spawn agent for task 12', source_branch: 'task-12', spec_ref: null },
  ];

  it('prefers the most recent MR matching the template patterns', () => {
    const tpl = { mr_match: ['spawn', 'agent'] };
    expect(pickTraceMr(tpl, MRS).id).toBe('mr-spawn');
    const specTpl = { mr_match: ['spec'] };
    expect(pickTraceMr(specTpl, MRS).id).toBe('mr-spec');
  });

  it('falls back to the most recent traced MR when nothing matches', () => {
    expect(pickTraceMr({ mr_match: ['nonexistent'] }, MRS).id).toBe('mr-newest');
    expect(pickTraceMr({}, MRS).id).toBe('mr-newest');
  });

  it('returns null for an empty list — no fabricated trace source', () => {
    expect(pickTraceMr({ mr_match: ['spawn'] }, [])).toBeNull();
    expect(pickTraceMr({}, null)).toBeNull();
  });
});
