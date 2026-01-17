<script lang="ts">
  import {
    ImagePlus,
    Loader2,
    Send,
    Settings,
    StopCircle,
    X,
  } from 'lucide-svelte';

  export let input: string;
  export let onKeyDown: (e: KeyboardEvent) => void;

  export let isLoading: boolean;
  export let isStreaming: boolean;

  export let onSend: () => void;
  export let sendDisabled: boolean;

  export let onStop: () => void;

  export let imageDataUrl: string | null;
  export let imageError: string | null;
  export let onRemoveImage: () => void;

  export let imageInput: HTMLInputElement | null = null;
  export let onImageChange: (e: Event) => void;
  export let onPickImage: () => void;
  export let attachImageDisabled: boolean;
  export let attachImageTitle: string;

  export let selectedModel: string;
  export let onSelectModel: (value: string) => void;
  export let modelOptions: Array<{ value: string; label: string }>;

  export let onOpenSettings: () => void;
</script>

<div class="px-4 py-3">
  <div class="mx-auto w-full max-w-3xl">
    {#if imageDataUrl}
      <div class="mb-3 flex items-center space-x-3">
        <img
          src={imageDataUrl}
          alt="preview"
          class="h-16 w-16 object-cover rounded-lg border border-gray-200 dark:border-gray-600"
        />
        <button
          type="button"
          class="btn btn-secondary"
          title="Remove image"
          onclick={onRemoveImage}
        >
          <X class="w-4 h-4" />
        </button>
      </div>
    {/if}

    {#if imageError}
      <p class="mb-3 text-sm text-red-600 dark:text-red-400">{imageError}</p>
    {/if}

    <div
      class="rounded-2xl bg-gray-50 dark:bg-gray-700/30 ring-1 ring-gray-200/80 dark:ring-gray-600 shadow-sm p-3"
    >
      <textarea
        bind:value={input}
        onkeydown={onKeyDown}
        placeholder="Type your message... (Press Enter to send, Shift+Enter for new line)"
        class="w-full bg-transparent text-gray-900 dark:text-white placeholder-gray-400 dark:placeholder-gray-400 focus:outline-none resize-none"
        rows={3}
        disabled={isLoading}
      ></textarea>

      <div class="mt-2 flex items-center justify-between gap-2">
        <div class="flex items-center space-x-2">
          <input
            bind:this={imageInput}
            type="file"
            accept="image/*"
            onchange={onImageChange}
            class="hidden"
          />
          <button
            type="button"
            onclick={onPickImage}
            disabled={attachImageDisabled}
            class="h-10 w-10 inline-flex items-center justify-center rounded-full bg-white dark:bg-gray-800 ring-1 ring-gray-200 dark:ring-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-50 disabled:cursor-not-allowed"
            title={attachImageTitle}
          >
            <ImagePlus class="w-5 h-5" />
          </button>
        </div>

        <div class="flex items-center gap-2">
          <select
            value={selectedModel}
            onchange={e => onSelectModel((e.target as HTMLSelectElement).value)}
            disabled={isLoading || modelOptions.length === 0}
            class="h-10 w-[10rem] sm:w-[14rem] bg-white dark:bg-gray-800 ring-1 ring-gray-200 dark:ring-gray-600 text-gray-900 dark:text-white rounded-full focus:ring-2 focus:ring-primary-500 focus:outline-none px-3 text-sm"
            title="Model"
          >
            {#if modelOptions.length === 0}
              <option value="">Set credential key in Settings</option>
            {/if}
            {#each modelOptions as model (model.value)}
              <option value={model.value}>{model.label}</option>
            {/each}
          </select>

          <button
            type="button"
            onclick={onOpenSettings}
            class="h-10 w-10 inline-flex items-center justify-center rounded-full bg-white dark:bg-gray-800 ring-1 ring-gray-200 dark:ring-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700 focus:outline-none focus:ring-2 focus:ring-primary-500"
            title="Settings (set credential key)"
            aria-label="Settings"
          >
            <Settings class="w-5 h-5" />
          </button>

          {#if isStreaming}
            <button
              onclick={onStop}
              class="h-10 w-10 inline-flex items-center justify-center rounded-full bg-red-600 text-white hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-red-500 disabled:opacity-50 disabled:cursor-not-allowed"
              title="Stop Generation"
            >
              <StopCircle class="w-5 h-5" />
            </button>
          {:else}
            <button
              onclick={onSend}
              disabled={sendDisabled}
              class="h-10 w-10 inline-flex items-center justify-center rounded-full bg-primary-600 text-white hover:bg-primary-700 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-50 disabled:cursor-not-allowed"
              title="Send Message"
            >
              {#if isLoading}
                <Loader2 class="w-5 h-5 animate-spin" />
              {:else}
                <Send class="w-5 h-5" />
              {/if}
            </button>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>
