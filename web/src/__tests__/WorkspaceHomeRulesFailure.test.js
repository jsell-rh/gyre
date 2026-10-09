// Independent regression: a failed rules lookup must never look like an empty
// successful effective rule set, and Retry must perform the lookup again.
import { beforeEach, expect, it, vi } from 'vitest';
import { render, waitFor, fireEvent, within } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => {
  const methods = {
    getMetaSpecs: vi.fn(),
    getWorkspaceBriefing: vi.fn().mockResolvedValue({ narrative: '', sections: [] }),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    workspaceGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
  };
  // Unrelated dashboard panels have no data in this focused fixture.
  return { api: new Proxy(methods, {
    get(target, name) {
      if (!(name in target)) target[name] = vi.fn().mockResolvedValue([]);
      return target[name];
    },
  }) };
});

import { api } from '../lib/api.js';
import WorkspaceHome from '../components/WorkspaceHome.svelte';

beforeEach(() => vi.clearAllMocks());

it('reports a failed effective-rules lookup and retries instead of claiming an empty set', async () => {
  const rule = { id: 'rule-1', name: 'Required workspace rule', kind: 'Standard', required: true, version: 1, updated_at: 0 };
  api.getMetaSpecs.mockImplementation(({ scope }) => scope === 'Workspace'
    ? Promise.reject(new Error('rules lookup unavailable'))
    : Promise.resolve([]));
  const { getByTestId } = render(WorkspaceHome, {
    workspace: { id: 'ws-1', name: 'Payments', slug: 'payments', trust_level: 'Guided' },
  });
  const section = within(getByTestId('section-agent-rules'));
  await waitFor(() => expect(section.getByRole('alert').textContent).toContain('rules lookup unavailable'));
  expect(section.queryByTestId('rules-summary')).toBeNull();
  expect(api.getMetaSpecs).toHaveBeenCalledWith({ scope: 'Workspace', scope_id: 'ws-1' });

  api.getMetaSpecs.mockImplementation(({ scope }) => Promise.resolve(scope === 'Workspace' ? [rule] : []));
  await fireEvent.click(section.getByRole('button', { name: /retry/i }));
  await waitFor(() => expect(section.getByTestId('rules-summary').textContent).toContain('1 meta-spec'));
  expect(section.queryByRole('alert')).toBeNull();
  expect(section.getByText(rule.name)).toBeTruthy();
});

it('keeps workspace B rules when workspace A finishes loading after navigation', async () => {
  let finishA;
  const pendingA = new Promise(resolve => { finishA = resolve; });
  const ruleA = { id: 'rule-a', name: 'Workspace A private rule', kind: 'Standard', required: true, version: 1, updated_at: 0 };
  const ruleB = { ...ruleA, id: 'rule-b', name: 'Workspace B required rule' };
  api.getMetaSpecs.mockImplementation(({ scope, scope_id }) => {
    if (scope === 'Global') return Promise.resolve([]);
    return scope_id === 'ws-a' ? pendingA : Promise.resolve([ruleB]);
  });
  const { getByTestId, rerender } = render(WorkspaceHome, {
    workspace: { id: 'ws-a', name: 'A', slug: 'a', trust_level: 'Guided' },
  });
  await waitFor(() => expect(api.getMetaSpecs).toHaveBeenCalledWith({ scope: 'Workspace', scope_id: 'ws-a' }));
  await rerender({ workspace: { id: 'ws-b', name: 'B', slug: 'b', trust_level: 'Guided' } });
  const section = within(getByTestId('section-agent-rules'));
  await waitFor(() => expect(section.getByText(ruleB.name)).toBeTruthy());
  finishA([ruleA]);
  // Yield one event-loop turn so the entire delayed Promise.all and Svelte
  // update settle before checking the final visible ownership.
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(section.getByText(ruleB.name)).toBeTruthy();
  expect(section.queryByText(ruleA.name)).toBeNull();
});
