import { useState, useEffect, useCallback, ReactNode } from 'react';
import { ApiClient } from '../api/client';
import type { AuthState } from '../types';
import {
  AuthContext,
  AUTH_STORAGE_KEY,
  API_BASE_URL,
  defaultAuthState,
} from './auth-context';

export function AuthProvider({ children }: { children: ReactNode }) {
  const [authState, setAuthState] = useState<AuthState>(defaultAuthState);
  const [apiClient, setApiClient] = useState<ApiClient | null>(null);

  // Initialize auth state from localStorage on mount
  useEffect(() => {
    const stored = localStorage.getItem(AUTH_STORAGE_KEY);
    if (stored) {
      try {
        const parsedAuth = JSON.parse(stored);
        if (parsedAuth.isAuthenticated && parsedAuth.apiKey) {
          setAuthState(parsedAuth);
          setApiClient(new ApiClient(API_BASE_URL, parsedAuth.apiKey));
        }
      } catch {
        localStorage.removeItem(AUTH_STORAGE_KEY);
      }
    }
  }, []);

  const login = useCallback(async (apiKey: string): Promise<void> => {
    // Test the connection by attempting to get health status
    const testClient = new ApiClient(API_BASE_URL, apiKey);
    await testClient.health();

    // If successful, update auth state
    const newAuthState: AuthState = {
      isAuthenticated: true,
      apiKey,
    };

    setAuthState(newAuthState);
    setApiClient(testClient);

    // Save to localStorage
    localStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify(newAuthState));
  }, []);

  const logout = useCallback(() => {
    setAuthState(defaultAuthState);
    setApiClient(null);
    localStorage.removeItem(AUTH_STORAGE_KEY);
  }, []);

  const value = {
    authState,
    apiClient,
    login,
    logout,
    isAuthenticated: authState.isAuthenticated,
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
