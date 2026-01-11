import { writable } from 'svelte/store';
import { browser } from '$app/environment';

export type ChatSettings = {
  credentialKey: string;
  selectedModel: string;
  maxTokens: number;
  systemPrompt: string;
};

const STORAGE_KEY = 'chat-settings';

const defaultSettings: ChatSettings = {
  credentialKey: '',
  selectedModel: '',
  maxTokens: 2000,
  systemPrompt: '',
};

function safeParseJson(value: string | null): unknown {
  if (!value) return null;
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}

function readLegacySettings(): Partial<ChatSettings> {
  if (!browser) return {};
  const credentialKey = localStorage.getItem('chat-credential-key') ?? '';
  const selectedModel = localStorage.getItem('chat-selected-model') ?? '';
  const systemPrompt = localStorage.getItem('chat-system-prompt') ?? '';
  const maxTokensRaw = localStorage.getItem('chat-max-tokens');
  const maxTokens = maxTokensRaw ? Number.parseInt(maxTokensRaw, 10) : NaN;
  return {
    credentialKey,
    selectedModel,
    systemPrompt,
    maxTokens: Number.isFinite(maxTokens)
      ? maxTokens
      : defaultSettings.maxTokens,
  };
}

function loadSettings(): ChatSettings {
  if (!browser) return defaultSettings;
  const parsed = safeParseJson(localStorage.getItem(STORAGE_KEY));
  if (parsed && typeof parsed === 'object') {
    const obj = parsed as Partial<ChatSettings>;
    return {
      credentialKey: obj.credentialKey ?? '',
      selectedModel: obj.selectedModel ?? '',
      maxTokens:
        typeof obj.maxTokens === 'number'
          ? obj.maxTokens
          : defaultSettings.maxTokens,
      systemPrompt: obj.systemPrompt ?? '',
    };
  }
  return { ...defaultSettings, ...readLegacySettings() };
}

export const chatSettings = writable<ChatSettings>(defaultSettings);

let initialized = false;

export function initChatSettings() {
  if (!browser || initialized) return;
  initialized = true;

  chatSettings.set(loadSettings());

  chatSettings.subscribe(value => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
    localStorage.setItem('chat-credential-key', value.credentialKey.trim());
    localStorage.setItem('chat-selected-model', value.selectedModel.trim());
    localStorage.setItem('chat-max-tokens', String(value.maxTokens));
    if (value.systemPrompt.trim()) {
      localStorage.setItem('chat-system-prompt', value.systemPrompt);
    } else {
      localStorage.removeItem('chat-system-prompt');
    }
  });
}

export function updateChatSettings(patch: Partial<ChatSettings>) {
  chatSettings.update(current => ({ ...current, ...patch }));
}
