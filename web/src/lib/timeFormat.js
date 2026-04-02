/**
 * Shared time formatting utilities.
 *
 * Handles both epoch seconds (number) and ISO strings.
 */

/**
 * Convert a timestamp to epoch seconds.
 * Accepts: epoch seconds (number), epoch milliseconds (number > 1e12),
 * ISO string, or SystemTime object ({secs_since_epoch, nanos_since_epoch} or {tv_sec}).
 */
function toEpochSec(ts) {
  if (ts == null) return null;
  if (typeof ts === 'object') {
    // Rust SystemTime serialization
    if (ts.secs_since_epoch != null) return ts.secs_since_epoch;
    if (ts.tv_sec != null) return ts.tv_sec;
    return null;
  }
  if (typeof ts === 'string') {
    const ms = Date.parse(ts);
    return isNaN(ms) ? null : ms / 1000;
  }
  if (typeof ts === 'number') {
    // If > 1e12, assume milliseconds
    return ts > 1e12 ? ts / 1000 : ts;
  }
  return null;
}

/**
 * Format a timestamp as a relative time string.
 * "just now", "3m ago", "2h ago", "1d ago", "3w ago"
 */
export function relativeTime(ts) {
  const sec = toEpochSec(ts);
  if (sec == null) return '';
  const diff = Date.now() / 1000 - sec;
  if (diff < 0) return 'just now';
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 604800) return `${Math.floor(diff / 86400)}d ago`;
  return `${Math.floor(diff / 604800)}w ago`;
}

/**
 * Format a timestamp as a full locale date/time string (for tooltips).
 */
export function absoluteTime(ts) {
  const sec = toEpochSec(ts);
  if (sec == null) return '';
  return new Date(sec * 1000).toLocaleString();
}

/**
 * Format a duration between two timestamps.
 * Returns "45s", "3m", "1h 20m", etc.
 */
export function formatDuration(startTs, endTs) {
  const start = toEpochSec(startTs);
  const end = toEpochSec(endTs);
  if (start == null || end == null) return '';
  const sec = Math.round(Math.abs(end - start));
  if (sec < 60) return `${sec}s`;
  if (sec < 3600) return `${Math.floor(sec / 60)}m`;
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

/**
 * Format a timestamp as a short date string (e.g., "Mar 30, 2:15 PM").
 */
export function formatDate(ts) {
  const sec = toEpochSec(ts);
  if (sec == null) return '';
  const d = new Date(sec * 1000);
  const now = new Date();
  // Same day: time only
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  }
  // Same year: month + day + time
  if (d.getFullYear() === now.getFullYear()) {
    return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) +
      ', ' + d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  }
  // Different year: full date
  return d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}
