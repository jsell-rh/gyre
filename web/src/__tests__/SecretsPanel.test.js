/**
 * SecretsPanel.test.js — Tests for the scoped secrets admin panel
 * (platform-model.md §7 UI, task-098)
 *
 * Covers:
 *   - Renders the metadata table (name, type, created, expires, rotated, status)
 *   - NEVER renders a secret value — the value column is always the
 *     write-only placeholder, even when the API response is fresh
 *   - Loads with the caller's (scope, scopeId)
 *   - Create form sends the value to secretCreate and reloads
 *   - Rotate sends the new value to secretRotate (id-keyed)
 *   - Delete asks for confirmation before calling secretDelete
 *   - Error state surfaces the API failure
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    secretsList: vi.fn().mockResolvedValue([]),
    secretCreate: vi.fn().mockResolvedValue({}),
    secretDelete: vi.fn().mockResolvedValue(null),
    secretRotate: vi.fn().mockResolvedValue({}),
  },
  setAuthToken: vi.fn(),
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  toastInfo: vi.fn(),
}));

import SecretsPanel from '../components/SecretsPanel.svelte';
import { api } from '../lib/api.js';

const mockSecrets = [
  {
    id: 'sec-1',
    name: 'DATABASE_URL',
    scope: 'workspace',
    scope_id: 'ws-1',
    secret_type: 'static',
    created_at: 1_700_000_000,
    expires_at: null,
    last_rotated_at: null,
  },
  {
    id: 'sec-2',
    name: 'OAUTH_TOKEN',
    scope: 'workspace',
    scope_id: 'ws-1',
    secret_type: 'rotated',
    created_at: 1_700_000_000,
    expires_at: Math.floor(Date.now() / 1000) + 1800, // expiring within 1h
    last_rotated_at: 1_700_000_500,
  },
];

describe('SecretsPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders without throwing', () => {
    expect(() => render(SecretsPanel, { props: { scope: 'workspace', scopeId: 'ws-1' } })).not.toThrow();
  });

  it('loads secrets for the given scope and scopeId', async () => {
    api.secretsList.mockResolvedValue(mockSecrets);
    const { container } = render(SecretsPanel, { props: { scope: 'workspace', scopeId: 'ws-1' } });
    await waitFor(() => {
      expect(api.secretsList).toHaveBeenCalledWith('workspace', 'ws-1');
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="secrets-table"]')).toBeTruthy();
    });
  });

  it('renders secret names, types, rotation metadata, and expiry status', async () => {
    api.secretsList.mockResolvedValue(mockSecrets);
    const { container } = render(SecretsPanel, { props: { scope: 'workspace', scopeId: 'ws-1' } });
    await waitFor(() => {
      const table = container.querySelector('[data-testid="secrets-table"]');
      expect(table.textContent).toContain('DATABASE_URL');
      expect(table.textContent).toContain('OAUTH_TOKEN');
      expect(table.textContent).toContain('rotated');
    });
    const badges = container.querySelectorAll('[data-testid="secret-status"]');
    expect(badges.length).toBe(2);
    expect(badges[0].textContent.trim()).toBe('none');
    expect(badges[1].textContent.trim()).toBe('expiring');
  });

  it('never renders a secret value — the value column is the write-only placeholder', async () => {
    // Even if a buggy server leaked a value into the list response, the
    // panel must not display it: the value column is a fixed placeholder.
    api.secretsList.mockResolvedValue([
      { ...mockSecrets[0], value: 'postgres://leaked:password@host/db' },
    ]);
    const { container } = render(SecretsPanel, { props: { scope: 'workspace', scopeId: 'ws-1' } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="secrets-table"]')).toBeTruthy();
    });
    expect(container.textContent).not.toContain('postgres://leaked:password@host/db');
    const valueCells = container.querySelectorAll('.secret-value');
    expect(valueCells.length).toBe(1);
    expect(valueCells[0].textContent).toBe('••••••••');
  });

  it('shows the empty state when the scope has no secrets', async () => {
    api.secretsList.mockResolvedValue([]);
    const { container } = render(SecretsPanel, { props: { scope: 'tenant', scopeId: null } });
    await waitFor(() => {
      expect(api.secretsList).toHaveBeenCalledWith('tenant', null);
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="secrets-empty"]')).toBeTruthy();
    });
  });

  it('create form sends name/type/value to secretCreate, then reloads', async () => {
    api.secretsList.mockResolvedValue([]);
    const { container } = render(SecretsPanel, { props: { scope: 'workspace', scopeId: 'ws-1' } });
    await fireEvent.click(container.querySelector('[data-testid="secrets-add-btn"]'));
    const nameInput = container.querySelector('[data-testid="secrets-new-name"]');
    const valueInput = container.querySelector('[data-testid="secrets-new-value"]');
    await fireEvent.input(nameInput, { target: { value: 'API_KEY' } });
    await fireEvent.input(valueInput, { target: { value: 'super-secret-value' } });
    await fireEvent.submit(container.querySelector('[data-testid="secrets-create-form"]'));
    await waitFor(() => {
      expect(api.secretCreate).toHaveBeenCalledWith({
        name: 'API_KEY',
        scope: 'workspace',
        scope_id: 'ws-1',
        secret_type: 'static',
        value: 'super-secret-value',
        expires_at: undefined,
      });
    });
    await waitFor(() => {
      expect(api.secretsList).toHaveBeenCalledTimes(2); // initial + reload
    });
  });

  it('rotate sends the new value keyed by the secret id', async () => {
    api.secretsList.mockResolvedValue(mockSecrets);
    const { container } = render(SecretsPanel, { props: { scope: 'workspace', scopeId: 'ws-1' } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="secrets-rotate-btn"]')).toBeTruthy();
    });
    const rotateButtons = container.querySelectorAll('[data-testid="secrets-rotate-btn"]');
    await fireEvent.click(rotateButtons[1]); // OAUTH_TOKEN (sec-2)
    const valueInput = container.querySelector('[data-testid="secrets-rotate-value"]');
    await fireEvent.input(valueInput, { target: { value: 'rotated-new-value' } });
    await fireEvent.submit(container.querySelector('[data-testid="secrets-rotate-form"]'));
    await waitFor(() => {
      expect(api.secretRotate).toHaveBeenCalledWith('sec-2', {
        new_value: 'rotated-new-value',
        expires_at: undefined,
      });
    });
  });

  it('delete asks for confirmation before calling the API', async () => {
    api.secretsList.mockResolvedValue(mockSecrets);
    const { container } = render(SecretsPanel, { props: { scope: 'workspace', scopeId: 'ws-1' } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="secrets-delete-btn"]')).toBeTruthy();
    });
    const deleteButtons = container.querySelectorAll('[data-testid="secrets-delete-btn"]');
    await fireEvent.click(deleteButtons[0]);
    // Confirmation shown, API not yet called.
    expect(container.querySelector('[data-testid="secrets-delete-confirm"]')).toBeTruthy();
    expect(api.secretDelete).not.toHaveBeenCalled();
    // Confirm → delete by id.
    const confirm = container.querySelector('[data-testid="secrets-delete-confirm"] button');
    await fireEvent.click(confirm);
    await waitFor(() => {
      expect(api.secretDelete).toHaveBeenCalledWith('sec-1');
    });
  });

  it('surfaces list errors', async () => {
    api.secretsList.mockRejectedValue(new Error('API /secrets: 403 Forbidden'));
    const { container } = render(SecretsPanel, { props: { scope: 'workspace', scopeId: 'ws-1' } });
    await waitFor(() => {
      const err = container.querySelector('[data-testid="secrets-error"]');
      expect(err).toBeTruthy();
      expect(err.textContent).toContain('403');
    });
  });
});
