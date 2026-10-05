// Byte-offset log tailing with truncation/rotation detection and a bounded
// per-task activity ring. One tailer per file; SSE pumps and the activity
// builder both consume from it.
import { open, stat } from "node:fs/promises";

const MAX_RING = 400; // events per task

export function createLogTailer() {
  const files = new Map(); // path -> { offset, carry, inode }

  // opts.backfill: on first sighting, feed the last N bytes of existing
  // history through onLines (flagged) so consumers can seed state without
  // treating it as live output.
  async function pumpFile(path, onLines, opts = {}) {
    let t = files.get(path);
    try {
      const s = await stat(path);
      if (!t) {
        t = { offset: s.size, carry: "", inode: s.ino };
        files.set(path, t);
        if (opts.backfill && s.size > 0) {
          const len = Math.min(s.size, opts.backfill);
          const start = s.size - len;
          const fh = await open(path, "r");
          try {
            const buf = Buffer.alloc(len);
            await fh.read(buf, 0, len, start);
            onLines(buf.toString("utf8"), start, true);
          } finally { await fh.close(); }
        }
        return; // live tail starts at EOF
      }
      // Rotation/replacement (new inode) or truncation (size < offset):
      // the file is a different stream — restart from the beginning.
      if (s.ino !== t.inode) {
        t.inode = s.ino;
        t.offset = 0;
        t.carry = "";
      } else if (s.size < t.offset) {
        t.offset = 0;
        t.carry = "";
      }
      if (s.size > t.offset) {
        const fh = await open(path, "r");
        try {
          const len = Math.min(s.size - t.offset, 256 * 1024);
          const buf = Buffer.alloc(len);
          await fh.read(buf, 0, len, t.offset);
          // Advance the offset by exactly the bytes read; carry holds
          // partial-line bytes NOT yet counted (they were consumed from
          // the file, so they must not be re-read next pump).
          t.offset += len;
          t.carry += buf.toString("utf8");
          const lastNl = t.carry.lastIndexOf("\n");
          if (lastNl === -1) return; // no complete line yet
          const chunk = t.carry.slice(0, lastNl + 1);
          t.carry = t.carry.slice(lastNl + 1);
          onLines(chunk, t.offset - t.carry.length - chunk.length);
        } finally {
          await fh.close();
        }
      }
    } catch {
      // file gone (driver between rounds) — keep state; it may return
      if (t) files.set(path, t);
    }
  }

  return {
    pumpFile,
    forget(path) { files.delete(path); },
    state(path) { return files.get(path) || null; },
  };
}

// Bounded per-task event ring, fed by classified events from any source.
export function createActivityRing() {
  const rings = new Map(); // taskId -> event[]

  return {
    push(taskId, event) {
      let ring = rings.get(taskId);
      if (!ring) { ring = []; rings.set(taskId, ring); }
      ring.push(event);
      if (ring.length > MAX_RING) ring.splice(0, ring.length - MAX_RING);
    },
    get(taskId) { return rings.get(taskId) || []; },
    tasks() { return [...rings.keys()]; },
    drop(taskId) { rings.delete(taskId); },
  };
}
