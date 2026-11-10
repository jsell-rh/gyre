/**
 * Client-side cache of the `X-Gyre-Last-Seen` response header (HSI §3 Change View).
 *
 * The server's `last_seen_middleware` stamps `X-Gyre-Last-Seen: <epoch_seconds>`
 * on workspace-scoped responses (at most once per 60s per user+workspace —
 * only responses that actually fired a tracking upsert carry the header).
 *
 * The Change View answers "what changed since my last visit?", so the `since`
 * value must be the last-seen timestamp from the PREVIOUS visit — not the one
 * the current visit just produced. This module therefore:
 *
 *   1. Snapshots the persisted value once per page load (module init, before
 *      any response of this visit can overwrite it) — that snapshot is the
 *      end of the previous visit.
 *   2. Updates the persisted value whenever a response carries a NEWER header
 *      (older or equal values are ignored — the 60s debounce means repeats).
 *
 * First-ever visit (no persisted value): falls back to 24h ago, consistent
 * with the Briefing's `since` fallback (HSI §9).
 */

const STORAGE_KEY = 'gyre_last_seen';
const FALLBACK_WINDOW_SECS = 24 * 3600;

let sessionStartValue = null;
let initialized = false;

function readStored() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (typeof parsed?.value !== 'number' || !Number.isFinite(parsed.value) || parsed.value <= 0) {
      return null;
    }
    return parsed;
  } catch {
    return null;
  }
}

function ensureInit() {
  if (initialized) return;
  initialized = true;
  sessionStartValue = readStored()?.value ?? null;
}

/**
 * Capture the header from an API response. Called by the shared `request()`
 * helper in api.js on every response (the header is only present when the
 * server actually tracked the visit, so this is cheap in practice).
 *
 * @param {Headers} headers - the fetch() response headers object.
 */
export function captureLastSeenHeader(headers) {
  ensureInit();
  let value = null;
  try {
    value = headers?.get?.('x-gyre-last-seen');
  } catch {
    return;
  }
  if (!value) return;
  const ts = Number(value);
  if (!Number.isFinite(ts) || ts <= 0) return;
  const stored = readStored();
  if (stored && stored.value >= ts) return;
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ value: ts, updated_at: Date.now() })
    );
  } catch {
    // localStorage unavailable (private mode, quota) — cache stays in memory-less
    // state; the Change View falls back to the 24h window.
  }
}

/**
 * The last-seen value cached before this visit's first response — i.e. the
 * timestamp of the previous visit. `null` on a first-ever visit.
 * @returns {number|null}
 */
export function getLastSeenAtVisitStart() {
  ensureInit();
  return sessionStartValue;
}

/**
 * The `since` epoch-seconds for the Change View query
 * (`GET /repos/:id/graph/timeline?since=<...>`): previous visit's last-seen,
 * else 24h ago (matching the Briefing fallback semantics).
 * @returns {number}
 */
export function getChangeViewSince() {
  ensureInit();
  if (sessionStartValue != null && sessionStartValue > 0) return sessionStartValue;
  return Math.floor(Date.now() / 1000) - FALLBACK_WINDOW_SECS;
}

/** Test-only: reset module state and persisted cache. */
export function __resetForTest() {
  initialized = false;
  sessionStartValue = null;
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // ignore
  }
}
