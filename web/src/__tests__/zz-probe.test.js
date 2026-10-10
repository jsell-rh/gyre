import { describe, it, expect, vi } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';
import { writeFileSync } from 'node:fs';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { compile } from 'svelte/compiler';
import Inbox from '../components/Inbox.svelte';

vi.mock('../lib/api.js', () => ({
  api: { myNotifications: vi.fn().mockResolvedValue([]) },
}));

describe('probe compile api', () => {
  it('compiles Inbox and dumps css + warnings + runtime classes', async () => {
    const file = resolve('src/components/Inbox.svelte');
    const source = readFileSync(file, 'utf-8');
    const result = await compile(source, { filename: file, generate: 'client' });
    const out = {
      warnings: result.warnings?.map(w => ({ code: w.code, message: w.message })),
      cssKeys: result.css ? Object.keys(result.css) : null,
      cssCode: result.css?.code?.slice(0, 3000),
      cssCodeLength: result.css?.code?.length,
    };
    // Runtime classes
    const { container } = render(Inbox);
    await waitFor(() => document.querySelector('.inbox-header'));
    const root = container.querySelector('.inbox');
    out.runtimeRootClass = root?.className;
    out.runtimeInboxCardClass = container.querySelector('.inbox-card')?.className ?? null;
    writeFileSync('/tmp/probe-compile.json', JSON.stringify(out, null, 2));
    expect(true).toBe(true);
  });
});
