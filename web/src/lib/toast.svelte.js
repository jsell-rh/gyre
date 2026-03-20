let toasts = $state([]);
let nextId = 1;

export function getToasts() {
  return toasts;
}

export function toast(message, { type = 'info', duration = 4000 } = {}) {
  const id = nextId++;
  toasts.push({ id, message, type });

  if (duration > 0) {
    setTimeout(() => dismiss(id), duration);
  }

  return id;
}

export function dismiss(id) {
  const idx = toasts.findIndex(t => t.id === id);
  if (idx !== -1) toasts.splice(idx, 1);
}

export const toastSuccess = (msg, opts) => toast(msg, { type: 'success', ...opts });
export const toastError   = (msg, opts) => toast(msg, { type: 'error',   ...opts });
export const toastWarning = (msg, opts) => toast(msg, { type: 'warning', ...opts });
export const toastInfo    = (msg, opts) => toast(msg, { type: 'info',    ...opts });
