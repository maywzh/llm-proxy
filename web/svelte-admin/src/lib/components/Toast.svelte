<script lang="ts">
  import { toasts, type Toast } from '$lib/toast';
  import { CheckCircle, XCircle, Info, AlertTriangle, X } from 'lucide-svelte';

  const iconMap = {
    success: CheckCircle,
    error: XCircle,
    info: Info,
    warning: AlertTriangle,
  };

  const colorMap = {
    success: 'text-green-500',
    error: 'text-red-500',
    info: 'text-blue-500',
    warning: 'text-yellow-500',
  };

  const borderMap = {
    success: 'border-l-green-500',
    error: 'border-l-red-500',
    info: 'border-l-blue-500',
    warning: 'border-l-yellow-500',
  };
</script>

{#if $toasts.length > 0}
  <div
    class="fixed bottom-4 right-4 z-50 space-y-2"
    role="region"
    aria-label="Notifications"
    aria-live="polite"
  >
    {#each $toasts as toast (toast.id)}
      {@const Icon = iconMap[toast.type]}
      <div
        class="flex items-start space-x-3 bg-white dark:bg-gray-800 shadow-lg rounded-lg border border-gray-200 dark:border-gray-700 border-l-4 {borderMap[
          toast.type
        ]} p-4 min-w-[300px] max-w-md {toast.isExiting
          ? 'animate-slide-out-right'
          : 'animate-fade-in'}"
        role="alert"
      >
        <Icon class="w-5 h-5 shrink-0 {colorMap[toast.type]}" />
        <p class="flex-1 text-sm text-gray-900 dark:text-gray-100">
          {toast.message}
        </p>
        <button
          type="button"
          class="shrink-0 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
          onclick={() => toasts.remove(toast.id)}
          aria-label="Close notification"
        >
          <X class="w-4 h-4" />
        </button>
      </div>
    {/each}
  </div>
{/if}
