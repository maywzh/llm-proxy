import React, { useState, useEffect, useRef } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { useAuth } from '../hooks/useAuth';
import { useTheme } from '../hooks/useTheme';
import {
  Plug,
  Key,
  RefreshCw,
  LogOut,
  Menu,
  X,
  LayoutDashboard,
  Sun,
  Moon,
  Monitor,
  MessageSquare,
} from 'lucide-react';
import type { ConfigVersionResponse } from '../types';

interface LayoutProps {
  children: React.ReactNode;
}

const Layout: React.FC<LayoutProps> = ({ children }) => {
  const { apiClient, logout } = useAuth();
  const { theme, setTheme } = useTheme();
  const location = useLocation();
  const [configVersion, setConfigVersion] =
    useState<ConfigVersionResponse | null>(null);
  const [isReloading, setIsReloading] = useState(false);
  const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);
  const [showThemeMenu, setShowThemeMenu] = useState(false);
  const themeMenuRef = useRef<HTMLDivElement>(null);

  const navItems = [
    { href: '/dashboard', label: 'Dashboard', icon: LayoutDashboard },
    { href: '/providers', label: 'Providers', icon: Plug },
    { href: '/credentials', label: 'Credentials', icon: Key },
    { href: '/chat', label: 'Chat', icon: MessageSquare },
  ];

  useEffect(() => {
    if (apiClient) {
      apiClient
        .getConfigVersion()
        .then(setConfigVersion)
        .catch(() => setConfigVersion(null));
    }
  }, [apiClient]);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        themeMenuRef.current &&
        !themeMenuRef.current.contains(event.target as Node)
      ) {
        setShowThemeMenu(false);
      }
    };

    if (showThemeMenu) {
      document.addEventListener('mousedown', handleClickOutside);
      return () =>
        document.removeEventListener('mousedown', handleClickOutside);
    }
  }, [showThemeMenu]);

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
    } finally {
      setIsReloading(false);
    }
  };

  const getThemeIcon = () => {
    if (theme === 'light') return <Sun className="w-4 h-4" />;
    if (theme === 'dark') return <Moon className="w-4 h-4" />;
    return <Monitor className="w-4 h-4" />;
  };

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-gray-900">
      {/* Sidebar - Desktop */}
      <aside className="hidden lg:fixed lg:inset-y-0 lg:flex lg:w-64 lg:flex-col">
        <div className="flex flex-col grow bg-gray-900 overflow-y-auto">
          <div className="flex items-center shrink-0 px-4 py-5 border-b border-gray-800">
            <div className="flex items-center space-x-3">
              <div className="w-8 h-8 bg-primary-600 rounded-lg flex items-center justify-center">
                <Plug className="w-5 h-5 text-white" />
              </div>
              <span className="text-xl font-semibold text-white">
                LLM Proxy
              </span>
            </div>
          </div>

          <nav className="flex-1 px-2 py-4 space-y-1">
            {navItems.map(item => {
              const Icon = item.icon;
              const isActive = location.pathname === item.href;
              return (
                <Link
                  key={item.href}
                  to={item.href}
                  className={`sidebar-nav-item ${isActive ? 'active' : ''}`}
                >
                  <Icon className="w-5 h-5" />
                  <span>{item.label}</span>
                </Link>
              );
            })}
          </nav>

          <div className="shrink-0 border-t border-gray-800 p-4">
            <button
              onClick={handleLogout}
              className="flex items-center space-x-3 w-full px-4 py-3 text-gray-300 hover:bg-gray-800 hover:text-white transition-colors duration-200 rounded-lg"
            >
              <LogOut className="w-5 h-5" />
              <span>Logout</span>
            </button>
          </div>
        </div>
      </aside>

      {isMobileMenuOpen && (
        <div className="lg:hidden">
          <div
            className="fixed inset-0 bg-black bg-opacity-50 dark:bg-opacity-70 z-40"
            onClick={() => setIsMobileMenuOpen(false)}
          />
          <aside className="fixed inset-y-0 left-0 flex flex-col w-64 bg-gray-900 z-50">
            <div className="flex items-center justify-between px-4 py-5 border-b border-gray-800">
              <div className="flex items-center space-x-3">
                <div className="w-8 h-8 bg-primary-600 rounded-lg flex items-center justify-center">
                  <Plug className="w-5 h-5 text-white" />
                </div>
                <span className="text-xl font-semibold text-white">
                  LLM Proxy
                </span>
              </div>
              <button
                onClick={() => setIsMobileMenuOpen(false)}
                className="text-gray-400 hover:text-white"
              >
                <X className="w-6 h-6" />
              </button>
            </div>
            <nav className="flex-1 px-2 py-4 space-y-1">
              {navItems.map(item => {
                const Icon = item.icon;
                const isActive = location.pathname === item.href;
                return (
                  <Link
                    key={item.href}
                    to={item.href}
                    onClick={() => setIsMobileMenuOpen(false)}
                    className={`sidebar-nav-item ${isActive ? 'active' : ''}`}
                  >
                    <Icon className="w-5 h-5" />
                    <span>{item.label}</span>
                  </Link>
                );
              })}
            </nav>
            <div className="shrink-0 border-t border-gray-800 p-4">
              <button
                onClick={() => {
                  handleLogout();
                  setIsMobileMenuOpen(false);
                }}
                className="flex items-center space-x-3 w-full px-4 py-3 text-gray-300 hover:bg-gray-800 hover:text-white transition-colors duration-200 rounded-lg"
              >
                <LogOut className="w-5 h-5" />
                <span>Logout</span>
              </button>
            </div>
          </aside>
        </div>
      )}

      <div className="lg:pl-64 flex flex-col flex-1">
        <header className="sticky top-0 z-30 bg-white dark:bg-gray-800 shadow-sm border-b border-gray-200 dark:border-gray-700">
          <div className="px-4 sm:px-6 lg:px-8">
            <div className="flex items-center justify-between h-16">
              <button
                onClick={() => setIsMobileMenuOpen(true)}
                className="lg:hidden btn-icon"
              >
                <Menu className="w-6 h-6" />
              </button>

              <div className="hidden lg:block">
                <h1 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
                  {navItems.find(item => item.href === location.pathname)
                    ?.label || 'Admin'}
                </h1>
              </div>

              <div className="flex items-center space-x-4 ml-auto">
                {configVersion && (
                  <span className="badge badge-info">
                    v{configVersion.version}
                  </span>
                )}

                <div className="relative" ref={themeMenuRef}>
                  <button
                    onClick={() => setShowThemeMenu(!showThemeMenu)}
                    className="btn btn-secondary text-sm flex items-center space-x-2"
                    title="Theme"
                  >
                    {getThemeIcon()}
                  </button>

                  {showThemeMenu && (
                    <div
                      className="absolute right-0 mt-2 w-40 bg-white dark:bg-gray-800 rounded-lg shadow-lg border border-gray-200 dark:border-gray-700 py-1 z-50"
                      onClick={e => e.stopPropagation()}
                    >
                      <button
                        onClick={() => {
                          setTheme('light');
                          setShowThemeMenu(false);
                        }}
                        className={`w-full px-4 py-2 text-left text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 flex items-center space-x-2 ${
                          theme === 'light'
                            ? 'bg-gray-100 dark:bg-gray-700'
                            : ''
                        }`}
                      >
                        <Sun className="w-4 h-4" />
                        <span>Light</span>
                      </button>
                      <button
                        onClick={() => {
                          setTheme('dark');
                          setShowThemeMenu(false);
                        }}
                        className={`w-full px-4 py-2 text-left text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 flex items-center space-x-2 ${
                          theme === 'dark' ? 'bg-gray-100 dark:bg-gray-700' : ''
                        }`}
                      >
                        <Moon className="w-4 h-4" />
                        <span>Dark</span>
                      </button>
                      <button
                        onClick={() => {
                          setTheme('system');
                          setShowThemeMenu(false);
                        }}
                        className={`w-full px-4 py-2 text-left text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 flex items-center space-x-2 ${
                          theme === 'system'
                            ? 'bg-gray-100 dark:bg-gray-700'
                            : ''
                        }`}
                      >
                        <Monitor className="w-4 h-4" />
                        <span>System</span>
                      </button>
                    </div>
                  )}
                </div>

                <button
                  onClick={handleReloadConfig}
                  disabled={isReloading}
                  className="btn btn-secondary text-sm flex items-center space-x-2"
                  title="Reload Configuration"
                >
                  <RefreshCw
                    className={`w-4 h-4 ${isReloading ? 'animate-spin' : ''}`}
                  />
                  <span className="hidden sm:inline">Reload</span>
                </button>
              </div>
            </div>
          </div>
        </header>

        <main className="flex-1 py-6 px-4 sm:px-6 lg:px-8">{children}</main>
      </div>
    </div>
  );
};

export default Layout;
