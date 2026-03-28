import { describe, it, expect, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import WorkspaceHome from '../components/WorkspaceHome.svelte';

describe('WorkspaceHome', () => {
  it('renders without throwing', () => {
    expect(() => render(WorkspaceHome)).not.toThrow();
  });

  it('shows "Select a workspace" when workspace is null', () => {
    const { getByText } = render(WorkspaceHome, { props: { workspace: null } });
    expect(getByText('Select a workspace')).toBeTruthy();
  });

  it('shows all five sections when workspace is provided', () => {
    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });
    expect(container.querySelector('[data-testid="section-decisions"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-repos"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-briefing"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-specs"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-agent-rules"]')).toBeTruthy();
  });

  it('shows decisions badge when decisionsCount > 0', () => {
    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, {
      props: { workspace: ws, decisionsCount: 7 },
    });
    const badge = container.querySelector('.section-badge');
    expect(badge).toBeTruthy();
    expect(badge.textContent).toBe('7');
  });

  it('does not show decisions badge when decisionsCount is 0', () => {
    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, {
      props: { workspace: ws, decisionsCount: 0 },
    });
    expect(container.querySelector('.section-badge')).toBeNull();
  });

  it('shows autonomous message when decisionsCount is 0', () => {
    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { getByText } = render(WorkspaceHome, {
      props: { workspace: ws, decisionsCount: 0 },
    });
    expect(getByText(/running autonomously/)).toBeTruthy();
  });

  it('shows item count message when decisionsCount > 0', () => {
    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { getByText } = render(WorkspaceHome, {
      props: { workspace: ws, decisionsCount: 3 },
    });
    expect(getByText(/3 items require your attention/)).toBeTruthy();
  });

  it('each section has correct aria-labelledby', () => {
    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });
    const sections = container.querySelectorAll('.home-section');
    sections.forEach(section => {
      const labelledBy = section.getAttribute('aria-labelledby');
      expect(labelledBy).toBeTruthy();
      expect(container.querySelector(`#${labelledBy}`)).toBeTruthy();
    });
  });

  it('Manage rules link has correct href', () => {
    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });
    const link = container.querySelector('.section-action');
    expect(link).toBeTruthy();
    expect(link.getAttribute('href')).toBe('/workspaces/test/agent-rules');
  });

  it('uses workspace id as fallback when slug is missing', () => {
    const ws = { id: 'ws-1', name: 'Test' };
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });
    const link = container.querySelector('.section-action');
    expect(link.getAttribute('href')).toBe('/workspaces/ws-1/agent-rules');
  });
});
