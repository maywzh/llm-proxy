<script lang="ts">
  import { onMount } from 'svelte';
  import { browser } from '$app/environment';
  import { auth } from '$lib/stores';
  import { renderMarkdownToHtml } from '$lib/markdown';
  import {
    Send,
    Loader2,
    Trash2,
    Settings,
    Zap,
    StopCircle,
    ImagePlus,
    X,
  } from 'lucide-svelte';
  import type {
    ChatMessage,
    ChatRequest,
    StreamChunk,
    Model,
    ChatContentPart,
  } from '$lib/types';

  function isContentString(content: ChatMessage['content']): content is string {
    return typeof content === 'string';
  }

  const VISION_MODEL_ALLOWLIST = (
    import.meta.env.VITE_CHAT_VISION_MODEL_ALLOWLIST as string | undefined
  )
    ?.split(',')
    .map(s => s.trim())
    .filter(Boolean);

  function isVisionModel(model: string) {
    if (!VISION_MODEL_ALLOWLIST || VISION_MODEL_ALLOWLIST.length === 0)
      return false;
    const normalized = model.trim().toLowerCase();
    return VISION_MODEL_ALLOWLIST.some(prefix =>
      normalized.startsWith(prefix.toLowerCase())
    );
  }

  function maskCredentialKey(key: string) {
    const trimmed = key.trim();
    if (!trimmed) return '';
    if (trimmed.startsWith('sk-')) {
      const prefixLen = Math.min(5, trimmed.length);
      return `${trimmed.slice(0, prefixLen)}*****`;
    }
    const prefixLen = Math.min(2, trimmed.length);
    return `${trimmed.slice(0, prefixLen)}*****`;
  }

  let messages: ChatMessage[] = $state([]);
  let input = $state('');
  let credentialKey = $state('');
  let isEditingCredentialKey = $state(false);
  let imageDataUrl = $state<string | null>(null);
  let imageError = $state<string | null>(null);
  let isLoading = $state(false);
  let isStreaming = $state(false);
  let isWaitingFirstToken = $state(false);
  let abortController: (() => void) | null = $state(null);
  let selectedModel = $state('');
  let models: Model[] = $state([]);
  let modelsError = $state<string | null>(null);
  let showSettings = $state(false);
  let maxTokens = $state(2000);
  let messagesEnd = $state<HTMLElement | null>(null);
  let credentialKeyInput: HTMLInputElement | null = $state(null);
  let imageInput: HTMLInputElement | null = $state(null);

  onMount(() => {
    if (browser) {
      const stored = localStorage.getItem('chat-credential-key');
      if (stored) credentialKey = stored;
    }
    loadModels();
  });

  $effect(() => {
    if (!auth.apiClient) return;
    if (!credentialKey.trim()) {
      models = [];
      selectedModel = '';
      modelsError = null;
      return;
    }

    const timer = window.setTimeout(() => {
      loadModels();
    }, 400);

    return () => window.clearTimeout(timer);
  });

  $effect(() => {
    if (browser && credentialKey.trim()) {
      localStorage.setItem('chat-credential-key', credentialKey.trim());
    }
  });

  $effect(() => {
    scrollToBottom();
  });

  $effect(() => {
    if (showSettings && isEditingCredentialKey) {
      credentialKeyInput?.focus();
    }
  });

  function scrollToBottom() {
    messagesEnd?.scrollIntoView({ behavior: 'smooth' });
  }

  async function loadModels() {
    try {
      if (!auth.apiClient) return;
      if (!credentialKey.trim()) return;

      const response = await auth.apiClient.listModels(credentialKey.trim());
      models = response.data;
      modelsError = null;

      if (!selectedModel && response.data.length > 0) {
        selectedModel = response.data[0].id;
      }
    } catch (error) {
      console.error('Failed to load models:', error);
      models = [];
      selectedModel = '';
      modelsError =
        error instanceof Error ? error.message : 'Failed to load models';
    }
  }

  function handlePickImage() {
    imageError = null;
    if (!isVisionModel(selectedModel)) {
      imageError = '当前选择的模型不支持图片输入';
      return;
    }
    imageInput?.click();
  }

  function handleImageChange(e: Event) {
    imageError = null;
    const target = e.target as HTMLInputElement;
    const file = target.files?.[0];
    if (!file) return;

    if (!isVisionModel(selectedModel)) {
      imageError = '当前选择的模型不支持图片输入';
      target.value = '';
      return;
    }

    const maxBytes = 5 * 1024 * 1024;
    if (file.size > maxBytes) {
      imageError = '图片过大（最大 5MB）';
      target.value = '';
      return;
    }

    if (!file.type.startsWith('image/')) {
      imageError = '仅支持图片文件';
      target.value = '';
      return;
    }

    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result === 'string') {
        imageDataUrl = result;
      } else {
        imageError = '读取图片失败';
      }
    };
    reader.onerror = () => {
      imageError = '读取图片失败';
    };
    reader.readAsDataURL(file);

    target.value = '';
  }

  async function handleSend() {
    if (
      (!input.trim() && !imageDataUrl) ||
      !selectedModel ||
      !credentialKey.trim() ||
      isLoading
    )
      return;

    if (imageDataUrl && !isVisionModel(selectedModel)) {
      imageError = '当前选择的模型不支持图片输入';
      return;
    }

    const contentText = input.trim();
    let content: ChatMessage['content'];
    if (imageDataUrl) {
      const parts: ChatContentPart[] = [];
      if (contentText) parts.push({ type: 'text', text: contentText });
      parts.push({ type: 'image_url', image_url: { url: imageDataUrl } });
      content = parts;
    } else {
      content = contentText;
    }

    const userMessage: ChatMessage = { role: 'user', content };

    const newMessages = [...messages, userMessage];
    messages = newMessages;
    input = '';
    imageDataUrl = null;
    isLoading = true;
    isStreaming = true;
    isWaitingFirstToken = true;

    try {
      const request: ChatRequest = {
        model: selectedModel,
        messages: newMessages,
        stream: true,
        max_tokens: maxTokens,
      };

      let assistantContent = '';
      let receivedFirstToken = false;
      const assistantMessage: ChatMessage = {
        role: 'assistant',
        content: '',
      };
      messages = [...messages, assistantMessage];

      const stopStreaming = await auth.apiClient!.createChatCompletionStream(
        request,
        credentialKey.trim(),
        (chunk: StreamChunk) => {
          const delta = chunk.choices[0]?.delta;
          if (delta?.content) {
            if (!receivedFirstToken) {
              receivedFirstToken = true;
              isWaitingFirstToken = false;
            }
            assistantContent += delta.content;
            const updated = [...messages];
            updated[updated.length - 1] = {
              role: 'assistant',
              content: assistantContent,
            };
            messages = updated;
          }
        },
        () => {
          isLoading = false;
          isStreaming = false;
          isWaitingFirstToken = false;
          abortController = null;
        },
        (error: Error) => {
          console.error('Stream error:', error);
          const updated = [...messages];
          updated[updated.length - 1] = {
            role: 'assistant',
            content: `Error: ${error.message}`,
          };
          messages = updated;
          isLoading = false;
          isStreaming = false;
          isWaitingFirstToken = false;
          abortController = null;
        }
      );

      abortController = stopStreaming;
    } catch (error) {
      console.error('Failed to send message:', error);
      messages = [
        ...messages,
        {
          role: 'assistant',
          content: `Error: ${error instanceof Error ? error.message : 'Unknown error'}`,
        },
      ];
      isLoading = false;
      isStreaming = false;
      isWaitingFirstToken = false;
    }
  }

  function handleStop() {
    if (abortController) {
      abortController();
      isLoading = false;
      isStreaming = false;
      isWaitingFirstToken = false;
      abortController = null;
    }
  }

  function handleClear() {
    messages = [];
    imageDataUrl = null;
    imageError = null;
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function getAllModels() {
    return models.map(m => ({ value: m.id, label: m.id }));
  }
</script>

<div class="max-w-7xl mx-auto">
  <div
    class="bg-white dark:bg-gray-800 rounded-lg shadow-sm border border-gray-200 dark:border-gray-700 h-[calc(100vh-180px)] flex flex-col"
  >
    <div class="border-b border-gray-200 dark:border-gray-700 p-4">
      <div class="flex items-center justify-between">
        <div class="flex items-center space-x-4 flex-1">
          <select
            bind:value={selectedModel}
            disabled={isLoading || getAllModels().length === 0}
            class="flex-1 max-w-md bg-gray-50 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-2.5"
          >
            {#if getAllModels().length === 0}
              <option value=""
                >Set credential key in Settings to load models</option
              >
            {/if}
            {#each getAllModels() as model}
              <option value={model.value}>{model.label}</option>
            {/each}
          </select>
        </div>

        <div class="flex items-center space-x-2">
          <button
            onclick={() => (showSettings = !showSettings)}
            class="btn btn-secondary flex items-center space-x-2"
            title="Settings (set credential key)"
          >
            <Settings class="w-4 h-4" />
            <span>Settings</span>
          </button>
          <button
            onclick={handleClear}
            class="btn btn-secondary flex items-center space-x-2"
            title="Clear Chat"
          >
            <Trash2 class="w-4 h-4" />
            <span>Clear</span>
          </button>
        </div>
      </div>

      {#if showSettings}
        <div class="mt-4 p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
          <div class="space-y-4">
            <div>
              <label
                for="credential-key"
                class="block mb-2 text-sm font-medium text-gray-900 dark:text-white"
              >
                Credential Key
              </label>
              <div class="flex items-center space-x-2">
                <input
                  bind:this={credentialKeyInput}
                  id="credential-key"
                  value={isEditingCredentialKey
                    ? credentialKey
                    : maskCredentialKey(credentialKey)}
                  oninput={e =>
                    (credentialKey = (e.target as HTMLInputElement).value)}
                  placeholder="sk-... (used for /v1/models and /v1/chat/completions)"
                  disabled={isLoading}
                  readonly={!isEditingCredentialKey}
                  autocomplete="off"
                  spellcheck="false"
                  inputmode="text"
                  class="flex-1 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-2.5"
                />
                <button
                  type="button"
                  class="btn btn-secondary"
                  disabled={isLoading}
                  title={isEditingCredentialKey
                    ? 'Hide credential key'
                    : 'Edit credential key'}
                  onclick={() =>
                    (isEditingCredentialKey = !isEditingCredentialKey)}
                >
                  {isEditingCredentialKey ? 'Hide' : 'Edit'}
                </button>
              </div>
              {#if modelsError}
                <p class="mt-2 text-sm text-red-600 dark:text-red-400">
                  {modelsError}
                </p>
              {/if}
            </div>
            <div>
              <label
                for="max-tokens"
                class="block mb-2 text-sm font-medium text-gray-900 dark:text-white"
              >
                Max Tokens: {maxTokens}
              </label>
              <input
                id="max-tokens"
                type="range"
                bind:value={maxTokens}
                min="100"
                max="8000"
                step="100"
                disabled={isLoading}
                class="w-full"
              />
            </div>
          </div>
        </div>
      {/if}
    </div>

    <div class="flex-1 overflow-y-auto p-4 space-y-4">
      {#if messages.length === 0}
        <div
          class="h-full flex flex-col items-center justify-center text-gray-500 dark:text-gray-400"
        >
          <Zap class="w-16 h-16 mb-4" />
          <p class="text-lg">Start a conversation</p>
          <p class="text-sm">Select a model and type your message below</p>
        </div>
      {:else}
        {#each messages as msg (msg)}
          <div
            class="flex {msg.role === 'user' ? 'justify-end' : 'justify-start'}"
          >
            <div
              class="max-w-[80%] rounded-lg px-4 py-3 {msg.role === 'user'
                ? 'bg-primary-600 text-white'
                : 'bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white'}"
            >
              {#if isContentString(msg.content)}
                {#if msg.content}
                  <div class="markdown break-words">
                    {@html renderMarkdownToHtml(msg.content)}
                  </div>
                {:else if msg.role === 'assistant' && isWaitingFirstToken}
                  <span class="inline-block animate-pulse" aria-label="typing">
                    ▍
                  </span>
                {/if}
              {:else}
                <div class="space-y-2">
                  {#each msg.content as part, partIndex (partIndex)}
                    {#if part.type === 'text'}
                      <div class="markdown break-words">
                        {@html renderMarkdownToHtml(part.text)}
                      </div>
                    {:else}
                      <img
                        src={part.image_url.url}
                        alt="uploaded"
                        class="max-h-64 rounded-lg border border-gray-200 dark:border-gray-600"
                      />
                    {/if}
                  {/each}
                </div>
              {/if}
            </div>
          </div>
        {/each}
        <div bind:this={messagesEnd}></div>
      {/if}
    </div>

    <div class="border-t border-gray-200 dark:border-gray-700 p-4">
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
            onclick={() => (imageDataUrl = null)}
          >
            <X class="w-4 h-4" />
          </button>
        </div>
      {/if}
      {#if imageError}
        <p class="mb-3 text-sm text-red-600 dark:text-red-400">{imageError}</p>
      {/if}
      <div class="flex space-x-2">
        <textarea
          bind:value={input}
          onkeydown={handleKeyDown}
          placeholder="Type your message... (Press Enter to send, Shift+Enter for new line)"
          class="flex-1 bg-gray-50 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-3 resize-none"
          rows={3}
          disabled={isLoading}
        ></textarea>
        <div class="flex flex-col space-y-2">
          <input
            bind:this={imageInput}
            type="file"
            accept="image/*"
            class="hidden"
            onchange={handleImageChange}
          />
          <button
            type="button"
            class="btn btn-secondary flex items-center justify-center"
            disabled={!isVisionModel(selectedModel) || isLoading}
            title={isVisionModel(selectedModel)
              ? 'Attach image'
              : 'Image input is disabled for this model'}
            onclick={handlePickImage}
          >
            <ImagePlus class="w-5 h-5" />
          </button>
          {#if isStreaming}
            <button
              onclick={handleStop}
              class="btn btn-danger flex items-center justify-center"
              title="Stop Generation"
            >
              <StopCircle class="w-5 h-5" />
            </button>
          {:else}
            <button
              onclick={handleSend}
              disabled={(!input.trim() && !imageDataUrl) ||
                !credentialKey.trim() ||
                isLoading}
              class="btn btn-primary flex items-center justify-center"
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
