import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import ScopeBreadcrumb from '../lib/ScopeBreadcrumb.svelte';

describe('ScopeBreadcrumb', () => {
  const tenant = { id: 't1', name: 'Acme Corp' };
  const workspace = { id: 'ws-1', name: 'Platform' };
  const workspace2 = { id: 'ws-2', name: 'Payments' };
  const repo = { id: 'r1', name: 'billing-service' };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  // --- Basic rendering ---

  it('renders "Gyre" when no tenant or workspace is provided', () => {
    const { getByText } = render(ScopeBreadcrumb);
    expect(getByText('Gyre')).toBeTruthy();
  });

  it('renders tenant name as a button', () => {
    const { getByText } = render(ScopeBreadcrumb, { props: { tenant, workspace } });
    const btn = getByText('Acme Corp');
    expect(btn.tagName).toBe('BUTTON');
  });

  it('renders workspace name', () => {
    const { getByText } = render(ScopeBreadcrumb, { props: { tenant, workspace } });
    expect(getByText('Platform')).toBeTruthy();
  });

  it('renders repo crumb after workspace', () => {
    const { getByText } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, repo },
    });
    expect(getByText('billing-service')).toBeTruthy();
  });

  it('renders separator between tenant and workspace', () => {
    const { container } = render(ScopeBreadcrumb, { props: { tenant, workspace } });
    const seps = container.querySelectorAll('.sep');
    expect(seps.length).toBeGreaterThanOrEqual(1);
  });

  it('renders two separators when tenant + workspace + repo', () => {
    const { container } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, repo },
    });
    const seps = container.querySelectorAll('.sep');
    expect(seps.length).toBe(2);
  });

  it('has nav with aria-label="Scope"', () => {
    const { container } = render(ScopeBreadcrumb);
    const nav = container.querySelector('nav[aria-label="Scope"]');
    expect(nav).toBeTruthy();
  });

  // --- Navigation callbacks ---

  it('clicking tenant crumb calls onnavigate with tenant scope', async () => {
    const onnavigate = vi.fn();
    const { getByText } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, onnavigate },
    });
    await fireEvent.click(getByText('Acme Corp'));
    expect(onnavigate).toHaveBeenCalledWith('explorer', { scope: 'tenant' });
  });

  it('clicking workspace crumb (single workspace) calls onnavigate with workspace scope', async () => {
    const onnavigate = vi.fn();
    const { getByText } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace], onnavigate },
    });
    await fireEvent.click(getByText('Platform'));
    expect(onnavigate).toHaveBeenCalledWith('explorer', { scope: 'workspace', workspace });
  });

  it('clicking repo crumb calls onnavigate with repo scope', async () => {
    const onnavigate = vi.fn();
    const { getByText } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, repo, onnavigate },
    });
    await fireEvent.click(getByText('billing-service'));
    expect(onnavigate).toHaveBeenCalledWith('explorer', { scope: 'repo', repo });
  });

  // --- Workspace dropdown ---

  it('does NOT show dropdown when only one workspace', () => {
    const { container } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace] },
    });
    expect(container.querySelector('.ws-dropdown')).toBeNull();
  });

  it('shows dropdown caret when multiple workspaces', () => {
    const { container } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace, workspace2] },
    });
    expect(container.querySelector('.dropdown-caret')).toBeTruthy();
  });

  it('workspace button has aria-haspopup="listbox" with multiple workspaces', () => {
    const { container } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace, workspace2] },
    });
    const btn = container.querySelector('.workspace-crumb');
    expect(btn.getAttribute('aria-haspopup')).toBe('listbox');
  });

  it('opens dropdown on workspace click when multiple workspaces', async () => {
    const { container, getByText } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace, workspace2] },
    });
    expect(container.querySelector('.ws-dropdown')).toBeNull();

    await fireEvent.click(getByText('Platform'));

    await waitFor(() => {
      expect(container.querySelector('.ws-dropdown')).toBeTruthy();
    });
  });

  it('dropdown lists all workspaces', async () => {
    const { getByText } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace, workspace2] },
    });
    await fireEvent.click(getByText('Platform'));
    await waitFor(() => {
      expect(getByText('Payments')).toBeTruthy();
    });
  });

  it('current workspace option has aria-selected="true"', async () => {
    const { getByText, container } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace, workspace2] },
    });
    await fireEvent.click(getByText('Platform'));
    await waitFor(() => {
      const active = container.querySelector('.ws-option[aria-selected="true"]');
      expect(active).toBeTruthy();
      expect(active.textContent).toContain('Platform');
    });
  });

  it('selecting a different workspace calls onnavigate and closes dropdown', async () => {
    const onnavigate = vi.fn();
    const { getByText, container } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace, workspace2], onnavigate },
    });
    await fireEvent.click(getByText('Platform'));
    await waitFor(() => getByText('Payments'));
    await fireEvent.click(getByText('Payments'));

    expect(onnavigate).toHaveBeenCalledWith('explorer', {
      scope: 'workspace',
      workspace: workspace2,
    });
    // Dropdown should close
    await waitFor(() => {
      expect(container.querySelector('.ws-dropdown')).toBeNull();
    });
  });

  it('Escape key closes the dropdown', async () => {
    const { getByText, container } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace, workspace2] },
    });
    await fireEvent.click(getByText('Platform'));
    await waitFor(() => container.querySelector('.ws-dropdown'));

    const listbox = container.querySelector('[role="listbox"]');
    await fireEvent.keyDown(listbox, { key: 'Escape' });

    await waitFor(() => {
      expect(container.querySelector('.ws-dropdown')).toBeNull();
    });
  });

  it('dropdown has role="listbox" with aria-label', async () => {
    const { getByText, container } = render(ScopeBreadcrumb, {
      props: { tenant, workspace, workspaces: [workspace, workspace2] },
    });
    await fireEvent.click(getByText('Platform'));
    await waitFor(() => {
      const listbox = container.querySelector('[role="listbox"]');
      expect(listbox).toBeTruthy();
      expect(listbox.getAttribute('aria-label')).toBe('Select workspace');
    });
  });

  // --- Extra CSS class ---

  it('applies extra class to the nav element', () => {
    const { container } = render(ScopeBreadcrumb, {
      props: { class: 'my-custom-class' },
    });
    const nav = container.querySelector('nav');
    expect(nav.className).toContain('my-custom-class');
  });
});
