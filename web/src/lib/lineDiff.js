/**
 * lineDiff — LCS-based line diff between two texts.
 *
 * Used by EditorSplit's locked editor (ui-layout.md §9 State 2: the left
 * panel is locked during preview and "shows diff of changes" — the draft
 * against the published baseline).
 *
 * Returns an array of { op: 'context' | 'remove' | 'add', text } rows,
 * where 'remove' lines come from the old text and 'add' lines from the new
 * one. Identical leading/trailing lines are kept as context without entering
 * the DP; the middle is diffed with a classic LCS table. Inputs too large
 * for the quadratic table degrade to a single remove-block + add-block for
 * the changed middle (still a correct diff, just not minimal).
 */

/** Cap on mid-section cells (old × new lines) before degrading to block diff. */
const MAX_DP_CELLS = 4_000_000;

function toLines(text) {
  const s = String(text ?? '');
  if (s === '') return [];
  const lines = s.split('\n');
  // A single trailing newline is a line terminator, not an empty final line.
  if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop();
  return lines;
}

/** LCS ops for two line arrays (quadratic DP, bounded by MAX_DP_CELLS). */
function lcsOps(a, b) {
  const n = a.length;
  const m = b.length;
  const w = m + 1;
  // dp[i * w + j] = LCS length of a[i..] and b[j..]
  const dp = new Int32Array((n + 1) * w);
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i * w + j] =
        a[i] === b[j]
          ? dp[(i + 1) * w + j + 1] + 1
          : Math.max(dp[(i + 1) * w + j], dp[i * w + j + 1]);
    }
  }
  const ops = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      ops.push({ op: 'context', text: a[i] });
      i++;
      j++;
    } else if (dp[(i + 1) * w + j] >= dp[i * w + j + 1]) {
      ops.push({ op: 'remove', text: a[i] });
      i++;
    } else {
      ops.push({ op: 'add', text: b[j] });
      j++;
    }
  }
  while (i < n) {
    ops.push({ op: 'remove', text: a[i] });
    i++;
  }
  while (j < m) {
    ops.push({ op: 'add', text: b[j] });
    j++;
  }
  return ops;
}

/**
 * Line diff of `oldText` → `newText`.
 *
 * @param {string} oldText
 * @param {string} newText
 * @returns {Array<{op: 'context'|'remove'|'add', text: string}>}
 */
export function lineDiff(oldText, newText) {
  const a = toLines(oldText);
  const b = toLines(newText);

  // Trim common prefix/suffix so the DP only covers the changed middle.
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }

  const midA = a.slice(start, endA);
  const midB = b.slice(start, endB);

  let midOps;
  if (midA.length * midB.length > MAX_DP_CELLS) {
    // Too large for the quadratic table — one replace block for the middle.
    midOps = [
      ...midA.map((text) => ({ op: 'remove', text })),
      ...midB.map((text) => ({ op: 'add', text })),
    ];
  } else {
    midOps = lcsOps(midA, midB);
  }

  const rows = [];
  for (let k = 0; k < start; k++) rows.push({ op: 'context', text: a[k] });
  rows.push(...midOps);
  for (let k = endA; k < a.length; k++) rows.push({ op: 'context', text: a[k] });
  return rows;
}

/**
 * Row counts for a diff summary.
 * @returns {{added: number, removed: number, context: number}}
 */
export function diffSummary(rows) {
  let added = 0;
  let removed = 0;
  let context = 0;
  for (const r of rows ?? []) {
    if (r.op === 'add') added++;
    else if (r.op === 'remove') removed++;
    else context++;
  }
  return { added, removed, context };
}
