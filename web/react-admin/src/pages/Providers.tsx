import React, { useState, useEffect, useCallback } from 'react';
import { useAuth } from '../hooks/useAuth';
import { useDebounce } from '../hooks/useDebounce';
import { generateApiKey } from '../api/client';
import JsonEditor from '../components/JsonEditor';
import { TableSkeleton } from '../components/Skeleton';
import {
  Plus,
  Pencil,
  Trash2,
  Loader2,
  AlertCircle,
  X,
  Check,
  Shuffle,
  Inbox,
} from 'lucide-react';
import type {
  Provider,
  ProviderFormData,
  ProviderUpdate,
  ProviderCreate,
} from '../types';

const Providers: React.FC = () => {
  const { apiClient } = useAuth();
  const [providers, setProviders] = useState<Provider[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState('');
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<Provider | null>(null);
  const [isModalClosing, setIsModalClosing] = useState(false);
  const [modelMappingError, setModelMappingError] = useState<string | null>(
    null
  );
  const [formData, setFormData] = useState<ProviderFormData>({
    provider_key: '',
    provider_type: 'openai',
    api_base: '',
    api_key: '',
    model_mapping: {},
    is_enabled: true,
    gcp_project: '',
    gcp_location: '',
    gcp_publisher: '',
  });

  const debouncedSearch = useDebounce(searchTerm, 300);

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

  const resetForm = () => {
    setFormData({
      provider_key: '',
      provider_type: 'openai',
      api_base: '',
      api_key: '',
      model_mapping: {},
      is_enabled: true,
      gcp_project: '',
      gcp_location: '',
      gcp_publisher: '',
    });
    setModelMappingError(null);
    setEditingProvider(null);
    setShowCreateForm(false);
    setIsModalClosing(false);
  };

  const handleCloseModal = () => {
    setIsModalClosing(true);
    setTimeout(() => {
      resetForm();
    }, 150);
  };

  const handleCreate = () => {
    resetForm();
    setShowCreateForm(true);
  };

  const handleEdit = (provider: Provider) => {
    setEditingProvider(provider);
    setModelMappingError(null);
    setFormData({
      provider_key: provider.provider_key,
      provider_type: provider.provider_type,
      api_base: provider.api_base,
      api_key: '', // Don't populate existing key for security
      model_mapping: provider.model_mapping,
      is_enabled: provider.is_enabled,
      gcp_project: (provider.provider_params?.gcp_project as string) || '',
      gcp_location: (provider.provider_params?.gcp_location as string) || '',
      gcp_publisher: (provider.provider_params?.gcp_publisher as string) || '',
    });
    setShowCreateForm(true);
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!apiClient) return;
    if (modelMappingError) return;

    // Validate gcp_project is required when provider_type is gcp-vertex
    if (
      formData.provider_type === 'gcp-vertex' &&
      !formData.gcp_project.trim()
    ) {
      setError('GCP Project ID is required for GCP Vertex provider');
      return;
    }

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

        // Include provider_params for GCP Vertex
        if (formData.provider_type === 'gcp-vertex') {
          updateData.provider_params = {
            gcp_project: formData.gcp_project,
            gcp_location: formData.gcp_location.trim() || 'us-central1',
            gcp_publisher: formData.gcp_publisher.trim() || 'anthropic',
          };
        } else {
          updateData.provider_params = {};
        }

        await apiClient.updateProvider(editingProvider.id, updateData);
      } else {
        // Create new provider
        const createData: ProviderCreate = {
          provider_key: formData.provider_key,
          provider_type: formData.provider_type,
          api_base: formData.api_base,
          api_key: formData.api_key,
          model_mapping: formData.model_mapping,
        };

        // Include provider_params for GCP Vertex
        if (formData.provider_type === 'gcp-vertex') {
          createData.provider_params = {
            gcp_project: formData.gcp_project,
            gcp_location: formData.gcp_location.trim() || 'us-central1',
            gcp_publisher: formData.gcp_publisher.trim() || 'anthropic',
          };
        } else {
          createData.provider_params = {};
        }

        await apiClient.createProvider(createData);
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

    setLoading(true);
    setError(null);

    try {
      await apiClient.deleteProvider(provider.id);
      setDeleteConfirm(null);
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

  const generateRandomKey = () => {
    setFormData(prev => ({ ...prev, api_key: generateApiKey() }));
  };

  // Filtered providers based on search
  const filteredProviders = providers.filter(
    provider =>
      provider.provider_key
        .toLowerCase()
        .includes(debouncedSearch.toLowerCase()) ||
      provider.provider_type
        .toLowerCase()
        .includes(debouncedSearch.toLowerCase()) ||
      provider.api_base.toLowerCase().includes(debouncedSearch.toLowerCase())
  );

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Providers
          </h1>
          <p className="text-gray-600 dark:text-gray-400">
            Manage your LLM provider configurations
          </p>
        </div>
        <button
          onClick={handleCreate}
          className="btn btn-primary flex items-center space-x-2"
        >
          <Plus className="w-5 h-5" />
          <span>Add Provider</span>
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
        <div className="alert-error">
          <div className="flex">
            <div className="shrink-0">
              <AlertCircle className="h-5 w-5 text-red-400" />
            </div>
            <div className="ml-3">
              <p className="text-sm text-red-700">{error}</p>
            </div>
            <div className="ml-auto pl-3">
              <button
                onClick={() => setError(null)}
                className="text-red-400 hover:text-red-600"
              >
                <X className="h-5 w-5" />
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Create/Edit Form Modal */}
      {showCreateForm && (
        <div className="modal-overlay" onClick={handleCloseModal}>
          <div
            className={`modal ${isModalClosing ? 'animate-modal-exit' : 'animate-modal-enter'}`}
            onClick={e => e.stopPropagation()}
          >
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                {editingProvider ? 'Edit Provider' : 'Add Provider'}
              </h3>
              <button onClick={handleCloseModal} className="btn-icon">
                <X className="w-5 h-5" />
              </button>
            </div>

            <form onSubmit={handleSubmit} className="modal-body space-y-4">
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label htmlFor="provider_key" className="label">
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
                  <label htmlFor="provider_type" className="label">
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
                    <option value="gcp-vertex">GCP Vertex AI</option>
                    <option value="custom">Custom</option>
                  </select>
                </div>
              </div>

              <div>
                <label htmlFor="api_base" className="label">
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
                  placeholder={
                    formData.provider_type === 'gcp-vertex'
                      ? 'https://us-central1-aiplatform.googleapis.com'
                      : 'https://api.openai.com/v1'
                  }
                  required
                />
              </div>

              {/* GCP Vertex AI specific fields */}
              {formData.provider_type === 'gcp-vertex' && (
                <div className="grid grid-cols-1 md:grid-cols-3 gap-4 p-4 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
                  <div>
                    <label htmlFor="gcp_project" className="label">
                      GCP Project ID <span className="text-red-500">*</span>
                    </label>
                    <input
                      id="gcp_project"
                      type="text"
                      value={formData.gcp_project}
                      onChange={e =>
                        setFormData(prev => ({
                          ...prev,
                          gcp_project: e.target.value,
                        }))
                      }
                      className="input"
                      placeholder="my-project-id"
                      required
                    />
                    <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                      Your GCP project identifier
                    </p>
                  </div>

                  <div>
                    <label htmlFor="gcp_location" className="label">
                      GCP Location
                    </label>
                    <input
                      id="gcp_location"
                      type="text"
                      value={formData.gcp_location}
                      onChange={e =>
                        setFormData(prev => ({
                          ...prev,
                          gcp_location: e.target.value,
                        }))
                      }
                      className="input"
                      placeholder="us-central1"
                    />
                    <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                      Default: us-central1
                    </p>
                  </div>

                  <div>
                    <label htmlFor="gcp_publisher" className="label">
                      GCP Publisher
                    </label>
                    <input
                      id="gcp_publisher"
                      type="text"
                      value={formData.gcp_publisher}
                      onChange={e =>
                        setFormData(prev => ({
                          ...prev,
                          gcp_publisher: e.target.value,
                        }))
                      }
                      className="input"
                      placeholder="anthropic"
                    />
                    <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                      Default: anthropic
                    </p>
                  </div>
                </div>
              )}

              <div>
                <label htmlFor="api_key" className="label">
                  API Key{' '}
                  {editingProvider ? '(leave empty to keep current)' : ''}
                </label>
                <div className="flex space-x-2">
                  <input
                    id="api_key"
                    type="password"
                    value={formData.api_key}
                    onChange={e =>
                      setFormData(prev => ({
                        ...prev,
                        api_key: e.target.value,
                      }))
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
                    className="btn btn-secondary flex items-center space-x-2"
                    title="Generate random key"
                  >
                    <Shuffle className="w-4 h-4" />
                  </button>
                </div>
              </div>

              <div>
                <JsonEditor
                  id="model_mapping"
                  label="Model Mapping (optional)"
                  value={formData.model_mapping}
                  onChange={next =>
                    setFormData(prev => ({ ...prev, model_mapping: next }))
                  }
                  onErrorChange={setModelMappingError}
                  rows={6}
                  placeholder='{\n  "gpt-4": "gpt-4-turbo",\n  "gpt-3.5-turbo": "gpt-3.5-turbo-16k"\n}'
                  helperText='JSON object in format: {"source_model":"target_model"}'
                />
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
                  className="h-4 w-4 text-primary-600 focus:ring-primary-500 border-gray-300 rounded"
                />
                <label
                  htmlFor="is_enabled"
                  className="ml-2 block text-sm text-gray-900 dark:text-gray-100"
                >
                  Enable this provider
                </label>
              </div>
            </form>

            <div className="modal-footer">
              <button
                type="button"
                onClick={handleCloseModal}
                className="btn btn-secondary"
              >
                Cancel
              </button>
              <button
                type="submit"
                onClick={handleSubmit}
                className="btn btn-primary flex items-center space-x-2"
                disabled={loading || !!modelMappingError}
              >
                {loading && <Loader2 className="w-4 h-4 animate-spin" />}
                <span>{editingProvider ? 'Update' : 'Create'} Provider</span>
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Providers List */}
      <div className="card">
        <div className="card-header flex justify-between items-center">
          <h2 className="card-title">Providers ({filteredProviders.length})</h2>
          {loading && (
            <div className="flex items-center text-gray-500 dark:text-gray-400">
              <Loader2 className="w-5 h-5 animate-spin mr-2" />
              <span className="text-sm">Loading...</span>
            </div>
          )}
        </div>

        <div className="card-body p-0">
          {loading && providers.length === 0 ? (
            <TableSkeleton rows={5} columns={6} />
          ) : filteredProviders.length === 0 ? (
            <div className="text-center py-12 text-gray-500 dark:text-gray-400">
              <Inbox className="w-16 h-16 mx-auto mb-4 text-gray-300 dark:text-gray-600" />
              <p className="text-lg font-medium mb-1">
                {searchTerm ? 'No results found' : 'No providers yet'}
              </p>
              <p className="text-sm">
                {searchTerm
                  ? 'Try adjusting your search terms'
                  : 'Click "Add Provider" to get started'}
              </p>
            </div>
          ) : (
            <div className="table-container">
              <table className="table">
                <thead>
                  <tr>
                    <th>Provider</th>
                    <th>Type</th>
                    <th>API Base</th>
                    <th>Models</th>
                    <th>Status</th>
                    <th className="text-right">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredProviders.map(provider => (
                    <tr key={provider.id}>
                      <td>
                        <div className="text-sm font-medium text-gray-900 dark:text-gray-100">
                          {provider.provider_key}
                        </div>
                        <div className="text-xs text-gray-500 dark:text-gray-400">
                          ID: {provider.id}
                        </div>
                      </td>
                      <td>
                        <span className="badge badge-info">
                          {provider.provider_type}
                        </span>
                      </td>
                      <td>
                        <div
                          className="text-sm text-gray-900 dark:text-gray-100 max-w-xs truncate"
                          title={provider.api_base}
                        >
                          {provider.api_base}
                        </div>
                      </td>
                      <td>
                        <div className="text-sm text-gray-500 dark:text-gray-400">
                          {Object.keys(provider.model_mapping).length} mappings
                        </div>
                      </td>
                      <td>
                        <button
                          onClick={() => handleToggleStatus(provider)}
                          className={`badge transition-colors ${
                            provider.is_enabled
                              ? 'badge-success hover:opacity-80'
                              : 'badge-danger hover:opacity-80'
                          }`}
                        >
                          {provider.is_enabled ? (
                            <>
                              <Check className="w-3 h-3 mr-1" />
                              Enabled
                            </>
                          ) : (
                            <>
                              <X className="w-3 h-3 mr-1" />
                              Disabled
                            </>
                          )}
                        </button>
                      </td>
                      <td>
                        <div className="flex justify-end space-x-2">
                          <button
                            onClick={() => handleEdit(provider)}
                            className="btn-icon text-primary-600 hover:text-primary-900"
                            title="Edit provider"
                          >
                            <Pencil className="w-4 h-4" />
                          </button>
                          <button
                            onClick={() => setDeleteConfirm(provider)}
                            className="btn-icon text-red-600 hover:text-red-900"
                            title="Delete provider"
                          >
                            <Trash2 className="w-4 h-4" />
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

      {/* Delete Confirmation Modal */}
      {deleteConfirm && (
        <div className="modal-overlay" onClick={() => setDeleteConfirm(null)}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                Delete Provider
              </h3>
              <button
                onClick={() => setDeleteConfirm(null)}
                className="btn-icon"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="modal-body">
              <p className="text-sm text-gray-600 dark:text-gray-400">
                Are you sure you want to delete provider{' '}
                <strong>{deleteConfirm.provider_key}</strong>? This action
                cannot be undone.
              </p>
            </div>
            <div className="modal-footer">
              <button
                onClick={() => setDeleteConfirm(null)}
                className="btn btn-secondary"
              >
                Cancel
              </button>
              <button
                onClick={() => handleDelete(deleteConfirm)}
                className="btn btn-danger flex items-center space-x-2"
                disabled={loading}
              >
                {loading && <Loader2 className="w-4 h-4 animate-spin" />}
                <span>Delete</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default Providers;
