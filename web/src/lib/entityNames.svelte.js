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
  return id.length > 12 ? id.slice(0, 8) + '\u2026' : id;
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
 * and returns a short ID as fallback.
 */
export function entityName(type, id) {
  if (!id) return '';
  const key = `${type}:${id}`;
  const cached = cache[key];
  if (cached) return cached;
  queueNameResolution(type, id);
  return shortId(id);
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
