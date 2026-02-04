import { writable } from 'svelte/store';

export type ToastType = 'success' | 'error' | 'info' | 'warning';

export interface Toast {
  id: string;
  type: ToastType;
  message: string;
  duration: number;
  isExiting?: boolean;
}

let toastCounter = 0;

function createToastStore() {
  const { subscribe, update } = writable<Toast[]>([]);

  function addToast(type: ToastType, message: string, duration = 5000) {
    const id = `toast-${++toastCounter}`;
    const toast: Toast = { id, type, message, duration };

    update(toasts => [...toasts, toast]);

    if (duration > 0) {
      setTimeout(() => {
        removeToast(id);
      }, duration);
    }
  }

  function removeToast(id: string) {
    // Start exit animation
    update(toasts =>
      toasts.map(t => (t.id === id ? { ...t, isExiting: true } : t))
    );

    // Remove after animation
    setTimeout(() => {
      update(toasts => toasts.filter(t => t.id !== id));
    }, 300);
  }

  return {
    subscribe,
    success: (message: string, duration?: number) =>
      addToast('success', message, duration),
    error: (message: string, duration?: number) =>
      addToast('error', message, duration),
    info: (message: string, duration?: number) =>
      addToast('info', message, duration),
    warning: (message: string, duration?: number) =>
      addToast('warning', message, duration),
    remove: removeToast,
  };
}

export const toasts = createToastStore();
