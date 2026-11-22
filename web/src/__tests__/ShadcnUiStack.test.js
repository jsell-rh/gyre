/**
 * ShadcnUiStack.test.js — verification probes for the shadcn-svelte UI
 * stack migration (specs/development/architecture.md §Tech Stack:
 * "Svelte 5 + shadcn-svelte for the web UI").
 *
 * The defect these probes exclude: a bolted-on configuration (tailwind and
 * bits-ui present in package.json) while the bespoke design-system CSS
 * still does the actual component styling. The probes assert the real
 * behavior instead:
 *
 *   1. The consumed Button (web/src/lib/Button.svelte) renders as a
 *      shadcn-svelte component: cva/variant Tailwind class structure and
 *      the bits-ui Button primitive — not the retired
 *      `btn btn-{variant} btn-{size}` class layer.
 *   2. The Badge renders the shadcn cva variant classes.
 *   3. The Modal renders through the bits-ui Dialog primitive.
 *   4. The Tailwind pipeline is genuinely wired: the app entry imports the
 *      Tailwind stylesheet, and that stylesheet imports the Tailwind
 *      theme/utilities (removing either unstyles every ui component).
 */
import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/svelte';
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import Button from '../lib/Button.svelte';
import Badge from '../lib/Badge.svelte';
import Modal from '../lib/Modal.svelte';

const __dirname = dirname(fileURLToPath(import.meta.url));

describe('shadcn-svelte UI stack (architecture.md §Tech Stack)', () => {
  it('Button renders the shadcn cva variant classes on the bits-ui button primitive', () => {
    const { container } = render(Button, { props: { variant: 'primary', size: 'md' } });
    const btn = container.querySelector('button');
    // bits-ui Button primitive marker
    expect(btn.hasAttribute('data-button-root')).toBe(true);
    // cva base + variant classes from the Tailwind pipeline
    expect(btn.className).toContain('inline-flex');
    expect(btn.className).toContain('bg-primary');
    expect(btn.className).toContain('text-primary-foreground');
    // the retired bespoke class layer must not style the button
    expect(btn.className).not.toMatch(/\bbtn\b/);
    expect(btn.className).not.toContain('btn-primary');
  });

  it('Button variant switching changes the cva variant classes', () => {
    const { container } = render(Button, { props: { variant: 'secondary' } });
    const btn = container.querySelector('button');
    expect(btn.className).toContain('bg-secondary');
    expect(btn.className).not.toContain('bg-primary');
  });

  it('Badge renders the shadcn cva variant classes (bespoke class layer retired from styling)', () => {
    const { container } = render(Badge, { props: { value: 'Done' } });
    const badge = container.querySelector('span');
    // cva variant classes (success tone) from the Tailwind pipeline
    expect(badge.className).toContain('bg-success/15');
    expect(badge.className).toContain('text-success');
    // legacy DOM hooks remain (inert — no stylesheet defines them)
    expect(badge.className).toContain('badge');
  });

  it('Modal renders through the bits-ui Dialog primitive', () => {
    const { container } = render(Modal, { props: { open: true, title: 'Bits' } });
    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog.hasAttribute('data-dialog-content')).toBe(true);
    expect(dialog.getAttribute('aria-modal')).toBe('true');
  });

  it('the app entry imports the Tailwind stylesheet (pipeline wired, not dead config)', () => {
    const mainJs = readFileSync(resolve(__dirname, '../main.js'), 'utf8');
    expect(mainJs).toMatch(/import\s+['"]\.\/app\.css['"]/);
  });

  it('the Tailwind entry stylesheet imports the Tailwind theme and utilities', () => {
    const appCss = readFileSync(resolve(__dirname, '../app.css'), 'utf8');
    expect(appCss).toMatch(/@import\s+["']tailwindcss\/theme\.css["']/);
    expect(appCss).toMatch(/@import\s+["']tailwindcss\/utilities\.css["']/);
    // the shadcn theme tokens carry the Red Hat brand palette
    expect(appCss).toContain('--primary: #ee0000');
  });

  it('the shadcn-svelte foundations are declared dependencies of the web app', () => {
    const pkg = JSON.parse(readFileSync(resolve(__dirname, '../../package.json'), 'utf8'));
    expect(pkg.dependencies['tailwindcss']).toBeTruthy();
    expect(pkg.dependencies['bits-ui']).toBeTruthy();
    expect(pkg.dependencies['clsx']).toBeTruthy();
    expect(pkg.dependencies['tailwind-merge']).toBeTruthy();
    expect(pkg.dependencies['class-variance-authority']).toBeTruthy();
  });
});
