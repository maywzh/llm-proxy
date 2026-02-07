import React, { useState, useEffect, useRef, useCallback } from 'react';
import { useAuth } from '../hooks/useAuth';
import { API_BASE_URL } from '../contexts/auth-context';
import {
  Trash2,
  X,
  SquarePen,
  Sparkles,
  Code,
  Bug,
  FileText,
} from 'lucide-react';
import type {
  ChatMessage,
  ChatRequest,
  ChatRequestMessage,
  StreamChunk,
  Model,
  ChatContentPart,
} from '../types';
import { renderMarkdownToHtml } from '../utils/markdown';
import { ChatMessageActions } from '../components/ChatMessageActions';
import { ChatComposer } from '../components/ChatComposer';
import { useChatSettings } from '../stores/chat-settings';

const TypingIndicator = () => (
  <div className="typing-indicator text-gray-400">
    <span />
    <span />
    <span />
  </div>
);

const VISION_MODEL_ALLOWLIST = (
  import.meta.env.VITE_CHAT_VISION_MODEL_ALLOWLIST as string | undefined
)
  ?.split(',')
  .map(s => s.trim())
  .filter(Boolean);

const isVisionModel = (model: string) => {
  if (!VISION_MODEL_ALLOWLIST || VISION_MODEL_ALLOWLIST.length === 0)
    return false;
  const normalized = model.trim().toLowerCase();
  return VISION_MODEL_ALLOWLIST.some(prefix =>
    normalized.startsWith(prefix.toLowerCase())
  );
};

const maskCredentialKey = (key: string) => {
  const trimmed = key.trim();
  if (!trimmed) return '';
  if (trimmed.startsWith('sk-')) {
    const prefixLen = Math.min(5, trimmed.length);
    return `${trimmed.slice(0, prefixLen)}*****`;
  }
  const prefixLen = Math.min(2, trimmed.length);
  return `${trimmed.slice(0, prefixLen)}*****`;
};

const getMessageCopyText = (msg: ChatMessage): string => {
  if (typeof msg.content === 'string') return msg.content;
  return msg.content
    .filter(
      (p): p is Extract<ChatContentPart, { type: 'text' }> => p.type === 'text'
    )
    .map(p => p.text)
    .join('\n');
};

const copyToClipboard = async (text: string) => {
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
};

const getMessagesForGeneration = (current: ChatMessage[]): ChatMessage[] => {
  if (current.length === 0) return [];
  const last = current[current.length - 1];
  if (last?.role === 'assistant') return current.slice(0, -1);
  return current;
};

const getMessagesForShareAt = (
  current: ChatMessage[],
  index: number
): ChatMessage[] => {
  if (index <= 0) return [];
  return current.slice(0, index);
};

const withSystemPrompt = (
  conversationMessages: ChatRequestMessage[],
  systemPrompt: string
): ChatRequestMessage[] => {
  const prompt = systemPrompt.trim();
  if (!prompt) return conversationMessages;
  return [{ role: 'system', content: prompt }, ...conversationMessages];
};

const toRequestMessages = (messages: ChatMessage[]): ChatRequestMessage[] =>
  messages.map(({ role, content }) => ({ role, content }));

const buildChatCurl = (request: ChatRequest, key: string): string => {
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
};

const Chat: React.FC = () => {
  const { apiClient } = useAuth();
  const {
    credentialKey,
    selectedModel,
    maxTokens,
    systemPrompt,
    setCredentialKey,
    setSelectedModel,
    setMaxTokens,
    setSystemPrompt,
  } = useChatSettings();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [isEditingCredentialKey, setIsEditingCredentialKey] = useState(false);
  const [imageDataUrl, setImageDataUrl] = useState<string | null>(null);
  const [imageError, setImageError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [isWaitingFirstToken, setIsWaitingFirstToken] = useState(false);
  const [abortController, setAbortController] = useState<(() => void) | null>(
    null
  );
  const [models, setModels] = useState<Model[]>([]);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);
  const [sharedIndex, setSharedIndex] = useState<number | null>(null);
  const shareResetTimerRef = useRef<number | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const credentialKeyInputRef = useRef<HTMLInputElement>(null);
  const imageInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    return () => {
      if (shareResetTimerRef.current) {
        window.clearTimeout(shareResetTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!showSettings) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setShowSettings(false);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [showSettings]);

  const loadModels = useCallback(async () => {
    if (!apiClient) return;
    try {
      if (!credentialKey.trim()) return;
      const response = await apiClient.listModels(credentialKey.trim());
      setModels(response.data);
      setModelsError(null);
      const available = new Set(response.data.map(m => m.id));
      const nextModel =
        (selectedModel && available.has(selectedModel) && selectedModel) ||
        (response.data[0]?.id ?? '');

      if (nextModel !== selectedModel) setSelectedModel(nextModel);
    } catch (error) {
      setModels([]);
      setSelectedModel('');
      setModelsError(
        error instanceof Error ? error.message : 'Failed to load models'
      );
    }
  }, [apiClient, credentialKey, selectedModel, setSelectedModel]);

  useEffect(() => {
    if (!apiClient) return;
    if (!credentialKey.trim()) {
      setModels([]);
      setSelectedModel('');
      setModelsError(null);
      return;
    }

    const timer = window.setTimeout(() => {
      void loadModels();
    }, 400);
    return () => window.clearTimeout(timer);
  }, [apiClient, credentialKey, loadModels, setSelectedModel]);

  useEffect(() => {
    scrollToBottom();
  }, [messages]);

  useEffect(() => {
    if (showSettings && isEditingCredentialKey) {
      credentialKeyInputRef.current?.focus();
    }
  }, [showSettings, isEditingCredentialKey]);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  // loadModels moved to useCallback above

  const startStreaming = async (conversationMessages: ChatMessage[]) => {
    if (!apiClient) return;
    if (!credentialKey.trim() || !selectedModel) return;

    setIsLoading(true);
    setIsStreaming(true);
    setIsWaitingFirstToken(true);

    const request: ChatRequest = {
      model: selectedModel,
      messages: withSystemPrompt(
        toRequestMessages(conversationMessages),
        systemPrompt
      ),
      stream: true,
      max_tokens: maxTokens,
    };

    let assistantContent = '';
    let assistantThinking = '';
    let receivedFirstToken = false;
    const assistantMessage: ChatMessage = {
      role: 'assistant',
      content: '',
      thinking: '',
      timestamp: Date.now(),
    };
    setMessages([...conversationMessages, assistantMessage]);

    try {
      const stopStreaming = await apiClient.createChatCompletionStream(
        request,
        credentialKey.trim(),
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
            setIsWaitingFirstToken(false);
          }

          assistantThinking += thinkingDelta;
          assistantContent += contentDelta;
          setMessages(prev => {
            const updated = [...prev];
            updated[updated.length - 1] = {
              role: 'assistant',
              content: assistantContent,
              ...(assistantThinking ? { thinking: assistantThinking } : {}),
            };
            return updated;
          });
        },
        () => {
          setIsLoading(false);
          setIsStreaming(false);
          setIsWaitingFirstToken(false);
          setAbortController(null);
        },
        (error: Error) => {
          setMessages(prev => {
            const updated = [...prev];
            updated[updated.length - 1] = {
              role: 'assistant',
              content: `Error: ${error.message}`,
            };
            return updated;
          });
          setIsLoading(false);
          setIsStreaming(false);
          setIsWaitingFirstToken(false);
          setAbortController(null);
        }
      );

      setAbortController(() => stopStreaming);
    } catch (error) {
      setMessages([
        ...conversationMessages,
        {
          role: 'assistant',
          content: `Error: ${error instanceof Error ? error.message : 'Unknown error'}`,
        },
      ]);
      setIsLoading(false);
      setIsStreaming(false);
      setIsWaitingFirstToken(false);
    }
  };

  const handlePickImage = () => {
    setImageError(null);
    if (!isVisionModel(selectedModel)) {
      setImageError('当前选择的模型不支持图片输入');
      return;
    }
    imageInputRef.current?.click();
  };

  const handleImageChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setImageError(null);
    const file = e.target.files?.[0];
    if (!file) return;

    if (!isVisionModel(selectedModel)) {
      setImageError('当前选择的模型不支持图片输入');
      e.target.value = '';
      return;
    }

    const maxBytes = 5 * 1024 * 1024;
    if (file.size > maxBytes) {
      setImageError('图片过大（最大 5MB）');
      e.target.value = '';
      return;
    }

    if (!file.type.startsWith('image/')) {
      setImageError('仅支持图片文件');
      e.target.value = '';
      return;
    }

    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result === 'string') {
        setImageDataUrl(result);
      } else {
        setImageError('读取图片失败');
      }
    };
    reader.onerror = () => setImageError('读取图片失败');
    reader.readAsDataURL(file);

    e.target.value = '';
  };

  const handleSend = async () => {
    if (
      (!input.trim() && !imageDataUrl) ||
      !selectedModel ||
      !credentialKey.trim() ||
      isLoading
    )
      return;

    if (imageDataUrl && !isVisionModel(selectedModel)) {
      setImageError('当前选择的模型不支持图片输入');
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

    const userMessage: ChatMessage = {
      role: 'user',
      content,
      timestamp: Date.now(),
    };

    const newMessages = [...messages, userMessage];
    setInput('');
    setImageDataUrl(null);
    await startStreaming(newMessages);
  };

  const handleShareAt = async (index: number) => {
    if (isLoading || isStreaming) return;
    if (!credentialKey.trim() || !selectedModel) return;
    const requestMessages = getMessagesForShareAt(messages, index);
    if (!requestMessages.some(m => m.role === 'user')) return;
    const request: ChatRequest = {
      model: selectedModel,
      messages: withSystemPrompt(
        toRequestMessages(requestMessages),
        systemPrompt
      ),
      stream: true,
      max_tokens: maxTokens,
    };
    await copyToClipboard(buildChatCurl(request, credentialKey.trim()));
    setSharedIndex(index);
    if (shareResetTimerRef.current) {
      window.clearTimeout(shareResetTimerRef.current);
    }
    shareResetTimerRef.current = window.setTimeout(() => {
      setSharedIndex(current => (current === index ? null : current));
    }, 1500);
  };

  const handleRegenerate = async () => {
    if (isLoading || isStreaming) return;
    if (!credentialKey.trim() || !selectedModel) return;
    const requestMessages = getMessagesForGeneration(messages);
    if (!requestMessages.some(m => m.role === 'user')) return;
    await startStreaming(requestMessages);
  };

  const handleStop = () => {
    if (abortController) {
      abortController();
      setIsLoading(false);
      setIsStreaming(false);
      setIsWaitingFirstToken(false);
      setAbortController(null);
    }
  };

  const handleClear = () => {
    setMessages([]);
    setImageDataUrl(null);
    setImageError(null);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

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

  const handleSuggestionClick = (prompt: string) => {
    setInput(prompt);
  };

  const getAllModels = () => {
    return models.map(m => ({ value: m.id, label: m.id }));
  };

  const renderMessageContent = (msg: ChatMessage, index: number) => {
    if (
      msg.role === 'assistant' &&
      typeof msg.content === 'string' &&
      !msg.content &&
      isWaitingFirstToken &&
      index === messages.length - 1
    ) {
      return <TypingIndicator />;
    }

    if (typeof msg.content === 'string') {
      return (
        <div
          className="markdown break-words"
          dangerouslySetInnerHTML={{
            __html: renderMarkdownToHtml(msg.content),
          }}
        />
      );
    }

    return (
      <div className="space-y-2">
        {msg.content.map((part, partIndex) => {
          if (part.type === 'text') {
            return (
              <div
                key={partIndex}
                className="markdown break-words"
                dangerouslySetInnerHTML={{
                  __html: renderMarkdownToHtml(part.text),
                }}
              />
            );
          }
          return (
            <img
              key={partIndex}
              src={part.image_url.url}
              alt="uploaded"
              className="max-h-64 rounded-lg border border-gray-200 dark:border-gray-600"
            />
          );
        })}
      </div>
    );
  };

  const renderThinkingContent = (msg: ChatMessage, index: number) => {
    if (msg.role !== 'assistant') return null;
    const thinking = msg.thinking?.trim();
    if (!thinking) return null;
    const open = isStreaming && index === messages.length - 1;
    return (
      <div
        className={
          isStreaming && index === messages.length - 1 ? 'thinking-border' : ''
        }
      >
        <details
          open={open}
          className="mb-3 rounded-lg border border-gray-200 dark:border-gray-700 bg-gray-50/80 dark:bg-gray-800/50"
        >
          <summary className="cursor-pointer select-none px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400">
            Thinking
          </summary>
          <div
            className="px-3 pb-3 markdown break-words text-sm text-gray-600 dark:text-gray-300"
            dangerouslySetInnerHTML={{
              __html: renderMarkdownToHtml(thinking),
            }}
          />
        </details>
      </div>
    );
  };

  const handleCopy = async (msg: ChatMessage, index: number) => {
    await copyToClipboard(getMessageCopyText(msg));
    setCopiedIndex(index);
    window.setTimeout(() => {
      setCopiedIndex(current => (current === index ? null : current));
    }, 1500);
  };

  return (
    <div className="w-full h-[calc(100vh-120px)]">
      <div className="relative bg-white dark:bg-gray-800 rounded-2xl overflow-hidden h-full flex flex-col shadow-sm">
        {/* New Chat Button */}
        {messages.length > 0 && (
          <button
            type="button"
            onClick={handleClear}
            disabled={isLoading || isStreaming}
            className="absolute top-3 left-3 z-10 p-2 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            title="New chat"
            aria-label="New chat"
          >
            <SquarePen className="w-5 h-5" />
          </button>
        )}
        {/* Messages Area */}
        <div className="flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-3xl h-full px-4 py-4">
            {messages.length === 0 ? (
              <div className="h-full flex flex-col items-center justify-center">
                <h1 className="text-2xl font-semibold text-gray-900 dark:text-white mb-8">
                  What can I help with?
                </h1>
                <div className="grid grid-cols-2 gap-3 w-full max-w-md">
                  {suggestions.map(s => (
                    <button
                      key={s.label}
                      type="button"
                      onClick={() => handleSuggestionClick(s.prompt)}
                      disabled={!credentialKey.trim() || !selectedModel}
                      className="flex items-start gap-3 p-4 rounded-xl border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700/40 hover:bg-gray-50 dark:hover:bg-gray-700 text-left transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
                    >
                      <s.icon className="w-5 h-5 text-gray-400 dark:text-gray-500 shrink-0 mt-0.5" />
                      <span className="text-sm text-gray-700 dark:text-gray-300 leading-snug">
                        {s.label}
                      </span>
                    </button>
                  ))}
                </div>
                {!credentialKey.trim() && (
                  <p className="mt-6 text-xs text-gray-400 dark:text-gray-500">
                    Set a credential key in Settings to get started
                  </p>
                )}
              </div>
            ) : (
              <div className="divide-y divide-gray-100 dark:divide-gray-700/50">
                {messages.map((msg, index) => (
                  <div key={index} className="py-6 first:pt-2">
                    <div className="group relative">
                      <div className="mb-1 text-xs font-medium text-gray-500 dark:text-gray-400 select-none">
                        {msg.role === 'user' ? 'You' : 'Assistant'}
                      </div>
                      <div className="text-gray-900 dark:text-white">
                        {renderThinkingContent(msg, index)}
                        {renderMessageContent(msg, index)}
                      </div>
                      {msg.role === 'assistant' &&
                      (!isStreaming || index !== messages.length - 1) ? (
                        <ChatMessageActions
                          copied={copiedIndex === index}
                          onCopy={() => handleCopy(msg, index)}
                          shared={sharedIndex === index}
                          onShare={() => void handleShareAt(index)}
                          shareDisabled={
                            isLoading ||
                            isStreaming ||
                            !credentialKey.trim() ||
                            !selectedModel ||
                            !getMessagesForShareAt(messages, index).some(
                              m => m.role === 'user'
                            )
                          }
                          showRegenerate={index === messages.length - 1}
                          regenerateDisabled={
                            isLoading ||
                            isStreaming ||
                            !getMessagesForGeneration(messages).some(
                              m => m.role === 'user'
                            )
                          }
                          onRegenerate={() => void handleRegenerate()}
                          disabled={
                            isWaitingFirstToken &&
                            typeof msg.content === 'string' &&
                            !msg.content &&
                            index === messages.length - 1
                          }
                        />
                      ) : null}
                    </div>
                  </div>
                ))}
              </div>
            )}
            <div ref={messagesEndRef} />
          </div>
        </div>

        {showSettings ? (
          <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
            <button
              type="button"
              className="absolute inset-0 bg-black/40"
              aria-label="Close settings"
              onClick={() => setShowSettings(false)}
            />
            <div
              role="dialog"
              aria-modal="true"
              aria-label="Settings"
              className="relative w-full max-w-lg rounded-2xl bg-white dark:bg-gray-800 shadow-xl ring-1 ring-gray-200 dark:ring-gray-700"
            >
              <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-gray-700">
                <div className="text-sm font-semibold text-gray-900 dark:text-white">
                  Settings
                </div>
                <button
                  type="button"
                  className="btn-icon"
                  title="Close"
                  aria-label="Close"
                  onClick={() => setShowSettings(false)}
                >
                  <X className="w-4 h-4" />
                </button>
              </div>

              <div className="p-4 space-y-4">
                <div>
                  <label
                    htmlFor="credential-key"
                    className="block mb-2 text-sm font-medium text-gray-900 dark:text-white"
                  >
                    Credential Key
                  </label>
                  <div className="flex items-center space-x-2">
                    <input
                      ref={credentialKeyInputRef}
                      id="credential-key"
                      value={
                        isEditingCredentialKey
                          ? credentialKey
                          : maskCredentialKey(credentialKey)
                      }
                      onChange={e => setCredentialKey(e.target.value)}
                      placeholder="sk-... (used for /v1/models and /v1/chat/completions)"
                      className="flex-1 bg-white dark:bg-gray-900 border border-gray-300 dark:border-gray-700 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-2.5"
                      disabled={isLoading}
                      readOnly={!isEditingCredentialKey}
                      autoComplete="off"
                      spellCheck={false}
                      inputMode="text"
                    />
                    <button
                      type="button"
                      className="btn btn-secondary"
                      onClick={() =>
                        setIsEditingCredentialKey(editing => !editing)
                      }
                      disabled={isLoading}
                      title={
                        isEditingCredentialKey
                          ? 'Hide credential key'
                          : 'Edit credential key'
                      }
                    >
                      {isEditingCredentialKey ? 'Hide' : 'Edit'}
                    </button>
                  </div>
                  {modelsError ? (
                    <p className="mt-2 text-sm text-red-600 dark:text-red-400">
                      {modelsError}
                    </p>
                  ) : null}
                </div>

                <div>
                  <label
                    htmlFor="max-tokens"
                    className="block mb-2 text-sm font-medium text-gray-900 dark:text-white"
                  >
                    Max Tokens: {maxTokens}
                  </label>
                  <input
                    id="max-tokens"
                    type="range"
                    min="100"
                    max="8000"
                    step="100"
                    value={maxTokens}
                    onChange={e => setMaxTokens(parseInt(e.target.value))}
                    className="w-full"
                    disabled={isLoading}
                  />
                </div>

                <div>
                  <label
                    htmlFor="system-prompt"
                    className="block mb-2 text-sm font-medium text-gray-900 dark:text-white"
                  >
                    System Prompt
                  </label>
                  <textarea
                    id="system-prompt"
                    value={systemPrompt}
                    onChange={e => setSystemPrompt(e.target.value)}
                    placeholder="Optional. Prepended as the first system message."
                    className="w-full bg-white dark:bg-gray-900 border border-gray-300 dark:border-gray-700 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-2.5 resize-none"
                    disabled={isLoading}
                    rows={3}
                  />
                </div>
              </div>

              <div className="flex items-center justify-between gap-2 px-4 py-3 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900/30">
                <button
                  type="button"
                  className="btn btn-secondary flex items-center gap-2"
                  title="Clear chat"
                  onClick={handleClear}
                  disabled={isLoading || isStreaming}
                >
                  <Trash2 className="w-4 h-4" />
                  <span>Clear</span>
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => setShowSettings(false)}
                >
                  Done
                </button>
              </div>
            </div>
          </div>
        ) : null}

        <ChatComposer
          input={input}
          onInputChange={setInput}
          onKeyDown={handleKeyDown}
          isLoading={isLoading}
          isStreaming={isStreaming}
          onSend={() => void handleSend()}
          sendDisabled={
            (!input.trim() && !imageDataUrl) ||
            !credentialKey.trim() ||
            !selectedModel ||
            isLoading
          }
          onStop={handleStop}
          imageDataUrl={imageDataUrl}
          onRemoveImage={() => setImageDataUrl(null)}
          imageError={imageError}
          imageInputRef={imageInputRef}
          onImageChange={handleImageChange}
          onPickImage={handlePickImage}
          showImageButton={isVisionModel(selectedModel)}
          selectedModel={selectedModel}
          onSelectModel={setSelectedModel}
          modelOptions={getAllModels()}
          onOpenSettings={() => setShowSettings(true)}
        />
      </div>
    </div>
  );
};

export default Chat;
