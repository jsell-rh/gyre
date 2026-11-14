import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';
import NotificationSettings from '../components/NotificationSettings.svelte';

const channels = {
  in_app: true,
  email: {
    enabled: true,
    digest: 'Daily',
    min_priority: 'Medium',
    type_overrides: { SuggestedSpecLink: true },
  },
  webhook: null,
  slack: null,
};

// The mock factory is hoisted above this const, so it dispatches through
// indirection instead of closing over `apiMocks` directly.
const apiMocks = {
  notificationChannels: vi.fn(),
  updateNotificationChannels: vi.fn(),
};

vi.mock('../lib/api.js', () => ({
  api: {
    notificationChannels: (...args) => apiMocks.notificationChannels(...args),
    updateNotificationChannels: (...args) => apiMocks.updateNotificationChannels(...args),
  },
}));
vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

describe('NotificationSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.notificationChannels.mockResolvedValue({ preferences: [], channels: JSON.parse(JSON.stringify(channels)) });
    window.history.replaceState({}, '', '/settings/notifications');
  });

  it('loads and renders the channel configuration', async () => {
    const { container } = render(NotificationSettings);
    await waitFor(() => expect(apiMocks.notificationChannels).toHaveBeenCalled());

    await waitFor(() => {
      expect(container.querySelector('[data-testid="email-enabled"]').checked).toBe(true);
    });
    expect(container.querySelector('[data-testid="digest-frequency"]').value).toBe('Daily');
    expect(container.querySelector('[data-testid="email-min-priority"]').value).toBe('Medium');

    // Per-type unsubscribe state from the server: SuggestedSpecLink is
    // unsubscribed (unchecked), GateFailure still subscribed (checked).
    expect(container.querySelector('[data-testid="type-override-SuggestedSpecLink"]').checked).toBe(false);
    expect(container.querySelector('[data-testid="type-override-GateFailure"]').checked).toBe(true);
  });

  it('saves the full channel payload with in_app forced on', async () => {
    const { container } = render(NotificationSettings);
    await waitFor(() => expect(container.querySelector('[data-testid="email-enabled"]')).toBeTruthy());

    await fireEvent.change(container.querySelector('[data-testid="digest-frequency"]'), { target: { value: 'Hourly' } });
    await fireEvent.click(container.querySelector('[data-testid="save-notification-settings"]'));

    await waitFor(() => expect(apiMocks.updateNotificationChannels).toHaveBeenCalled());
    const body = apiMocks.updateNotificationChannels.mock.calls[0][0];
    expect(body.channels.in_app).toBe(true, 'in_app is always true');
    expect(body.channels.email.digest).toBe('Hourly');
    expect(body.channels.email.min_priority).toBe('Medium');
    // Only true overrides are persisted.
    expect(body.channels.email.type_overrides).toEqual({ SuggestedSpecLink: true });
  });

  it('unsubscribing a type in the UI flips its override in the saved payload', async () => {
    const { container } = render(NotificationSettings);
    await waitFor(() => expect(container.querySelector('[data-testid="type-override-GateFailure"]')).toBeTruthy());

    await fireEvent.click(container.querySelector('[data-testid="type-override-GateFailure"]'));
    await fireEvent.click(container.querySelector('[data-testid="save-notification-settings"]'));

    await waitFor(() => expect(apiMocks.updateNotificationChannels).toHaveBeenCalled());
    const body = apiMocks.updateNotificationChannels.mock.calls[0][0];
    expect(body.channels.email.type_overrides).toEqual({
      SuggestedSpecLink: true,
      GateFailure: true,
    });
  });

  it('webhook and slack sections send structured configs when enabled', async () => {
    const { container } = render(NotificationSettings);
    await waitFor(() => expect(container.querySelector('[data-testid="webhook-enabled"]')).toBeTruthy());

    await fireEvent.click(container.querySelector('[data-testid="webhook-enabled"]'));
    await fireEvent.input(container.querySelector('[data-testid="webhook-url"]'), { target: { value: 'https://hooks.example.com/x' } });
    await fireEvent.input(container.querySelector('[data-testid="webhook-secret"]'), { target: { value: 's3cret' } });

    await fireEvent.click(container.querySelector('[data-testid="slack-enabled"]'));
    await fireEvent.input(container.querySelector('[data-testid="slack-url"]'), { target: { value: 'https://hooks.slack.com/y' } });

    await fireEvent.click(container.querySelector('[data-testid="save-notification-settings"]'));
    await waitFor(() => expect(apiMocks.updateNotificationChannels).toHaveBeenCalled());
    const body = apiMocks.updateNotificationChannels.mock.calls[0][0];
    expect(body.channels.webhook).toEqual({
      url: 'https://hooks.example.com/x',
      secret: 's3cret',
      min_priority: 'Low',
    });
    expect(body.channels.slack).toEqual({
      webhook_url: 'https://hooks.slack.com/y',
      channel: null,
      min_priority: 'Low',
    });
  });

  it('email unsubscribe link (?unsubscribe=TYPE) applies the per-type opt-out on load', async () => {
    // The link in every rendered email lands here (§Email Notifications).
    window.history.replaceState({}, '', '/settings/notifications?unsubscribe=GateFailure');
    const { container } = render(NotificationSettings);

    // The opt-out is persisted without further user action…
    await waitFor(() => expect(apiMocks.updateNotificationChannels).toHaveBeenCalled());
    const body = apiMocks.updateNotificationChannels.mock.calls[0][0];
    expect(body.channels.email.type_overrides).toEqual({
      SuggestedSpecLink: true,
      GateFailure: true,
    });
    // …and the confirmation banner names the unsubscribed type.
    await waitFor(() => {
      expect(container.querySelector('[data-testid="unsubscribe-banner"]').textContent).toContain('GateFailure');
    });
    // The checkbox reflects the new state.
    expect(container.querySelector('[data-testid="type-override-GateFailure"]').checked).toBe(false);
  });
});
