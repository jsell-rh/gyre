/**
 * AccordionScoping.test.js — compile-time guard for the AccordionItem
 * header-button CSS scoping contract (ui-layout.md §3 Inline Expansion).
 *
 * Review-round defect class this guards: AccordionItem authors the header
 * <button> inside its own file, so at runtime the button carries ONLY
 * AccordionItem's scoping hash class. Parent <style> rules that target the
 * button via a plain scoped selector (`.card-header` in Inbox,
 * `.accordion-header.section-heading` in Briefing) either get pruned by
 * Svelte as "Unused CSS selector" or survive with the parent's hash class
 * and silently never match — the header renders with the component's
 * reset only, and jsdom tests stay green because they cannot see CSS.
 *
 * The honest probe is compiling the consumer components and asserting on
 * the emitted CSS: the rules must exist, be live (not `(unused)`), and be
 * emitted WITHOUT the consumer's scoping hash on the header class (i.e.
 * :global-wrapped). Node environment — no DOM needed.
 *
 * Vitest per-file environment override:
 *   @vitest-environment node
 */

// @vitest-environment node
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compile } from 'svelte/compiler';

const __dirname = dirname(fileURLToPath(import.meta.url));

const compiledCss = (rel) => {
  const src = readFileSync(resolve(__dirname, rel), 'utf8');
  const { css, warnings } = compile(src, { generate: 'client' });
  return { cssCode: css.code, warnings };
};

describe('AccordionItem header CSS scoping (ui-layout.md §3)', () => {
  it('Inbox: card-header rules are live and :global (match the child-authored button)', () => {
    const { cssCode, warnings } = compiledCss('../components/Inbox.svelte');
    // Live rule targeting the runtime header element. The emitted selector
    // is `.inbox-card.svelte-HASH .accordion-header.card-header` — scoped on
    // the parent-owned wrapper, unscoped on the child-owned button class.
    expect(cssCode).toMatch(/\.inbox-card\.svelte-[a-z0-9]+ \.accordion-header\.card-header\s*\{/);
    expect(cssCode).toMatch(/\.inbox-card\.svelte-[a-z0-9]+ \.accordion-header\.card-header:focus-visible\s*\{/);
    // A regression to a plain `.card-header {` rule compiles to either a
    // pruned `/* (unused) ... */` comment or a hash-suffixed dead selector.
    expect(cssCode).not.toMatch(/\(unused\)[^{]*\.card-header/);
    expect(cssCode).not.toMatch(/\.card-header\.svelte-[a-z0-9]+\s*\{/);
    expect(warnings.filter((w) => /Unused CSS selector/.test(w.message))).toEqual([]);
  });

  it('Briefing: section-heading rules are live and :global, base rule present for metrics h2', () => {
    const { cssCode, warnings } = compiledCss('../components/Briefing.svelte');
    expect(cssCode).toMatch(/\.briefing-section\.svelte-[a-z0-9]+ \.accordion-header\.section-heading\s*\{/);
    expect(cssCode).toMatch(/\.briefing-section\.svelte-[a-z0-9]+ \.accordion-header\.section-heading:hover\s*\{/);
    // The base .section-heading rule (used by the non-accordion metrics h2)
    // must survive with Briefing's own hash — it was lost in the accordion
    // conversion and the metrics heading rendered unstyled.
    expect(cssCode).toMatch(/\.section-heading\.svelte-[a-z0-9]+\s*\{/);
    expect(cssCode).not.toMatch(/\(unused\)[^{]*section-heading/);
    expect(warnings.filter((w) => /Unused CSS selector/.test(w.message))).toEqual([]);
  });

  it('AccordionItem: header/body/actions rows carry no `all: unset` that erases consumer styling', () => {
    // The component's own reset must be limited to explicit properties
    // (font/color/text-align inherit etc.), not `all: unset` — an `all:
    // unset` on the header button would also defeat :global parent rules
    // that set properties the component does not re-declare.
    const { cssCode } = compiledCss('../lib/AccordionItem.svelte');
    expect(cssCode).not.toMatch(/all:\s*unset/);
  });
});
