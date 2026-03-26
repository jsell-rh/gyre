import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, screen } from '@testing-library/svelte';
import DetailPanel from '../lib/DetailPanel.svelte';

const mrEntity = {
  type: 'mr',
  id: 'mr-uuid-1',
  data: {
    name: 'Fix auth retry',
    status: 'open',
    conversation_sha: null,
  },
};

const agentEntity = {
  type: 'agent',
  id: 'worker-12',
  data: {
    name: 'worker-12',
    status: 'active',
    conversation_sha: 'abc123',
  },
};

const nodeEntity = {
  type: 'node',
  id: 'node-1',
  data: {
    name: 'AuthMiddleware',
    spec_path: 'specs/system/identity-security.md',
    author_agent_id: 'worker-12',
  },
};

const specEntity = {
  type: 'spec',
  id: 'spec-1',
  data: {
    name: 'identity-security.md',
  },
};

const mergedMrEntity = {
  type: 'mr',
  id: 'mr-uuid-2',
  data: {
    name: 'Merged MR',
    status: 'merged',
    conversation_sha: 'deadbeef',
  },
};

describe('DetailPanel', () => {
  it('renders nothing when entity is null', () => {
    const { container } = render(DetailPanel, { props: { entity: null } });
    const header = container.querySelector('.panel-header');
    expect(header).toBeNull();
  });

  it('shows entity name in header', () => {
    render(DetailPanel, { props: { entity: agentEntity } });
    expect(screen.getAllByText('worker-12').length).toBeGreaterThan(0);
  });

  describe('Tab routing by entity type', () => {
    it('MR entity: shows Info, Diff, Gates, Ask Why tabs (no Attestation for open MR)', () => {
      render(DetailPanel, { props: { entity: mrEntity } });
      expect(screen.getByRole('tab', { name: /info/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /diff/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /gates/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /ask why/i })).toBeTruthy();
      expect(screen.queryByRole('tab', { name: /attestation/i })).toBeNull();
    });

    it('merged MR: includes Attestation tab', () => {
      render(DetailPanel, { props: { entity: mergedMrEntity } });
      expect(screen.getByRole('tab', { name: /attestation/i })).toBeTruthy();
    });

    it('Ask Why disabled when conversation_sha is null', () => {
      render(DetailPanel, { props: { entity: mrEntity } });
      const askWhy = screen.getByRole('tab', { name: /ask why/i });
      expect(askWhy.disabled).toBe(true);
    });

    it('Ask Why enabled when conversation_sha is set', () => {
      render(DetailPanel, { props: { entity: mergedMrEntity } });
      const askWhy = screen.getByRole('tab', { name: /ask why/i });
      expect(askWhy.disabled).toBe(false);
    });

    it('agent entity: shows Info, Chat, History, Trace tabs', () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      expect(screen.getByRole('tab', { name: /info/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /chat/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /history/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /trace/i })).toBeTruthy();
    });

    it('graph node with spec_path + author: shows Info, Spec, Chat, History', () => {
      render(DetailPanel, { props: { entity: nodeEntity } });
      expect(screen.getByRole('tab', { name: /info/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /spec/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /chat/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /history/i })).toBeTruthy();
    });

    it('spec entity: shows Content, Edit, Progress, Links, History tabs', () => {
      render(DetailPanel, { props: { entity: specEntity } });
      expect(screen.getByRole('tab', { name: /content/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /edit/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /progress/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /links/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /history/i })).toBeTruthy();
      // Info tab should NOT appear for spec entities (subsumed by Content)
      expect(screen.queryByRole('tab', { name: /^info$/i })).toBeNull();
    });
  });

  describe('Info tab content', () => {
    it('shows entity type and ID', () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      expect(screen.getAllByText('agent').length).toBeGreaterThan(0);
      expect(screen.getAllByText('worker-12').length).toBeGreaterThan(0);
    });

    it('shows status when present', () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      expect(screen.getByText('active')).toBeTruthy();
    });
  });

  describe('Close behavior', () => {
    it('calls onclose when ✕ button is clicked', async () => {
      const onclose = vi.fn();
      render(DetailPanel, { props: { entity: agentEntity, onclose } });
      const closeBtn = screen.getByRole('button', { name: /close detail panel/i });
      await fireEvent.click(closeBtn);
      expect(onclose).toHaveBeenCalledOnce();
    });

    it('calls onclose on Escape key', async () => {
      const onclose = vi.fn();
      const { container } = render(DetailPanel, { props: { entity: agentEntity, onclose } });
      const panel = container.querySelector('.detail-panel');
      await fireEvent.keyDown(panel, { key: 'Escape' });
      expect(onclose).toHaveBeenCalledOnce();
    });
  });

  describe('Pop Out behavior', () => {
    it('pop out button is present', () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      expect(screen.getByRole('button', { name: /pop out/i })).toBeTruthy();
    });

    it('toggling expanded changes panel-btn aria-label', async () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      const popBtn = screen.getByRole('button', { name: /pop out/i });
      await fireEvent.click(popBtn);
      expect(screen.getByRole('button', { name: /collapse/i })).toBeTruthy();
    });
  });

  describe('CSS classes', () => {
    it('panel has open class when entity is set', () => {
      const { container } = render(DetailPanel, { props: { entity: agentEntity } });
      expect(container.querySelector('.detail-panel.open')).toBeTruthy();
    });

    it('panel does not have open class when entity is null', () => {
      const { container } = render(DetailPanel, { props: { entity: null } });
      expect(container.querySelector('.detail-panel.open')).toBeNull();
    });
  });
});
