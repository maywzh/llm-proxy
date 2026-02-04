import React, { useState } from 'react';
import {
  LayoutDashboard,
  ExternalLink,
  RefreshCw,
  Maximize2,
  X,
} from 'lucide-react';
import { DashboardSkeleton } from '../components/Skeleton';

const Dashboard: React.FC = () => {
  const [isLoading, setIsLoading] = useState(true);
  const [isFullscreen, setIsFullscreen] = useState(false);

  // Public Dashboard URL - from environment variable
  const publicDashboardUrl =
    import.meta.env.VITE_PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL || '';

  const handleIframeLoad = () => {
    setIsLoading(false);
  };

  const handleRefresh = () => {
    setIsLoading(true);
    const iframe = document.getElementById(
      'grafana-iframe'
    ) as HTMLIFrameElement;
    if (iframe) {
      const currentSrc = iframe.src;
      iframe.src = '';
      // Use setTimeout to ensure the src is cleared before reassigning
      setTimeout(() => {
        iframe.src = currentSrc;
      }, 0);
    }
  };

  const handleOpenExternal = () => {
    window.open(publicDashboardUrl, '_blank');
  };

  const toggleFullscreen = () => {
    setIsFullscreen(!isFullscreen);
  };

  if (!publicDashboardUrl) {
    return (
      <div className="space-y-6">
        <div className="flex items-center space-x-3">
          <LayoutDashboard className="w-6 h-6 text-primary-600" />
          <h1 className="text-2xl font-bold text-gray-900">Dashboard</h1>
        </div>

        <div className="card p-8">
          <div className="text-center mb-6">
            <LayoutDashboard className="w-16 h-16 text-gray-300 mx-auto mb-4" />
            <h2 className="text-xl font-semibold text-gray-700 dark:text-gray-300 mb-2">
              Grafana Dashboard Not Configured
            </h2>
            <p className="text-gray-500 dark:text-gray-400">
              Follow the steps below to configure your Grafana public dashboard.
            </p>
          </div>

          <div className="bg-gray-50 dark:bg-gray-800 rounded-lg p-6 mb-6">
            <h3 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-4 uppercase tracking-wide">
              Setup Guide
            </h3>
            <ol className="space-y-3 text-gray-600 dark:text-gray-400">
              <li className="flex items-start">
                <span className="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3">
                  1
                </span>
                <span>Create or open a dashboard in Grafana</span>
              </li>
              <li className="flex items-start">
                <span className="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3">
                  2
                </span>
                <span>Click the share button and select "Public Dashboard"</span>
              </li>
              <li className="flex items-start">
                <span className="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3">
                  3
                </span>
                <span>Enable public access and copy the generated URL</span>
              </li>
              <li className="flex items-start">
                <span className="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3">
                  4
                </span>
                <span>
                  Set the{' '}
                  <code className="bg-gray-200 dark:bg-gray-700 px-2 py-0.5 rounded text-sm">
                    VITE_PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL
                  </code>{' '}
                  environment variable
                </span>
              </li>
              <li className="flex items-start">
                <span className="flex-shrink-0 w-6 h-6 bg-primary-100 dark:bg-primary-900 text-primary-600 dark:text-primary-400 rounded-full flex items-center justify-center text-sm font-medium mr-3">
                  5
                </span>
                <span>Restart the application to apply the changes</span>
              </li>
            </ol>
          </div>

          <div className="text-center">
            <a
              href="https://grafana.com/docs/grafana/latest/dashboards/dashboard-public/"
              target="_blank"
              rel="noopener noreferrer"
              className="text-primary-600 hover:text-primary-700 inline-flex items-center"
            >
              Learn more about Public Dashboards
              <ExternalLink className="w-4 h-4 ml-1" />
            </a>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      className={`space-y-4 transition-all duration-300 ${isFullscreen ? 'fixed inset-0 z-50 bg-white dark:bg-gray-900 p-4 animate-fade-in' : ''}`}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center space-x-3">
          <LayoutDashboard className="w-6 h-6 text-primary-600" />
          <h1 className="text-2xl font-bold text-gray-900">Dashboard</h1>
        </div>

        <div className="flex items-center space-x-2">
          <button
            onClick={handleRefresh}
            className="btn btn-secondary text-sm flex items-center space-x-2"
            title="Refresh Dashboard"
          >
            <RefreshCw
              className={`w-4 h-4 ${isLoading ? 'animate-spin' : ''}`}
            />
            <span className="hidden sm:inline">Refresh</span>
          </button>

          <button
            onClick={toggleFullscreen}
            className="btn btn-secondary text-sm flex items-center space-x-2"
            title={isFullscreen ? 'Exit Fullscreen' : 'Fullscreen'}
          >
            {isFullscreen ? (
              <X className="w-4 h-4" />
            ) : (
              <Maximize2 className="w-4 h-4" />
            )}
          </button>

          <button
            onClick={handleOpenExternal}
            className="btn btn-secondary text-sm flex items-center space-x-2"
            title="Open in New Tab"
          >
            <ExternalLink className="w-4 h-4" />
            <span className="hidden sm:inline">Open</span>
          </button>
        </div>
      </div>

      <div className="card p-0 overflow-hidden relative">
        {isLoading && (
          <div className="absolute inset-0 flex items-center justify-center bg-gray-50 dark:bg-gray-800 z-10 animate-fade-in">
            <div className="w-full max-w-3xl px-8">
              <DashboardSkeleton />
            </div>
          </div>
        )}

        <iframe
          id="grafana-iframe"
          src={publicDashboardUrl}
          title="Grafana Dashboard"
          className="w-full border-0"
          style={{
            height: isFullscreen
              ? 'calc(100vh - 120px)'
              : 'calc(100vh - 200px)',
            minHeight: '600px',
          }}
          onLoad={handleIframeLoad}
          allow="fullscreen"
        />
      </div>
    </div>
  );
};

export default Dashboard;
