// Disable SSR for the chat page to prevent 500 errors when directly accessing /chat
// The chat page relies heavily on:
// 1. Browser localStorage for chat settings (credential key, selected model, etc.)
// 2. Client-side authentication state from stores
// 3. Browser APIs for streaming responses and clipboard operations
export const ssr = false;
