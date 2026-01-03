import { useState, useEffect, useCallback } from 'react';
import { ApiClient } from '../api/client';
import type { AuthState } from '../types';

const AUTH_STORAGE_KEY = 'admin-auth';
const API_BASE_URL =
  import.meta.env.VITE_PUBLIC_API_BASE_URL || 'http://127.0.0.1:18000';

const defaultAuthState: AuthState = {
  isAuthenticated: false,
  apiKey: '',
};

export function useAuth() {
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
      } catch (error) {
        console.error('Failed to parse stored auth:', error);
        localStorage.removeItem(AUTH_STORAGE_KEY);
      }
    }
  }, []);

  const login = useCallback(async (apiKey: string): Promise<void> => {
    // Validate admin key using the dedicated endpoint
    const testClient = new ApiClient(API_BASE_URL, apiKey);
    const result = await testClient.validateAdminKey();

    if (!result.valid) {
      throw new Error(result.message || 'Invalid admin key');
    }

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

  return {
    authState,
    apiClient,
    login,
    logout,
    isAuthenticated: authState.isAuthenticated,
  };
}
