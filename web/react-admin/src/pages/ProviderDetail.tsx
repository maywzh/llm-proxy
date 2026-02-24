import React, { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate, Link } from 'react-router-dom';
import { useAuth } from '../hooks/useAuth';
import { generateApiKey } from '../api/client';
import JsonEditor from '../components/JsonEditor';
import LuaEditor from '../components/LuaEditor';
import {
  Loader2,
  AlertCircle,
  Shuffle,
  Trash2,
  X,
  Plus,
  Minus,
  Eye,
  EyeOff,
  ChevronDown,
  ChevronRight,
  ArrowLeft,
} from 'lucide-react';
import type { Provider, ProviderFormData, ProviderUpdate } from '../types';

const Section: React.FC<{
  title: string;
  collapsible?: boolean;
  defaultOpen?: boolean;
  badge?: string;
  children: React.ReactNode;
}> = ({ title, collapsible = false, defaultOpen = true, badge, children }) => {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className="card">
      <button
        type="button"
        className={`card-header flex items-center justify-between w-full ${collapsible ? 'cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors' : ''}`}
        onClick={() => collapsible && setOpen(v => !v)}
        disabled={!collapsible}
      >
        <div className="flex items-center gap-2">
          {collapsible &&
            (open ? (
              <ChevronDown className="w-4 h-4 text-gray-400" />
            ) : (
              <ChevronRight className="w-4 h-4 text-gray-400" />
            ))}
          <h2 className="card-title">{title}</h2>
          {badge && (
            <span className="text-xs px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400">
              {badge}
            </span>
          )}
        </div>
      </button>
      {open && <div className="card-body space-y-4">{children}</div>}
    </div>
  );
};

const ProviderDetail: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { apiClient } = useAuth();

  const [provider, setProvider] = useState<Provider | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notFound, setNotFound] = useState(false);
  const [modelMappingError, setModelMappingError] = useState<string | null>(
    null
  );
  const [deleteConfirm, setDeleteConfirm] = useState(false);
  const [showApiKey, setShowApiKey] = useState(false);

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
    gcp_blocking_action: '',
    gcp_streaming_action: '',
    custom_headers: {},
    lua_script: '',
  });

  const loadProvider = useCallback(async () => {
    if (!apiClient || !id) return;

    setLoading(true);
    setError(null);

    try {
      const data = await apiClient.getProvider(Number(id));
      setProvider(data);
      setFormData({
        provider_key: data.provider_key,
        provider_type: data.provider_type,
        api_base: data.api_base,
        api_key: '',
        model_mapping: data.model_mapping,
        is_enabled: data.is_enabled,
        gcp_project: (data.provider_params?.gcp_project as string) || '',
        gcp_location: (data.provider_params?.gcp_location as string) || '',
        gcp_publisher: (data.provider_params?.gcp_publisher as string) || '',
        gcp_blocking_action:
          (data.provider_params?.gcp_vertex_actions as Record<string, string>)
            ?.blocking || '',
        gcp_streaming_action:
          (data.provider_params?.gcp_vertex_actions as Record<string, string>)
            ?.streaming || '',
        custom_headers:
          (data.provider_params?.custom_headers as Record<string, string>) ||
          {},
        lua_script: data.lua_script ?? '',
      });
    } catch {
      setNotFound(true);
    } finally {
      setLoading(false);
    }
  }, [apiClient, id]);

  useEffect(() => {
    loadProvider();
  }, [loadProvider]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!apiClient || !provider) return;
    if (modelMappingError) return;

    if (
      (formData.provider_type === 'gcp-vertex' ||
        formData.provider_type === 'gemini') &&
      !formData.gcp_project.trim()
    ) {
      setError('GCP Project ID is required for GCP Vertex / Gemini provider');
      return;
    }

    setSaving(true);
    setError(null);

    try {
      const updateData: ProviderUpdate = {
        provider_type: formData.provider_type,
        api_base: formData.api_base,
        model_mapping: formData.model_mapping,
        is_enabled: formData.is_enabled,
        lua_script: formData.lua_script || null,
      };

      if (formData.api_key.trim()) {
        updateData.api_key = formData.api_key;
      }

      if (
        formData.provider_type === 'gcp-vertex' ||
        formData.provider_type === 'gemini'
      ) {
        const params: Record<string, unknown> = {
          gcp_project: formData.gcp_project,
          gcp_location: formData.gcp_location.trim() || 'us-central1',
          gcp_publisher:
            formData.gcp_publisher.trim() ||
            (formData.provider_type === 'gemini' ? 'google' : 'anthropic'),
        };
        if (
          formData.provider_type === 'gcp-vertex' &&
          (formData.gcp_blocking_action.trim() ||
            formData.gcp_streaming_action.trim())
        ) {
          params.gcp_vertex_actions = {
            blocking: formData.gcp_blocking_action.trim() || 'rawPredict',
            streaming:
              formData.gcp_streaming_action.trim() || 'streamRawPredict',
          };
        }
        if (Object.keys(formData.custom_headers).length > 0) {
          params.custom_headers = formData.custom_headers;
        }
        updateData.provider_params = params;
      } else {
        const params: Record<string, unknown> = {};
        if (Object.keys(formData.custom_headers).length > 0) {
          params.custom_headers = formData.custom_headers;
        }
        updateData.provider_params = params;
      }

      await apiClient.updateProvider(provider.id, updateData);
      navigate('/providers');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save provider');
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!apiClient || !provider) return;

    setSaving(true);
    setError(null);

    try {
      await apiClient.deleteProvider(provider.id);
      navigate('/providers');
    } catch (err) {
      setError(
        err instanceof Error ? err.message : 'Failed to delete provider'
      );
    } finally {
      setSaving(false);
    }
  };

  const generateRandomKey = () => {
    setFormData(prev => ({ ...prev, api_key: generateApiKey() }));
  };

  const addCustomHeader = () => {
    setFormData(prev => ({
      ...prev,
      custom_headers: { ...prev.custom_headers, '': '' },
    }));
  };

  const removeCustomHeader = (key: string) => {
    setFormData(prev => {
      const next = { ...prev.custom_headers };
      delete next[key];
      return { ...prev, custom_headers: next };
    });
  };

  const updateCustomHeaderKey = (oldKey: string, newKey: string) => {
    setFormData(prev => {
      const entries = Object.entries(prev.custom_headers);
      const updated: Record<string, string> = {};
      for (const [k, v] of entries) {
        updated[k === oldKey ? newKey : k] = v;
      }
      return { ...prev, custom_headers: updated };
    });
  };

  const updateCustomHeaderValue = (key: string, value: string) => {
    setFormData(prev => ({
      ...prev,
      custom_headers: { ...prev.custom_headers, [key]: value },
    }));
  };

  const isGcpType =
    formData.provider_type === 'gcp-vertex' ||
    formData.provider_type === 'gemini';
  const headerCount = Object.keys(formData.custom_headers).length;

  if (loading) {
    return (
      <div className="flex items-center justify-center py-24">
        <Loader2 className="w-8 h-8 animate-spin text-gray-400" />
      </div>
    );
  }

  if (notFound || !provider) {
    return (
      <div className="space-y-6">
        <div className="flex items-center gap-2 text-sm text-gray-500">
          <Link to="/providers" className="hover:text-gray-700">
            Providers
          </Link>
          <span>/</span>
          <span className="text-gray-900 dark:text-gray-100">Not Found</span>
        </div>
        <div className="card">
          <div className="card-body text-center py-12">
            <AlertCircle className="w-12 h-12 mx-auto mb-4 text-gray-400" />
            <p className="text-lg font-medium text-gray-900 dark:text-gray-100">
              Provider not found
            </p>
            <p className="text-sm text-gray-500 mt-1">
              The provider you are looking for does not exist or has been
              deleted.
            </p>
            <Link to="/providers" className="btn btn-primary mt-4 inline-block">
              Back to Providers
            </Link>
          </div>
        </div>
      </div>
    );
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-6">
      {/* Page Header */}
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-3">
          <Link
            to="/providers"
            className="btn-icon text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
            aria-label="Back to providers"
          >
            <ArrowLeft className="w-5 h-5" />
          </Link>
          <div>
            <div className="flex items-center gap-2.5">
              <h1 className="text-xl font-bold text-gray-900 dark:text-gray-100">
                {provider.provider_key}
              </h1>
              <span className="badge badge-info">{provider.provider_type}</span>
            </div>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-0.5">
              ID: {provider.id}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() =>
              setFormData(prev => ({ ...prev, is_enabled: !prev.is_enabled }))
            }
            className="flex items-center gap-2 cursor-pointer"
          >
            <span
              className={`text-sm font-medium ${formData.is_enabled ? 'text-green-600 dark:text-green-400' : 'text-gray-400'}`}
            >
              {formData.is_enabled ? 'Enabled' : 'Disabled'}
            </span>
            <div
              className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${formData.is_enabled ? 'bg-green-500' : 'bg-gray-300 dark:bg-gray-600'}`}
            >
              <span
                className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${formData.is_enabled ? 'translate-x-4.5' : 'translate-x-0.75'}`}
              />
            </div>
          </button>
          <button
            type="button"
            onClick={() => setDeleteConfirm(true)}
            className="btn-icon text-red-500 hover:text-red-700 hover:bg-red-50 dark:hover:bg-red-900/20"
            title="Delete provider"
          >
            <Trash2 className="w-4 h-4" />
          </button>
        </div>
      </div>

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
                type="button"
                onClick={() => setError(null)}
                className="text-red-400 hover:text-red-600"
              >
                <X className="h-5 w-5" />
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Basic Configuration */}
      <Section title="Configuration">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <label htmlFor="provider_key" className="label">
              Provider Key
            </label>
            <input
              id="provider_key"
              type="text"
              value={formData.provider_key}
              disabled
              className="input bg-gray-100 dark:bg-gray-800 cursor-not-allowed"
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
              <option value="gemini">Gemini</option>
              <option value="gcp-vertex">GCP Vertex AI</option>
              <option value="response_api">Response API</option>
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
              isGcpType
                ? 'https://us-central1-aiplatform.googleapis.com'
                : 'https://api.openai.com/v1'
            }
            required
          />
        </div>

        {isGcpType && (
          <div className="space-y-3 p-4 bg-gray-50 dark:bg-gray-800/50 rounded-lg border border-gray-200 dark:border-gray-700">
            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
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
                <p className="helper-text">Your GCP project identifier</p>
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
                <p className="helper-text">Default: us-central1</p>
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
                  placeholder={
                    formData.provider_type === 'gemini' ? 'google' : 'anthropic'
                  }
                />
                <p className="helper-text">
                  Default:{' '}
                  {formData.provider_type === 'gemini' ? 'google' : 'anthropic'}
                </p>
              </div>
            </div>

            {formData.provider_type === 'gcp-vertex' && (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label htmlFor="gcp_blocking_action" className="label">
                    Blocking Action
                  </label>
                  <input
                    id="gcp_blocking_action"
                    type="text"
                    value={formData.gcp_blocking_action}
                    onChange={e =>
                      setFormData(prev => ({
                        ...prev,
                        gcp_blocking_action: e.target.value,
                      }))
                    }
                    className="input"
                    placeholder="rawPredict"
                  />
                  <p className="helper-text">
                    Default: rawPredict (Gemini: generateContent)
                  </p>
                </div>
                <div>
                  <label htmlFor="gcp_streaming_action" className="label">
                    Streaming Action
                  </label>
                  <input
                    id="gcp_streaming_action"
                    type="text"
                    value={formData.gcp_streaming_action}
                    onChange={e =>
                      setFormData(prev => ({
                        ...prev,
                        gcp_streaming_action: e.target.value,
                      }))
                    }
                    className="input"
                    placeholder="streamRawPredict"
                  />
                  <p className="helper-text">
                    Default: streamRawPredict (Gemini: streamGenerateContent)
                  </p>
                </div>
              </div>
            )}
          </div>
        )}
      </Section>

      {/* Authentication */}
      <Section title="Authentication">
        <div>
          <label htmlFor="api_key" className="label">
            API Key
          </label>
          <div className="flex space-x-2">
            <div className="relative flex-1">
              <input
                id="api_key"
                type={showApiKey ? 'text' : 'password'}
                value={formData.api_key}
                onChange={e =>
                  setFormData(prev => ({
                    ...prev,
                    api_key: e.target.value,
                  }))
                }
                className="input pr-10"
                placeholder="Enter new API key to update..."
              />
              <button
                type="button"
                onClick={() => setShowApiKey(v => !v)}
                className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                aria-label={showApiKey ? 'Hide API key' : 'Show API key'}
              >
                {showApiKey ? (
                  <EyeOff className="w-4 h-4" />
                ) : (
                  <Eye className="w-4 h-4" />
                )}
              </button>
            </div>
            <button
              type="button"
              onClick={generateRandomKey}
              className="btn btn-secondary"
              title="Generate random key"
            >
              <Shuffle className="w-4 h-4" />
            </button>
          </div>
          <p className="helper-text">
            Leave empty to keep the current key unchanged
          </p>
        </div>
      </Section>

      {/* Model Mapping */}
      <Section
        title="Model Mapping"
        collapsible
        defaultOpen={Object.keys(formData.model_mapping).length > 0}
        badge="optional"
      >
        <JsonEditor
          id="model_mapping"
          label="Mapping Rules"
          value={formData.model_mapping}
          onChange={next =>
            setFormData(prev => ({ ...prev, model_mapping: next }))
          }
          onErrorChange={setModelMappingError}
          rows={16}
          placeholder='{\n  "gpt-4": "gpt-4-turbo",\n  "gpt-3.5-turbo": "gpt-3.5-turbo-16k"\n}'
          helperText='JSON object in format: {"source_model":"target_model"}'
        />
      </Section>

      {/* Custom Headers */}
      <Section
        title="Custom Headers"
        collapsible
        defaultOpen={headerCount > 0}
        badge="optional"
      >
        <div>
          <div className="flex items-center justify-between mb-2">
            <p className="text-sm text-gray-500 dark:text-gray-400">
              Additional HTTP headers sent with upstream requests
            </p>
            <button
              type="button"
              onClick={addCustomHeader}
              className="text-xs text-blue-600 hover:text-blue-800 flex items-center gap-1"
            >
              <Plus className="w-3 h-3" />
              Add Header
            </button>
          </div>
          {headerCount > 0 && (
            <div className="space-y-2">
              {Object.entries(formData.custom_headers).map(
                ([key, value], idx) => (
                  <div key={idx} className="flex items-center gap-2">
                    <input
                      type="text"
                      value={key}
                      onChange={e => updateCustomHeaderKey(key, e.target.value)}
                      className="input flex-1"
                      placeholder="Header name"
                    />
                    <input
                      type="text"
                      value={value}
                      onChange={e =>
                        updateCustomHeaderValue(key, e.target.value)
                      }
                      className="input flex-1"
                      placeholder="Header value"
                    />
                    <button
                      type="button"
                      onClick={() => removeCustomHeader(key)}
                      className="btn-icon text-red-500 hover:text-red-700"
                    >
                      <Minus className="w-4 h-4" />
                    </button>
                  </div>
                )
              )}
            </div>
          )}
          {headerCount === 0 && (
            <div className="text-center py-4 text-sm text-gray-400 dark:text-gray-500">
              No custom headers configured
            </div>
          )}
        </div>
      </Section>

      {/* Lua Script */}
      <Section
        title="Lua Script"
        collapsible
        defaultOpen={!!formData.lua_script}
        badge="optional"
      >
        <LuaEditor
          id="lua_script"
          label="Script"
          value={formData.lua_script}
          onChange={next =>
            setFormData(prev => ({ ...prev, lua_script: next }))
          }
          providerId={provider.id}
        />
      </Section>

      {/* Sticky Action Bar */}
      <div className="sticky bottom-0 -mx-6 px-6 py-4 bg-white/80 dark:bg-gray-900/80 backdrop-blur border-t border-gray-200 dark:border-gray-700 flex items-center justify-end gap-3 z-10">
        <Link to="/providers" className="btn btn-secondary">
          Cancel
        </Link>
        <button
          type="submit"
          className="btn btn-primary flex items-center space-x-2"
          disabled={saving || !!modelMappingError}
        >
          {saving && <Loader2 className="w-4 h-4 animate-spin" />}
          <span>Save Changes</span>
        </button>
      </div>

      {/* Delete Confirmation Modal */}
      {deleteConfirm && (
        <div className="modal-overlay" onClick={() => setDeleteConfirm(false)}>
          <div className="modal" onClick={e => e.stopPropagation()}>
            <div className="modal-header">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                Delete Provider
              </h3>
              <button
                type="button"
                onClick={() => setDeleteConfirm(false)}
                className="btn-icon"
              >
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="modal-body">
              <p className="text-sm text-gray-600 dark:text-gray-400">
                Are you sure you want to delete provider{' '}
                <strong>{provider.provider_key}</strong>? This action cannot be
                undone.
              </p>
            </div>
            <div className="modal-footer">
              <button
                type="button"
                onClick={() => setDeleteConfirm(false)}
                className="btn btn-secondary"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleDelete}
                className="btn btn-danger flex items-center space-x-2"
                disabled={saving}
              >
                {saving && <Loader2 className="w-4 h-4 animate-spin" />}
                <span>Delete</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </form>
  );
};

export default ProviderDetail;
