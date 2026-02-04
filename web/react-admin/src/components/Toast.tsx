import { useState } from 'react';
import { X, CheckCircle, AlertCircle, Info, AlertTriangle } from 'lucide-react';
import { useToast } from '../hooks/useToast';
import type { Toast as ToastType } from '../contexts/toast-context';

const iconMap = {
  success: CheckCircle,
  error: AlertCircle,
  info: Info,
  warning: AlertTriangle,
};

const colorMap = {
  success: 'toast-success text-green-700 dark:text-green-400',
  error: 'toast-error text-red-700 dark:text-red-400',
  info: 'toast-info text-blue-700 dark:text-blue-400',
  warning: 'toast-warning text-yellow-700 dark:text-yellow-400',
};

function ToastItem({ toast }: { toast: ToastType }) {
  const { removeToast } = useToast();
  const [isExiting, setIsExiting] = useState(false);
  const Icon = iconMap[toast.type];

  const handleClose = () => {
    setIsExiting(true);
    setTimeout(() => removeToast(toast.id), 150);
  };

  return (
    <div
      className={`toast ${colorMap[toast.type]} ${
        isExiting ? 'animate-[slideOutRight_0.15s_ease-in_forwards]' : ''
      }`}
      role="alert"
      aria-live="polite"
    >
      <div className="flex items-start gap-3">
        <Icon className="w-5 h-5 shrink-0 mt-0.5" />
        <p className="flex-1 text-sm text-gray-900 dark:text-gray-100">
          {toast.message}
        </p>
        <button
          onClick={handleClose}
          className="btn-icon p-1 -m-1 shrink-0"
          aria-label="Dismiss"
        >
          <X className="w-4 h-4" />
        </button>
      </div>
    </div>
  );
}

export function ToastContainer() {
  const { toasts } = useToast();

  if (toasts.length === 0) return null;

  return (
    <div
      className="toast-container"
      aria-live="polite"
      aria-label="Notifications"
    >
      {toasts.map(toast => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </div>
  );
}
