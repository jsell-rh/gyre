// Stream hub: one multiplexed SSE connection carries every task's decoded
// stream tail + classified events (browser HTTP/1.1 caps at ~6 connections).
// Message shapes:
//   { t: "<task>", d: "<decoded text>", ev: <classifiedEvent|null> }
//   { reload: true } — dev hot reload signal
import { readdir } from "node:fs/promises";
import { join } from "node:path";
import { decodeAgentEvents } from "./agent-events.mjs";
import { classifyTranscriptLine, classifyDriverLine } from "./classify.mjs";
import { createLogTailer, createActivityRing } from "./log-tail.mjs";

export function createStreamHub({ sbxStatusDir, onEvent }) {
  const tailer = createLogTailer();
  const activityRing = createActivityRing();
  const clients = new Set(); // res writers
  let lastTaskList = [];

  function broadcast(obj) {
    const line = `data: ${JSON.stringify(obj)}\n\n`;
    for (const c of clients) {
      try { c.write(line); } catch { clients.delete(c); }
    }
  }

  async function listTasks() {
    try {
      const names = (await readdir(sbxStatusDir)).filter((n) => /^task-/.test(n));
      lastTaskList = names;
      return names;
    } catch {
      return lastTaskList;
    }
  }

  // stream.log: decoded agent transcript — the live action stream.
  async function pumpStream(task) {
    const file = join(sbxStatusDir, task, "stream.log");
    await tailer.pumpFile(file, (chunk, offset, backfill) => {
      const decoded = decodeAgentEvents(chunk);
      const now = Date.now();
      let lastEvent = null;
      for (const line of decoded.split("\n")) {
        if (!line.trim()) continue;
        const rec = classifyTranscriptLine({ taskId: task, time: now, text: line, rawOffset: offset });
        if (rec) {
          activityRing.push(task, rec);
          lastEvent = rec;
        }
      }
      if (onEvent) onEvent(task, lastEvent);
      if (!backfill) broadcast({ t: task, d: decoded, ev: lastEvent });
    }, { backfill: 128 * 1024 });
  }

  // driver.log: driver milestones + failures. Lines carry their own
  // [HH:MM:SS] clock, so backfilled history keeps honest timestamps.
  async function pumpDriver(task) {
    const file = join(sbxStatusDir, task, "driver.log");
    await tailer.pumpFile(file, (chunk, offset, backfill) => {
      const now = Date.now();
      let lastEvent = null;
      for (const line of chunk.split("\n")) {
        if (!line.trim()) continue;
        const rec = classifyDriverLine({ taskId: task, now, text: line, rawOffset: offset });
        if (rec) {
          activityRing.push(task, rec);
          lastEvent = rec;
        }
      }
      if (onEvent && lastEvent) onEvent(task, lastEvent);
      // driver lines aren't transcript text — never broadcast as `d`.
    }, { backfill: 128 * 1024 });
  }

  async function pumpTask(task) {
    await Promise.all([pumpStream(task), pumpDriver(task)]);
  }

  async function pump() {
    const tasks = await listTasks();
    await Promise.all(tasks.map(pumpTask));
  }

  let timer = null;
  function start(intervalMs = 1000) {
    if (timer) return;
    timer = setInterval(() => {
      pump().catch(() => {});
      for (const c of [...clients]) {
        try { c.write(": hb\n\n"); } catch { clients.delete(c); }
      }
    }, intervalMs);
  }
  function stop() { if (timer) { clearInterval(timer); timer = null; } }

  function attach(res) {
    clients.add(res);
    try { res.write(": connected\n\n"); } catch { clients.delete(res); }
    return () => clients.delete(res);
  }

  return { start, stop, pump, attach, broadcast, tailer, activityRing };
}
