<script lang="ts">
  import { X, ChevronLeft, ChevronRight } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import type { ImageAttachment } from '$lib/types';

  type Props = {
    images: ImageAttachment[];
    currentIndex: number;
    onClose: () => void;
    onNavigate: (index: number) => void;
  };

  let { images, currentIndex, onClose, onNavigate }: Props = $props();

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    } else if (e.key === 'ArrowLeft' && currentIndex > 0) {
      onNavigate(currentIndex - 1);
    } else if (e.key === 'ArrowRight' && currentIndex < images.length - 1) {
      onNavigate(currentIndex + 1);
    }
  }

  onMount(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  });
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center"
  onclick={onClose}
  onkeydown={e => e.key === 'Enter' && onClose()}
  role="dialog"
  aria-modal="true"
  aria-label="Image preview"
  tabindex="-1"
>
  <!-- Backdrop -->
  <button
    type="button"
    class="absolute inset-0 bg-black/80"
    aria-label="Close preview"
    onclick={onClose}
  />

  <!-- Modal content -->
  <div
    class="relative z-10 max-w-[90vw] max-h-[90vh] flex flex-col items-center"
    onclick={e => e.stopPropagation()}
  >
    <!-- Close button -->
    <button
      type="button"
      class="absolute -top-12 right-0 p-2 text-white hover:text-gray-300 transition-colors"
      onclick={onClose}
      aria-label="Close preview"
    >
      <X class="w-6 h-6" />
    </button>

    <!-- Image display -->
    <img
      src={images[currentIndex].dataUrl}
      alt={images[currentIndex].name}
      class="max-w-full max-h-[90vh] object-contain rounded-lg"
    />

    <!-- Image counter -->
    {#if images.length > 1}
      <div class="mt-4 px-3 py-1 rounded-full bg-black/60 text-white text-sm">
        {currentIndex + 1} / {images.length}
      </div>
    {/if}

    <!-- Navigation buttons -->
    {#if images.length > 1}
      {#if currentIndex > 0}
        <button
          type="button"
          class="absolute left-4 top-1/2 -translate-y-1/2 p-3 rounded-full bg-gray-800/80 text-white hover:bg-gray-700 transition-colors"
          onclick={() => onNavigate(currentIndex - 1)}
          aria-label="Previous image"
        >
          <ChevronLeft class="w-6 h-6" />
        </button>
      {/if}

      {#if currentIndex < images.length - 1}
        <button
          type="button"
          class="absolute right-4 top-1/2 -translate-y-1/2 p-3 rounded-full bg-gray-800/80 text-white hover:bg-gray-700 transition-colors"
          onclick={() => onNavigate(currentIndex + 1)}
          aria-label="Next image"
        >
          <ChevronRight class="w-6 h-6" />
        </button>
      {/if}
    {/if}
  </div>
</div>
