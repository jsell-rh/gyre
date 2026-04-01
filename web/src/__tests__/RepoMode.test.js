import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';

// Mock child components to isolate RepoMode testing
vi.mock('../components/ExplorerView.svelte', () => ({ default: function Stub() {} }));
vi.mock('../components/SpecDashboard.svelte', () => ({ default: function Stub() {} }));
vi.mock('../components/Inbox.svelte', () => ({ default: function Stub() {} }));
vi.mock('../components/ExplorerCodeTab.svelte', () => ({ default: function Stub() {} }));
vi.mock('../components/RepoSettings.svelte', () => ({
  default: function RepoSettingsStub(opts) {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'repo-settings');
    if (opts?.target) opts.target.appendChild(el);
    return { destroy() {} };
  },
}));

import RepoMode from '../components/RepoMode.svelte';

describe('RepoMode', () => {
  const ws = { id: 'ws-1', name: 'Payments' };
  const repo = { id: 'repo-1', name: 'payment-api' };

  it('renders without throwing', () => {
    expect(() => render(RepoMode, { props: { workspace: ws, repo } })).not.toThrow();
  });

  it('renders tab bar with all eight tabs', () => {
    const { container } = render(RepoMode, { props: { workspace: ws, repo } });
    const tabs = container.querySelectorAll('[role="tab"]');
    expect(tabs.length).toBe(8);
    const labels = Array.from(tabs).map(t => t.textContent.trim());
    expect(labels).toContain('Specs');
    expect(labels).toContain('Tasks');
    expect(labels).toContain('MRs');
    expect(labels).toContain('Agents');
    expect(labels).toContain('Architecture');
    expect(labels).toContain('Decisions');
    expect(labels).toContain('Code');
  });

  it('marks active tab with aria-selected=true', () => {
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'architecture' },
    });
    const activeTab = container.querySelector('#tab-architecture');
    expect(activeTab.getAttribute('aria-selected')).toBe('true');
  });

  it('marks inactive tabs with aria-selected=false', () => {
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'specs' },
    });
    const archTab = container.querySelector('#tab-architecture');
    expect(archTab.getAttribute('aria-selected')).toBe('false');
  });

  it('active tab has tabindex=0, inactive tabs have tabindex=-1', () => {
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'decisions' },
    });
    const active = container.querySelector('#tab-decisions');
    const inactive = container.querySelector('#tab-specs');
    expect(active.getAttribute('tabindex')).toBe('0');
    expect(inactive.getAttribute('tabindex')).toBe('-1');
  });

  it('calls onTabChange when tab is clicked', async () => {
    const onTabChange = vi.fn();
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'specs', onTabChange },
    });
    const codeTab = container.querySelector('#tab-code');
    await fireEvent.click(codeTab);
    expect(onTabChange).toHaveBeenCalledWith('code');
  });

  it('tablist has correct aria-label', () => {
    const { container } = render(RepoMode, { props: { workspace: ws, repo } });
    const tablist = container.querySelector('[role="tablist"]');
    expect(tablist.getAttribute('aria-label')).toBe('Repo navigation');
  });

  it('tab panel has correct id and aria-labelledby', () => {
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'specs' },
    });
    const panel = container.querySelector('[role="tabpanel"]');
    expect(panel.id).toBe('tabpanel-specs');
    expect(panel.getAttribute('aria-labelledby')).toBe('tab-specs');
  });

  it('each tab has aria-controls pointing to the panel', () => {
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'specs' },
    });
    const specsTab = container.querySelector('#tab-specs');
    expect(specsTab.getAttribute('aria-controls')).toBe('tabpanel-specs');
  });

  it('ArrowRight calls onTabChange with next tab', async () => {
    const onTabChange = vi.fn();
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'specs', onTabChange },
    });
    const tablist = container.querySelector('[role="tablist"]');
    await fireEvent.keyDown(tablist, { key: 'ArrowRight' });
    expect(onTabChange).toHaveBeenCalledWith('tasks');
  });

  it('ArrowLeft wraps around to last tab from first', async () => {
    const onTabChange = vi.fn();
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'specs', onTabChange },
    });
    const tablist = container.querySelector('[role="tablist"]');
    await fireEvent.keyDown(tablist, { key: 'ArrowLeft' });
    // Settings is the last tab (index 4), wrapping from index 0
    expect(onTabChange).toHaveBeenCalledWith('settings');
  });

  it('Home key moves to first tab', async () => {
    const onTabChange = vi.fn();
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'code', onTabChange },
    });
    const tablist = container.querySelector('[role="tablist"]');
    await fireEvent.keyDown(tablist, { key: 'Home' });
    expect(onTabChange).toHaveBeenCalledWith('specs');
  });

  it('End key moves to last tab', async () => {
    const onTabChange = vi.fn();
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'specs', onTabChange },
    });
    const tablist = container.querySelector('[role="tablist"]');
    await fireEvent.keyDown(tablist, { key: 'End' });
    expect(onTabChange).toHaveBeenCalledWith('settings');
  });

  it('settings tab renders without throwing', () => {
    // RepoSettings replaces the old placeholder in Slice 4
    expect(() => render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'settings' },
    })).not.toThrow();
    // Old placeholder is gone
  });

  it('code tab shows placeholder when repo has no id', () => {
    const noIdRepo = { id: null, name: 'test-repo' };
    const { getByText } = render(RepoMode, {
      props: { workspace: ws, repo: noIdRepo, activeTab: 'code' },
    });
    expect(getByText('No repo selected.')).toBeTruthy();
  });
});
