import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

export type ChatSettingsState = {
  credentialKey: string;
  selectedModel: string;
  maxTokens: number;
  systemPrompt: string;
  setCredentialKey: (credentialKey: string) => void;
  setSelectedModel: (selectedModel: string) => void;
  setMaxTokens: (maxTokens: number) => void;
  setSystemPrompt: (systemPrompt: string) => void;
};

type StorageLike = {
  getItem: (name: string) => string | null;
  setItem: (name: string, value: string) => void;
  removeItem: (name: string) => void;
};

const noopStorage: StorageLike = {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
};

const storage = createJSONStorage(() =>
  typeof window !== 'undefined' ? window.localStorage : noopStorage
);

function readLegacySettings(): Partial<ChatSettingsState> {
  if (typeof window === 'undefined') return {};
  const credentialKey =
    window.localStorage.getItem('chat-credential-key') ?? '';
  const selectedModel =
    window.localStorage.getItem('chat-selected-model') ?? '';
  const systemPrompt = window.localStorage.getItem('chat-system-prompt') ?? '';
  const maxTokensRaw = window.localStorage.getItem('chat-max-tokens');
  const maxTokens = maxTokensRaw ? Number.parseInt(maxTokensRaw, 10) : NaN;
  return {
    credentialKey,
    selectedModel,
    systemPrompt,
    maxTokens: Number.isFinite(maxTokens) ? maxTokens : 2000,
  };
}

export const useChatSettings = create<ChatSettingsState>()(
  persist(
    set => ({
      credentialKey: '',
      selectedModel: '',
      maxTokens: 2000,
      systemPrompt: '',
      ...readLegacySettings(),
      setCredentialKey: credentialKey => set({ credentialKey }),
      setSelectedModel: selectedModel => set({ selectedModel }),
      setMaxTokens: maxTokens => set({ maxTokens }),
      setSystemPrompt: systemPrompt => set({ systemPrompt }),
    }),
    {
      name: 'chat-settings-react',
      version: 1,
      storage,
    }
  )
);

if (typeof window !== 'undefined') {
  useChatSettings.subscribe(state => {
    window.localStorage.setItem(
      'chat-credential-key',
      state.credentialKey.trim()
    );
    window.localStorage.setItem(
      'chat-selected-model',
      state.selectedModel.trim()
    );
    window.localStorage.setItem('chat-max-tokens', String(state.maxTokens));
    if (state.systemPrompt.trim()) {
      window.localStorage.setItem('chat-system-prompt', state.systemPrompt);
    } else {
      window.localStorage.removeItem('chat-system-prompt');
    }
  });
}
