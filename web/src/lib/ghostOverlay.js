/**
 * ghostOverlay.js — shared ghost-overlay model (system-explorer.md §3
 * "Inline Spec Editing with Progressive Preview", fast tier).
 *
 * Pure functions: prediction normalization, the confidence→opacity map,
 * ghost positioning relative to existing canvas nodes, and predicted
 * ghost-edge derivation. Used by GhostOverlay.svelte (rendering),
 * ArchPreviewCanvas.svelte (canvas integration), EditorSplit.svelte
 * (debounced prediction trigger) and DetailPanel.svelte (arch tab) so
 * every surface renders the SAME ghost visual language — never divergent
 * copies.
 *
 * Wire forms accepted by `normalizeGhosts`:
 *   §3 prediction contract (POST /repos/:id/graph/predict):
 *     { action, node_type, name, node_id?, confidence, reason, changes?,
 *       parent_trait? }
 *   legacy overlay shape (pre-§3 DetailPanel/EditorSplit):
 *     { nodeId, type: 'new'|'modified'|'removed' }
 */

/** Ghost node body size — matches the ArchPreviewCanvas node box. */
export const GHOST_NODE_W = 64;
export const GHOST_NODE_H = 28;

const ADD_SYNONYMS = new Set(['add', 'added', 'new', 'create', 'created']);
const REMOVE_SYNONYMS = new Set(['remove', 'removed', 'delete', 'deleted']);
const MODIFY_SYNONYMS = new Set([
  'modify',
  'modified',
  'change',
  'changed',
  'update',
  'updated',
]);

function firstString(...values) {
  for (const v of values) {
    if (typeof v === 'string' && v.trim() !== '') return v;
    if (typeof v === 'number' && Number.isFinite(v)) return String(v);
  }
  return null;
}

/**
 * Map a prediction's action field to the §3 ghost action
 * ('add' | 'modify' | 'remove'). Accepts the §3 `action` field and the
 * legacy `change_type` / `type` synonyms. Wire forms with a free-form
 * `type` category and no recognizable action default to 'modify' — the
 * same keep-visible behavior the pre-§3 normalization had.
 */
export function ghostAction(prediction) {
  const raw = firstString(
    prediction?.action,
    prediction?.change_type,
    prediction?.type,
  );
  if (raw === null) return null;
  const key = raw.trim().toLowerCase();
  if (ADD_SYNONYMS.has(key)) return 'add';
  if (REMOVE_SYNONYMS.has(key)) return 'remove';
  if (MODIFY_SYNONYMS.has(key)) return 'modify';
  return 'modify';
}

/** Clamp a confidence value to [0, 1]; accepts 0-1 floats or percentages. */
export function clampConfidence(value) {
  const n = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(n)) return 0.5;
  // Whole-number percents ("85" for 85%) are a common wire form; a value
  // just above 1 (e.g. 1.2) is over-unity noise and clamps to 1.
  if (n > 1 && n >= 10 && n <= 100) return n / 100;
  return Math.min(1, Math.max(0, n));
}

/**
 * §3 opacity mapping: "higher confidence = more opaque ghost" with the
 * documented anchors — 0.3 opacity at 50% confidence, 1.0 at 100%
 * (linear between the anchors; below 50% clamps to a faint 0.15 floor so
 * a low-confidence ghost stays visible at all). Rounded to 4 decimals so
 * the anchor points land exactly (1.4·1−0.4 is 0.999… in float math).
 */
export function ghostOpacity(confidence) {
  const c = clampConfidence(confidence);
  return Math.round(Math.min(1, Math.max(0.15, 1.4 * c - 0.4)) * 10000) / 10000;
}

/**
 * Normalize prediction/overlay inputs into the canonical ghost shape:
 *   { id, nodeId, name, action, rawType, nodeType, confidence, reason,
 *     changes, parentTrait }
 *
 * Drops entries with neither a name nor a node id (nothing to render or
 * position). Idempotent: a canonical ghost normalizes to an equivalent
 * ghost, so callers may normalize caller-supplied and locally derived
 * overlays through the same function.
 */
export function normalizeGhosts(input) {
  if (!Array.isArray(input)) return [];
  const ghosts = [];
  for (let i = 0; i < input.length; i++) {
    const p = input[i];
    if (!p || typeof p !== 'object') continue;
    const action = ghostAction(p);
    if (!action) continue;
    const nodeId = firstString(p.node_id, p.nodeId);
    const name = firstString(p.name, p.qualified_name, p.node_name, p.label);
    if (!nodeId && !name) continue;
    ghosts.push({
      id: firstString(p.id) ?? `ghost-${i}-${nodeId ?? name}`,
      nodeId: nodeId ?? null,
      name: name ?? nodeId,
      action,
      rawType: firstString(p.change_type, p.type, p.action) ?? action,
      nodeType: firstString(p.node_type, p.nodeType, p.kind),
      confidence: clampConfidence(p.confidence),
      reason: firstString(p.reason, p.description) ?? '',
      changes: Array.isArray(p.changes)
        ? p.changes.map((c) => (typeof c === 'string' ? c : JSON.stringify(c)))
        : [],
      parentTrait: firstString(p.parent_trait, p.parentTrait, p.parent),
    });
  }
  return ghosts;
}

/** Legacy alias — the EditorSplit/DetailPanel prediction path. */
export const normalizePredictions = normalizeGhosts;

/**
 * Position ghosts relative to the existing canvas nodes
 * (system-explorer.md §3 canvas integration):
 *   - modify/remove ghosts overlay the existing node's position (node id,
 *     else name match);
 *   - add ghosts are placed near their predicted parent (parent_trait
 *     matched against existing node names), staggered so siblings do not
 *     stack;
 *   - ghosts with no resolvable anchor go to free space on the canvas.
 *
 * `nodePositions` maps existing node id → {x, y}; `nodes` is the existing
 * node object array (for name matching). Returns ghost id → {x, y}.
 */
export function positionGhosts(ghosts, nodePositions = {}, nodes = []) {
  const byName = new Map();
  for (const n of nodes) {
    for (const key of [n?.name, n?.label, n?.qualified_name]) {
      if (key != null && !byName.has(String(key))) byName.set(String(key), n);
    }
  }
  const occupied = Object.values(nodePositions).map((p) => ({ ...p }));
  const positions = {};

  const resolveExistingId = (g) => {
    if (g.nodeId && nodePositions[g.nodeId]) return g.nodeId;
    const n = byName.get(String(g.name));
    if (n && nodePositions[n.id]) return n.id;
    return null;
  };

  ghosts.forEach((g, idx) => {
    if (g.action === 'modify' || g.action === 'remove') {
      const existingId = resolveExistingId(g);
      if (existingId) {
        positions[g.id] = { ...nodePositions[existingId] };
        return;
      }
      positions[g.id] = freeSlot(occupied, idx);
      occupied.push(positions[g.id]);
      return;
    }
    // add: near the predicted parent, staggered per sibling index.
    const parent =
      g.parentTrait != null ? byName.get(String(g.parentTrait)) : null;
    if (parent && nodePositions[parent.id]) {
      const base = nodePositions[parent.id];
      const candidate = {
        x: base.x + GHOST_NODE_W + 26 + (idx % 3) * 18,
        y: base.y + GHOST_NODE_H / 2 + (idx % 2) * 34 - 8,
      };
      positions[g.id] = isFree(candidate, occupied, 46)
        ? candidate
        : freeSlot(occupied, idx);
    } else {
      positions[g.id] = freeSlot(occupied, idx);
    }
    occupied.push(positions[g.id]);
  });
  return positions;
}

/**
 * Predicted new relationships (§3 "Dotted edge, green — New relationship
 * predicted"): one dotted-green edge per add ghost that names a parent
 * trait/type resolvable to an existing node. Returns
 * [{ id, from: {x,y}, to: {x,y} }].
 */
export function ghostEdges(ghosts, ghostPositions, nodePositions = {}, nodes = []) {
  const byName = new Map();
  for (const n of nodes) {
    for (const key of [n?.name, n?.label, n?.qualified_name]) {
      if (key != null && !byName.has(String(key))) byName.set(String(key), n);
    }
  }
  const edges = [];
  for (const g of ghosts) {
    if (g.action !== 'add' || g.parentTrait == null) continue;
    let parentId = null;
    const parent = byName.get(String(g.parentTrait));
    if (parent && nodePositions[parent.id]) {
      parentId = parent.id;
    } else if (nodePositions[g.parentTrait]) {
      parentId = g.parentTrait;
    }
    const to = ghostPositions[g.id];
    if (!parentId || !to) continue;
    edges.push({
      id: `ghost-edge-${g.id}`,
      from: { ...nodePositions[parentId] },
      to: { ...to },
    });
  }
  return edges;
}

function isFree(candidate, occupied, minDistance) {
  return occupied.every(
    (p) => Math.hypot(p.x - candidate.x, p.y - candidate.y) > minDistance,
  );
}

/**
 * Deterministic free-space slot: scans a coarse grid for the first point
 * clear of every occupied position. Falls back to a stair-step offset so
 * a result always exists.
 */
function freeSlot(occupied, seed = 0) {
  for (let c = 0; c < 14; c++) {
    for (let r = 0; r < 14; r++) {
      const candidate = {
        x: 80 + c * 150 + (seed % 3) * 14,
        y: 60 + r * 90,
      };
      if (isFree(candidate, occupied, 70)) return candidate;
    }
  }
  return { x: 80 + (seed % 8) * 42, y: 60 + seed * 32 };
}
