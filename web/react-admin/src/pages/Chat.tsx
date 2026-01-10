import React, { useState, useEffect, useRef } from 'react';
import { useAuth } from '../contexts/AuthContext';
import { Send, Loader2, Trash2, Settings, Zap, StopCircle } from 'lucide-react';
import type { ChatMessage, ChatRequest, StreamChunk, Model } from '../types';

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

const Chat: React.FC = () => {
  const { apiClient } = useAuth();
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [credentialKey, setCredentialKey] = useState('');
  const [isEditingCredentialKey, setIsEditingCredentialKey] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [isWaitingFirstToken, setIsWaitingFirstToken] = useState(false);
  const [abortController, setAbortController] = useState<(() => void) | null>(
    null
  );
  const [selectedModel, setSelectedModel] = useState<string>('');
  const [models, setModels] = useState<Model[]>([]);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [maxTokens, setMaxTokens] = useState(2000);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const credentialKeyInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const stored = localStorage.getItem('chat-credential-key');
    if (stored) setCredentialKey(stored);
  }, []);

  useEffect(() => {
    if (credentialKey.trim()) {
      localStorage.setItem('chat-credential-key', credentialKey.trim());
    }
  }, [credentialKey]);

  useEffect(() => {
    if (apiClient) {
      loadModels();
    }
  }, [apiClient]);

  useEffect(() => {
    if (!apiClient) return;
    if (!credentialKey.trim()) {
      setModels([]);
      setSelectedModel('');
      setModelsError(null);
      return;
    }

    const timer = window.setTimeout(() => {
      loadModels();
    }, 400);
    return () => window.clearTimeout(timer);
  }, [apiClient, credentialKey]);

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

  const loadModels = async () => {
    if (!apiClient) return;
    try {
      if (!credentialKey.trim()) return;
      const response = await apiClient.listModels(credentialKey.trim());
      setModels(response.data);
      setModelsError(null);

      if (!selectedModel && response.data.length > 0) {
        setSelectedModel(response.data[0].id);
      }
    } catch (error) {
      console.error('Failed to load models:', error);
      setModels([]);
      setSelectedModel('');
      setModelsError(
        error instanceof Error ? error.message : 'Failed to load models'
      );
    }
  };

  const handleSend = async () => {
    if (!input.trim() || !selectedModel || !credentialKey.trim() || isLoading)
      return;

    const userMessage: ChatMessage = {
      role: 'user',
      content: input.trim(),
    };

    const newMessages = [...messages, userMessage];
    setMessages(newMessages);
    setInput('');
    setIsLoading(true);
    setIsStreaming(true);
    setIsWaitingFirstToken(true);

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
      setMessages(prev => [...prev, assistantMessage]);

      const stopStreaming = await apiClient!.createChatCompletionStream(
        request,
        credentialKey.trim(),
        (chunk: StreamChunk) => {
          const delta = chunk.choices[0]?.delta;
          if (delta?.content) {
            if (!receivedFirstToken) {
              receivedFirstToken = true;
              setIsWaitingFirstToken(false);
            }
            assistantContent += delta.content;
            setMessages(prev => {
              const updated = [...prev];
              updated[updated.length - 1] = {
                role: 'assistant',
                content: assistantContent,
              };
              return updated;
            });
          }
        },
        () => {
          setIsLoading(false);
          setIsStreaming(false);
          setIsWaitingFirstToken(false);
          setAbortController(null);
        },
        (error: Error) => {
          console.error('Stream error:', error);
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
      console.error('Failed to send message:', error);
      setMessages(prev => [
        ...prev,
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
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const getAllModels = () => {
    return models.map(m => ({ value: m.id, label: m.id }));
  };

  return (
    <div className="max-w-7xl mx-auto">
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow-sm border border-gray-200 dark:border-gray-700 h-[calc(100vh-180px)] flex flex-col">
        {/* Header */}
        <div className="border-b border-gray-200 dark:border-gray-700 p-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-4 flex-1">
              <select
                value={selectedModel}
                onChange={e => setSelectedModel(e.target.value)}
                className="flex-1 max-w-md bg-gray-50 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-2.5"
                disabled={isLoading || getAllModels().length === 0}
              >
                {getAllModels().length === 0 ? (
                  <option value="">
                    Set credential key in Settings to load models
                  </option>
                ) : null}
                {getAllModels().map(model => (
                  <option key={model.value} value={model.value}>
                    {model.label}
                  </option>
                ))}
              </select>
            </div>

            <div className="flex items-center space-x-2">
              <button
                onClick={() => setShowSettings(!showSettings)}
                className="btn btn-secondary flex items-center space-x-2"
                title="Settings (set credential key)"
              >
                <Settings className="w-4 h-4" />
                <span>Settings</span>
              </button>
              <button
                onClick={handleClear}
                className="btn btn-secondary flex items-center space-x-2"
                title="Clear Chat"
              >
                <Trash2 className="w-4 h-4" />
                <span>Clear</span>
              </button>
            </div>
          </div>

          {/* Settings Panel */}
          {showSettings && (
            <div className="mt-4 p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
              <div className="space-y-4">
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
                      className="flex-1 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-2.5"
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
              </div>
            </div>
          )}
        </div>

        {/* Messages Area */}
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          {messages.length === 0 ? (
            <div className="h-full flex flex-col items-center justify-center text-gray-500 dark:text-gray-400">
              <Zap className="w-16 h-16 mb-4" />
              <p className="text-lg">Start a conversation</p>
              <p className="text-sm">
                Select a model and type your message below
              </p>
            </div>
          ) : (
            messages.map((msg, index) => (
              <div
                key={index}
                className={`flex ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}
              >
                <div
                  className={`max-w-[80%] rounded-lg px-4 py-3 ${
                    msg.role === 'user'
                      ? 'bg-primary-600 text-white'
                      : 'bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white'
                  }`}
                >
                  <p className="whitespace-pre-wrap break-words">
                    {msg.content ||
                      (msg.role === 'assistant' &&
                      isWaitingFirstToken &&
                      index === messages.length - 1 ? (
                        <span
                          className="inline-block animate-pulse"
                          aria-label="typing"
                        >
                          ▍
                        </span>
                      ) : null)}
                  </p>
                </div>
              </div>
            ))
          )}
          <div ref={messagesEndRef} />
        </div>

        {/* Input Area */}
        <div className="border-t border-gray-200 dark:border-gray-700 p-4">
          <div className="flex space-x-2">
            <textarea
              value={input}
              onChange={e => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Type your message... (Press Enter to send, Shift+Enter for new line)"
              className="flex-1 bg-gray-50 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 text-gray-900 dark:text-white rounded-lg focus:ring-primary-500 focus:border-primary-500 block p-3 resize-none"
              rows={3}
              disabled={isLoading}
            />
            <div className="flex flex-col space-y-2">
              {isStreaming ? (
                <button
                  onClick={handleStop}
                  className="btn btn-danger flex items-center justify-center"
                  title="Stop Generation"
                >
                  <StopCircle className="w-5 h-5" />
                </button>
              ) : (
                <button
                  onClick={handleSend}
                  disabled={!input.trim() || !credentialKey.trim() || isLoading}
                  className="btn btn-primary flex items-center justify-center"
                  title="Send Message"
                >
                  {isLoading ? (
                    <Loader2 className="w-5 h-5 animate-spin" />
                  ) : (
                    <Send className="w-5 h-5" />
                  )}
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default Chat;
