import React, { useState, useEffect, useCallback } from 'react';
import { useAuth } from '../hooks/useAuth';
import { generateApiKey } from '../api/client';
import type { Provider, ProviderFormData, ProviderUpdate } from '../types';

const Providers: React.FC = () => {
  const { apiClient } = useAuth();
  const [providers, setProviders] = useState<Provider[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState('');
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [formData, setFormData] = useState<ProviderFormData>({
    provider_key: '',
    provider_type: 'openai',
    api_base: '',
    api_key: '',
    model_mapping: {},
    is_enabled: true,
  });
  const [modelMappingText, setModelMappingText] = useState('');

  const loadProviders = useCallback(async () => {
    if (!apiClient) return;

    setLoading(true);
    setError(null);

    try {
      const response = await apiClient.listProviders();
      setProviders(response.providers);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load providers');
    } finally {
      setLoading(false);
    }
  }, [apiClient]);

  // Load providers when apiClient becomes available
  useEffect(() => {
    if (apiClient) {
      loadProviders();
    }
  }, [apiClient, loadProviders]);

  // Update model mapping text when form data changes
  useEffect(() => {
    const text = Object.entries(formData.model_mapping)
      .map(([key, value]) => `${key}=${value}`)
      .join('\n');
    setModelMappingText(text);
  }, [formData.model_mapping]);

  const resetForm = () => {
    setFormData({
      provider_key: '',
      provider_type: 'openai',
      api_base: '',
      api_key: '',
      model_mapping: {},
      is_enabled: true,
    });
    setModelMappingText('');
    setEditingProvider(null);
    setShowCreateForm(false);
  };

  const handleCreate = () => {
    setShowCreateForm(true);
    resetForm();
  };

  const handleEdit = (provider: Provider) => {
    setEditingProvider(provider);
    setFormData({
      provider_key: provider.provider_key,
      provider_type: provider.provider_type,
      api_base: provider.api_base,
      api_key: '', // Don't populate existing key for security
      model_mapping: provider.model_mapping,
      is_enabled: provider.is_enabled,
    });
    setShowCreateForm(true);
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!apiClient) return;

    setLoading(true);
    setError(null);

    try {
      if (editingProvider) {
        // Update existing provider
        const updateData: ProviderUpdate = {
          provider_type: formData.provider_type,
          api_base: formData.api_base,
          model_mapping: formData.model_mapping,
          is_enabled: formData.is_enabled,
        };

        // Only include API key if it's provided
        if (formData.api_key.trim()) {
          updateData.api_key = formData.api_key;
        }

        await apiClient.updateProvider(editingProvider.id, updateData);
      } else {
        // Create new provider
        await apiClient.createProvider({
          provider_key: formData.provider_key,
          provider_type: formData.provider_type,
          api_base: formData.api_base,
          api_key: formData.api_key,
          model_mapping: formData.model_mapping,
        });
      }

      resetForm();
      await loadProviders();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save provider');
    } finally {
      setLoading(false);
    }
  };

  const handleDelete = async (provider: Provider) => {
    if (!apiClient) return;

    if (
      !confirm(
        `Are you sure you want to delete provider "${provider.provider_key}"?`
      )
    ) {
      return;
    }

    setLoading(true);
    setError(null);

    try {
      await apiClient.deleteProvider(provider.id);
      await loadProviders();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to delete provider'
      );
    } finally {
      setLoading(false);
    }
  };

  const handleToggleStatus = async (provider: Provider) => {
    if (!apiClient) return;

    try {
      await apiClient.setProviderStatus(provider.id, !provider.is_enabled);
      await loadProviders();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to update provider status'
      );
    }
  };

  const updateModelMapping = (text: string) => {
    setModelMappingText(text);
    const mapping: Record<string, string> = {};
    text.split('\n').forEach(line => {
      const [key, value] = line.split('=').map(s => s.trim());
      if (key && value) {
        mapping[key] = value;
      }
    });
    setFormData(prev => ({ ...prev, model_mapping: mapping }));
  };

  const generateRandomKey = () => {
    setFormData(prev => ({ ...prev, api_key: generateApiKey() }));
  };

  // Filtered providers based on search
  const filteredProviders = providers.filter(
    provider =>
      provider.provider_key.toLowerCase().includes(searchTerm.toLowerCase()) ||
      provider.provider_type.toLowerCase().includes(searchTerm.toLowerCase()) ||
      provider.api_base.toLowerCase().includes(searchTerm.toLowerCase())
  );

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">Providers</h1>
          <p className="text-gray-600">
            Manage your LLM provider configurations
          </p>
        </div>
        <button onClick={handleCreate} className="btn btn-primary">
          + Add Provider
        </button>
      </div>

      {/* Search */}
      <div className="max-w-md">
        <input
          type="text"
          placeholder="Search providers..."
          value={searchTerm}
          onChange={e => setSearchTerm(e.target.value)}
          className="input"
        />
      </div>

      {/* Error Display */}
      {error && (
        <div className="bg-red-50 border-l-4 border-red-400 p-4">
          <div className="flex">
            <div className="shrink-0">
              <svg
                className="h-5 w-5 text-red-400"
                viewBox="0 0 20 20"
                fill="currentColor"
              >
                <path
                  fillRule="evenodd"
                  d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z"
                  clipRule="evenodd"
                ></path>
              </svg>
            </div>
            <div className="ml-3">
              <p className="text-sm text-red-700">{error}</p>
            </div>
            <div className="ml-auto pl-3">
              <button
                onClick={() => setError(null)}
                className="text-red-400 hover:text-red-600"
              >
                <svg
                  className="h-5 w-5"
                  viewBox="0 0 20 20"
                  fill="currentColor"
                >
                  <path
                    fillRule="evenodd"
                    d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"
                    clipRule="evenodd"
                  ></path>
                </svg>
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Create/Edit Form */}
      {showCreateForm && (
        <div className="card">
          <h2 className="text-lg font-semibold mb-4">
            {editingProvider ? 'Edit Provider' : 'Create New Provider'}
          </h2>

          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div>
                <label
                  htmlFor="provider_key"
                  className="block text-sm font-medium text-gray-700"
                >
                  Provider Key
                </label>
                <input
                  id="provider_key"
                  type="text"
                  value={formData.provider_key}
                  onChange={e =>
                    setFormData(prev => ({
                      ...prev,
                      provider_key: e.target.value,
                    }))
                  }
                  disabled={!!editingProvider}
                  className="input"
                  placeholder="e.g., openai-primary"
                  required={!editingProvider}
                />
              </div>

              <div>
                <label
                  htmlFor="provider_type"
                  className="block text-sm font-medium text-gray-700"
                >
                  Provider Type
                </label>
                <select
                  id="provider_type"
                  value={formData.provider_type}
                  onChange={e =>
                    setFormData(prev => ({
                      ...prev,
                      provider_type: e.target.value,
                    }))
                  }
                  className="input"
                  required
                >
                  <option value="openai">OpenAI</option>
                  <option value="azure">Azure OpenAI</option>
                  <option value="anthropic">Anthropic</option>
                  <option value="google">Google</option>
                  <option value="custom">Custom</option>
                </select>
              </div>
            </div>

            <div>
              <label
                htmlFor="api_base"
                className="block text-sm font-medium text-gray-700"
              >
                API Base URL
              </label>
              <input
                id="api_base"
                type="url"
                value={formData.api_base}
                onChange={e =>
                  setFormData(prev => ({ ...prev, api_base: e.target.value }))
                }
                className="input"
                placeholder="https://api.openai.com/v1"
                required
              />
            </div>

            <div>
              <label
                htmlFor="api_key"
                className="block text-sm font-medium text-gray-700"
              >
                API Key {editingProvider ? '(leave empty to keep current)' : ''}
              </label>
              <div className="flex space-x-2">
                <input
                  id="api_key"
                  type="password"
                  value={formData.api_key}
                  onChange={e =>
                    setFormData(prev => ({ ...prev, api_key: e.target.value }))
                  }
                  className="input flex-1"
                  placeholder={
                    editingProvider ? 'Enter new API key...' : 'sk-...'
                  }
                  required={!editingProvider}
                />
                <button
                  type="button"
                  onClick={generateRandomKey}
                  className="btn btn-secondary"
                  title="Generate random key"
                >
                  🎲
                </button>
              </div>
            </div>

            <div>
              <label
                htmlFor="model_mapping"
                className="block text-sm font-medium text-gray-700"
              >
                Model Mapping (optional)
              </label>
              <textarea
                id="model_mapping"
                value={modelMappingText}
                onChange={e => updateModelMapping(e.target.value)}
                className="input"
                rows={3}
                placeholder="gpt-4=gpt-4-turbo&#10;gpt-3.5-turbo=gpt-3.5-turbo-16k"
              />
              <p className="text-xs text-gray-500 mt-1">
                One mapping per line in format: source_model=target_model
              </p>
            </div>

            <div className="flex items-center">
              <input
                id="is_enabled"
                type="checkbox"
                checked={formData.is_enabled}
                onChange={e =>
                  setFormData(prev => ({
                    ...prev,
                    is_enabled: e.target.checked,
                  }))
                }
                className="h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300 rounded"
              />
              <label
                htmlFor="is_enabled"
                className="ml-2 block text-sm text-gray-900"
              >
                Enable this provider
              </label>
            </div>

            <div className="flex justify-end space-x-3">
              <button
                type="button"
                onClick={resetForm}
                className="btn btn-secondary"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="btn btn-primary"
                disabled={loading}
              >
                {loading && (
                  <svg
                    className="animate-spin -ml-1 mr-3 h-5 w-5 text-white"
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
                )}
                {editingProvider ? 'Update' : 'Create'} Provider
              </button>
            </div>
          </form>
        </div>
      )}

      {/* Providers List */}
      <div className="card">
        <div className="flex justify-between items-center mb-4">
          <h2 className="text-lg font-semibold">
            Providers ({filteredProviders.length})
          </h2>
          {loading && (
            <div className="flex items-center text-gray-500">
              <svg
                className="animate-spin -ml-1 mr-3 h-5 w-5"
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
              Loading...
            </div>
          )}
        </div>

        {filteredProviders.length === 0 ? (
          <div className="text-center py-8 text-gray-500">
            {searchTerm
              ? 'No providers match your search.'
              : 'No providers configured yet.'}
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="min-w-full divide-y divide-gray-200">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Provider
                  </th>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Type
                  </th>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    API Base
                  </th>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Models
                  </th>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Status
                  </th>
                  <th className="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Actions
                  </th>
                </tr>
              </thead>
              <tbody className="bg-white divide-y divide-gray-200">
                {filteredProviders.map(provider => (
                  <tr key={provider.id}>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <div className="text-sm font-medium text-gray-900">
                        {provider.provider_key}
                      </div>
                      <div className="text-xs text-gray-500">
                        ID: {provider.id}
                      </div>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <span className="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-blue-100 text-blue-800">
                        {provider.provider_type}
                      </span>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <div
                        className="text-sm text-gray-900 max-w-xs truncate"
                        title={provider.api_base}
                      >
                        {provider.api_base}
                      </div>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <div className="text-sm text-gray-500">
                        {Object.keys(provider.model_mapping).length} mappings
                      </div>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <button
                        onClick={() => handleToggleStatus(provider)}
                        className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors ${
                          provider.is_enabled
                            ? 'bg-green-100 text-green-800 hover:bg-green-200'
                            : 'bg-red-100 text-red-800 hover:bg-red-200'
                        }`}
                      >
                        {provider.is_enabled ? 'Enabled' : 'Disabled'}
                      </button>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap text-right text-sm font-medium">
                      <div className="flex justify-end space-x-2">
                        <button
                          onClick={() => handleEdit(provider)}
                          className="text-blue-600 hover:text-blue-900"
                          title="Edit provider"
                        >
                          ✏️
                        </button>
                        <button
                          onClick={() => handleDelete(provider)}
                          className="text-red-600 hover:text-red-900"
                          title="Delete provider"
                        >
                          🗑️
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
};

export default Providers;
