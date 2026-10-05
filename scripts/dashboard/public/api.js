// API client: snapshot polling + the single multiplexed SSE stream.
export async function fetchSnapshot() {
  const r = await fetch("/api/snapshot");
  if (!r.ok) throw new Error(`snapshot ${r.status}`);
  return r.json();
}

export async function fetchActivity(task) {
  const r = await fetch(`/api/activity?task=${encodeURIComponent(task)}`);
  if (!r.ok) return { events: [] };
  return r.json();
}

export async function fetchRawLog(task) {
  const r = await fetch(`/api/raw?task=${encodeURIComponent(task)}`);
  if (!r.ok) return null;
  const j = await r.json();
  return j.content || null;
}

export async function fetchFile(path) {
  const r = await fetch(`/api/file?path=${encodeURIComponent(path)}`);
  if (!r.ok) return null;
  const j = await r.json();
  return j.content || null;
}

export async function setParallelism(value) {
  const r = await fetch("/api/parallelism", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ value }),
  });
  return r.json();
}

// One EventSource for every task's stream tail + classified events.
// onChunk({t, d, ev}) per message; onOpen/onClose for connection state.
export function connectStreams({ onChunk, onOpen, onClose }) {
  const es = new EventSource("/api/streams");
  es.onmessage = (e) => {
    try { onChunk(JSON.parse(e.data)); } catch {}
  };
  es.onopen = onOpen;
  es.onerror = onClose;
  return es;
}
