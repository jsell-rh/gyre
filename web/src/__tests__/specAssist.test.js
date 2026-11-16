/**
 * specAssist.test.js — unit tests for the shared LLM-assisted spec editing
 * logic (ui-layout.md §3 LLM-Assisted Spec Editing).
 *
 * Both §3 surfaces (DetailPanel edit tab, EditorSplit left pane) consume
 * this module; the component tests only exercise `add` ops with header
 * anchors, so the line-range fallback, replace/remove semantics, and the
 * SSE error path are pinned here.
 */

import { describe, it, expect } from 'vitest';
import { applyDiffOps, parseLineRange, suggestionText, streamAssist } from '../lib/specAssist.js';

const DOC = `# Title

Intro text.

## Existing

Existing body.

## Later

Later body.
`;

describe('parseLineRange', () => {
  it('parses "L15-L22" and single-line "L7" ranges', () => {
    expect(parseLineRange('L15-L22')).toEqual({ start: 15, end: 22 });
    expect(parseLineRange('L7')).toEqual({ start: 7, end: 7 });
    expect(parseLineRange('L22-L15')).toEqual({ start: 22, end: 22 }); // start ≤ end
  });

  it('returns null for header paths and non-strings', () => {
    expect(parseLineRange('## Error Handling')).toBeNull();
    expect(parseLineRange('L15-L')).toBeNull();
    expect(parseLineRange(null)).toBeNull();
    expect(parseLineRange(15)).toBeNull();
  });
});

describe('applyDiffOps', () => {
  it('add inserts the content right after the header identified by path', () => {
    const out = applyDiffOps(DOC, [
      { op: 'add', path: '## Existing', content: 'Inserted body.' },
    ]);
    // Inserted directly after the "## Existing" header line, before the
    // existing body (§3: "For additions, path is the header after which to
    // insert").
    expect(out).toBe(
      '# Title\n\nIntro text.\n\n## Existing\nInserted body.\n\nExisting body.\n\n## Later\n\nLater body.\n',
    );
  });

  it('add falls back to a line range when no header matches', () => {
    const out = applyDiffOps('a\nb\nc\n', [{ op: 'add', path: 'L2', content: 'x' }]);
    // Insert after line 2 (1-based, inclusive) → between b and c.
    expect(out).toBe('a\nb\nx\nc\n');
  });

  it('add appends at the end when nothing anchors it', () => {
    const out = applyDiffOps('body', [{ op: 'add', path: '## Missing', content: 'tail' }]);
    expect(out).toBe('body\ntail');
  });

  it('replace substitutes the section body but keeps the header line', () => {
    const out = applyDiffOps(DOC, [
      { op: 'replace', path: '## Existing', content: 'New body.' },
    ]);
    expect(out).toBe(
      '# Title\n\nIntro text.\n\n## Existing\nNew body.\n## Later\n\nLater body.\n',
    );
  });

  it('replace falls back to substituting the line range', () => {
    const out = applyDiffOps('a\nb\nc\nd\n', [
      { op: 'replace', path: 'L2-L3', content: 'x\ny' },
    ]);
    expect(out).toBe('a\nx\ny\nd\n');
  });

  it('remove deletes the whole section (header line + body)', () => {
    const out = applyDiffOps(DOC, [{ op: 'remove', path: '## Existing' }]);
    expect(out).toBe('# Title\n\nIntro text.\n\n## Later\n\nLater body.\n');
  });

  it('remove falls back to deleting the line range', () => {
    const out = applyDiffOps('a\nb\nc\n', [{ op: 'remove', path: 'L2' }]);
    expect(out).toBe('a\nc\n');
  });

  it('replace/remove without a resolvable anchor leave the content unchanged', () => {
    expect(applyDiffOps(DOC, [{ op: 'replace', path: '## Missing', content: 'x' }])).toBe(DOC);
    expect(applyDiffOps(DOC, [{ op: 'remove', path: 'L999' }])).toBe(DOC);
  });

  it('unknown ops and malformed entries are no-ops', () => {
    expect(applyDiffOps(DOC, [{ op: 'rename', path: '## Existing', content: 'x' }])).toBe(DOC);
    expect(applyDiffOps(DOC, [null, { op: 'add' }, { content: 'no path' }])).toBe(DOC);
    expect(applyDiffOps(DOC, undefined)).toBe(DOC);
  });

  it('applies multiple ops in sequence', () => {
    const out = applyDiffOps(DOC, [
      { op: 'replace', path: '## Existing', content: 'Rewritten.' },
      { op: 'add', path: '## Later', content: 'Added.' },
    ]);
    expect(out).toContain('## Existing\nRewritten.');
    expect(out).toContain('## Later\nAdded.\n\nLater body.');
  });

  it('replace substituting the final section preserves the trailing newline', () => {
    const out = applyDiffOps('## A\n\nA body.\n', [
      { op: 'replace', path: '## A', content: 'Rewritten.' },
    ]);
    expect(out).toBe('## A\nRewritten.\n');
  });

  it('clamps line ranges beyond the end of the content', () => {
    // 'a\n' is two lines ('a' and the empty trailing line) — an insert
    // after L5-L9 clamps to the end.
    const out = applyDiffOps('a\n', [{ op: 'add', path: 'L5-L9', content: 'x' }]);
    expect(out).toBe('a\n\nx');
  });
});

describe('suggestionText', () => {
  it('joins the diff-op contents with blank lines for the Edit action', () => {
    expect(
      suggestionText({
        diff: [
          { op: 'add', path: '## A', content: 'First.' },
          { op: 'add', path: '## B', content: 'Second.' },
        ],
      }),
    ).toBe('First.\n\nSecond.');
  });

  it('tolerates empty diffs and missing contents', () => {
    expect(suggestionText({ diff: [] })).toBe('');
    expect(suggestionText({})).toBe('');
    expect(suggestionText(null)).toBe('');
  });
});

/** Build an SSE Response-like object from raw text chunks. */
function sseFromChunks(chunks) {
  let i = 0;
  return {
    ok: true,
    status: 200,
    body: {
      getReader: () => ({
        read: () =>
          i < chunks.length
            ? Promise.resolve({ value: new TextEncoder().encode(chunks[i++]), done: false })
            : Promise.resolve({ value: undefined, done: true }),
      }),
    },
  };
}

describe('streamAssist', () => {
  it('streams partial events progressively and completes with {diff, explanation}', async () => {
    const partials = [];
    let completed = null;
    await streamAssist(
      sseFromChunks([
        'event: partial\ndata: {"text":"Adding "}\n\n',
        'event: partial\ndata: {"text":"error handling."}\n\n',
        'event: complete\ndata: {"diff":[{"op":"add","path":"## E","content":"X"}],"explanation":"Adding error handling."}\n\n',
      ]),
      {
        onPartial: (t) => partials.push(t),
        onComplete: (r) => { completed = r; },
      },
    );
    expect(partials).toEqual(['Adding ', 'error handling.']);
    expect(completed).toEqual({
      diff: [{ op: 'add', path: '## E', content: 'X' }],
      explanation: 'Adding error handling.',
    });
  });

  it('uses the SSE event: line when the data payload carries no event field', async () => {
    let completed = null;
    const partials = [];
    await streamAssist(
      sseFromChunks([
        'event: partial\ndata: {"text":"t1"}\n\nevent: complete\ndata: {"diff":[],"explanation":"e"}\n\n',
      ]),
      {
        onPartial: (t) => partials.push(t),
        onComplete: (r) => { completed = r; },
      },
    );
    expect(partials).toEqual(['t1']);
    expect(completed.diff).toEqual([]);
  });

  it('skips non-JSON data lines (keep-alives) instead of failing', async () => {
    let completed = null;
    await streamAssist(
      sseFromChunks([': ping\n\ndata: not-json\n\n', 'event: complete\ndata: {"diff":[],"explanation":""}\n\n']),
      { onComplete: (r) => { completed = r; } },
    );
    expect(completed).toEqual({ diff: [], explanation: '' });
  });

  it('throws on event: error — a failed assist must never masquerade as complete', async () => {
    await expect(
      streamAssist(sseFromChunks(['event: error\ndata: {"error":"no LLM configured"}\n\n'])),
    ).rejects.toThrow('no LLM configured');
  });

  it('throws on non-OK HTTP status and missing body', async () => {
    await expect(streamAssist({ ok: false, status: 503 })).rejects.toThrow('503');
    await expect(streamAssist({ ok: true, status: 200 })).rejects.toThrow('No response body');
  });

  it('a stream that ends without a complete event yields no suggestion', async () => {
    let completed = null;
    await streamAssist(sseFromChunks(['event: partial\ndata: {"text":"cut off"}\n\n']), {
      onComplete: (r) => { completed = r; },
    });
    expect(completed).toBeNull();
  });
});
