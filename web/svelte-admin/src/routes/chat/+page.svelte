<script lang="ts">
  import { onMount } from 'svelte';
  import { auth } from '$lib/stores';
  import {
    chatSettings,
    initChatSettings,
    updateChatSettings,
  } from '$lib/chat-settings';
  import { renderMarkdownToHtml } from '$lib/markdown';
  import ChatMessageActions from '$lib/components/ChatMessageActions.svelte';
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
    ChatRequestMessage,
    StreamChunk,
    Model,
    ChatContentPart,
  } from '$lib/types';

  function isContentString(content: ChatMessage['content']): content is string {
    return typeof content === 'string';
  }

  const API_BASE_URL =
    import.meta.env.VITE_PUBLIC_API_BASE_URL || 'http://127.0.0.1:18000';

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
  let isEditingCredentialKey = $state(false);
  let imageDataUrl = $state<string | null>(null);
  let imageError = $state<string | null>(null);
  let isLoading = $state(false);
  let isStreaming = $state(false);
  let isWaitingFirstToken = $state(false);
  let abortController: (() => void) | null = $state(null);
  let models: Model[] = $state([]);
  let modelsError = $state<string | null>(null);
  let showSettings = $state(false);
  let copiedIndex = $state<number | null>(null);
  let copyResetTimer: number | null = $state(null);
  let sharedIndex = $state<number | null>(null);
  let shareResetTimer: number | null = $state(null);
  let messagesEnd = $state<HTMLElement | null>(null);
  let credentialKeyInput: HTMLInputElement | null = $state(null);
  let imageInput: HTMLInputElement | null = $state(null);

  onMount(() => {
    initChatSettings();
    loadModels();

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && showSettings) showSettings = false;
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  });

  $effect(() => {
    if (!auth.apiClient) return;
    if (!$chatSettings.credentialKey.trim()) {
      models = [];
      if ($chatSettings.selectedModel) {
        updateChatSettings({ selectedModel: '' });
      }
      modelsError = null;
      return;
    }

    const timer = window.setTimeout(() => {
      loadModels();
    }, 400);

    return () => window.clearTimeout(timer);
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

  function getMessageCopyText(msg: ChatMessage): string {
    if (typeof msg.content === 'string') return msg.content;
    return msg.content
      .filter(
        (p): p is Extract<ChatContentPart, { type: 'text' }> =>
          p.type === 'text'
      )
      .map(p => p.text)
      .join('\n');
  }

  async function copyToClipboard(text: string) {
    const value = text ?? '';
    if (!value) return;
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      return;
    }
    const textarea = document.createElement('textarea');
    textarea.value = value;
    textarea.setAttribute('readonly', 'true');
    textarea.style.position = 'fixed';
    textarea.style.left = '-9999px';
    document.body.appendChild(textarea);
    textarea.select();
    document.execCommand('copy');
    document.body.removeChild(textarea);
  }

  async function handleCopy(msg: ChatMessage, index: number) {
    await copyToClipboard(getMessageCopyText(msg));
    copiedIndex = index;
    if (copyResetTimer) window.clearTimeout(copyResetTimer);
    copyResetTimer = window.setTimeout(() => {
      if (copiedIndex === index) copiedIndex = null;
    }, 1500);
  }

  function getMessagesForGeneration(): ChatMessage[] {
    if (messages.length === 0) return [];
    const last = messages[messages.length - 1];
    if (last?.role === 'assistant') return messages.slice(0, -1);
    return messages;
  }

  function getMessagesForShareAt(index: number): ChatMessage[] {
    if (index <= 0) return [];
    return messages.slice(0, index);
  }

  function withSystemPrompt(
    conversationMessages: ChatRequestMessage[]
  ): ChatRequestMessage[] {
    const prompt = $chatSettings.systemPrompt.trim();
    if (!prompt) return conversationMessages;
    return [{ role: 'system', content: prompt }, ...conversationMessages];
  }

  function toRequestMessages(messages: ChatMessage[]): ChatRequestMessage[] {
    return messages.map(({ role, content }) => ({ role, content }));
  }

  function buildChatCurl(request: ChatRequest, key: string): string {
    const baseUrl = API_BASE_URL.replace(/\/$/, '');
    const url = `${baseUrl}/v1/chat/completions`;
    const payload = JSON.stringify({ ...request, stream: true }, null, 2);
    const toSingleQuotedShellString = (value: string) =>
      `'${value.replace(/'/g, "'\\''")}'`;
    return [
      `curl --location --request POST ${toSingleQuotedShellString(url)} \\`,
      `--header ${toSingleQuotedShellString('Content-Type: application/json')} \\`,
      `--header ${toSingleQuotedShellString(`Authorization: Bearer ${key}`)} \\`,
      `--data-raw ${toSingleQuotedShellString(payload)}`,
    ].join('\n');
  }

  async function handleShareAt(index: number) {
    if (isLoading || isStreaming) return;
    if (!$chatSettings.selectedModel || !$chatSettings.credentialKey.trim())
      return;
    const requestMessages = getMessagesForShareAt(index);
    if (!requestMessages.some(m => m.role === 'user')) return;
    const request: ChatRequest = {
      model: $chatSettings.selectedModel,
      messages: withSystemPrompt(toRequestMessages(requestMessages)),
      stream: true,
      max_tokens: $chatSettings.maxTokens,
    };
    await copyToClipboard(
      buildChatCurl(request, $chatSettings.credentialKey.trim())
    );
    sharedIndex = index;
    if (shareResetTimer) window.clearTimeout(shareResetTimer);
    shareResetTimer = window.setTimeout(() => {
      if (sharedIndex === index) sharedIndex = null;
    }, 1500);
  }

  async function startStreaming(conversationMessages: ChatMessage[]) {
    if (!auth.apiClient) return;
    if (!$chatSettings.selectedModel || !$chatSettings.credentialKey.trim())
      return;

    isLoading = true;
    isStreaming = true;
    isWaitingFirstToken = true;

    const request: ChatRequest = {
      model: $chatSettings.selectedModel,
      messages: withSystemPrompt(toRequestMessages(conversationMessages)),
      stream: true,
      max_tokens: $chatSettings.maxTokens,
    };

    let assistantContent = '';
    let assistantThinking = '';
    let receivedFirstToken = false;
    const assistantMessage: ChatMessage = {
      role: 'assistant',
      content: '',
      thinking: '',
    };
    messages = [...conversationMessages, assistantMessage];

    try {
      const stopStreaming = await auth.apiClient.createChatCompletionStream(
        request,
        $chatSettings.credentialKey.trim(),
        (chunk: StreamChunk) => {
          const delta = chunk.choices[0]?.delta;
          const thinkingDelta =
            typeof delta?.reasoning_content === 'string'
              ? delta.reasoning_content
              : typeof delta?.thinking === 'string'
                ? delta.thinking
                : typeof delta?.reasoning === 'string'
                  ? delta.reasoning
                  : '';
          const contentDelta =
            typeof delta?.content === 'string' ? delta.content : '';

          if (!thinkingDelta && !contentDelta) return;

          if (!receivedFirstToken) {
            receivedFirstToken = true;
            isWaitingFirstToken = false;
          }

          assistantThinking += thinkingDelta;
          assistantContent += contentDelta;
          const updated = [...messages];
          updated[updated.length - 1] = {
            role: 'assistant',
            content: assistantContent,
            ...(assistantThinking ? { thinking: assistantThinking } : {}),
          };
          messages = updated;
        },
        () => {
          isLoading = false;
          isStreaming = false;
          isWaitingFirstToken = false;
          abortController = null;
        },
        (error: Error) => {
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
      messages = [
        ...conversationMessages,
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

  async function handleRegenerate() {
    if (isLoading || isStreaming) return;
    const requestMessages = getMessagesForGeneration();
    if (!requestMessages.some(m => m.role === 'user')) return;
    await startStreaming(requestMessages);
  }

  async function loadModels() {
    try {
      if (!auth.apiClient) return;
      if (!$chatSettings.credentialKey.trim()) return;

      const response = await auth.apiClient.listModels(
        $chatSettings.credentialKey.trim()
      );
      models = response.data;
      modelsError = null;

      const available = new Set(response.data.map(m => m.id));
      const nextModel =
        ($chatSettings.selectedModel &&
          available.has($chatSettings.selectedModel) &&
          $chatSettings.selectedModel) ||
        (response.data[0]?.id ?? '');

      if (nextModel !== $chatSettings.selectedModel)
        updateChatSettings({ selectedModel: nextModel });
    } catch (error) {
      models = [];
      updateChatSettings({ selectedModel: '' });
      modelsError =
        error instanceof Error ? error.message : 'Failed to load models';
    }
  }

  function handlePickImage() {
    imageError = null;
    if (!isVisionModel($chatSettings.selectedModel)) {
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

    if (!isVisionModel($chatSettings.selectedModel)) {
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
      !$chatSettings.selectedModel ||
      !$chatSettings.credentialKey.trim() ||
      isLoading
    )
      return;

    if (imageDataUrl && !isVisionModel($chatSettings.selectedModel)) {
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
    input = '';
    imageDataUrl = null;
    await startStreaming(newMessages);
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
    class="bg-white dark:bg-gray-800 rounded-2xl overflow-hidden h-[calc(100vh-180px)] flex flex-col"
  >
    <div class="flex-1 overflow-y-auto px-4 pt-4 pb-3">
      <div class="mx-auto w-full max-w-3xl h-full">
        {#if messages.length === 0}
          <div
            class="h-full flex flex-col items-center justify-center text-gray-500 dark:text-gray-400"
          >
            <Zap class="w-16 h-16 mb-4" />
            <p class="text-lg">Start a conversation</p>
            <p class="text-sm">Select a model and type your message below</p>
          </div>
        {:else}
          <div class="space-y-4">
            {#each messages as msg, msgIndex (msg)}
              <div
                class="flex {msg.role === 'user'
                  ? 'justify-end'
                  : 'justify-start'}"
              >
                <div class="group max-w-[80%]">
                  <div
                    class="rounded-lg px-4 py-3 {msg.role === 'user'
                      ? 'bg-primary-600 text-white'
                      : 'bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white'}"
                  >
                    {#if msg.role === 'assistant' && msg.thinking?.trim()}
                      <details
                        class="mb-2 rounded-md border border-gray-200 dark:border-gray-600 bg-white/60 dark:bg-black/10"
                        open={isStreaming && msgIndex === messages.length - 1}
                      >
                        <summary
                          class="cursor-pointer select-none px-3 py-2 text-xs text-gray-600 dark:text-gray-300"
                        >
                          Thinking
                        </summary>
                        <div
                          class="px-3 pb-3 markdown break-words text-sm text-gray-700 dark:text-gray-200"
                        >
                          <!-- eslint-disable-next-line svelte/no-at-html-tags --><!-- Sanitized by DOMPurify in renderMarkdownToHtml -->
                          {@html renderMarkdownToHtml(msg.thinking)}
                        </div>
                      </details>
                    {/if}
                    {#if isContentString(msg.content)}
                      {#if msg.content}
                        <div class="markdown break-words">
                          <!-- eslint-disable-next-line svelte/no-at-html-tags --><!-- Sanitized by DOMPurify in renderMarkdownToHtml -->
                          {@html renderMarkdownToHtml(msg.content)}
                        </div>
                      {:else if msg.role === 'assistant' && isWaitingFirstToken}
                        <span
                          class="inline-block animate-pulse"
                          aria-label="typing"
                        >
                          ▍
                        </span>
                      {/if}
                    {:else}
                      <div class="space-y-2">
                        {#each msg.content as part, partIndex (partIndex)}
                          {#if part.type === 'text'}
                            <div class="markdown break-words">
                              <!-- eslint-disable-next-line svelte/no-at-html-tags --><!-- Sanitized by DOMPurify in renderMarkdownToHtml -->
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
                  {#if msg.role === 'assistant' && (!isStreaming || msgIndex !== messages.length - 1)}
                    <ChatMessageActions
                      copied={copiedIndex === msgIndex}
                      onCopy={() => handleCopy(msg, msgIndex)}
                      shared={sharedIndex === msgIndex}
                      onShare={() => handleShareAt(msgIndex)}
                      shareDisabled={isLoading ||
                        isStreaming ||
                        !$chatSettings.credentialKey.trim() ||
                        !$chatSettings.selectedModel ||
                        !getMessagesForShareAt(msgIndex).some(
                          m => m.role === 'user'
                        )}
                      showRegenerate={msgIndex === messages.length - 1}
                      regenerateDisabled={isLoading ||
                        isStreaming ||
                        !getMessagesForGeneration().some(
                          m => m.role === 'user'
                        )}
                      onRegenerate={handleRegenerate}
                      disabled={isWaitingFirstToken &&
                        isContentString(msg.content) &&
                        !msg.content &&
                        msgIndex === messages.length - 1}
                    />
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        {/if}
        <div bind:this={messagesEnd}></div>
      </div>
    </div>

    {#if showSettings}
      <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
        <button
          type="button"
          class="absolute inset-0 bg-black/40"
          aria-label="Close settings"
          onclick={() => (showSettings = false)}
        ></button>
        <div
          role="dialog"
          aria-modal="true"
          aria-label="Settings"
          class="relative w-full max-w-lg rounded-2xl bg-white dark:bg-gray-800 shadow-xl ring-1 ring-gray-200 dark:ring-gray-700"
        >
          <div
            class="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-gray-700"
          >
            <div class="text-sm font-semibold text-gray-900 dark:text-white">
              Settings
            </div>
            <button
              type="button"
              class="btn-icon"
              title="Close"
              aria-label="Close"
              onclick={() => (showSettings = false)}
            >
              <X class="w-4 h-4" />
            </button>
          </div>

          <div class="p-4 space-y-4">
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
                    ? $chatSettings.credentialKey
                    : maskCredentialKey($chatSettings.credentialKey)}
                  oninput={e =>
                    updateChatSettings({
                      credentialKey: (e.target as HTMLInputElement).value,
                    })}
                  placeholder="sk-... (used for /v1/models and /v1/chat/completions)"
                  disabled={isLoading}
                  readonly={!isEditingCredentialKey}
                  autocomplete="off"
                  spellcheck="false"
                  inputmode="text"
                  class="flex-1 bg-white dark:bg-gray-900 border border-gray-300 dark:border-gray-700 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-2.5"
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
                Max Tokens: {$chatSettings.maxTokens}
              </label>
              <input
                id="max-tokens"
                type="range"
                value={$chatSettings.maxTokens}
                oninput={e =>
                  updateChatSettings({
                    maxTokens: Number.parseInt(
                      (e.target as HTMLInputElement).value,
                      10
                    ),
                  })}
                min="100"
                max="8000"
                step="100"
                disabled={isLoading}
                class="w-full"
              />
            </div>

            <div>
              <label
                for="system-prompt"
                class="block mb-2 text-sm font-medium text-gray-900 dark:text-white"
              >
                System Prompt
              </label>
              <textarea
                id="system-prompt"
                value={$chatSettings.systemPrompt}
                oninput={e =>
                  updateChatSettings({
                    systemPrompt: (e.target as HTMLTextAreaElement).value,
                  })}
                placeholder="Optional. Prepended as the first system message."
                disabled={isLoading}
                rows={3}
                class="w-full bg-white dark:bg-gray-900 border border-gray-300 dark:border-gray-700 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-2.5 resize-none"
              ></textarea>
            </div>
          </div>

          <div
            class="flex items-center justify-between gap-2 px-4 py-3 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900/30"
          >
            <button
              type="button"
              class="btn btn-secondary flex items-center gap-2"
              title="Clear chat"
              onclick={handleClear}
              disabled={isLoading || isStreaming}
            >
              <Trash2 class="w-4 h-4" />
              <span>Clear</span>
            </button>
            <div class="flex items-center gap-2">
              <button
                type="button"
                class="btn btn-primary"
                onclick={() => (showSettings = false)}
              >
                Done
              </button>
            </div>
          </div>
        </div>
      </div>
    {/if}

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
              onclick={() => (imageDataUrl = null)}
            >
              <X class="w-4 h-4" />
            </button>
          </div>
        {/if}
        {#if imageError}
          <p class="mb-3 text-sm text-red-600 dark:text-red-400">
            {imageError}
          </p>
        {/if}
        <div
          class="rounded-2xl bg-gray-50 dark:bg-gray-700/30 ring-1 ring-gray-200/80 dark:ring-gray-600 shadow-sm p-3"
        >
          <textarea
            bind:value={input}
            onkeydown={handleKeyDown}
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
                onchange={handleImageChange}
                class="hidden"
              />
              <button
                type="button"
                onclick={handlePickImage}
                disabled={!isVisionModel($chatSettings.selectedModel) ||
                  isLoading}
                class="h-10 w-10 inline-flex items-center justify-center rounded-full bg-white dark:bg-gray-800 ring-1 ring-gray-200 dark:ring-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700 focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-50 disabled:cursor-not-allowed"
                title={isVisionModel($chatSettings.selectedModel)
                  ? 'Attach image'
                  : 'Image input is disabled for this model'}
              >
                <ImagePlus class="w-5 h-5" />
              </button>
            </div>

            <div class="flex items-center gap-2">
              <select
                value={$chatSettings.selectedModel}
                onchange={e =>
                  updateChatSettings({
                    selectedModel: (e.target as HTMLSelectElement).value,
                  })}
                disabled={isLoading || getAllModels().length === 0}
                class="h-10 w-[10rem] sm:w-[14rem] bg-white dark:bg-gray-800 ring-1 ring-gray-200 dark:ring-gray-600 text-gray-900 dark:text-white rounded-full focus:ring-2 focus:ring-primary-500 focus:outline-none px-3 text-sm"
                title="Model"
              >
                {#if getAllModels().length === 0}
                  <option value="">Set credential key in Settings</option>
                {/if}
                {#each getAllModels() as model (model.value)}
                  <option value={model.value}>{model.label}</option>
                {/each}
              </select>

              <button
                type="button"
                onclick={() => (showSettings = true)}
                class="h-10 w-10 inline-flex items-center justify-center rounded-full bg-white dark:bg-gray-800 ring-1 ring-gray-200 dark:ring-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700 focus:outline-none focus:ring-2 focus:ring-primary-500"
                title="Settings (set credential key)"
                aria-label="Settings"
              >
                <Settings class="w-5 h-5" />
              </button>

              {#if isStreaming}
                <button
                  onclick={handleStop}
                  class="h-10 w-10 inline-flex items-center justify-center rounded-full bg-red-600 text-white hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-red-500 disabled:opacity-50 disabled:cursor-not-allowed"
                  title="Stop Generation"
                >
                  <StopCircle class="w-5 h-5" />
                </button>
              {:else}
                <button
                  onclick={handleSend}
                  disabled={(!input.trim() && !imageDataUrl) ||
                    !$chatSettings.credentialKey.trim() ||
                    !$chatSettings.selectedModel ||
                    isLoading}
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
  </div>
</div>
