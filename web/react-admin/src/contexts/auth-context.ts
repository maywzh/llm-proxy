import { createContext } from 'react';
import { ApiClient } from '../api/client';
import type { AuthState } from '../types';

export const AUTH_STORAGE_KEY = 'admin-auth';
export const API_BASE_URL =
  import.meta.env.VITE_PUBLIC_API_BASE_URL || 'http://127.0.0.1:18000';

export const defaultAuthState: AuthState = {
  isAuthenticated: false,
  apiKey: '',
};

export interface AuthContextType {
  authState: AuthState;
  apiClient: ApiClient | null;
  login: (apiKey: string) => Promise<void>;
  logout: () => void;
  isAuthenticated: boolean;
}

export const AuthContext = createContext<AuthContextType | undefined>(
  undefined
);
