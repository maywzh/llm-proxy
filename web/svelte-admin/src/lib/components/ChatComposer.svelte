<script lang="ts">
  import {
    ImagePlus,
    Loader2,
    Send,
    Settings,
    StopCircle,
    X,
  } from 'lucide-svelte';
  import { tick } from 'svelte';

  type Props = {
    input: string;
    onKeyDown: (e: KeyboardEvent) => void;
    isLoading: boolean;
    isStreaming: boolean;
    onSend: () => void;
    sendDisabled: boolean;
    onStop: () => void;
    imageDataUrl: string | null;
    imageError: string | null;
    onRemoveImage: () => void;
    imageInput?: HTMLInputElement | null;
    onImageChange: (e: Event) => void;
    onPickImage: () => void;
    showImageButton: boolean;
    selectedModel: string;
    onSelectModel: (value: string) => void;
    modelOptions: Array<{ value: string; label: string }>;
    onOpenSettings: () => void;
  };

  let {
    input = $bindable(),
    onKeyDown,
    isLoading,
    isStreaming,
    onSend,
    sendDisabled,
    onStop,
    imageDataUrl,
    imageError,
    onRemoveImage,
    imageInput = $bindable(null),
    onImageChange,
    onPickImage,
    showImageButton,
    selectedModel,
    onSelectModel,
    modelOptions,
    onOpenSettings,
  }: Props = $props();

  let textareaEl: HTMLTextAreaElement | null = $state(null);

  $effect(() => {
    void input;
    tick().then(() => {
      if (!textareaEl) return;
      textareaEl.style.height = 'auto';
      textareaEl.style.height = `${Math.min(textareaEl.scrollHeight, 180)}px`;
    });
  });
</script>

<div class="px-4 py-3">
  <div class="mx-auto w-full max-w-3xl">
    {#if imageError}
      <p class="mb-2 text-sm text-red-600 dark:text-red-400">{imageError}</p>
    {/if}

    <div
      class="rounded-2xl bg-gray-50 dark:bg-gray-700/30 ring-1 ring-gray-200/80 dark:ring-gray-600 shadow-sm"
    >
      {#if imageDataUrl}
        <div class="px-3 pt-3">
          <div class="relative inline-block">
            <img
              src={imageDataUrl}
              alt="preview"
              class="h-16 w-16 object-cover rounded-lg border border-gray-200 dark:border-gray-600"
            />
            <button
              type="button"
              class="absolute -top-1.5 -right-1.5 w-5 h-5 flex items-center justify-center rounded-full bg-gray-800/80 dark:bg-gray-600/90 text-white hover:bg-gray-900 dark:hover:bg-gray-500 transition-colors shadow-sm"
              title="Remove image"
              onclick={onRemoveImage}
            >
              <X class="w-3 h-3" />
            </button>
          </div>
        </div>
      {/if}

      <textarea
        bind:this={textareaEl}
        bind:value={input}
        onkeydown={onKeyDown}
        placeholder="Message..."
        class="w-full bg-transparent text-gray-900 dark:text-white placeholder-gray-400 dark:placeholder-gray-500 focus:outline-none resize-none px-4 py-3 min-h-11"
        rows={1}
        disabled={isLoading}
      ></textarea>

      <div class="flex items-center justify-between gap-2 px-3 pb-3">
        <div class="flex items-center gap-1.5">
          <input
            bind:this={imageInput}
            type="file"
            accept="image/*"
            onchange={onImageChange}
            class="hidden"
          />
          {#if showImageButton}
            <button
              type="button"
              onclick={onPickImage}
              disabled={isLoading}
              class="h-8 w-8 inline-flex items-center justify-center rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-200/60 dark:hover:bg-gray-600/50 focus:outline-none disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              title="Attach image"
              aria-label="Attach image"
            >
              <ImagePlus class="w-4 h-4" />
            </button>
          {/if}
        </div>

        <div class="flex items-center gap-2">
          <select
            value={selectedModel}
            onchange={e => onSelectModel((e.target as HTMLSelectElement).value)}
            disabled={isLoading || modelOptions.length === 0}
            class="h-8 w-36 sm:w-48 bg-transparent text-gray-500 dark:text-gray-400 rounded-lg focus:ring-1 focus:ring-primary-500 focus:outline-none px-2 text-xs border-none"
            title="Model"
          >
            {#if modelOptions.length === 0}
              <option value="">Set credential key</option>
            {/if}
            {#each modelOptions as model (model.value)}
              <option value={model.value}>{model.label}</option>
            {/each}
          </select>

          <button
            type="button"
            onclick={onOpenSettings}
            class="h-8 w-8 inline-flex items-center justify-center rounded-lg text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-200/60 dark:hover:bg-gray-600/50 focus:outline-none transition-colors"
            title="Settings"
            aria-label="Settings"
          >
            <Settings class="w-4 h-4" />
          </button>

          {#if isStreaming}
            <button
              onclick={onStop}
              class="h-8 w-8 inline-flex items-center justify-center rounded-lg bg-red-600 text-white hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-red-500 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              title="Stop"
              aria-label="Stop generation"
            >
              <StopCircle class="w-4 h-4" />
            </button>
          {:else}
            <button
              onclick={onSend}
              disabled={sendDisabled}
              class="h-8 w-8 inline-flex items-center justify-center rounded-lg bg-primary-600 text-white hover:bg-primary-700 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
              title="Send"
              aria-label="Send message"
            >
              {#if isLoading}
                <Loader2 class="w-4 h-4 animate-spin" />
              {:else}
                <Send class="w-4 h-4" />
              {/if}
            </button>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>
