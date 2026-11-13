/**
 * specAssist.js — shared logic for LLM-assisted spec editing
 * (ui-layout.md §3 LLM-Assisted Spec Editing).
 *
 * Both editor surfaces (DetailPanel Spec/Edit tab and EditorSplit left
 * pane) consume this module instead of carrying parallel copies:
 *
 *   1. `streamAssist` — SSE parsing for the specs/assist wire protocol
 *      (`event: partial` streams the explanation progressively,
 *       `event: complete` carries `{diff, explanation}`,
 *       `event: error` surfaces an Error).
 *   2. `applyDiffOps` — application of the `{op, path, content}` diff ops
 *      to editor content. `path` is a markdown section header; when no
 *      matching header exists it falls back to a `"L15-L22"` line range.
 *   3. `suggestionText` — the suggested text the Edit action copies into
 *      the editor for manual refinement (§3 step 5).
 */

/**
 * Parse a `"L15-L22"` line-range path (also accepts a single `"L7"`).
 * Returns `{ start, end }` (1-based, inclusive, start ≤ end) or `null`
 * when `path` is not a line range.
 */
export function parseLineRange(path) {
  if (typeof path !== 'string') return null;
  const m = path.match(/^L(\d+)(?:-L?(\d+))?$/);
  if (!m) return null;
  const start = Math.max(1, parseInt(m[1], 10));
  const end = m[2] === undefined ? start : Math.max(start, parseInt(m[2], 10));
  return { start, end };
}

/**
 * Find the exclusive end char index of the markdown section whose header
 * starts at char index `from`: the start of the next `#`-prefixed header
 * line, or the end of the content for the last section.
 */
export function findSectionEnd(content, from) {
  const rest = content.slice(from);
  const match = rest.match(/\n(#{1,6} )/);
  if (match?.index !== undefined) return from + match.index + 1;
  return content.length;
}

/**
 * Resolve an op's `path` to a 0-based line range in `content` when it is a
 * `"L15-L22"`-style line range. Clamped to the content's line count; the
 * returned `end` is exclusive.
 */
function lineRangeSlice(content, path) {
  const range = parseLineRange(path);
  if (!range) return null;
  const lineCount = content.split('\n').length;
  const startIdx = Math.min(range.start - 1, lineCount);
  const endIdx = Math.min(range.end, lineCount);
  return { start: startIdx, end: Math.max(startIdx, endIdx) };
}

function applyAdd(content, op) {
  const text = op.content ?? '';
  const idx = content.indexOf(op.path);
  if (idx !== -1) {
    // Header found: insert directly after the header line ("For additions,
    // path is the header after which to insert").
    const lineEnd = content.indexOf('\n', idx + op.path.length);
    const insertAt = lineEnd !== -1 ? lineEnd + 1 : content.length;
    return content.slice(0, insertAt) + text + '\n' + content.slice(insertAt);
  }
  const range = lineRangeSlice(content, op.path);
  if (range) {
    // Line-range fallback: insert after the range's last line.
    const lines = content.split('\n');
    lines.splice(range.end, 0, ...text.split('\n'));
    return lines.join('\n');
  }
  // No anchor at all — append at the end.
  return content + '\n' + text;
}

function applyReplace(content, op) {
  const idx = content.indexOf(op.path);
  if (idx !== -1) {
    // Header found: substitute the section body, keeping the header line.
    const end = findSectionEnd(content, idx + op.path.length);
    return content.slice(0, idx) + op.path + '\n' + (op.content ?? '') + content.slice(end);
  }
  const range = lineRangeSlice(content, op.path);
  if (range) {
    // Line-range fallback: substitute the lines with the new content.
    const lines = content.split('\n');
    lines.splice(range.start, range.end - range.start, ...(op.content ?? '').split('\n'));
    return lines.join('\n');
  }
  return content;
}

function applyRemove(content, op) {
  const idx = content.indexOf(op.path);
  if (idx !== -1) {
    // Header found: delete the whole section (header line + body).
    const end = findSectionEnd(content, idx + op.path.length);
    return content.slice(0, idx) + content.slice(end);
  }
  const range = lineRangeSlice(content, op.path);
  if (range) {
    // Line-range fallback: delete the lines.
    const lines = content.split('\n');
    lines.splice(range.start, range.end - range.start);
    return lines.join('\n');
  }
  return content;
}

/**
 * Apply a specs/assist `diff` array to editor content (in-memory — the
 * human must still save; §3 step 4).
 *
 * Ops: `add` (insert content after the section identified by `path`),
 * `remove` (delete the section identified by `path`), `replace`
 * (substitute the section content at `path` with `content`). `path` is a
 * markdown section header string; when no matching header exists it falls
 * back to a `"L15-L22"` line range (ui-layout.md §3). Unknown ops and ops
 * without a resolvable anchor for `replace`/`remove` leave the content
 * unchanged for that op.
 */
export function applyDiffOps(content, diff) {
  let out = content ?? '';
  for (const op of diff ?? []) {
    if (!op || typeof op.path !== 'string') continue;
    if (op.op === 'add') out = applyAdd(out, op);
    else if (op.op === 'replace') out = applyReplace(out, op);
    else if (op.op === 'remove') out = applyRemove(out, op);
  }
  return out;
}

/**
 * The suggested text for the Edit action (§3 step 5): the concatenation of
 * the suggestion's diff-op contents, joined with blank lines.
 */
export function suggestionText(suggestion) {
  return (suggestion?.diff ?? [])
    .map((d) => (typeof d?.content === 'string' ? d.content : ''))
    .filter(Boolean)
    .join('\n\n');
}

/**
 * Consume a specs/assist SSE `Response` (from `api.specsAssist` /
 * `api.metaSpecsAssist`) and dispatch the wire protocol events:
 *
 *   - `onPartial(text)` for every `event: partial` chunk (progressive
 *     explanation rendering)
 *   - `onComplete({ diff, explanation })` for `event: complete`
 *   - an `event: error` payload is thrown as an `Error` (the caller
 *     surfaces it; a truncated response must never masquerade as complete)
 *
 * Non-OK HTTP statuses and missing bodies also throw. Data lines that are
 * not valid JSON are skipped (SSE keep-alives and comment lines).
 */
export async function streamAssist(resp, { onPartial, onComplete } = {}) {
  if (!resp?.ok) throw new Error(`LLM request failed: ${resp.status}`);
  const reader = resp.body?.getReader?.();
  if (!reader) throw new Error('No response body');

  const decoder = new TextDecoder();
  let buf = '';
  let done = false;

  while (!done) {
    const { value, done: streamDone } = await reader.read();
    done = streamDone;
    if (!value) continue;
    buf += decoder.decode(value, { stream: true });
    const lines = buf.split('\n');
    buf = lines.pop() ?? '';
    let currentEvent = '';
    for (const line of lines) {
      // Track the SSE event type from `event:` lines (server format:
      // `event: partial\ndata: {...}\n\n` — see specs_assist.rs).
      if (line.startsWith('event:')) {
        currentEvent = line.slice(6).trim();
        continue;
      }
      if (!line.startsWith('data: ') && !line.startsWith('data:')) continue;
      const raw = line.startsWith('data: ') ? line.slice(6) : line.slice(5);
      if (raw === '[DONE]') { done = true; break; }
      let parsed;
      try {
        parsed = JSON.parse(raw);
      } catch {
        continue;
      }
      const evtType = parsed.event ?? parsed.type ?? currentEvent;
      if (evtType === 'partial' || (!evtType && parsed.text != null)) {
        onPartial?.(parsed.text ?? parsed.explanation ?? '');
      } else if (evtType === 'complete') {
        onComplete?.({
          diff: Array.isArray(parsed.diff) ? parsed.diff : [],
          explanation: parsed.explanation ?? parsed.text ?? '',
        });
        done = true;
        break;
      } else if (evtType === 'error') {
        throw new Error(parsed.error ?? parsed.message ?? 'LLM error');
      }
      currentEvent = '';
    }
  }
}
