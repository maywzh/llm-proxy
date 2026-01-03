import React, { useState, useEffect } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { useAuth } from '../contexts/AuthContext';
import type { ConfigVersionResponse } from '../types';

interface LayoutProps {
  children: React.ReactNode;
}

const Layout: React.FC<LayoutProps> = ({ children }) => {
  const { apiClient, logout } = useAuth();
  const location = useLocation();
  const [configVersion, setConfigVersion] =
    useState<ConfigVersionResponse | null>(null);
  const [isReloading, setIsReloading] = useState(false);

  // Navigation items
  const navItems = [
    { href: '/providers', label: 'Providers', icon: '🔌' },
    { href: '/master-keys', label: 'Master Keys', icon: '🔑' },
  ];

  // Load config version on mount
  useEffect(() => {
    if (apiClient) {
      apiClient.getConfigVersion().then(setConfigVersion).catch(console.error);
    }
  }, [apiClient]);

  const handleLogout = () => {
    logout();
  };

  const handleReloadConfig = async () => {
    if (!apiClient) return;

    setIsReloading(true);
    try {
      const response = await apiClient.reloadConfig();
      setConfigVersion({
        version: response.version,
        timestamp: response.timestamp,
      });
    } catch (error) {
      console.error('Failed to reload config:', error);
    } finally {
      setIsReloading(false);
    }
  };

  return (
    <div className="min-h-screen bg-gray-50">
      {/* Header */}
      <header className="bg-white shadow-sm border-b border-gray-200">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between items-center h-16">
            {/* Logo and Title */}
            <div className="flex items-center">
              <h1 className="text-xl font-semibold text-gray-900">
                LLM Proxy Admin
              </h1>
            </div>

            {/* Navigation */}
            <nav className="hidden md:flex space-x-8">
              {navItems.map(item => (
                <Link
                  key={item.href}
                  to={item.href}
                  className={`flex items-center space-x-2 px-3 py-2 rounded-md text-sm font-medium transition-colors duration-200 ${
                    location.pathname === item.href
                      ? 'bg-blue-100 text-blue-700'
                      : 'text-gray-600 hover:text-gray-900 hover:bg-gray-100'
                  }`}
                >
                  <span>{item.icon}</span>
                  <span>{item.label}</span>
                </Link>
              ))}
            </nav>

            {/* Config Version and Actions */}
            <div className="flex items-center space-x-4">
              {configVersion && (
                <div className="text-sm text-gray-500">
                  v{configVersion.version}
                </div>
              )}

              <button
                onClick={handleReloadConfig}
                disabled={isReloading}
                className="btn btn-secondary text-sm"
                title="Reload Configuration"
              >
                {isReloading ? (
                  <svg
                    className="animate-spin h-4 w-4"
                    xmlns="http://www.w3.org/2000/svg"
                    fill="none"
                    viewBox="0 0 24 24"
                  >
                    <circle
                      className="opacity-25"
                      cx="12"
                      cy="12"
                      r="10"
                      stroke="currentColor"
                      strokeWidth="4"
                    ></circle>
                    <path
                      className="opacity-75"
                      fill="currentColor"
                      d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                    ></path>
                  </svg>
                ) : (
                  '🔄'
                )}{' '}
                Reload
              </button>

              <button
                onClick={handleLogout}
                className="btn btn-secondary text-sm"
              >
                Logout
              </button>
            </div>
          </div>

          {/* Mobile Navigation */}
          <div className="md:hidden pb-3">
            <nav className="flex space-x-4">
              {navItems.map(item => (
                <Link
                  key={item.href}
                  to={item.href}
                  className={`flex items-center space-x-2 px-3 py-2 rounded-md text-sm font-medium transition-colors duration-200 ${
                    location.pathname === item.href
                      ? 'bg-blue-100 text-blue-700'
                      : 'text-gray-600 hover:text-gray-900 hover:bg-gray-100'
                  }`}
                >
                  <span>{item.icon}</span>
                  <span>{item.label}</span>
                </Link>
              ))}
            </nav>
          </div>
        </div>
      </header>

      {/* Main Content */}
      <main className="max-w-7xl mx-auto py-6 px-4 sm:px-6 lg:px-8">
        {children}
      </main>
    </div>
  );
};

export default Layout;
