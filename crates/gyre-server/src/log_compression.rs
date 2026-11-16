//! Log compression background job (observability.md §Log Retention &
//! Compression).
//!
//! Spec: agent logs are kept 30 days and "compressed after 7 days via the
//! `log-compression` background job"; audit-event details are compressed
//! with the same age threshold (the task-105 contract). Deletion past the
//! retention window (365d audit / 30d agent logs / 90d eBPF activity
//! streams) is owned by the nightly `retention_cleanup` job
//! (business-continuity.md §5, task-207) — this job only shrinks storage,
//! it never deletes.
//!
//! Two surfaces:
//!
//! - **Audit-event details** — zstd-compressed in the audit repository
//!   (SQLite/Postgres `detail_zstd` column, mem adapter equivalent) via
//!   [`AuditRepository::compress_details_before`]. Every read path
//!   (`query`, `since_timestamp`) decompresses transparently, so the HTTP
//!   audit API and SIEM forwarding keep receiving the original payloads.
//! - **Agent log lines** — [`AgentLogBuffer`] keeps recent lines raw and
//!   folds lines older than the compression age into a per-agent
//!   zstd-compressed cold segment. `GET /api/v1/agents/{id}/logs` and the
//!   TTY replay decompress transparently, preserving line order and
//!   content.
//!
//! The job runs on a fixed interval from process start (default 24 h),
//! configurable via `GYRE_LOG_COMPRESSION_INTERVAL` (seconds).

use anyhow::{Context, Result};
use std::collections::HashMap;
use tokio::sync::Mutex;
use tracing::info;

use crate::AppState;

/// Job name as registered in the admin job registry
/// (`POST /api/v1/admin/jobs/{name}/run`). The spec names this job
/// `log-compression`.
pub const JOB_NAME: &str = "log-compression";

/// Default run interval: daily.
pub const DEFAULT_INTERVAL_SECS: u64 = 86_400;

/// Age at which log payloads are compressed (spec: "compressed after 7
/// days").
pub const COMPRESSION_AGE_SECS: u64 = 7 * 86_400;

/// Audit-detail rows compressed per `compress_details_before` batch.
const AUDIT_BATCH_SIZE: usize = 500;

/// Safety valve: at most 1000 batches (500k events) per job run, so a
/// huge backlog cannot pin the job forever.
const MAX_AUDIT_BATCHES: usize = 1_000;

/// Run interval in seconds, from `GYRE_LOG_COMPRESSION_INTERVAL`.
/// Unset, empty, unparsable, or zero values fall back to the default.
pub fn interval_secs() -> u64 {
    interval_from(
        std::env::var("GYRE_LOG_COMPRESSION_INTERVAL")
            .ok()
            .as_deref(),
    )
}

/// Pure interval resolution (unit-testable without touching process
/// env): `None`/unparsable/zero → default.
fn interval_from(value: Option<&str>) -> u64 {
    value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

/// Counts from one job run, for observability logging.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogCompressionReport {
    /// Audit-event rows newly compressed.
    pub audit_events_compressed: u64,
    /// Raw detail JSON bytes of those rows before compression.
    pub audit_bytes_before: u64,
    /// Compressed payload bytes of those rows after compression.
    pub audit_bytes_after: u64,
    /// Agent log lines newly folded into cold segments this run.
    pub agent_log_lines_compressed: u64,
    /// Raw frame bytes of those newly folded lines before compression.
    pub agent_log_bytes_before: u64,
    /// Compressed bytes those newly folded lines occupy after folding.
    pub agent_log_bytes_after: u64,
}

/// One pass of the log-compression job: compress audit-event details and
/// agent log lines older than [`COMPRESSION_AGE_SECS`], then log the
/// counts. Idempotent — already-compressed payloads are skipped by the
/// adapters, so a second run compresses nothing new.
pub async fn run_once(state: &AppState) -> Result<LogCompressionReport> {
    let now = crate::jobs::now_secs();
    let cutoff = now.saturating_sub(COMPRESSION_AGE_SECS);

    let mut report = LogCompressionReport::default();
    for _ in 0..MAX_AUDIT_BATCHES {
        let stats = state
            .audit
            .compress_details_before(cutoff, AUDIT_BATCH_SIZE)
            .await?;
        report.audit_events_compressed += stats.compressed;
        report.audit_bytes_before += stats.bytes_before;
        report.audit_bytes_after += stats.bytes_after;
        if (stats.compressed as usize) < AUDIT_BATCH_SIZE {
            break;
        }
    }

    let logs = state.agent_logs.compress_older_than(cutoff).await?;
    report.agent_log_lines_compressed = logs.lines;
    report.agent_log_bytes_before = logs.bytes_before;
    report.agent_log_bytes_after = logs.bytes_after;

    info!(
        job = JOB_NAME,
        audit_events_compressed = report.audit_events_compressed,
        audit_bytes_before = report.audit_bytes_before,
        audit_bytes_after = report.audit_bytes_after,
        agent_log_lines_compressed = report.agent_log_lines_compressed,
        agent_log_bytes_before = report.agent_log_bytes_before,
        agent_log_bytes_after = report.agent_log_bytes_after,
        "log compression pass complete (deletion past retention is owned by the \
         retention_cleanup job)"
    );
    Ok(report)
}

// ── Agent log buffer ──────────────────────────────────────────────────────────

/// Maximum lines retained per agent (hot + cold), matching the previous
/// plain-`Vec<String>` buffer cap.
const MAX_LINES_PER_AGENT: usize = 10_000;

/// Per-agent log buffer with a zstd-compressed cold segment
/// (observability.md §Log Retention & Compression).
///
/// Hot lines (newer than the compression age) are kept as raw
/// `"[ts] message"` strings — the SSE/TTY live paths append and broadcast
/// them at line rate. Lines older than the compression age are folded by
/// [`AgentLogBuffer::compress_older_than`] into a single zstd-compressed
/// cold segment per agent (length-prefixed frames, so lines containing
/// embedded newlines round-trip byte-for-byte). Reads
/// ([`AgentLogBuffer::get`], [`AgentLogBuffer::replay`]) decompress the
/// cold segment transparently and preserve overall line order: cold
/// lines first (oldest), then hot lines.
///
/// Ephemeral by design (the buffer is process-local log tail memory, not
/// durable state); the 30-day retention purge is driven by the
/// `retention_cleanup` job via [`AgentLogBuffer::purge_older_than`].
pub struct AgentLogBuffer {
    inner: Mutex<HashMap<String, AgentLogBuf>>,
}

#[derive(Default)]
struct AgentLogBuf {
    /// Compressed cold lines (oldest first). `None` until the first
    /// compression pass ages something out of `hot`.
    cold: Option<ColdSegment>,
    /// Raw lines, oldest first. New lines are appended at the back.
    hot: Vec<String>,
}

struct ColdSegment {
    /// zstd-compressed frame stream (one `u64`-LE length prefix + UTF-8
    /// bytes per line).
    bytes: Vec<u8>,
    line_count: usize,
    /// `(min, max)` of the `[ts]` prefixes in the segment; `None` min
    /// means a line has no parseable timestamp (retention never purges
    /// those, so the segment cannot be dropped wholesale).
    min_ts: Option<u64>,
    max_ts: Option<u64>,
}

/// Counts from one agent-log compression pass. All fields count only
/// the lines NEWLY folded in this pass (matching the audit-event half
/// of the report, which counts only newly compressed rows) — lines
/// already cold from an earlier pass are not re-counted, so a daily
/// run that folds one line into a large segment reports one line, not
/// the whole segment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentLogCompressionStats {
    /// Lines folded into cold segments this pass.
    pub lines: u64,
    /// Raw frame-stream bytes of those newly folded lines before
    /// compression (length prefix + UTF-8 bytes per line).
    pub bytes_before: u64,
    /// Compressed bytes those newly folded lines occupy (the zstd size
    /// of the segment built from just this pass's folded lines).
    pub bytes_after: u64,
}

/// Parse the `[ts]` prefix of a canonical agent log line. Returns `None`
/// for lines without a parseable prefix (kept by retention, never aged
/// into the cold segment).
pub(crate) fn line_timestamp(line: &str) -> Option<u64> {
    line.strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .and_then(|(ts, _)| ts.trim().parse::<u64>().ok())
}

impl ColdSegment {
    /// Decode the cold segment into lines. A corrupt segment is a hard
    /// error — log data must not silently vanish.
    fn decode(&self) -> Result<Vec<String>> {
        let raw =
            zstd::decode_all(self.bytes.as_slice()).context("decompress agent log cold segment")?;
        decode_frames(&raw)
    }
}

impl AgentLogBuf {
    /// Total logical lines (cold + hot).
    fn len(&self) -> usize {
        self.hot.len() + self.cold.as_ref().map_or(0, |c| c.line_count)
    }

    /// Decode the cold segment into lines. A corrupt segment is a hard
    /// error — log data must not silently vanish.
    fn cold_lines(&self) -> Result<Vec<String>> {
        match &self.cold {
            None => Ok(Vec::new()),
            Some(seg) => seg.decode(),
        }
    }

    /// Enforce the per-agent line cap by evicting the OLDEST lines —
    /// the same policy as the previous plain-`Vec<String>` buffer
    /// (keep the newest `MAX_LINES_PER_AGENT`). Lines are evicted in
    /// overall age order: from the front of the cold segment first
    /// (it holds the oldest lines), then from the front of `hot`. This
    /// keeps newly appended lines alive even when the cold segment
    /// alone is at the cap. A corrupt segment is kept (with a warning)
    /// rather than dropped — log data must not silently vanish.
    fn trim_to_cap(&mut self) {
        let surplus = self.len().saturating_sub(MAX_LINES_PER_AGENT);
        if surplus == 0 {
            return;
        }
        let cold_count = self.cold.as_ref().map_or(0, |c| c.line_count);
        let from_cold = surplus.min(cold_count);
        self.hot.drain(0..(surplus - from_cold));
        if from_cold == 0 {
            return;
        }
        let Some(cold) = self.cold.take() else {
            return;
        };
        match cold.decode() {
            Ok(lines) => {
                let kept: Vec<String> = lines.into_iter().skip(from_cold).collect();
                if kept.is_empty() {
                    // The whole segment was surplus; drop it.
                    return;
                }
                match build_cold(kept) {
                    Ok((seg, _)) => self.cold = Some(seg),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "agent log cold segment rebuild failed while trimming; \
                             keeping the previous segment"
                        );
                        self.cold = Some(cold);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "agent log cold segment decode failed while trimming; \
                     keeping the segment"
                );
                self.cold = Some(cold);
            }
        }
    }
}

/// Encode lines as a length-prefixed frame stream (one `u64`-LE length +
/// UTF-8 bytes per line). Framing — not newline joining — so lines that
/// contain embedded newlines round-trip byte-for-byte.
fn encode_frames(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines {
        let bytes = line.as_bytes();
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out
}

fn decode_frames(blob: &[u8]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < blob.len() {
        if blob.len() - i < 8 {
            anyhow::bail!("truncated frame header at byte {i}");
        }
        let len = u64::from_le_bytes(blob[i..i + 8].try_into().expect("8 byte slice")) as usize;
        i += 8;
        if blob.len() - i < len {
            anyhow::bail!("truncated frame body at byte {i}");
        }
        let line = String::from_utf8(blob[i..i + len].to_vec())
            .with_context(|| format!("frame at byte {i} is not UTF-8"))?;
        out.push(line);
        i += len;
    }
    Ok(out)
}

/// Build a cold segment from lines (oldest first). Returns the segment
/// and the raw (uncompressed) frame-stream byte length for stats.
fn build_cold(lines: Vec<String>) -> Result<(ColdSegment, usize)> {
    let line_count = lines.len();
    let mut min_ts = None;
    let mut max_ts = None;
    let mut has_unprefixed = false;
    for line in &lines {
        match line_timestamp(line) {
            Some(ts) => {
                min_ts = Some(min_ts.map_or(ts, |m: u64| m.min(ts)));
                max_ts = Some(max_ts.map_or(ts, |m: u64| m.max(ts)));
            }
            None => has_unprefixed = true,
        }
    }
    let raw = encode_frames(&lines);
    let bytes = zstd::encode_all(raw.as_slice(), 3).context("zstd compress agent log segment")?;
    let raw_len = raw.len();
    // Unprefixed lines are never purgeable, so the segment can never be
    // dropped wholesale — represent that with a `None` min timestamp.
    if has_unprefixed {
        min_ts = None;
    }
    Ok((
        ColdSegment {
            bytes,
            line_count,
            min_ts,
            max_ts,
        },
        raw_len,
    ))
}

impl AgentLogBuffer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for AgentLogBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentLogBuffer {
    /// Append a raw `"[ts] message"` line for an agent. The caller keeps
    /// ownership of the live-broadcast copy; this buffer is the replay
    /// surface.
    pub async fn append(&self, agent_id: String, line: String) {
        let mut logs = self.inner.lock().await;
        let buf = logs.entry(agent_id).or_default();
        buf.hot.push(line);
        buf.trim_to_cap();
    }

    /// Read up to `limit` lines for an agent, skipping the first
    /// `offset` (cold lines first, then hot — insertion order). Returns
    /// decompressed lines verbatim.
    pub async fn get(&self, agent_id: &str, offset: usize, limit: usize) -> Result<Vec<String>> {
        let logs = self.inner.lock().await;
        let Some(buf) = logs.get(agent_id) else {
            return Ok(Vec::new());
        };
        let mut lines = buf.cold_lines()?;
        lines.extend(buf.hot.iter().cloned());
        Ok(lines.into_iter().skip(offset).take(limit).collect())
    }

    /// All lines for an agent in insertion order (TTY replay).
    pub async fn replay(&self, agent_id: &str) -> Result<Vec<String>> {
        let logs = self.inner.lock().await;
        let Some(buf) = logs.get(agent_id) else {
            return Ok(Vec::new());
        };
        let mut lines = buf.cold_lines()?;
        lines.extend(buf.hot.iter().cloned());
        Ok(lines)
    }

    /// Retention purge (driven by the nightly `retention_cleanup` job,
    /// 30-day agent-log policy): drop lines whose `[ts]` prefix is older
    /// than `cutoff`. Lines without a parseable prefix are kept (same
    /// rule as the previous plain-Vec implementation). Returns the number
    /// of lines purged.
    pub async fn purge_older_than(&self, cutoff: u64) -> u64 {
        let mut logs = self.inner.lock().await;
        let mut purged = 0u64;
        for buf in logs.values_mut() {
            let before = buf.hot.len();
            buf.hot.retain(|line| !line_is_old(line, cutoff));
            purged += (before - buf.hot.len()) as u64;

            if let Some(cold) = buf.cold.take() {
                match cold.min_ts {
                    // Whole segment is old — drop it without decompressing.
                    Some(min) if min < cutoff && cold.max_ts.is_some_and(|m| m < cutoff) => {
                        purged += cold.line_count as u64;
                    }
                    // Nothing in the segment is old — keep it as-is.
                    Some(min) if min >= cutoff => {
                        buf.cold = Some(cold);
                    }
                    // Mixed (or unprefixed lines present): decompress,
                    // filter per line, rebuild.
                    _ => {
                        let decoded = zstd::decode_all(cold.bytes.as_slice())
                            .map_err(|e| anyhow::anyhow!("zstd decompress agent log segment: {e}"))
                            .and_then(|raw| decode_frames(&raw));
                        match decoded {
                            Ok(lines) => {
                                let kept: Vec<String> = lines
                                    .into_iter()
                                    .filter(|l| !line_is_old(l, cutoff))
                                    .collect();
                                purged += (cold.line_count - kept.len()) as u64;
                                if !kept.is_empty() {
                                    match build_cold(kept) {
                                        Ok((seg, _)) => buf.cold = Some(seg),
                                        Err(e) => {
                                            tracing::warn!(
                                                error = %e,
                                                "agent log cold segment rebuild failed; \
                                                 keeping the previous segment"
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    "agent log cold segment decode failed during purge; \
                                     keeping the segment"
                                );
                            }
                        }
                    }
                }
            }
        }
        purged
    }

    /// Compression pass (driven by the `log-compression` job): fold hot
    /// lines older than `cutoff` into the per-agent cold segment.
    /// Already-cold lines stay compressed. Idempotent: lines that moved
    /// in an earlier pass are no longer hot, and only `[ts]`-prefixed
    /// (old) lines are ever folded — unprefixed lines always stay hot.
    /// Stats count only the lines newly folded THIS pass (see
    /// [`AgentLogCompressionStats`]).
    pub async fn compress_older_than(&self, cutoff: u64) -> Result<AgentLogCompressionStats> {
        let mut logs = self.inner.lock().await;
        let mut stats = AgentLogCompressionStats::default();
        for buf in logs.values_mut() {
            let has_aged = buf.hot.iter().any(|l| line_is_old(l, cutoff));
            if !has_aged {
                continue;
            }
            let folded: Vec<String> = buf
                .hot
                .iter()
                .filter(|l| line_is_old(l, cutoff))
                .cloned()
                .collect();
            let recent: Vec<String> = buf
                .hot
                .iter()
                .filter(|l| !line_is_old(l, cutoff))
                .cloned()
                .collect();

            let mut all = buf.cold_lines()?;
            all.extend(folded.iter().cloned());
            let (segment, _whole_raw_len) = build_cold(all)?;
            // Byte totals are scoped to the newly folded lines, not the
            // whole rebuilt segment (which re-holds already-cold lines).
            let (folded_segment, folded_raw_len) = build_cold(folded)?;
            stats.lines += folded_segment.line_count as u64;
            stats.bytes_before += folded_raw_len as u64;
            stats.bytes_after += folded_segment.bytes.len() as u64;
            buf.cold = Some(segment);
            buf.hot = recent;
        }
        Ok(stats)
    }
}

/// A line is old when it carries a `[ts]` prefix and `ts < cutoff`.
/// Unprefixed lines are never old (kept).
fn line_is_old(line: &str, cutoff: u64) -> bool {
    line_timestamp(line).is_some_and(|ts| ts < cutoff)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_common::Id;
    use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome};

    const DAY: u64 = 86_400;

    #[test]
    fn interval_resolution() {
        assert_eq!(interval_from(None), DEFAULT_INTERVAL_SECS);
        assert_eq!(interval_from(Some("")), DEFAULT_INTERVAL_SECS);
        assert_eq!(interval_from(Some("garbage")), DEFAULT_INTERVAL_SECS);
        assert_eq!(interval_from(Some("0")), DEFAULT_INTERVAL_SECS);
        assert_eq!(interval_from(Some("3600")), 3600);
        assert_eq!(interval_from(Some(" 7200 ")), 7200);
    }

    #[test]
    fn line_timestamp_parses_canonical_prefix() {
        assert_eq!(line_timestamp("[1700000000] hello"), Some(1_700_000_000));
        assert_eq!(line_timestamp("[ 42 ] spaced"), Some(42));
        assert_eq!(line_timestamp("no prefix"), None);
        assert_eq!(line_timestamp("[abc] not numeric"), None);
        assert_eq!(line_timestamp(""), None);
    }

    fn now() -> u64 {
        crate::jobs::now_secs()
    }

    async fn seed(buf: &AgentLogBuffer, agent: &str, lines: &[String]) {
        for line in lines {
            buf.append(agent.to_string(), line.clone()).await;
        }
    }

    #[tokio::test]
    async fn compress_then_read_is_transparent_and_idempotent() {
        let buf = AgentLogBuffer::new();
        let t = now();
        let old_a = format!("[{}] first old line", t - 10 * DAY);
        let old_b = format!("[{}] second old line", t - 8 * DAY);
        let new_a = format!("[{}] recent line", t - DAY);
        let new_b = format!("[{}] fresh line", t);
        seed(
            &buf,
            "agent-1",
            &[old_a.clone(), old_b.clone(), new_a.clone(), new_b.clone()],
        )
        .await;

        let cutoff = t - 7 * DAY;
        let stats = buf.compress_older_than(cutoff).await.unwrap();
        // Both old lines are newly folded this pass (fresh buffer, no
        // pre-existing cold segment), so all counts are scoped to them.
        assert_eq!(stats.lines, 2);
        let folded = vec![old_a.clone(), old_b.clone()];
        assert_eq!(stats.bytes_before, encode_frames(&folded).len() as u64);
        assert!(
            stats.bytes_before > stats.bytes_after,
            "zstd must shrink the folded lines"
        );
        // Real compression happened: the cold blob is not the raw frames.
        assert!(stats.bytes_after > 0);

        // Reads are transparent and order-preserving (cold first, then hot).
        let all = buf.replay("agent-1").await.unwrap();
        assert_eq!(all, vec![old_a, old_b, new_a, new_b]);
        let paged = buf.get("agent-1", 1, 2).await.unwrap();
        assert_eq!(paged.len(), 2);
        assert!(paged[0].contains("second old line"));
        assert!(paged[1].contains("recent line"));

        // Second pass: nothing new to fold (idempotent).
        let again = buf.compress_older_than(cutoff).await.unwrap();
        assert_eq!(again.lines, 0);
        assert_eq!(again.bytes_before, 0);
        assert_eq!(again.bytes_after, 0);
        // And the buffer still reads identically.
        assert_eq!(buf.replay("agent-1").await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn compress_folds_incrementally_and_keeps_new_lines_hot() {
        let buf = AgentLogBuffer::new();
        let t = now();
        let first = format!("[{}] old one", t - 10 * DAY);
        let first_pass = vec![first.clone()];
        seed(&buf, "agent-1", &first_pass).await;
        buf.compress_older_than(t - 7 * DAY).await.unwrap();

        // A later line ages out in a second pass; the segment grows.
        let second = format!("[{}] old two", t - 9 * DAY);
        let fresh = format!("[{}] fresh", t);
        seed(&buf, "agent-1", &[second.clone(), fresh.clone()]).await;
        let stats = buf.compress_older_than(t - 7 * DAY).await.unwrap();
        // Only the NEWLY folded line counts — the line already cold from
        // the first pass is not re-counted, and the byte totals are
        // scoped to the new line's frames, not the whole rebuilt segment.
        assert_eq!(stats.lines, 1, "only newly folded lines count");
        let newly_folded = vec![second.clone()];
        assert_eq!(
            stats.bytes_before,
            encode_frames(&newly_folded).len() as u64
        );
        assert_eq!(
            buf.replay("agent-1").await.unwrap(),
            vec![first, second, fresh]
        );
    }

    #[tokio::test]
    async fn unprefixed_lines_stay_hot_and_round_trip() {
        let buf = AgentLogBuffer::new();
        let t = now();
        let unprefixed = "no prefix line".to_string();
        let multiline = format!("[{}] line with\nembedded newline", t - 10 * DAY);
        seed(&buf, "agent-1", &[unprefixed.clone(), multiline.clone()]).await;

        let stats = buf.compress_older_than(t - 7 * DAY).await.unwrap();
        // The multiline (prefixed, old) line is folded; the unprefixed
        // line is never old, so it stays hot.
        assert_eq!(stats.lines, 1);
        // Read order is cold (timestamped, old) first, then hot — the
        // unprefixed line is displaced after the cold segment, but both
        // lines round-trip byte-for-byte.
        assert_eq!(
            buf.replay("agent-1").await.unwrap(),
            vec![multiline, unprefixed],
            "embedded newlines must round-trip byte-for-byte"
        );
    }

    #[tokio::test]
    async fn purge_after_compress_drops_old_cold_lines() {
        let buf = AgentLogBuffer::new();
        let t = now();
        let old = format!("[{}] ancient line", t - 40 * DAY);
        let mid = format!("[{}] middle line", t - 20 * DAY);
        let new = format!("[{}] recent line", t - DAY);
        seed(&buf, "agent-1", &[old.clone(), mid.clone(), new.clone()]).await;
        buf.compress_older_than(t - 7 * DAY).await.unwrap();

        // 30-day retention cutoff: the ancient line goes, the rest stay.
        let purged = buf.purge_older_than(t - 30 * DAY).await;
        assert_eq!(purged, 1);
        assert_eq!(buf.replay("agent-1").await.unwrap(), vec![mid, new]);

        // Purging again is idempotent.
        assert_eq!(buf.purge_older_than(t - 30 * DAY).await, 0);
        assert_eq!(buf.replay("agent-1").await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn purge_whole_segment_without_decompressing() {
        let buf = AgentLogBuffer::new();
        let t = now();
        let a = format!("[{}] ancient a", t - 40 * DAY);
        let b = format!("[{}] ancient b", t - 35 * DAY);
        let new = format!("[{}] recent", t);
        seed(&buf, "agent-1", &[a, b.clone(), new.clone()]).await;
        buf.compress_older_than(t - 7 * DAY).await.unwrap();

        // Both cold lines are older than the 30d cutoff → the whole
        // segment is dropped in one count.
        let purged = buf.purge_older_than(t - 30 * DAY).await;
        assert_eq!(purged, 2);
        assert_eq!(buf.replay("agent-1").await.unwrap(), vec![new]);
    }

    #[tokio::test]
    async fn line_cap_enforced_across_cold_and_hot() {
        let buf = AgentLogBuffer::new();
        let t = now();
        for i in 0..(MAX_LINES_PER_AGENT + 5) {
            buf.append("agent-1".to_string(), format!("[{}] line {}", t, i))
                .await;
        }
        assert_eq!(
            buf.replay("agent-1").await.unwrap().len(),
            MAX_LINES_PER_AGENT
        );
        // Oldest lines were drained: line 0 is gone, the last line kept.
        let lines = buf.replay("agent-1").await.unwrap();
        assert!(!lines[0].contains("line 0"));
        assert!(lines
            .last()
            .unwrap()
            .contains(&format!("line {}", MAX_LINES_PER_AGENT + 4)));
    }

    #[tokio::test]
    async fn line_cap_evicts_oldest_when_cold_segment_is_at_cap() {
        // F2 regression: with a full cold segment (agent emitted >= cap
        // lines, went quiet past the fold age, and the job folded the
        // whole buffer), a newly appended line must survive and the
        // OLDEST cold line must be evicted — not the fresh line.
        let buf = AgentLogBuffer::new();
        let t = now();
        let old_ts = t - 10 * DAY;
        for i in 0..MAX_LINES_PER_AGENT {
            buf.append(
                "agent-1".to_string(),
                format!("[{}] cold line {}", old_ts, i),
            )
            .await;
        }
        buf.compress_older_than(t - 7 * DAY).await.unwrap();
        // Whole buffer is now one cold segment at the cap, hot is empty.

        let fresh = format!("[{}] fresh line", t);
        buf.append("agent-1".to_string(), fresh.clone()).await;

        let lines = buf.replay("agent-1").await.unwrap();
        assert_eq!(lines.len(), MAX_LINES_PER_AGENT);
        // Newest line kept...
        assert_eq!(lines.last().unwrap(), &fresh);
        // ...oldest cold line evicted, the rest of the cold segment intact.
        assert!(!lines[0].contains("cold line 0"), "oldest line must go");
        assert!(lines[0].contains("cold line 1"));
        assert!(lines[MAX_LINES_PER_AGENT - 2]
            .contains(&format!("cold line {}", MAX_LINES_PER_AGENT - 1)));

        // Appending more keeps evicting from the cold front: the cap
        // holds and every fresh line survives.
        for i in 0..50 {
            buf.append("agent-1".to_string(), format!("[{}] newer line {}", t, i))
                .await;
        }
        let lines = buf.replay("agent-1").await.unwrap();
        assert_eq!(lines.len(), MAX_LINES_PER_AGENT);
        assert!(lines.last().unwrap().contains("newer line 49"));
        assert!(!lines.iter().any(|l| l.ends_with("cold line 49")));
    }

    #[tokio::test]
    async fn get_unknown_agent_is_empty() {
        let buf = AgentLogBuffer::new();
        assert!(buf.get("nobody", 0, 10).await.unwrap().is_empty());
        assert!(buf.replay("nobody").await.unwrap().is_empty());
    }

    fn audit_event(id: &str, ts: u64, detail: serde_json::Value) -> AuditEvent {
        AuditEvent::new(
            Id::new(id),
            AuditEventType::FileAccess,
            Some(Id::new("agent-1")),
            None,
            None,
            None,
            None,
            "agent".to_string(),
            Some("agent-1".to_string()),
            AuditOutcome::Success,
            detail,
            None,
            None,
            ts,
        )
    }

    #[tokio::test]
    async fn run_once_compresses_audit_details_and_agent_logs() {
        let state = test_state();
        let t = now();

        // One old audit event (compressible), one new (untouched).
        let old_detail = serde_json::json!({
            "path": "/tmp/some/path/that/is/quite/long/and/repetitive/aaaa",
            "action": "read",
            "pid": 1000,
            "padding": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        });
        gyre_ports::AuditRepository::record(
            &*state.audit,
            &audit_event("a-old", t - 10 * DAY, old_detail.clone()),
        )
        .await
        .unwrap();
        gyre_ports::AuditRepository::record(
            &*state.audit,
            &audit_event("a-new", t, serde_json::json!({ "path": "/tmp/x" })),
        )
        .await
        .unwrap();

        // Agent logs: two old lines (long + repetitive so zstd really
        // shrinks them), one new.
        let old_line = format!("[{}] {}", t - 10 * DAY, "old agent log line ".repeat(20));
        let new_line = format!("[{}] new agent log line", t);
        state
            .agent_logs
            .append("agent-1".to_string(), old_line.clone())
            .await;
        state
            .agent_logs
            .append(
                "agent-1".to_string(),
                format!("[{}] {}", t - 9 * DAY, "old agent log line ".repeat(20)),
            )
            .await;
        state
            .agent_logs
            .append("agent-1".to_string(), new_line.clone())
            .await;

        let report = run_once(&state).await.unwrap();
        assert_eq!(
            report.audit_events_compressed, 1,
            "only the old audit event"
        );
        assert!(report.audit_bytes_before > report.audit_bytes_after);
        assert_eq!(
            report.agent_log_lines_compressed, 2,
            "both old log lines folded"
        );
        assert!(report.agent_log_bytes_before > report.agent_log_bytes_after);

        // Audit query is transparent: original detail survives.
        let events = gyre_ports::AuditRepository::query(
            &*state.audit,
            &gyre_ports::AuditQueryFilter {
                limit: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 2);
        let old = events.iter().find(|e| e.id.as_str() == "a-old").unwrap();
        assert_eq!(old.detail, old_detail);

        // Agent log read is transparent.
        let lines = state.agent_logs.replay("agent-1").await.unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], old_line);
        assert_eq!(lines[2], new_line);

        // Second run compresses nothing new (idempotent).
        let again = run_once(&state).await.unwrap();
        assert_eq!(again.audit_events_compressed, 0);
        assert_eq!(again.agent_log_lines_compressed, 0);
    }
}
