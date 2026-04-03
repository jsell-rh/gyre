/**
 * Shared entity name resolution module.
 *
 * Singleton reactive cache using Svelte 5 $state so resolved names persist
 * across component mount/unmount cycles (no more flicker on navigation).
 */
import { api } from './api.js';

// Module-level singleton cache — survives component destruction
let cache = $state({});

/**
 * Truncate a UUID to 8 characters with ellipsis.
 * Returns the full string if it's 12 chars or shorter.
 */
export function shortId(id) {
  if (!id) return '';
  return id.length > 12 ? id.slice(0, 8) : id;
}

/**
 * Format a git SHA for display — 7 chars, the git standard.
 */
export function formatSha(sha) {
  if (!sha) return '';
  return sha.slice(0, 7);
}

/**
 * Queue async name resolution for an entity.
 * Does nothing if the name is already cached or pending.
 * Uses queueMicrotask to defer state mutations — avoids Svelte 5's
 * state_unsafe_mutation error when called during template rendering.
 */
function queueNameResolution(type, id) {
  if (!id) return;
  const key = `${type}:${id}`;
  if (cache[key] !== undefined) return;

  queueMicrotask(() => {
    if (cache[key] !== undefined) return;
    // Mark as pending (null) to prevent duplicate fetches
    cache = { ...cache, [key]: null };

    try {
      const fetcher =
        type === 'agent' && api.agent ? api.agent(id).then(a => a?.name) :
        type === 'task' && api.task ? api.task(id).then(t => t?.title) :
        type === 'mr' && api.mergeRequest ? api.mergeRequest(id).then(m => m?.title) :
        type === 'repo' && api.repo ? api.repo(id).then(r => r?.name) :
        type === 'workspace' && api.workspace ? api.workspace(id).then(w => w?.name) :
        Promise.resolve(null);

      fetcher.then(name => {
        if (name) cache = { ...cache, [key]: name };
      }).catch(() => {});
    } catch {
      // API not available (e.g., in tests with partial mocks)
    }
  });
}

/**
 * Get a human-friendly name for an entity.
 * Returns the cached name if available, otherwise triggers async resolution
 * and returns a loading placeholder followed by short ID.
 *
 * For specs: returns last path segment without .md extension.
 */
export function entityName(type, id) {
  if (!id) return '';
  // Specs use path-based IDs — render them directly
  if (type === 'spec') {
    return id.split('/').pop()?.replace(/\.md$/, '') ?? id;
  }
  const key = `${type}:${id}`;
  const cached = cache[key];
  if (cached) {
    // Truncate long names for display
    return cached.length > 35 ? cached.slice(0, 32) + '\u2026' : cached;
  }
  // null means resolution in progress — show type-prefixed short ID
  if (cached === null) return formatId(type, id);
  queueNameResolution(type, id);
  return formatId(type, id);
}

/**
 * Format an entity ID for display with a type prefix.
 * Used when showing IDs in contexts where the type isn't otherwise visible.
 * Returns human-friendly short IDs like "MR #abc1234" or the cached name.
 */
export function formatId(type, id) {
  if (!id) return '';
  const prefixes = { mr: 'MR', task: 'Task', agent: 'Agent', spec: 'Spec', repo: 'Repo' };
  const prefix = prefixes[type] ?? '';
  // Check cache directly (avoid recursion through entityName)
  const key = `${type}:${id}`;
  const cached = cache[key];
  if (cached && cached !== null) {
    return cached.length > 35 ? cached.slice(0, 32) + '\u2026' : cached;
  }
  return prefix ? `${prefix} #${id.slice(0, 7)}` : shortId(id);
}

/**
 * Pre-seed the cache with a known name (e.g., from data already loaded).
 * Useful when you have entity data and want to avoid a redundant API call.
 */
export function seedEntityName(type, id, name) {
  if (!id || !name) return;
  const key = `${type}:${id}`;
  if (cache[key] && cache[key] !== null) return; // don't overwrite existing
  cache = { ...cache, [key]: name };
}

/**
 * Seed names from bulk-loaded entity arrays.
 * Avoids individual API calls for entities we already have data for.
 */
export function seedFromEntities(type, entities) {
  if (!Array.isArray(entities) || entities.length === 0) return;
  const updates = {};
  for (const e of entities) {
    const id = e.id;
    if (!id) continue;
    const key = `${type}:${id}`;
    if (cache[key] && cache[key] !== null) continue;
    const name =
      type === 'agent' ? (e.name ?? null) :
      type === 'task' ? (e.title ?? null) :
      type === 'mr' ? (e.title ?? null) :
      type === 'repo' ? (e.name ?? null) :
      type === 'workspace' ? (e.name ?? null) :
      null;
    if (name) updates[key] = name;
  }
  if (Object.keys(updates).length > 0) {
    cache = { ...cache, ...updates };
  }
}
