/**
 * Predefined flow templates (system-explorer.md §5 Flow Traces):
 *
 *   "Predefined flows for common operations (agent spawn, MR merge, spec
 *    approval). Users can trace custom flows by selecting a starting
 *    function."
 *
 * Custom flows are the canvas's causal trace (start a trace from any node);
 * these templates are the curated entry points offered in the View
 * selector's "Trace flows" section. A template names a starting point and
 * the expected call path through the knowledge graph — each step carries
 * name patterns matched against graph nodes and the governing spec, the
 * "annotated with spec linkage" the spec requires.
 *
 * Selecting a template scopes the flow layout to the matched path; when a
 * gate trace exists for a matching MR, its spans animate as particles over
 * that path (spans never map onto the path → no particles, never fabricated
 * ones).
 */

/**
 * Split an identifier into lowercase word tokens: snake_case, kebab-case,
 * path segments, and camelCase boundaries all break.
 * @param {string} s
 * @returns {string[]}
 */
export function tokenize(s) {
  return String(s ?? '')
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .split(/[^a-zA-Z0-9]+/)
    .filter(Boolean)
    .map((t) => t.toLowerCase());
}

/**
 * Whether a graph node matches a step pattern. A pattern matches when it
 * equals a whole token of the node's name/qualified_name ('spawn' matches
 * 'spawn_agent', 'agent_spawn', 'SpawnAgent' — not 'unsawn'), or when the
 * pattern is itself multi-word and appears as a substring ('merge_queue'
 * in 'run_merge_queue_now').
 *
 * @param {any} node
 * @param {string} pattern
 * @returns {boolean}
 */
export function nodeMatchesPattern(node, pattern) {
  const p = String(pattern ?? '').trim().toLowerCase();
  if (!p) return false;
  const name = String(node?.name ?? '');
  const qname = String(node?.qualified_name ?? '');
  if (name.toLowerCase() === p || qname.toLowerCase() === p) return true;
  const tokens = tokenize(`${name} ${qname}`);
  if (tokens.includes(p)) return true;
  if (p.includes('_') || p.includes('-') || p.includes(' ')) {
    const lower = `${name} ${qname}`.toLowerCase();
    if (lower.includes(p)) return true;
  }
  return false;
}

// Node types that never represent a call-path step: spec/file nodes are
// documentation artifacts, test nodes are not the production path.
const NON_STEP_TYPES = new Set(['spec', 'file', 'test']);

/**
 * Resolve one template step to the graph nodes it matches, in graph order.
 *
 * @param {{patterns: string[]}} step
 * @param {any[]} nodes
 * @returns {any[]}
 */
export function matchFlowStep(step, nodes) {
  if (!step?.patterns?.length || !Array.isArray(nodes)) return [];
  const out = [];
  for (const n of nodes) {
    if (!n?.id || NON_STEP_TYPES.has(String(n.node_type ?? '').toLowerCase())) continue;
    if (step.patterns.some((p) => nodeMatchesPattern(n, p))) out.push(n);
  }
  return out;
}

/**
 * Resolve a whole template against a graph: every step (the starting point
 * first) is matched, node ids deduped across steps in path order.
 *
 * @param {any} template
 * @param {any[]} nodes
 * @returns {{template: any, steps: Array<{step: any, nodes: any[]}>, matchedNodeIds: string[], matchedNodes: any[]}}
 */
export function matchFlowTemplate(template, nodes) {
  const steps = [];
  const seen = new Set();
  const matchedNodes = [];
  const all = [template?.start, ...(template?.steps ?? [])].filter(Boolean);
  for (const step of all) {
    const matched = matchFlowStep(step, nodes);
    steps.push({ step, nodes: matched });
    for (const n of matched) {
      if (!seen.has(n.id)) {
        seen.add(n.id);
        matchedNodes.push(n);
      }
    }
  }
  return { template, steps, matchedNodeIds: [...seen], matchedNodes };
}

/**
 * The ViewQuery for an applied flow template: a concept scope seeded with
 * the matched path nodes (depth 0 — the view is the template's expected
 * call path itself), annotated with the expected call path and its spec
 * linkage. Rendered by the flow layout (FlowCanvas), which filters its
 * node set by the query scope.
 *
 * @param {any} template
 * @param {ReturnType<typeof matchFlowTemplate>} match
 * @returns {any} ViewQuery
 */
export function flowTemplateQuery(template, match) {
  const chain = [template?.start, ...(template?.steps ?? [])]
    .filter(Boolean)
    .map((s) => s.label)
    .join(' → ');
  const specNote = template?.spec ? ` Spec: ${template.spec}` : '';
  return {
    scope: {
      type: 'concept',
      seed_nodes: [...(match?.matchedNodeIds ?? [])],
      expand_edges: ['calls', 'contains'],
      // Depth 0 — the flow view is the template's expected call path
      // itself; one-hop context would pull unrelated neighbors onto the
      // path (a caller of a path step is not part of the traced flow).
      expand_depth: 0,
    },
    annotation: {
      title: `${template?.label ?? 'Flow'} trace`,
      description: `Expected call path: ${chain}.${specNote}`,
    },
  };
}

/**
 * Pick the MR whose captured gate trace best backs a flow template: the
 * most recent MR (the list is newest-first) whose title, spec_ref, or
 * source branch matches one of the template's mr_match patterns; with no
 * match, the most recent traced MR — gate traces record the merge/gate
 * pipeline, the closest real data, and spans that do not map onto the
 * template's path simply produce no particles.
 *
 * @param {any} template
 * @param {any[]} mrs MRs with captured traces, newest first
 * @returns {any|null}
 */
export function pickTraceMr(template, mrs) {
  if (!Array.isArray(mrs) || mrs.length === 0) return null;
  const patterns = (template?.mr_match ?? []).map((p) => String(p).toLowerCase());
  if (patterns.length > 0) {
    for (const mr of mrs) {
      if (!mr?.id) continue;
      const hay = `${mr.title ?? ''} ${mr.spec_ref ?? ''} ${mr.source_branch ?? ''}`.toLowerCase();
      if (patterns.some((p) => p && hay.includes(p))) return mr;
    }
  }
  return mrs.find((mr) => mr?.id) ?? null;
}

/**
 * The predefined flow templates (system-explorer.md §5). Steps are word
 * patterns matched against knowledge-graph node names; each flow cites its
 * governing spec and the MR patterns used to find a backing gate trace.
 *
 * @type {Array<{id: string, label: string, description: string, spec: string,
 *   start: {label: string, patterns: string[]},
 *   steps: Array<{label: string, patterns: string[]}>,
 *   mr_match: string[]}>}
 */
export const FLOW_TEMPLATES = [
  {
    id: 'agent-spawn',
    label: 'Agent spawn flow',
    description: 'What happens when an agent is spawned: task assignment, worktree, compute target, runtime start',
    spec: 'specs/system/agent-runtime.md',
    start: { label: 'Agent spawn request', patterns: ['spawn', 'create_agent'] },
    steps: [
      { label: 'Task assignment', patterns: ['task', 'assign', 'claim'] },
      { label: 'Worktree creation', patterns: ['worktree'] },
      { label: 'Compute target / container', patterns: ['compute', 'container', 'target'] },
      { label: 'Budget allocation', patterns: ['budget'] },
      { label: 'Agent runtime start', patterns: ['runtime', 'heartbeat', 'execute'] },
    ],
    mr_match: ['spawn', 'agent'],
  },
  {
    id: 'mr-merge',
    label: 'MR merge flow',
    description: 'What happens when a merge request merges: queue, gates, speculative merge, merge commit',
    spec: 'specs/system/forge-advantages.md',
    start: { label: 'Merge request', patterns: ['merge_request', 'merge'] },
    steps: [
      { label: 'Merge queue', patterns: ['queue', 'enqueue'] },
      { label: 'Gate execution', patterns: ['gate'] },
      { label: 'Speculative merge attempt', patterns: ['speculative', 'attempt'] },
      { label: 'Merge commit', patterns: ['merge', 'commit'] },
    ],
    mr_match: ['merge', 'mr'],
  },
  {
    id: 'spec-approval',
    label: 'Spec approval flow',
    description: 'What happens when a spec is approved: spec-edit branch, MR, approval, spec lifecycle check',
    spec: 'specs/system/spec-lifecycle.md',
    start: { label: 'Spec save', patterns: ['spec', 'save'] },
    steps: [
      { label: 'Spec-edit branch', patterns: ['branch'] },
      { label: 'Spec MR creation', patterns: ['merge_request', 'mr'] },
      { label: 'Approval', patterns: ['approve', 'approval', 'review'] },
      { label: 'Spec lifecycle check', patterns: ['lifecycle', 'reconcil'] },
    ],
    mr_match: ['spec', 'spec-edit'],
  },
];
