<script lang="ts">
  import { Check, Copy } from 'lucide-svelte';

  export let copied: boolean;
  export let disabled: boolean = false;
  export let onCopy: () => void;
</script>

<div
  class="mt-1 px-4 flex items-center gap-2 text-gray-500 dark:text-gray-300 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 transition-opacity"
>
  <div class="relative group/copy">
    <button
      type="button"
      class="p-1.5 rounded-md hover:bg-black/5 dark:hover:bg-white/10 cursor-pointer disabled:cursor-not-allowed disabled:opacity-60"
      onclick={onCopy}
      {disabled}
      aria-label="Copy message"
    >
      {#if copied}
        <Check class="w-4 h-4" />
      {:else}
        <Copy class="w-4 h-4" />
      {/if}
    </button>
    <div
      class="pointer-events-none absolute -top-9 left-1/2 -translate-x-1/2 whitespace-nowrap rounded-md bg-black px-2 py-1 text-xs text-white opacity-0 group-hover/copy:opacity-100 transition-opacity shadow"
    >
      {copied ? 'Copied' : 'Copy'}
    </div>
  </div>
</div>

