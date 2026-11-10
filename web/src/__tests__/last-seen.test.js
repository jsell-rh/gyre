/**
 * last-seen.test.js — client-side X-Gyre-Last-Seen cache (HSI §3 Change View).
 *
 * The Change View's `since` value must be the last-seen timestamp from the
 * PREVIOUS visit: the value persisted before this page load's first response
 * overwrites it. These tests pin that semantics:
 *   - capture stores newer header values (and ignores older/equal ones)
 *   - the session-start snapshot stays anchored to the pre-visit value even
 *     after this visit's responses arrive
 *   - first-ever visit falls back to ~24h ago (Briefing-consistent)
 */
import { describe, it, expect, beforeEach } from 'vitest';
import {
  captureLastSeenHeader,
  getChangeViewSince,
  getLastSeenAtVisitStart,
  __resetForTest,
} from '../lib/last-seen.js';

function fakeHeaders(value) {
  return new Map([['x-gyre-last-seen', String(value)]]);
}

beforeEach(() => {
  __resetForTest();
  localStorage.clear();
});

describe('captureLastSeenHeader', () => {
  it('persists a header value that is newer than the stored one', () => {
    captureLastSeenHeader(fakeHeaders(1000));
    expect(JSON.parse(localStorage.getItem('gyre_last_seen')).value).toBe(1000);
  });

  it('ignores a header value older than or equal to the stored one', () => {
    captureLastSeenHeader(fakeHeaders(2000));
    captureLastSeenHeader(fakeHeaders(1500)); // older — ignored
    captureLastSeenHeader(fakeHeaders(2000)); // equal — ignored
    expect(JSON.parse(localStorage.getItem('gyre_last_seen')).value).toBe(2000);
  });

  it('ignores missing, non-numeric, and non-positive values', () => {
    captureLastSeenHeader(fakeHeaders('abc'));
    captureLastSeenHeader(fakeHeaders(-5));
    captureLastSeenHeader(fakeHeaders(0));
    captureLastSeenHeader(new Map());
    expect(localStorage.getItem('gyre_last_seen')).toBeNull();
  });
});

describe('getChangeViewSince / getLastSeenAtVisitStart', () => {
  it('returns the pre-visit persisted value even after this visit updates it', () => {
    // Previous visit persisted 111.
    localStorage.setItem('gyre_last_seen', JSON.stringify({ value: 111, updated_at: 1 }));
    // First read initialises the session snapshot BEFORE any response arrives.
    expect(getLastSeenAtVisitStart()).toBe(111);
    expect(getChangeViewSince()).toBe(111);

    // This visit's first tracked response stamps a new value…
    captureLastSeenHeader(fakeHeaders(999999));
    // …but "since last visit" must stay anchored to the previous visit.
    expect(getLastSeenAtVisitStart()).toBe(111);
    expect(getChangeViewSince()).toBe(111);
    expect(JSON.parse(localStorage.getItem('gyre_last_seen')).value).toBe(999999);
  });

  it('falls back to ~24h ago on a first-ever visit (no persisted value)', () => {
    const before = Math.floor(Date.now() / 1000);
    const since = getChangeViewSince();
    const after = Math.floor(Date.now() / 1000);
    expect(since).toBeGreaterThanOrEqual(before - 24 * 3600);
    expect(since).toBeLessThanOrEqual(after - 24 * 3600 + 1);
    expect(getLastSeenAtVisitStart()).toBeNull();
  });

  it('ignores a corrupted persisted value and falls back', () => {
    localStorage.setItem('gyre_last_seen', 'not json{');
    expect(getLastSeenAtVisitStart()).toBeNull();
    expect(Number.isFinite(getChangeViewSince())).toBe(true);
  });
});
