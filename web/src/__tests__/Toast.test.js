import { describe, it, expect, beforeEach } from 'vitest';
import { toast, dismiss, getToasts, toastSuccess, toastError, toastWarning, toastInfo } from '../lib/toast.svelte.js';

describe('toast.svelte.js', () => {
  it('toast() returns an id', () => {
    const id = toast('hello');
    expect(typeof id).toBe('number');
  });

  it('toast() adds to toast list', () => {
    toast('test message');
    const toasts = getToasts();
    expect(toasts.length).toBeGreaterThan(0);
  });

  it('dismiss() removes a toast', () => {
    const id = toast('to be dismissed', { duration: 0 });
    const before = getToasts().length;
    dismiss(id);
    const after = getToasts().length;
    expect(after).toBe(before - 1);
  });

  it('toastSuccess creates success toast', () => {
    const id = toastSuccess('Success!', { duration: 0 });
    const toasts = getToasts();
    const found = toasts.find(t => t.id === id);
    expect(found?.type).toBe('success');
    expect(found?.message).toBe('Success!');
    dismiss(id);
  });

  it('toastError creates error toast', () => {
    const id = toastError('Error!', { duration: 0 });
    const toasts = getToasts();
    const found = toasts.find(t => t.id === id);
    expect(found?.type).toBe('error');
    dismiss(id);
  });

  it('toastWarning creates warning toast', () => {
    const id = toastWarning('Warning!', { duration: 0 });
    const toasts = getToasts();
    const found = toasts.find(t => t.id === id);
    expect(found?.type).toBe('warning');
    dismiss(id);
  });

  it('toastInfo creates info toast', () => {
    const id = toastInfo('Info!', { duration: 0 });
    const toasts = getToasts();
    const found = toasts.find(t => t.id === id);
    expect(found?.type).toBe('info');
    dismiss(id);
  });

  it('toast default type is info', () => {
    const id = toast('default type', { duration: 0 });
    const toasts = getToasts();
    const found = toasts.find(t => t.id === id);
    expect(found?.type).toBe('info');
    dismiss(id);
  });
});
