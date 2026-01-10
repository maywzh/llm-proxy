import React, { useState } from 'react';
import { useAuth } from '../contexts/AuthContext';
import { Loader2, AlertCircle, Plug } from 'lucide-react';

const Login: React.FC = () => {
  const { login } = useAuth();
  const [apiKey, setApiKey] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState('');

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    // Clear previous error
    setError('');

    // Validate inputs
    if (!apiKey.trim()) {
      setError('API key is required');
      return;
    }

    setIsLoading(true);

    try {
      await login(apiKey);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to connect to the API'
      );
    } finally {
      setIsLoading(false);
    }
  };

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleSubmit(e as React.FormEvent);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-gradient-to-br from-gray-50 to-gray-100 dark:from-gray-900 dark:to-gray-800 py-12 px-4 sm:px-6 lg:px-8">
      <div className="max-w-md w-full">
        <div className="card p-8">
          {/* Logo and Title */}
          <div className="text-center mb-8">
            <div className="inline-flex items-center justify-center w-16 h-16 bg-primary-600 rounded-2xl mb-4">
              <Plug className="w-8 h-8 text-white" />
            </div>
            <h2 className="text-3xl font-bold text-gray-900 dark:text-gray-100">
              LLM Proxy
            </h2>
            <p className="mt-2 text-sm text-gray-600 dark:text-gray-400">
              Sign in to manage your proxy configuration
            </p>
          </div>

          <form onSubmit={handleSubmit} className="space-y-6">
            {error && (
              <div className="alert-error">
                <div className="flex">
                  <div className="shrink-0">
                    <AlertCircle className="h-5 w-5 text-red-400" />
                  </div>
                  <div className="ml-3">
                    <p className="text-sm text-red-700 dark:text-red-400">
                      {error}
                    </p>
                  </div>
                </div>
              </div>
            )}

            <div>
              <label htmlFor="apiKey" className="label">
                Admin API Key
              </label>
              <div className="mt-1">
                <input
                  id="apiKey"
                  name="apiKey"
                  type="password"
                  value={apiKey}
                  onChange={e => setApiKey(e.target.value)}
                  onKeyPress={handleKeyPress}
                  disabled={isLoading}
                  className="input"
                  placeholder="Enter your admin API key"
                  required
                />
              </div>
              <p className="helper-text">
                The admin API key configured in your server&apos;s ADMIN_KEY
                environment variable
              </p>
            </div>

            <div>
              <button
                type="submit"
                disabled={isLoading}
                className="btn btn-primary w-full flex justify-center items-center space-x-2"
              >
                {isLoading ? (
                  <>
                    <Loader2 className="w-5 h-5 animate-spin" />
                    <span>Connecting...</span>
                  </>
                ) : (
                  <span>Sign In</span>
                )}
              </button>
            </div>

            <div className="text-center">
              <div className="text-sm text-gray-600 dark:text-gray-400">
                <p className="mb-2 font-medium">Need help?</p>
                <ul className="text-xs space-y-1 text-left bg-gray-50 dark:bg-gray-800 rounded-lg p-3">
                  <li className="flex items-start">
                    <span className="text-primary-600 mr-2">•</span>
                    <span>Make sure your LLM Proxy server is running</span>
                  </li>
                  <li className="flex items-start">
                    <span className="text-primary-600 mr-2">•</span>
                    <span>
                      Verify the ADMIN_KEY environment variable is set on the
                      server
                    </span>
                  </li>
                  <li className="flex items-start">
                    <span className="text-primary-600 mr-2">•</span>
                    <span>
                      Check that VITE_PUBLIC_API_BASE_URL is configured in your
                      .env file
                    </span>
                  </li>
                </ul>
              </div>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
};

export default Login;
