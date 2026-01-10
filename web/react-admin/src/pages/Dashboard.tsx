import React, { useState } from 'react';
import {
  LayoutDashboard,
  ExternalLink,
  RefreshCw,
  Maximize2,
  X,
} from 'lucide-react';

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

        <div className="card p-8 text-center">
          <LayoutDashboard className="w-16 h-16 text-gray-300 mx-auto mb-4" />
          <h2 className="text-xl font-semibold text-gray-700 dark:text-gray-300 mb-2">
            Grafana Dashboard Not Configured
          </h2>
          <p className="text-gray-500 dark:text-gray-400 mb-4">
            Please set the{' '}
            <code className="bg-gray-100 dark:bg-gray-700 px-2 py-1 rounded">
              VITE_PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL
            </code>{' '}
            environment variable.
          </p>
          <a
            href="https://grafana.com/docs/grafana/latest/dashboards/dashboard-public/"
            target="_blank"
            rel="noopener noreferrer"
            className="text-primary-600 hover:text-primary-700 inline-flex items-center"
          >
            Learn how to create a Public Dashboard
            <ExternalLink className="w-4 h-4 ml-1" />
          </a>
        </div>
      </div>
    );
  }

  return (
    <div
      className={`space-y-4 ${isFullscreen ? 'fixed inset-0 z-50 bg-white dark:bg-gray-900 p-4' : ''}`}
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
          <div className="absolute inset-0 flex items-center justify-center bg-gray-50 dark:bg-gray-800 z-10">
            <div className="text-center">
              <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-primary-600 mx-auto"></div>
              <p className="mt-4 text-gray-600 dark:text-gray-400">
                Loading dashboard...
              </p>
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
