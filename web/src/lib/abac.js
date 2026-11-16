/**
 * abac.js — shared ABAC UI helpers for the policy management surface
 * (abac-policy-engine.md §UI).
 *
 * - Attribute catalog for the policy editor's condition-builder autocomplete
 *   (mirrors the §Attributes table of the spec).
 * - Known operators / actions / resource types for the editor pickers.
 * - Client-side policy conflict detection (§UI "Policy conflict detector"):
 *   highlights pairs of enabled policies whose scope + resource_type +
 *   action coverage overlaps with conflicting effects.
 *
 * Pure functions only — no imports — so both components and unit tests
 * exercise the same logic.
 */

/**
 * Subject attributes (abac-policy-engine.md §Attributes).
 * `examples` feeds the condition-builder value hints.
 */
export const SUBJECT_ATTRIBUTES = [
  { name: 'subject.type', category: 'Subject', description: 'Auth context subject type', examples: 'user, agent, system' },
  { name: 'subject.id', category: 'Subject', description: 'Auth context subject identity', examples: 'user:jsell, agent:worker-42' },
  { name: 'subject.tenant_id', category: 'Subject', description: 'Subject tenant (auth context / OIDC)', examples: 'tenant-acme' },
  { name: 'subject.global_role', category: 'Subject', description: 'Global user role', examples: 'TenantAdmin, Member' },
  { name: 'subject.workspace_role', category: 'Subject', description: 'Role in the target workspace', examples: 'Owner, Admin, Developer, Viewer' },
  { name: 'subject.workspace_ids', category: 'Subject', description: 'Workspaces the subject belongs to (list)', examples: '["ws-platform"]' },
  { name: 'subject.team_ids', category: 'Subject', description: 'Teams the subject belongs to (list)', examples: '["team-platform"]' },
  { name: 'subject.persona', category: 'Subject', description: 'Agent persona (agent OIDC claim)', examples: 'security, accountability' },
  { name: 'subject.stack_hash', category: 'Subject', description: 'Agent stack hash (agent OIDC claim)', examples: 'sha256:abc123...' },
  { name: 'subject.attestation_level', category: 'Subject', description: 'Agent attestation level (agent OIDC claim)', examples: '1, 2, 3' },
  { name: 'subject.repo_scope', category: 'Subject', description: 'Agent repo scope (agent OIDC claim)', examples: 'repo:gyre-server' },
  { name: 'subject.chain_depth', category: 'Subject', description: 'Attestation chain depth (0 = root)', examples: '0, 1, 2' },
  { name: 'subject.root_signer', category: 'Subject', description: 'Root signer identity of the attestation chain', examples: 'user:jsell' },
  { name: 'subject.constraint_count', category: 'Subject', description: 'Accumulated constraint count (explicit + gate)', examples: '5' },
];

/** Resource attributes (§Attributes). */
export const RESOURCE_ATTRIBUTES = [
  { name: 'resource.type', category: 'Resource', description: 'Request resource type', examples: 'spec, task, mr, repo, agent' },
  { name: 'resource.id', category: 'Resource', description: 'Resource id from the request path', examples: 'system/identity-security.md' },
  { name: 'resource.tenant_id', category: 'Resource', description: 'Resource tenant (entity lookup)', examples: 'tenant-acme' },
  { name: 'resource.workspace_id', category: 'Resource', description: 'Resource workspace (entity lookup)', examples: 'ws-platform' },
  { name: 'resource.repo_id', category: 'Resource', description: 'Resource repo (entity lookup)', examples: 'repo-gyre-server' },
  { name: 'resource.owner', category: 'Resource', description: 'Resource owner (entity field)', examples: 'user:jsell' },
  { name: 'resource.team', category: 'Resource', description: 'Resource team (entity field)', examples: 'team:platform' },
  { name: 'resource.approval_status', category: 'Resource', description: 'Resource approval status (entity field)', examples: 'pending, approved' },
  { name: 'resource.visibility', category: 'Resource', description: 'Resource visibility (entity field)', examples: 'private, workspace, tenant' },
];

/** Environment attributes (§Attributes). */
export const ENV_ATTRIBUTES = [
  { name: 'env.time', category: 'Environment', description: 'Current timestamp (system clock)', examples: 'epoch seconds' },
  { name: 'env.ip', category: 'Environment', description: 'Client IP (socket peer)', examples: '203.0.113.7' },
  { name: 'env.budget_remaining', category: 'Environment', description: 'Tokens remaining in the agent budget', examples: '50000' },
  { name: 'env.main_health', category: 'Environment', description: 'Forge merge-queue health', examples: 'green, red' },
];

/** Full autocomplete catalog, grouped Subject / Resource / Environment. */
export const ALL_ATTRIBUTES = [...SUBJECT_ATTRIBUTES, ...RESOURCE_ATTRIBUTES, ...ENV_ATTRIBUTES];

/** Condition operators (§Conditions — the 8 ConditionOp variants). */
export const CONDITION_OPERATORS = [
  'equals',
  'not_equals',
  'in',
  'not_in',
  'greater_than',
  'less_than',
  'contains',
  'exists',
];

/**
 * Known action names (§Attributes action sources + the /policies/effective
 * sample set). '*' = all actions.
 */
export const KNOWN_ACTIONS = [
  '*', 'read', 'write', 'delete', 'approve', 'spawn', 'push', 'merge', 'generate',
];

/**
 * Known resource types (§Attributes resource.type values + the resource
 * types the route registry maps). '*' = all resource types.
 */
export const KNOWN_RESOURCE_TYPES = [
  '*', 'spec', 'task', 'mr', 'repo', 'agent', 'persona', 'explorer_view',
  'message', 'conversation', 'attestation', 'gate', 'team', 'workspace',
  'user', 'secret', 'policy', 'budget', 'compute_target', 'meta_spec',
];

/**
 * Filter the attribute catalog for autocomplete suggestions.
 * Matches on name prefix first, then substring anywhere (name or
 * description), so "work" finds subject.workspace_role.
 * @param {string} query partial attribute path
 * @returns {Array} catalog entries relevant to the query
 */
export function attributeSuggestions(query) {
  const q = (query ?? '').trim().toLowerCase();
  if (!q) return ALL_ATTRIBUTES;
  const starts = ALL_ATTRIBUTES.filter((a) => a.name.toLowerCase().startsWith(q));
  const contains = ALL_ATTRIBUTES.filter(
    (a) => !a.name.toLowerCase().startsWith(q) &&
      (a.name.toLowerCase().includes(q) || a.description.toLowerCase().includes(q))
  );
  return [...starts, ...contains];
}

/** Scope specificity for conflict resolution (repo > workspace > tenant). */
const SCOPE_SPECIFICITY = { tenant: 0, workspace: 1, repo: 2 };

function normScope(scope) {
  return typeof scope === 'string' ? scope.toLowerCase() : 'tenant';
}

function normEffect(effect) {
  return typeof effect === 'string' ? effect.toLowerCase() : 'allow';
}

/**
 * Do two wildcard lists overlap? '*' in either list overlaps everything.
 * Applies to both `actions` and `resource_types` (§Policy Entity: "*" = all).
 */
function listsOverlap(a, b) {
  const la = Array.isArray(a) && a.length > 0 ? a : ['*'];
  const lb = Array.isArray(b) && b.length > 0 ? b : ['*'];
  if (la.includes('*') || lb.includes('*')) return true;
  return la.some((x) => lb.includes(x));
}

/** Intersection of two wildcard lists ('*' expands to the other side). */
function listIntersection(a, b) {
  const la = Array.isArray(a) && a.length > 0 ? a : ['*'];
  const lb = Array.isArray(b) && b.length > 0 ? b : ['*'];
  if (la.includes('*')) return [...lb];
  if (lb.includes('*')) return [...la];
  return la.filter((x) => lb.includes(x));
}

/**
 * Can both policies apply to the same request, per the engine's canonical
 * scope filter (policy_engine::scope_applies)?
 * - Same scope: overlap unless both carry different non-null scope_ids
 *   (a null scope_id at a level applies to every entity at that level).
 * - Different scopes: the broader policy (tenant > workspace > repo)
 *   applies to everything the narrower one covers when its scope_id is
 *   null; a scoped broader policy may also overlap, but that depends on
 *   entity ownership the client cannot see, so it is not flagged.
 */
function scopesOverlap(a, b) {
  const sa = normScope(a.scope);
  const sb = normScope(b.scope);
  if (sa === sb) {
    if (a.scope_id == null || b.scope_id == null) return true;
    return String(a.scope_id) === String(b.scope_id);
  }
  const [broad, narrow] = SCOPE_SPECIFICITY[sa] <= SCOPE_SPECIFICITY[sb] ? [a, b] : [b, a];
  return broad.scope_id == null;
}

/**
 * Which of two overlapping policies would win an evaluation, mirroring the
 * engine's composition rules (§Policy Composition): immutable Deny first,
 * then higher priority, then scope specificity (repo > workspace > tenant),
 * then Deny over Allow.
 */
function resolveWinner(a, b) {
  const aDeny = normEffect(a.effect) === 'deny';
  const bDeny = normEffect(b.effect) === 'deny';
  if (a.immutable && aDeny && !(b.immutable && bDeny)) return a;
  if (b.immutable && bDeny && !(a.immutable && aDeny)) return b;
  if ((a.priority ?? 0) !== (b.priority ?? 0)) return (a.priority ?? 0) > (b.priority ?? 0) ? a : b;
  const sa = SCOPE_SPECIFICITY[normScope(a.scope)];
  const sb = SCOPE_SPECIFICITY[normScope(b.scope)];
  if (sa !== sb) return sa > sb ? a : b;
  return aDeny ? a : b;
}

function scopeLabel(p) {
  const scope = normScope(p.scope);
  return p.scope_id == null || p.scope_id === '' ? scope : `${scope}:${p.scope_id}`;
}

/**
 * Detect policy conflicts (§UI "Policy conflict detector").
 *
 * A conflict is a pair of ENABLED policies that can both match the same
 * request (overlapping scope + resource_types + actions, per the engine's
 * scope filter and wildcard semantics) with opposing effects — the
 * "unexpected interactions" the spec asks to highlight.
 *
 * @param {Array} policies loaded policy objects (server serialization)
 * @returns {Array<{policyIds: string[], aName: string, bName: string,
 *    actions: string[], resourceTypes: string[], scope: string,
 *    explanation: string}>} conflict descriptions
 */
export function detectPolicyConflicts(policies) {
  if (!Array.isArray(policies)) return [];
  const active = policies.filter((p) => p && p.enabled !== false);
  const conflicts = [];
  for (let i = 0; i < active.length; i++) {
    for (let j = i + 1; j < active.length; j++) {
      const a = active[i];
      const b = active[j];
      if (normEffect(a.effect) === normEffect(b.effect)) continue;
      if (!scopesOverlap(a, b)) continue;
      if (!listsOverlap(a.actions, b.actions)) continue;
      if (!listsOverlap(a.resource_types, b.resource_types)) continue;
      const winner = resolveWinner(a, b);
      const loser = winner === a ? b : a;
      const actions = listIntersection(a.actions, b.actions);
      const resourceTypes = listIntersection(a.resource_types, b.resource_types);
      const scope = scopeLabel(a.scope === b.scope ? a : (SCOPE_SPECIFICITY[normScope(a.scope)] <= SCOPE_SPECIFICITY[normScope(b.scope)] ? a : b));
      const explanation =
        `"${a.name}" (${normEffect(a.effect)}, priority ${a.priority ?? 0}) and ` +
        `"${b.name}" (${normEffect(b.effect)}, priority ${b.priority ?? 0}) both match ` +
        `${actions.join(', ')} on ${resourceTypes.join(', ')} within scope ${scope}. ` +
        `Expected outcome: "${winner.name}" (${normEffect(winner.effect)}) wins — ` +
        `${winner.immutable ? 'immutable Deny is evaluated first' : `higher precedence than "${loser.name}"`}.`;
      conflicts.push({
        policyIds: [a.id, b.id].filter(Boolean),
        aName: a.name,
        bName: b.name,
        actions,
        resourceTypes,
        scope,
        explanation,
      });
    }
  }
  return conflicts;
}
