import { describe, it, expect } from 'vitest';
import { lineDiff, diffSummary } from '../lib/lineDiff.js';

describe('lineDiff', () => {
  it('returns no rows for identical texts', () => {
    expect(lineDiff('a\nb\nc', 'a\nb\nc')).toEqual([
      { op: 'context', text: 'a' },
      { op: 'context', text: 'b' },
      { op: 'context', text: 'c' },
    ]);
  });

  it('marks an added line as add', () => {
    const rows = lineDiff('You are a backend dev.', 'You are a backend dev.\nHandle errors with Result<T, E>.');
    expect(rows).toEqual([
      { op: 'context', text: 'You are a backend dev.' },
      { op: 'add', text: 'Handle errors with Result<T, E>.' },
    ]);
  });

  it('marks a removed line as remove', () => {
    const rows = lineDiff('first\nsecond\nthird', 'first\nthird');
    expect(rows).toEqual([
      { op: 'context', text: 'first' },
      { op: 'remove', text: 'second' },
      { op: 'context', text: 'third' },
    ]);
  });

  it('detects a middle modification as remove+add with shared context kept', () => {
    const rows = lineDiff('# Persona\nOld body.\n# End', '# Persona\nNew body.\n# End');
    expect(rows).toEqual([
      { op: 'context', text: '# Persona' },
      { op: 'remove', text: 'Old body.' },
      { op: 'add', text: 'New body.' },
      { op: 'context', text: '# End' },
    ]);
  });

  it('matches moved/multi-line edits minimally (LCS, not index-wise)', () => {
    // A naive index diff would report 3 removes + 3 adds here; LCS keeps the
    // two shared lines as context and moves the third with one delete+insert.
    const rows = lineDiff('a\nb\nc', 'a\nc\nb');
    const summary = diffSummary(rows);
    expect(summary).toEqual({ added: 1, removed: 1, context: 2 });
    // Both sides reconstruct exactly from the single remove + single add.
    expect(rows.filter((r) => r.op !== 'add').map((r) => r.text).join('\n')).toBe('a\nb\nc');
    expect(rows.filter((r) => r.op !== 'remove').map((r) => r.text).join('\n')).toBe('a\nc\nb');
  });

  it('reconstructs both sides exactly (round-trip property)', () => {
    const oldText = 'line 1\nline 2\nline 3\nline 4\nline 5';
    const newText = 'line 1\nline 2 changed\nline 3\nline 4\nline 6';
    const rows = lineDiff(oldText, newText);
    expect(
      rows.filter((r) => r.op !== 'add').map((r) => r.text).join('\n'),
    ).toBe(oldText);
    expect(
      rows.filter((r) => r.op !== 'remove').map((r) => r.text).join('\n'),
    ).toBe(newText);
  });

  it('handles empty old text (pure insertion)', () => {
    expect(lineDiff('', 'new content')).toEqual([
      { op: 'add', text: 'new content' },
    ]);
  });

  it('handles empty new text (pure deletion)', () => {
    expect(lineDiff('old content', '')).toEqual([
      { op: 'remove', text: 'old content' },
    ]);
  });

  it('treats null/undefined inputs as empty text', () => {
    expect(lineDiff(null, undefined)).toEqual([]);
    expect(lineDiff(null, 'x')).toEqual([{ op: 'add', text: 'x' }]);
  });

  it('does not treat a trailing newline as an extra empty line', () => {
    // Draft saved with trailing newline vs baseline without: the terminator
    // itself is not a phantom content change.
    expect(lineDiff('a\nb\n', 'a\nb')).toEqual([
      { op: 'context', text: 'a' },
      { op: 'context', text: 'b' },
    ]);
  });

  it('degrades to a block diff for huge inputs without hanging', () => {
    const big = Array.from({ length: 6000 }, (_, i) => `old-${i}`).join('\n');
    const big2 = Array.from({ length: 6000 }, (_, i) => `new-${i}`).join('\n');
    const rows = lineDiff(big, big2);
    const summary = diffSummary(rows);
    expect(summary.added).toBe(6000);
    expect(summary.removed).toBe(6000);
    expect(summary.context).toBe(0);
  });
});

describe('diffSummary', () => {
  it('counts adds, removes and context rows', () => {
    expect(
      diffSummary([
        { op: 'context', text: 'a' },
        { op: 'add', text: 'b' },
        { op: 'add', text: 'c' },
        { op: 'remove', text: 'd' },
      ]),
    ).toEqual({ added: 2, removed: 1, context: 1 });
  });

  it('returns zeros for null input', () => {
    expect(diffSummary(null)).toEqual({ added: 0, removed: 0, context: 0 });
  });
});
