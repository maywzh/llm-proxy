import React, { useState, useEffect, useCallback } from 'react';
import { useAuth } from '../hooks/useAuth';
import { generateApiKey } from '../api/client';
import type { MasterKey, MasterKeyFormData } from '../types';

const MasterKeys: React.FC = () => {
  const { apiClient } = useAuth();
  const [masterKeys, setMasterKeys] = useState<MasterKey[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState('');
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [editingKey, setEditingKey] = useState<MasterKey | null>(null);
  const [formData, setFormData] = useState<MasterKeyFormData>({
    id: '',
    key: '',
    name: '',
    allowed_models: [],
    rate_limit: null,
    is_enabled: true,
  });
  const [allowedModelsText, setAllowedModelsText] = useState('');

  const loadMasterKeys = useCallback(async () => {
    if (!apiClient) return;

    setLoading(true);
    setError(null);

    try {
      const response = await apiClient.listMasterKeys();
      setMasterKeys(response.keys);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to load master keys'
      );
    } finally {
      setLoading(false);
    }
  }, [apiClient]);

  // Load master keys when apiClient becomes available
  useEffect(() => {
    if (apiClient) {
      loadMasterKeys();
    }
  }, [apiClient, loadMasterKeys]);

  // Update allowed models text when form data changes
  useEffect(() => {
    setAllowedModelsText(formData.allowed_models.join('\n'));
  }, [formData.allowed_models]);

  const resetForm = () => {
    setFormData({
      id: '',
      key: '',
      name: '',
      allowed_models: [],
      rate_limit: null,
      is_enabled: true,
    });
    setAllowedModelsText('');
    setEditingKey(null);
    setShowCreateForm(false);
  };

  const handleCreate = () => {
    setShowCreateForm(true);
    resetForm();
    setFormData(prev => ({ ...prev, key: generateApiKey() }));
  };

  const handleEdit = (key: MasterKey) => {
    setEditingKey(key);
    setFormData({
      id: key.id,
      key: '', // Don't populate for security
      name: key.name,
      allowed_models: key.allowed_models,
      rate_limit: key.rate_limit,
      is_enabled: key.is_enabled,
    });
    setAllowedModelsText(key.allowed_models.join('\n'));
    setShowCreateForm(true);
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!apiClient) return;

    // Update allowed models from text
    const allowedModels = allowedModelsText
      .split('\n')
      .map(s => s.trim())
      .filter(s => s.length > 0);

    setLoading(true);
    setError(null);

    try {
      if (editingKey) {
        // Update existing master key
        const updateData = {
          name: formData.name,
          allowed_models: allowedModels,
          rate_limit: formData.rate_limit,
          is_enabled: formData.is_enabled,
        };

        await apiClient.updateMasterKey(editingKey.id, updateData);
      } else {
        // Create new master key
        await apiClient.createMasterKey({
          id: formData.id,
          key: formData.key,
          name: formData.name,
          allowed_models: allowedModels,
          rate_limit: formData.rate_limit,
        });
      }

      resetForm();
      await loadMasterKeys();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to save master key'
      );
    } finally {
      setLoading(false);
    }
  };

  const handleDelete = async (key: MasterKey) => {
    if (!apiClient) return;

    if (!confirm(`Are you sure you want to delete master key "${key.name}"?`)) {
      return;
    }

    setLoading(true);
    setError(null);

    try {
      await apiClient.deleteMasterKey(key.id);
      await loadMasterKeys();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to delete master key'
      );
    } finally {
      setLoading(false);
    }
  };

  const handleToggleStatus = async (key: MasterKey) => {
    if (!apiClient) return;

    try {
      await apiClient.setMasterKeyStatus(key.id, !key.is_enabled);
      await loadMasterKeys();
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : 'Failed to update master key status'
      );
    }
  };

  const handleRotate = async (key: MasterKey) => {
    if (!apiClient) return;

    if (
      !confirm(
        `Are you sure you want to rotate the key for "${key.name}"? The old key will be invalidated.`
      )
    ) {
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const response = await apiClient.rotateMasterKey(key.id);
      await loadMasterKeys();
      alert(
        `New key generated: ${response.new_key}\n\nSave this key securely. It will not be shown again.`
      );
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to rotate master key'
      );
    } finally {
      setLoading(false);
    }
  };

  // Filtered master keys based on search
  const filteredKeys = masterKeys.filter(
    key =>
      key.id.toLowerCase().includes(searchTerm.toLowerCase()) ||
      key.name.toLowerCase().includes(searchTerm.toLowerCase())
  );

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center">
        <div>
          <h1 className="text-2xl font-bold text-gray-900">Master Keys</h1>
          <p className="text-gray-600">
            Manage API keys for client authentication
          </p>
        </div>
        <button onClick={handleCreate} className="btn btn-primary">
          + Add Master Key
        </button>
      </div>

      {/* Search */}
      <div className="max-w-md">
        <input
          type="text"
          placeholder="Search master keys..."
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
            {editingKey ? 'Edit Master Key' : 'Create New Master Key'}
          </h2>

          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div>
                <label
                  htmlFor="id"
                  className="block text-sm font-medium text-gray-700"
                >
                  Key ID
                </label>
                <input
                  id="id"
                  type="text"
                  value={formData.id}
                  onChange={e =>
                    setFormData(prev => ({ ...prev, id: e.target.value }))
                  }
                  disabled={!!editingKey}
                  className="input"
                  placeholder="e.g., key-1"
                  required
                />
              </div>

              <div>
                <label
                  htmlFor="name"
                  className="block text-sm font-medium text-gray-700"
                >
                  Name
                </label>
                <input
                  id="name"
                  type="text"
                  value={formData.name}
                  onChange={e =>
                    setFormData(prev => ({ ...prev, name: e.target.value }))
                  }
                  className="input"
                  placeholder="e.g., Production Key"
                  required
                />
              </div>
            </div>

            {!editingKey && (
              <div>
                <label
                  htmlFor="key"
                  className="block text-sm font-medium text-gray-700"
                >
                  API Key
                </label>
                <div className="flex space-x-2">
                  <input
                    id="key"
                    type="text"
                    value={formData.key}
                    onChange={e =>
                      setFormData(prev => ({ ...prev, key: e.target.value }))
                    }
                    className="input flex-1 font-mono text-sm"
                    required
                  />
                  <button
                    type="button"
                    onClick={() =>
                      setFormData(prev => ({ ...prev, key: generateApiKey() }))
                    }
                    className="btn btn-secondary"
                    title="Generate new key"
                  >
                    🎲
                  </button>
                </div>
              </div>
            )}

            <div>
              <label
                htmlFor="allowed_models"
                className="block text-sm font-medium text-gray-700"
              >
                Allowed Models (optional)
              </label>
              <textarea
                id="allowed_models"
                value={allowedModelsText}
                onChange={e => setAllowedModelsText(e.target.value)}
                className="input"
                rows={3}
                placeholder="gpt-4&#10;gpt-3.5-turbo&#10;claude-3-sonnet"
              />
              <p className="text-xs text-gray-500 mt-1">
                One model per line. Leave empty to allow all models.
              </p>
            </div>

            <div>
              <label
                htmlFor="rate_limit"
                className="block text-sm font-medium text-gray-700"
              >
                Rate Limit (requests per second)
              </label>
              <input
                id="rate_limit"
                type="number"
                value={formData.rate_limit || ''}
                onChange={e =>
                  setFormData(prev => ({
                    ...prev,
                    rate_limit: e.target.value
                      ? parseInt(e.target.value)
                      : null,
                  }))
                }
                className="input"
                placeholder="100"
                min="1"
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
                className="h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300 rounded"
              />
              <label
                htmlFor="is_enabled"
                className="ml-2 block text-sm text-gray-900"
              >
                Enable this master key
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
                {editingKey ? 'Update' : 'Create'} Master Key
              </button>
            </div>
          </form>
        </div>
      )}

      {/* Master Keys List */}
      <div className="card">
        <div className="flex justify-between items-center mb-4">
          <h2 className="text-lg font-semibold">
            Master Keys ({filteredKeys.length})
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

        {filteredKeys.length === 0 ? (
          <div className="text-center py-8 text-gray-500">
            {searchTerm
              ? 'No master keys match your search.'
              : 'No master keys configured yet.'}
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="min-w-full divide-y divide-gray-200">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Name
                  </th>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Key Preview
                  </th>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Models
                  </th>
                  <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Rate Limit
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
                {filteredKeys.map(key => (
                  <tr key={key.id}>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <div className="text-sm font-medium text-gray-900">
                        {key.name}
                      </div>
                      <div className="text-sm text-gray-500">{key.id}</div>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <code className="text-sm bg-gray-100 px-2 py-1 rounded">
                        {key.key_preview}
                      </code>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <div className="text-sm text-gray-500">
                        {key.allowed_models.length === 0
                          ? 'All models'
                          : `${key.allowed_models.length} models`}
                      </div>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <div className="text-sm text-gray-500">
                        {key.rate_limit ? `${key.rate_limit}/s` : 'No limit'}
                      </div>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap">
                      <button
                        onClick={() => handleToggleStatus(key)}
                        className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors ${
                          key.is_enabled
                            ? 'bg-green-100 text-green-800 hover:bg-green-200'
                            : 'bg-red-100 text-red-800 hover:bg-red-200'
                        }`}
                      >
                        {key.is_enabled ? 'Enabled' : 'Disabled'}
                      </button>
                    </td>
                    <td className="px-6 py-4 whitespace-nowrap text-right text-sm font-medium">
                      <div className="flex justify-end space-x-2">
                        <button
                          onClick={() => handleEdit(key)}
                          className="text-blue-600 hover:text-blue-900"
                          title="Edit master key"
                        >
                          ✏️
                        </button>
                        <button
                          onClick={() => handleRotate(key)}
                          className="text-yellow-600 hover:text-yellow-900"
                          title="Rotate key"
                        >
                          🔄
                        </button>
                        <button
                          onClick={() => handleDelete(key)}
                          className="text-red-600 hover:text-red-900"
                          title="Delete master key"
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

export default MasterKeys;
