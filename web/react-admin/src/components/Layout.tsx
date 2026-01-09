import React, { useState, useEffect } from 'react';
import { Link, useLocation } from 'react-router-dom';
import { useAuth } from '../contexts/AuthContext';
import {
  Plug,
  Key,
  RefreshCw,
  LogOut,
  Menu,
  X,
  LayoutDashboard,
} from 'lucide-react';
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
  const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);

  // Navigation items
  const navItems = [
    { href: '/dashboard', label: 'Dashboard', icon: LayoutDashboard },
    { href: '/providers', label: 'Providers', icon: Plug },
    { href: '/credentials', label: 'Credentials', icon: Key },
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
      {/* Sidebar - Desktop */}
      <aside className="hidden lg:fixed lg:inset-y-0 lg:flex lg:w-64 lg:flex-col">
        <div className="flex flex-col grow bg-gray-900 overflow-y-auto">
          {/* Logo */}
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

          {/* Navigation */}
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

          {/* User Section */}
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

      {/* Mobile Sidebar */}
      {isMobileMenuOpen && (
        <div className="lg:hidden">
          <div
            className="fixed inset-0 bg-black bg-opacity-50 z-40"
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
                onClick={handleLogout}
                className="flex items-center space-x-3 w-full px-4 py-3 text-gray-300 hover:bg-gray-800 hover:text-white transition-colors duration-200 rounded-lg"
              >
                <LogOut className="w-5 h-5" />
                <span>Logout</span>
              </button>
            </div>
          </aside>
        </div>
      )}

      {/* Main Content */}
      <div className="lg:pl-64 flex flex-col flex-1">
        {/* Top Header */}
        <header className="sticky top-0 z-30 bg-white shadow-sm border-b border-gray-200">
          <div className="px-4 sm:px-6 lg:px-8">
            <div className="flex items-center justify-between h-16">
              {/* Mobile menu button */}
              <button
                onClick={() => setIsMobileMenuOpen(true)}
                className="lg:hidden btn-icon"
              >
                <Menu className="w-6 h-6" />
              </button>

              {/* Page title - hidden on mobile, shown on desktop */}
              <div className="hidden lg:block">
                <h1 className="text-xl font-semibold text-gray-900">
                  {navItems.find(item => item.href === location.pathname)
                    ?.label || 'Admin'}
                </h1>
              </div>

              {/* Right side actions */}
              <div className="flex items-center space-x-4 ml-auto">
                {configVersion && (
                  <span className="badge badge-info">
                    v{configVersion.version}
                  </span>
                )}

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

        {/* Main Content Area */}
        <main className="flex-1 py-6 px-4 sm:px-6 lg:px-8">{children}</main>
      </div>
    </div>
  );
};

export default Layout;
