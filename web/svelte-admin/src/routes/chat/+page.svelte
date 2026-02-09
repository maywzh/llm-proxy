<script lang="ts">
  import { onMount } from 'svelte';
  import { auth } from '$lib/stores';
  import {
    chatSettings,
    initChatSettings,
    updateChatSettings,
  } from '$lib/chat-settings';
  import { renderMarkdownToHtml } from '$lib/markdown';
  import { renderMermaidBlocks } from '$lib/mermaid';
  import ChatComposer from '$lib/components/ChatComposer.svelte';
  import ChatMessageActions from '$lib/components/ChatMessageActions.svelte';
  import TypingIndicator from '$lib/components/TypingIndicator.svelte';
  import ImagePreviewModal from '$lib/components/ImagePreviewModal.svelte';
  import {
    Trash2,
    X,
    SquarePen,
    Sparkles,
    Code,
    Bug,
    FileText,
  } from 'lucide-svelte';
  import type {
    ChatMessage,
    ChatRequest,
    ChatRequestMessage,
    StreamChunk,
    Model,
    ChatContentPart,
    ImageAttachment,
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

  const IMAGE_FORMAT_WHITELIST = [
    'image/png',
    'image/jpeg',
    'image/jpg',
    'image/webp',
    'image/gif',
  ];

  const MAX_IMAGES = 5;
  const MAX_IMAGE_SIZE = 5 * 1024 * 1024;

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

  function validateImage(file: File, currentModel: string): string | null {
    if (!isVisionModel(currentModel)) {
      return '当前选择的模型不支持图片输入';
    }
    if (file.size > MAX_IMAGE_SIZE) {
      return '图片过大（最大 5MB）';
    }
    if (!IMAGE_FORMAT_WHITELIST.includes(file.type)) {
      return `不支持的图片格式（支持: ${IMAGE_FORMAT_WHITELIST.join(', ')}）`;
    }
    return null;
  }

  function isDuplicateImage(dataUrl: string): boolean {
    return images.some(img => img.dataUrl === dataUrl);
  }

  function addImages(newImages: ImageAttachment[]): void {
    const availableSlots = MAX_IMAGES - images.length;
    if (availableSlots <= 0) {
      imageError = `最多只能添加 ${MAX_IMAGES} 张图片`;
      return;
    }

    const toAdd = newImages.slice(0, availableSlots);
    const truncated = newImages.length > availableSlots;

    const uniqueImages = toAdd.filter(img => !isDuplicateImage(img.dataUrl));
    if (uniqueImages.length < toAdd.length) {
      const duplicateCount = toAdd.length - uniqueImages.length;
      imageError = `已跳过 ${duplicateCount} 张重复图片`;
      setTimeout(() => {
        if (imageError?.includes('重复图片')) imageError = null;
      }, 3000);
    }

    if (uniqueImages.length > 0) {
      images = [...images, ...uniqueImages];
    }

    if (truncated) {
      imageError = `已达到最大图片数量限制（${MAX_IMAGES}张），仅添加前 ${availableSlots} 张`;
    }
  }

  let messages: ChatMessage[] = $state([]);
  let input = $state('');
  let isEditingCredentialKey = $state(false);
  let images = $state<ImageAttachment[]>([]);
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
  let messagesArea = $state<HTMLElement | null>(null);
  let credentialKeyInput: HTMLInputElement | null = $state(null);
  let imageInput: HTMLInputElement | null = $state(null);
  let showPreviewModal = $state(false);
  let previewImageIndex = $state(0);

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
    void messages;
    void isStreaming;
    if (!isStreaming) {
      requestAnimationFrame(() => renderMermaidBlocks(messagesArea));
    }
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
          const delta = chunk.choices?.[0]?.delta;
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

    const validationError = validateImage(file, $chatSettings.selectedModel);
    if (validationError) {
      imageError = validationError;
      target.value = '';
      return;
    }

    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result === 'string') {
        const attachment: ImageAttachment = {
          id: `upload-${Date.now()}`,
          dataUrl: result,
          name: file.name,
          type: file.type,
          size: file.size,
          source: 'upload',
        };
        addImages([attachment]);
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

  async function handlePaste(e: ClipboardEvent) {
    imageError = null;
    const items = e.clipboardData?.items;
    if (!items) return;

    const imageFiles: File[] = [];
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      if (item.kind === 'file' && item.type.startsWith('image/')) {
        const file = item.getAsFile();
        if (file) imageFiles.push(file);
      }
    }

    if (imageFiles.length === 0) return;
    e.preventDefault();

    const attachments: ImageAttachment[] = [];
    const errors: string[] = [];

    for (const file of imageFiles) {
      const validationError = validateImage(file, $chatSettings.selectedModel);
      if (validationError) {
        errors.push(validationError);
        continue;
      }

      try {
        const dataUrl = await new Promise<string>((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => {
            if (typeof reader.result === 'string') resolve(reader.result);
            else reject(new Error('Invalid result type'));
          };
          reader.onerror = () => reject(new Error('Failed to read file'));
          reader.readAsDataURL(file);
        });

        const ext =
          file.name.split('.').pop() || file.type.split('/')[1] || 'png';
        const timestamp = Date.now();
        const attachment: ImageAttachment = {
          id: `paste-${timestamp}-${attachments.length}`,
          dataUrl,
          name: `pasted-${timestamp}-${attachments.length}.${ext}`,
          type: file.type,
          size: file.size,
          source: 'paste',
        };
        attachments.push(attachment);
      } catch {
        errors.push('读取图片失败');
      }
    }

    if (attachments.length > 0) {
      addImages(attachments);
    }

    if (errors.length > 0 && !imageError) {
      imageError = errors[0];
    }
  }

  async function handleSend() {
    if (
      (!input.trim() && images.length === 0) ||
      !$chatSettings.selectedModel ||
      !$chatSettings.credentialKey.trim() ||
      isLoading
    )
      return;

    if (images.length > 0 && !isVisionModel($chatSettings.selectedModel)) {
      imageError = '当前选择的模型不支持图片输入';
      return;
    }

    const contentText = input.trim();
    let content: ChatMessage['content'];
    if (images.length > 0) {
      const parts: ChatContentPart[] = [];
      if (contentText) parts.push({ type: 'text', text: contentText });
      for (const img of images) {
        parts.push({ type: 'image_url', image_url: { url: img.dataUrl } });
      }
      content = parts;
    } else {
      content = contentText;
    }

    const userMessage: ChatMessage = { role: 'user', content };
    const newMessages = [...messages, userMessage];
    input = '';
    images = [];
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
    images = [];
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

  const suggestions = [
    {
      icon: Sparkles,
      label: 'Explain how this API works',
      prompt: 'Explain how this API works',
    },
    {
      icon: Code,
      label: 'Write a Python script for...',
      prompt: 'Write a Python script for ',
    },
    {
      icon: Bug,
      label: 'Debug this error message',
      prompt: 'Debug this error message:\n',
    },
    {
      icon: FileText,
      label: 'Summarize the key points',
      prompt: 'Summarize the key points of ',
    },
  ];

  function handleSuggestionClick(prompt: string) {
    input = prompt;
  }

  function handleThumbnailClick(index: number) {
    previewImageIndex = index;
    showPreviewModal = true;
  }

  function handleClosePreview() {
    showPreviewModal = false;
  }

  function handleNavigatePreview(index: number) {
    previewImageIndex = index;
  }
</script>

<div class="mx-auto">
  <div
    class="relative bg-white dark:bg-gray-800 rounded-2xl overflow-hidden h-[calc(100vh-120px)] flex flex-col shadow-sm"
  >
    {#if messages.length > 0}
      <button
        type="button"
        onclick={handleClear}
        disabled={isLoading || isStreaming}
        class="absolute top-3 left-3 z-10 p-2 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        title="New chat"
        aria-label="New chat"
      >
        <SquarePen class="w-5 h-5" />
      </button>
    {/if}
    <div class="flex-1 overflow-y-auto" bind:this={messagesArea}>
      <div class="mx-auto w-full max-w-3xl h-full px-4 py-4">
        {#if messages.length === 0}
          <div class="h-full flex flex-col items-center justify-center">
            <h1
              class="text-2xl font-semibold text-gray-900 dark:text-white mb-8"
            >
              What can I help with?
            </h1>
            <div class="grid grid-cols-2 gap-3 w-full max-w-md">
              {#each suggestions as s (s.label)}
                <button
                  type="button"
                  onclick={() => handleSuggestionClick(s.prompt)}
                  disabled={!$chatSettings.credentialKey.trim() ||
                    !$chatSettings.selectedModel}
                  class="flex items-start gap-3 p-4 rounded-xl border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700/40 hover:bg-gray-50 dark:hover:bg-gray-700 text-left transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
                >
                  <s.icon
                    class="w-5 h-5 text-gray-400 dark:text-gray-500 shrink-0 mt-0.5"
                  />
                  <span
                    class="text-sm text-gray-700 dark:text-gray-300 leading-snug"
                  >
                    {s.label}
                  </span>
                </button>
              {/each}
            </div>
            {#if !$chatSettings.credentialKey.trim()}
              <p class="mt-6 text-xs text-gray-400 dark:text-gray-500">
                Set a credential key in Settings to get started
              </p>
            {/if}
          </div>
        {:else}
          <div class="divide-y divide-gray-100 dark:divide-gray-700/50">
            {#each messages as msg, msgIndex (msg)}
              <div class="py-6 first:pt-2">
                <div class="group relative">
                  <div
                    class="mb-1 text-xs font-medium text-gray-500 dark:text-gray-400 select-none"
                  >
                    {msg.role === 'user' ? 'You' : 'Assistant'}
                  </div>
                  <div class="text-gray-900 dark:text-white">
                    {#if msg.role === 'assistant' && msg.thinking?.trim()}
                      <div
                        class={isStreaming && msgIndex === messages.length - 1
                          ? 'thinking-border'
                          : ''}
                      >
                        <details
                          class="mb-3 rounded-lg border border-gray-200 dark:border-gray-700 bg-gray-50/80 dark:bg-gray-800/50"
                          open={isStreaming && msgIndex === messages.length - 1}
                        >
                          <summary
                            class="cursor-pointer select-none px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400"
                          >
                            Thinking
                          </summary>
                          <div
                            class="px-3 pb-3 markdown break-words text-sm text-gray-600 dark:text-gray-300"
                          >
                            <!-- eslint-disable-next-line svelte/no-at-html-tags --><!-- Sanitized by DOMPurify in renderMarkdownToHtml -->
                            {@html renderMarkdownToHtml(msg.thinking)}
                          </div>
                        </details>
                      </div>
                    {/if}
                    {#if isContentString(msg.content)}
                      {#if msg.content}
                        <div class="markdown break-words">
                          <!-- eslint-disable-next-line svelte/no-at-html-tags --><!-- Sanitized by DOMPurify in renderMarkdownToHtml -->
                          {@html renderMarkdownToHtml(msg.content)}
                        </div>
                      {:else if msg.role === 'assistant' && isWaitingFirstToken}
                        <TypingIndicator />
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

    {#if showPreviewModal && images.length > 0}
      <ImagePreviewModal
        {images}
        currentIndex={previewImageIndex}
        onClose={handleClosePreview}
        onNavigate={handleNavigatePreview}
      />
    {/if}

    <ChatComposer
      bind:input
      bind:imageInput
      onKeyDown={handleKeyDown}
      onPaste={handlePaste}
      {isLoading}
      {isStreaming}
      onSend={handleSend}
      sendDisabled={(!input.trim() && images.length === 0) ||
        !$chatSettings.credentialKey.trim() ||
        !$chatSettings.selectedModel ||
        isLoading}
      onStop={handleStop}
      {images}
      {imageError}
      onRemoveImage={id => (images = images.filter(img => img.id !== id))}
      onImageChange={handleImageChange}
      onPickImage={handlePickImage}
      onThumbnailClick={handleThumbnailClick}
      showImageButton={isVisionModel($chatSettings.selectedModel)}
      selectedModel={$chatSettings.selectedModel}
      onSelectModel={value => updateChatSettings({ selectedModel: value })}
      modelOptions={getAllModels()}
      onOpenSettings={() => (showSettings = true)}
    />
  </div>
</div>
