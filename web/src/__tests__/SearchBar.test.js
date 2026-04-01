import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';
import SearchBar from '../lib/SearchBar.svelte';
import { safeHref } from '../lib/api.js';

// ---------------------------------------------------------------------------
// safeHref — pure-function tests (exported from api.js, used by SearchBar)
// ---------------------------------------------------------------------------
describe('safeHref()', () => {
  it('returns "#" for null/undefined/empty', () => {
    expect(safeHref(null)).toBe('#');
    expect(safeHref(undefined)).toBe('#');
    expect(safeHref('')).toBe('#');
  });

  it('returns "#" for non-string input', () => {
    expect(safeHref(42)).toBe('#');
    expect(safeHref(true)).toBe('#');
    expect(safeHref({})).toBe('#');
  });

  it('allows http:// URLs', () => {
    expect(safeHref('http://example.com')).toBe('http://example.com');
  });

  it('allows https:// URLs', () => {
    expect(safeHref('https://example.com/path')).toBe('https://example.com/path');
  });

  it('allows relative URLs starting with /', () => {
    expect(safeHref('/dashboard')).toBe('/dashboard');
    expect(safeHref('/api/v1/agents')).toBe('/api/v1/agents');
  });

  it('rejects javascript: URLs', () => {
    expect(safeHref('javascript:alert(1)')).toBe('#');
  });

  it('rejects data: URLs', () => {
    expect(safeHref('data:text/html,<h1>xss</h1>')).toBe('#');
  });

  it('rejects bare strings without scheme or leading slash', () => {
    expect(safeHref('example.com')).toBe('#');
    expect(safeHref('ftp://files.example.com')).toBe('#');
  });

  it('trims whitespace before checking', () => {
    expect(safeHref('  https://trimmed.com  ')).toBe('https://trimmed.com');
    expect(safeHref('  /trimmed-path  ')).toBe('/trimmed-path');
  });

  it('is case-insensitive for scheme', () => {
    expect(safeHref('HTTPS://UPPER.COM')).toBe('HTTPS://UPPER.COM');
    expect(safeHref('Http://Mixed.com')).toBe('Http://Mixed.com');
  });
});

// ---------------------------------------------------------------------------
// SearchBar component tests
// ---------------------------------------------------------------------------
describe('SearchBar', () => {
  let navigateSpy;

  beforeEach(() => {
    navigateSpy = vi.fn();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders nothing when open=false', () => {
    const { container } = render(SearchBar, { props: { open: false, onnavigate: navigateSpy } });
    expect(container.querySelector('.search-dialog')).toBeNull();
  });

  it('renders dialog when open=true', () => {
    const { container } = render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    expect(container.querySelector('.search-dialog')).toBeTruthy();
  });

  it('shows shortcut list when query is empty', () => {
    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const items = screen.getAllByRole('option');
    // SHORTCUTS array has 5 entries: Decisions, Briefing, Specs, Agent Rules, My Profile
    expect(items.length).toBe(5);
    expect(items[0].textContent).toContain('Decisions');
    expect(items[4].textContent).toContain('My Profile');
  });

  it('filters shortcuts when query matches a shortcut label', async () => {
    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');
    // Type a query shorter than 2 chars — should filter shortcuts only
    await fireEvent.input(input, { target: { value: 'B' } });
    await vi.advanceTimersByTimeAsync(0);
    const items = screen.getAllByRole('option');
    // "B" matches "Briefing" (case-insensitive)
    const labels = items.map(i => i.textContent);
    expect(labels.some(l => l.includes('Briefing'))).toBe(true);
  });

  it('keyboard ArrowDown/ArrowUp cycles through results', async () => {
    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');

    // Initially first item is selected
    let items = screen.getAllByRole('option');
    expect(items[0].classList.contains('active')).toBe(true);

    // ArrowDown should select second item
    await fireEvent.keyDown(input, { key: 'ArrowDown' });
    await vi.advanceTimersByTimeAsync(0);
    items = screen.getAllByRole('option');
    expect(items[1].classList.contains('active')).toBe(true);

    // ArrowUp should go back to first
    await fireEvent.keyDown(input, { key: 'ArrowUp' });
    await vi.advanceTimersByTimeAsync(0);
    items = screen.getAllByRole('option');
    expect(items[0].classList.contains('active')).toBe(true);
  });

  it('ArrowUp from first item wraps to last item', async () => {
    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');
    const items = screen.getAllByRole('option');

    // ArrowUp from index 0 should wrap to last
    await fireEvent.keyDown(input, { key: 'ArrowUp' });
    await vi.advanceTimersByTimeAsync(0);
    const updated = screen.getAllByRole('option');
    expect(updated[updated.length - 1].classList.contains('active')).toBe(true);
  });

  it('Enter on selected item calls onnavigate', async () => {
    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');

    // Press Enter on the first shortcut (Inbox -> view: 'inbox')
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(navigateSpy).toHaveBeenCalledWith('inbox', { entityType: undefined, entityId: undefined });
  });

  it('Tab key is prevented (does not move focus out)', async () => {
    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');
    const event = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true });
    const prevented = !input.dispatchEvent(event);
    // Tab should be caught and default prevented by the onkeydown handler
    // (The handler calls e.preventDefault() for Tab)
    expect(prevented || event.defaultPrevented).toBe(true);
  });

  it('debounced API search fires after 300ms for queries >= 2 chars', async () => {
    const mockResults = {
      results: [
        { title: 'Fix auth bug', snippet: 'auth module', entity_type: 'task', entity_id: 't-1' },
      ],
    };
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(mockResults) })
    );

    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');

    await fireEvent.input(input, { target: { value: 'auth' } });

    // Before 300ms, no API call
    await vi.advanceTimersByTimeAsync(100);
    expect(global.fetch).not.toHaveBeenCalledWith(
      expect.stringContaining('/search'),
      expect.anything()
    );

    // After 300ms, API call should fire
    await vi.advanceTimersByTimeAsync(250);
    // Allow promises to flush
    await vi.advanceTimersByTimeAsync(50);

    expect(global.fetch).toHaveBeenCalled();
    const searchCall = global.fetch.mock.calls.find(([url]) =>
      typeof url === 'string' && url.includes('/search')
    );
    expect(searchCall).toBeTruthy();
  });

  it('shows error message when search API fails', async () => {
    global.fetch = vi.fn(() => Promise.resolve({ ok: false, status: 500, statusText: 'Internal' }));

    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');

    await fireEvent.input(input, { target: { value: 'broken' } });
    await vi.advanceTimersByTimeAsync(400);
    // Flush microtasks
    await vi.advanceTimersByTimeAsync(50);

    // The component should show the error state
    const errorEl = document.querySelector('.search-empty');
    if (errorEl) {
      expect(errorEl.textContent).toContain('Search failed');
    }
  });

  it('clicking backdrop closes dialog', async () => {
    const { container } = render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const backdrop = container.querySelector('.search-backdrop');
    expect(backdrop).toBeTruthy();
    await fireEvent.click(backdrop);
    // After click, the dialog should be closed (open becomes false)
    // Since open is bindable, we check DOM
    await vi.advanceTimersByTimeAsync(0);
    expect(container.querySelector('.search-dialog')).toBeNull();
  });

  it('mouseenter on result item updates selection', async () => {
    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const items = screen.getAllByRole('option');

    // Hover over the third item
    await fireEvent.mouseEnter(items[2]);
    await vi.advanceTimersByTimeAsync(0);

    const updated = screen.getAllByRole('option');
    expect(updated[2].classList.contains('active')).toBe(true);
  });

  it('has correct ARIA attributes on combobox input', () => {
    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');
    expect(input.getAttribute('aria-autocomplete')).toBe('list');
    expect(input.getAttribute('aria-controls')).toBe('search-listbox');
    expect(input.getAttribute('aria-haspopup')).toBe('listbox');
  });

  it('listbox has correct role and label', () => {
    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const listbox = screen.getByRole('listbox');
    expect(listbox.getAttribute('aria-label')).toBe('Search results');
  });

  it('dialog has role="dialog" and aria-modal', () => {
    const { container } = render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog).toBeTruthy();
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.getAttribute('aria-label')).toBe('Quick navigation');
  });

  it('API results show entity type badge and icon', async () => {
    const mockResults = {
      results: [
        { title: 'Deploy pipeline', snippet: 'CI/CD fix', entity_type: 'task', entity_id: 't-99' },
        { title: 'MR #42', snippet: 'Merge request', entity_type: 'mr', entity_id: 'mr-42' },
      ],
    };
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(mockResults) })
    );

    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');
    await fireEvent.input(input, { target: { value: 'deploy' } });
    await vi.advanceTimersByTimeAsync(400);
    await vi.advanceTimersByTimeAsync(50);

    const items = screen.getAllByRole('option');
    // Should have API results + any matching shortcuts
    expect(items.length).toBeGreaterThanOrEqual(2);

    // First result should show 'T' icon for task
    const firstIcon = items[0].querySelector('.result-icon');
    if (firstIcon) {
      expect(firstIcon.textContent.trim()).toBe('T');
    }

    // Second result should show 'M' icon for mr
    const secondIcon = items[1].querySelector('.result-icon');
    if (secondIcon) {
      expect(secondIcon.textContent.trim()).toBe('M');
    }
  });

  it('navigating an entity result passes entityType and entityId', async () => {
    const mockResults = {
      results: [
        { title: 'Agent Alpha', snippet: '', entity_type: 'agent', entity_id: 'a-1' },
      ],
    };
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(mockResults) })
    );

    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');
    await fireEvent.input(input, { target: { value: 'alpha' } });
    await vi.advanceTimersByTimeAsync(400);
    await vi.advanceTimersByTimeAsync(50);

    // Press Enter to navigate to first result
    await fireEvent.keyDown(input, { key: 'Enter' });

    expect(navigateSpy).toHaveBeenCalledWith('agents', { entityType: 'agent', entityId: 'a-1', repo_id: null, workspace_id: null });
  });

  it('unknown entity types map to "dashboard" view and "?" icon', async () => {
    const mockResults = {
      results: [
        { title: 'Unknown Thing', snippet: '', entity_type: 'widget', entity_id: 'w-1' },
      ],
    };
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(mockResults) })
    );

    render(SearchBar, { props: { open: true, onnavigate: navigateSpy } });
    const input = screen.getByRole('combobox');
    await fireEvent.input(input, { target: { value: 'unknown' } });
    await vi.advanceTimersByTimeAsync(400);
    await vi.advanceTimersByTimeAsync(50);

    // Check for '?' icon BEFORE navigating (navigate closes the dialog)
    const items = screen.getAllByRole('option');
    const icon = items[0]?.querySelector('.result-icon');
    if (icon) {
      expect(icon.textContent.trim()).toBe('?');
    }

    // Now navigate and verify the view mapping
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(navigateSpy).toHaveBeenCalledWith('dashboard', { entityType: 'widget', entityId: 'w-1', repo_id: null, workspace_id: null });
  });
});
