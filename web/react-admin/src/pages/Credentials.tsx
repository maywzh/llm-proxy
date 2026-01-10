import React, { useState, useEffect, useCallback } from 'react';
import { useAuth } from '../hooks/useAuth';
import { generateApiKey } from '../api/client';
import {
  Plus,
  Pencil,
  Trash2,
  Loader2,
  AlertCircle,
  X,
  Check,
  Shuffle,
  RefreshCw,
} from 'lucide-react';
import type { Credential, CredentialFormData } from '../types';

const Credentials: React.FC = () => {
  const { apiClient } = useAuth();
  const [credentials, setCredentials] = useState<Credential[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState('');
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [editingCredential, setEditingCredential] = useState<Credential | null>(
    null
  );
  const [deleteConfirm, setDeleteConfirm] = useState<Credential | null>(null);
  const [rotateConfirm, setRotateConfirm] = useState<Credential | null>(null);
  const [newRotatedKey, setNewRotatedKey] = useState<string | null>(null);
  const [formData, setFormData] = useState<CredentialFormData>({
    key: '',
    name: '',
    allowed_models: [],
    rate_limit: null,
    is_enabled: true,
  });
  const [allowedModelsText, setAllowedModelsText] = useState('');

  const loadCredentials = useCallback(async () => {
    if (!apiClient) return;

    setLoading(true);
    setError(null);

    try {
      const response = await apiClient.listCredentials();
      setCredentials(response.credentials);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to load credentials'
      );
    } finally {
      setLoading(false);
    }
  }, [apiClient]);

  // Load credentials when apiClient becomes available
  useEffect(() => {
    if (apiClient) {
      loadCredentials();
    }
  }, [apiClient, loadCredentials]);

  // Update allowed models text when form data changes
  useEffect(() => {
    setAllowedModelsText(formData.allowed_models.join('\n'));
  }, [formData.allowed_models]);

  const resetForm = () => {
    setFormData({
      key: '',
      name: '',
      allowed_models: [],
      rate_limit: null,
      is_enabled: true,
    });
    setAllowedModelsText('');
    setEditingCredential(null);
    setShowCreateForm(false);
  };

  const handleCreate = () => {
    resetForm();
    setShowCreateForm(true);
    setFormData(prev => ({ ...prev, key: generateApiKey() }));
  };

  const handleEdit = (credential: Credential) => {
    setEditingCredential(credential);
    setFormData({
      key: '', // Don't populate for security
      name: credential.name,
      allowed_models: credential.allowed_models,
      rate_limit: credential.rate_limit,
      is_enabled: credential.is_enabled,
    });
    setAllowedModelsText(credential.allowed_models.join('\n'));
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
      if (editingCredential) {
        // Update existing credential
        const updateData = {
          name: formData.name,
          allowed_models: allowedModels,
          rate_limit: formData.rate_limit,
          is_enabled: formData.is_enabled,
        };

        await apiClient.updateCredential(editingCredential.id, updateData);
      } else {
        // Create new credential
        await apiClient.createCredential({
          key: formData.key,
          name: formData.name,
          allowed_models: allowedModels,
          rate_limit: formData.rate_limit,
        });
      }

      resetForm();
      await loadCredentials();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to save credential'
      );
    } finally {
      setLoading(false);
    }
  };

  const handleDelete = async (credential: Credential) => {
    if (!apiClient) return;

    setLoading(true);
    setError(null);

    try {
      await apiClient.deleteCredential(credential.id);
      setDeleteConfirm(null);
      await loadCredentials();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to delete credential'
      );
    } finally {
      setLoading(false);
    }
  };

  const handleToggleStatus = async (credential: Credential) => {
    if (!apiClient) return;

    try {
      await apiClient.setCredentialStatus(
        credential.id,
        !credential.is_enabled
      );
      await loadCredentials();
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : 'Failed to update credential status'
      );
    }
  };

  const handleRotate = async (credential: Credential) => {
    if (!apiClient) return;

    setLoading(true);
    setError(null);

    try {
      const response = await apiClient.rotateCredential(credential.id);
      setNewRotatedKey(response.new_key);
      setRotateConfirm(null);
      await loadCredentials();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to rotate credential'
      );
    } finally {
      setLoading(false);
    }
  };

  // Filtered credentials based on search
  const filteredCredentials = credentials.filter(
    credential =>
      credential.name.toLowerCase().includes(searchTerm.toLowerCase()) ||
      credential.key_preview.toLowerCase().includes(searchTerm.toLowerCase())
  );

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex justify-between items-center">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Credentials
          </h1>
          <p className="text-gray-600 dark:text-gray-400">
            Manage API credentials for client authentication
          </p>
        </div>
        <button
          onClick={handleCreate}
          className="btn btn-primary flex items-center space-x-2"
        >
          <Plus className="w-5 h-5" />
          <span>Add Credential</span>
        </button>
      </div>

      {/* Search */}
      <div className="max-w-md">
        <input
          type="text"
          placeholder="Search credentials..."
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
        <div className="modal-overlay" onClick={resetForm}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                {editingCredential ? 'Edit Credential' : 'Add Credential'}
              </h3>
              <button onClick={resetForm} className="btn-icon">
                <X className="w-5 h-5" />
              </button>
            </div>

            <form onSubmit={handleSubmit} className="modal-body space-y-4">
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label htmlFor="name" className="label">
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
                    placeholder="e.g., Production Credential"
                    required
                  />
                </div>

                <div>
                  <label htmlFor="rate_limit" className="label">
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
              </div>

              {!editingCredential && (
                <div>
                  <label htmlFor="key" className="label">
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
                        setFormData(prev => ({
                          ...prev,
                          key: generateApiKey(),
                        }))
                      }
                      className="btn btn-secondary flex items-center space-x-2"
                      title="Generate new key"
                    >
                      <Shuffle className="w-4 h-4" />
                    </button>
                  </div>
                </div>
              )}

              <div>
                <label htmlFor="allowed_models" className="label">
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
                <p className="helper-text">
                  One model per line. Leave empty to allow all models.
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
                  className="h-4 w-4 text-primary-600 focus:ring-primary-500 border-gray-300 rounded"
                />
                <label
                  htmlFor="is_enabled"
                  className="ml-2 block text-sm text-gray-900 dark:text-gray-100"
                >
                  Enable this credential
                </label>
              </div>
            </form>

            <div className="modal-footer">
              <button
                type="button"
                onClick={resetForm}
                className="btn btn-secondary"
              >
                Cancel
              </button>
              <button
                type="submit"
                onClick={handleSubmit}
                className="btn btn-primary flex items-center space-x-2"
                disabled={loading}
              >
                {loading && <Loader2 className="w-4 h-4 animate-spin" />}
                <span>
                  {editingCredential ? 'Update' : 'Create'} Credential
                </span>
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Credentials List */}
      <div className="card">
        <div className="card-header flex justify-between items-center">
          <h2 className="card-title">
            Credentials ({filteredCredentials.length})
          </h2>
          {loading && (
            <div className="flex items-center text-gray-500 dark:text-gray-400">
              <Loader2 className="w-5 h-5 animate-spin mr-2" />
              <span className="text-sm">Loading...</span>
            </div>
          )}
        </div>

        <div className="card-body p-0">
          {filteredCredentials.length === 0 ? (
            <div className="text-center py-12 text-gray-500 dark:text-gray-400">
              {searchTerm
                ? 'No credentials match your search.'
                : 'No credentials configured yet.'}
            </div>
          ) : (
            <div className="table-container">
              <table className="table">
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Key Preview</th>
                    <th>Models</th>
                    <th>Rate Limit</th>
                    <th>Status</th>
                    <th className="text-right">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {filteredCredentials.map(credential => (
                    <tr key={credential.id}>
                      <td>
                        <div className="text-sm font-medium text-gray-900 dark:text-gray-100">
                          {credential.name}
                        </div>
                        <div className="text-xs text-gray-500 dark:text-gray-400">
                          ID: {credential.id}
                        </div>
                      </td>
                      <td>
                        <code className="text-sm bg-gray-100 dark:bg-gray-700 px-2 py-1 rounded font-mono">
                          {credential.key_preview}
                        </code>
                      </td>
                      <td>
                        <div className="text-sm text-gray-500 dark:text-gray-400">
                          {credential.allowed_models.length === 0
                            ? 'All models'
                            : `${credential.allowed_models.length} models`}
                        </div>
                      </td>
                      <td>
                        <div className="text-sm text-gray-500 dark:text-gray-400">
                          {credential.rate_limit
                            ? `${credential.rate_limit}/s`
                            : 'No limit'}
                        </div>
                      </td>
                      <td>
                        <button
                          onClick={() => handleToggleStatus(credential)}
                          className={`badge transition-colors ${
                            credential.is_enabled
                              ? 'badge-success hover:opacity-80'
                              : 'badge-danger hover:opacity-80'
                          }`}
                        >
                          {credential.is_enabled ? (
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
                            onClick={() => handleEdit(credential)}
                            className="btn-icon text-primary-600 hover:text-primary-900"
                            title="Edit credential"
                          >
                            <Pencil className="w-4 h-4" />
                          </button>
                          <button
                            onClick={() => setRotateConfirm(credential)}
                            className="btn-icon text-yellow-600 hover:text-yellow-900"
                            title="Rotate key"
                          >
                            <RefreshCw className="w-4 h-4" />
                          </button>
                          <button
                            onClick={() => setDeleteConfirm(credential)}
                            className="btn-icon text-red-600 hover:text-red-900"
                            title="Delete credential"
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
                Delete Credential
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
                Are you sure you want to delete credential{' '}
                <strong>{deleteConfirm.name}</strong>? This action cannot be
                undone.
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

      {/* Rotate Confirmation Modal */}
      {rotateConfirm && (
        <div className="modal-overlay" onClick={() => setRotateConfirm(null)}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                Rotate API Key
              </h3>
              <button
                onClick={() => setRotateConfirm(null)}
                className="btn-icon"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="modal-body">
              <p className="text-sm text-gray-600 dark:text-gray-400">
                Are you sure you want to rotate the key for{' '}
                <strong>{rotateConfirm.name}</strong>? The old key will be
                invalidated immediately.
              </p>
            </div>
            <div className="modal-footer">
              <button
                onClick={() => setRotateConfirm(null)}
                className="btn btn-secondary"
              >
                Cancel
              </button>
              <button
                onClick={() => handleRotate(rotateConfirm)}
                className="btn btn-primary flex items-center space-x-2"
                disabled={loading}
              >
                {loading && <Loader2 className="w-4 h-4 animate-spin" />}
                <span>Rotate Key</span>
              </button>
            </div>
          </div>
        </div>
      )}

      {/* New Key Display Modal */}
      {newRotatedKey && (
        <div className="modal-overlay" onClick={() => setNewRotatedKey(null)}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                New API Key Generated
              </h3>
              <button
                onClick={() => setNewRotatedKey(null)}
                className="btn-icon"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="modal-body">
              <div className="alert-success mb-4">
                <p className="text-sm text-green-700 font-medium">
                  Key rotated successfully!
                </p>
              </div>
              <p className="text-sm text-gray-600 dark:text-gray-400 mb-3">
                Save this key securely. It will not be shown again.
              </p>
              <div className="bg-gray-50 dark:bg-gray-700 rounded-lg p-4 border border-gray-200 dark:border-gray-600">
                <code className="text-sm font-mono break-all">
                  {newRotatedKey}
                </code>
              </div>
            </div>
            <div className="modal-footer">
              <button
                onClick={() => {
                  navigator.clipboard.writeText(newRotatedKey);
                }}
                className="btn btn-secondary"
              >
                Copy to Clipboard
              </button>
              <button
                onClick={() => setNewRotatedKey(null)}
                className="btn btn-primary"
              >
                Close
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default Credentials;
