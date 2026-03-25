import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, screen, waitFor } from '@testing-library/svelte';
import InlineChat from '../lib/InlineChat.svelte';

describe('InlineChat', () => {
  describe('Recipient display', () => {
    it('shows "Message to X ▸" for agent type', () => {
      render(InlineChat, { props: { recipient: 'worker-12', recipientType: 'agent' } });
      expect(screen.getByText('Message to worker-12 ▸')).toBeTruthy();
    });

    it('shows "Ask about X ▸" for llm-qa type', () => {
      render(InlineChat, { props: { recipient: 'RetryPolicy', recipientType: 'llm-qa' } });
      expect(screen.getByText('Ask about RetryPolicy ▸')).toBeTruthy();
    });

    it('shows edit indicator for spec-edit type', () => {
      render(InlineChat, { props: { recipient: 'Add error handling', recipientType: 'spec-edit' } });
      expect(screen.getByText(/Edit spec:.*▸/)).toBeTruthy();
    });

    it('no recipient line when recipient is empty', () => {
      render(InlineChat, { props: { recipient: '', recipientType: 'agent' } });
      expect(screen.queryByText(/▸/)).toBeNull();
    });
  });

  describe('Input and Send', () => {
    it('Send button is disabled when input is empty', () => {
      render(InlineChat, { props: { recipient: 'worker-12', recipientType: 'agent' } });
      const btn = screen.getByRole('button', { name: /send/i });
      expect(btn.disabled).toBe(true);
    });

    it('Send button is enabled when input has text', async () => {
      render(InlineChat, { props: { recipient: 'worker-12', recipientType: 'agent' } });
      const input = screen.getByRole('textbox');
      await fireEvent.input(input, { target: { value: 'Hello agent' } });
      const btn = screen.getByRole('button', { name: /send/i });
      expect(btn.disabled).toBe(false);
    });

    it('calls onmessage with user text when Send is clicked', async () => {
      const onmessage = vi.fn().mockResolvedValue('agent response');
      render(InlineChat, { props: { recipient: 'worker-12', recipientType: 'agent', onmessage } });
      const input = screen.getByRole('textbox');
      await fireEvent.input(input, { target: { value: 'Hello!' } });
      const btn = screen.getByRole('button', { name: /send/i });
      await fireEvent.click(btn);
      expect(onmessage).toHaveBeenCalledWith('Hello!');
    });

    it('clears input after send', async () => {
      const onmessage = vi.fn().mockResolvedValue('ok');
      render(InlineChat, { props: { recipient: 'x', recipientType: 'agent', onmessage } });
      const input = screen.getByRole('textbox');
      await fireEvent.input(input, { target: { value: 'test message' } });
      await fireEvent.click(screen.getByRole('button', { name: /send/i }));
      await waitFor(() => {
        expect(input.value).toBe('');
      });
    });

    it('Ctrl+Enter submits message', async () => {
      const onmessage = vi.fn().mockResolvedValue('response');
      render(InlineChat, { props: { recipient: 'x', recipientType: 'agent', onmessage } });
      const input = screen.getByRole('textbox');
      await fireEvent.input(input, { target: { value: 'test' } });
      await fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true });
      expect(onmessage).toHaveBeenCalledWith('test');
    });
  });

  describe('Message history', () => {
    it('shows user message in history after send', async () => {
      const onmessage = vi.fn().mockResolvedValue('response text');
      render(InlineChat, { props: { recipient: 'worker-12', recipientType: 'agent', onmessage } });
      const input = screen.getByRole('textbox');
      await fireEvent.input(input, { target: { value: 'My message' } });
      await fireEvent.click(screen.getByRole('button', { name: /send/i }));
      await waitFor(() => {
        expect(screen.getByText('My message')).toBeTruthy();
      });
    });

    it('shows assistant response in history', async () => {
      const onmessage = vi.fn().mockResolvedValue('Agent says hello');
      render(InlineChat, { props: { recipient: 'worker-12', recipientType: 'agent', onmessage } });
      const input = screen.getByRole('textbox');
      await fireEvent.input(input, { target: { value: 'Hi' } });
      await fireEvent.click(screen.getByRole('button', { name: /send/i }));
      await waitFor(() => {
        expect(screen.getByText('Agent says hello')).toBeTruthy();
      });
    });

    it('Clear button removes history', async () => {
      const onmessage = vi.fn().mockResolvedValue('response');
      render(InlineChat, { props: { recipient: 'x', recipientType: 'agent', onmessage } });
      const input = screen.getByRole('textbox');
      await fireEvent.input(input, { target: { value: 'first msg' } });
      await fireEvent.click(screen.getByRole('button', { name: /send/i }));
      await waitFor(() => screen.getByText('first msg'));
      await fireEvent.click(screen.getByRole('button', { name: /clear/i }));
      expect(screen.queryByText('first msg')).toBeNull();
    });
  });

  describe('Hint text', () => {
    it('shows agent hint for agent type', () => {
      render(InlineChat, { props: { recipient: 'x', recipientType: 'agent' } });
      expect(screen.getByText(/signed and persisted/i)).toBeTruthy();
    });

    it('shows read-only hint for llm-qa type', () => {
      render(InlineChat, { props: { recipient: 'x', recipientType: 'llm-qa' } });
      expect(screen.getByText(/read-only/i)).toBeTruthy();
    });

    it('shows suggestions hint for spec-edit type', () => {
      render(InlineChat, { props: { recipient: 'x', recipientType: 'spec-edit' } });
      expect(screen.getByText(/you accept/i)).toBeTruthy();
    });
  });
});
