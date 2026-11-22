import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

// API surface the try-it affordance consumes.
const tryItMock = vi.fn();
const envsMock = vi.fn().mockResolvedValue([]);
const previewEnvironmentMock = vi.fn().mockResolvedValue(null);
const teardownMock = vi.fn().mockResolvedValue(null);
vi.mock('../lib/api.js', () => ({
  api: {
    tryIt: (...args) => tryItMock(...args),
    mrTryItEnvironments: (...args) => envsMock(...args),
    previewEnvironment: (...args) => previewEnvironmentMock(...args),
    teardownPreviewEnvironment: (...args) => teardownMock(...args),
  },
}));

import TryItButton from '../lib/TryItButton.svelte';

describe('TryItButton — merged-MR preview environment affordance', () => {
  beforeEach(() => {
    tryItMock.mockReset();
    envsMock.mockReset().mockResolvedValue([]);
    previewEnvironmentMock.mockReset().mockResolvedValue(null);
    teardownMock.mockReset().mockResolvedValue(null);
  });

  it('renders the Try it action and surfaces the existing environment on mount', async () => {
    envsMock.mockResolvedValue([
      { id: 'env-1', status: 'active', mr_id: 'mr-7', preview_url: 'http://10.0.0.7:18080' },
    ]);
    const { getByText, container } = render(TryItButton, { props: { mrId: 'mr-7' } });
    expect(getByText('Try it')).toBeTruthy();
    await waitFor(() => {
      expect(envsMock).toHaveBeenCalledWith('mr-7');
      // The existing environment's status (and its preview link) shows —
      // a re-visit must not offer provisioning over a live environment.
      expect(container.textContent).toContain('Preview running');
      expect(getByText('Open')).toBeTruthy();
    });
  });

  it('provisions on click via POST try-it and shows live status', async () => {
    tryItMock.mockResolvedValue({ id: 'env-2', status: 'active', preview_url: 'http://10.0.0.9:18080' });
    const { getByText, container } = render(TryItButton, { props: { mrId: 'mr-7' } });
    await waitFor(() => {
      expect(envsMock).toHaveBeenCalled();
    });
    await fireEvent.click(getByText('Try it'));
    await waitFor(() => {
      expect(tryItMock).toHaveBeenCalledWith('mr-7');
      expect(container.textContent).toContain('Preview running');
      expect(getByText('Open')).toBeTruthy();
    });
  });

  it('provisioning failure surfaces an error state with a retry affordance', async () => {
    tryItMock.mockRejectedValue(new Error('spawn failed'));
    const { getByText, container } = render(TryItButton, { props: { mrId: 'mr-7' } });
    await waitFor(() => {
      expect(envsMock).toHaveBeenCalled();
    });
    await fireEvent.click(getByText('Try it'));
    await waitFor(() => {
      expect(container.textContent).toContain('spawn failed');
      expect(getByText('Try again')).toBeTruthy();
    });
  });

  it('teardown tears the environment down via DELETE', async () => {
    envsMock.mockResolvedValue([
      { id: 'env-1', status: 'active', mr_id: 'mr-7', preview_url: 'http://10.0.0.7:18080' },
    ]);
    const { getByText, container } = render(TryItButton, { props: { mrId: 'mr-7' } });
    await waitFor(() => {
      expect(container.textContent).toContain('Preview running');
    });
    await fireEvent.click(getByText('Tear down'));
    await waitFor(() => {
      expect(teardownMock).toHaveBeenCalledWith('env-1');
      // After teardown the affordance returns to the provisioning state.
      expect(getByText('Try it')).toBeTruthy();
    });
  });
});
