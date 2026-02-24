<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { auth, loading, errors, actions } from '$lib/stores';
  import { generateApiKey } from '$lib/api';
  import JsonEditor from '$lib/components/JsonEditor.svelte';
  import LuaEditor from '$lib/components/LuaEditor.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import type { Provider, ProviderFormData, ProviderUpdate } from '$lib/types';
  import {
    Loader2,
    AlertCircle,
    X,
    Shuffle,
    ArrowLeft,
    Trash2,
    Save,
    Eye,
    EyeOff,
    ChevronDown,
    ChevronRight,
  } from 'lucide-svelte';

  const DEFAULT_COPILOT_HEADERS: Record<string, string> = {
    'Copilot-Integration-Id': 'vscode-chat',
    'Openai-Intent': 'conversation-agents',
  };

  let provider = $state<Provider | null>(null);
  let fetchLoading = $state(true);
  let fetchError = $state<string | null>(null);
  let deleteConfirm = $state(false);
  let saving = $state(false);
  let showApiKey = $state(false);

  let sectionOpen = $state({
    config: true,
    auth: true,
    modelMapping: true,
    headers: false,
    lua: false,
  });

  let formData: ProviderFormData = $state({
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
  let modelMappingError = $state<string | null>(null);
  let newHeaderKey = $state('');
  let newHeaderValue = $state('');

  const providerId = $derived(Number($page.params.id));

  onMount(async () => {
    const client = auth.apiClient;
    if (!client) {
      fetchError = 'Not authenticated';
      fetchLoading = false;
      return;
    }

    try {
      const data = await client.getProvider(providerId);
      provider = data;
      formData = {
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
      };
      // Auto-expand sections that have data
      sectionOpen.modelMapping = Object.keys(data.model_mapping).length > 0;
      sectionOpen.headers = Object.keys(formData.custom_headers).length > 0;
      sectionOpen.lua = !!formData.lua_script;
    } catch (e) {
      fetchError = e instanceof Error ? e.message : 'Failed to load provider';
    } finally {
      fetchLoading = false;
    }
  });

  async function handleSubmit() {
    if (modelMappingError || !provider) return;

    if (
      (formData.provider_type === 'gcp-vertex' ||
        formData.provider_type === 'gemini') &&
      !formData.gcp_project.trim()
    ) {
      errors.update(state => ({
        ...state,
        providers:
          'GCP Project ID is required for GCP Vertex / Gemini provider',
      }));
      return;
    }

    saving = true;
    errors.update(state => ({ ...state, providers: null }));

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
          streaming: formData.gcp_streaming_action.trim() || 'streamRawPredict',
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

    const success = await actions.updateProvider(provider.id, updateData);
    saving = false;
    if (success) {
      goto('/providers');
    }
  }

  async function handleDelete() {
    if (!provider) return;
    const success = await actions.deleteProvider(provider.id);
    if (success) {
      goto('/providers');
    }
  }

  function generateRandomKey() {
    formData.api_key = generateApiKey();
  }
</script>

<svelte:head>
  <title
    >{provider ? `${provider.provider_key} - ` : ''}Provider - HEN Admin</title
  >
</svelte:head>

{#if fetchLoading}
  <!-- Loading skeleton -->
  <div class="space-y-6">
    <div class="flex items-center gap-3">
      <Skeleton class="h-8 w-8 rounded-lg" />
      <div>
        <Skeleton class="h-6 w-40 mb-1" />
        <Skeleton class="h-4 w-20" />
      </div>
    </div>
    <div class="card">
      <div class="card-header"><Skeleton class="h-6 w-32" /></div>
      <div class="card-body space-y-4">
        {#each Array(4) as _}
          <div>
            <Skeleton class="h-4 w-24 mb-2" />
            <Skeleton class="h-10 w-full" />
          </div>
        {/each}
      </div>
    </div>
  </div>
{:else if fetchError}
  <!-- Error state -->
  <div class="space-y-6">
    <div class="flex items-center gap-2 text-sm text-gray-500">
      <a href="/providers" class="hover:text-gray-700 dark:hover:text-gray-300"
        >Providers</a
      >
      <span>/</span>
      <span class="text-gray-900 dark:text-gray-100">Not Found</span>
    </div>
    <div class="card">
      <div class="card-body text-center py-12">
        <AlertCircle class="w-12 h-12 text-red-400 mx-auto mb-4" />
        <h3 class="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
          Provider Not Found
        </h3>
        <p class="text-gray-500 dark:text-gray-400 mb-4">{fetchError}</p>
        <button onclick={() => goto('/providers')} class="btn btn-primary">
          <ArrowLeft class="w-4 h-4 mr-2" />
          Back to Providers
        </button>
      </div>
    </div>
  </div>
{:else if provider}
  <form
    onsubmit={e => {
      e.preventDefault();
      handleSubmit();
    }}
    class="space-y-6"
  >
    <!-- Page Header -->
    <div class="flex items-start justify-between">
      <div class="flex items-center gap-3">
        <a
          href="/providers"
          class="btn-icon text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
          aria-label="Back to providers"
        >
          <ArrowLeft class="w-5 h-5" />
        </a>
        <div>
          <div class="flex items-center gap-2.5">
            <h1 class="text-xl font-bold text-gray-900 dark:text-gray-100">
              {provider.provider_key}
            </h1>
            <span class="badge badge-info">{provider.provider_type}</span>
          </div>
          <p class="text-sm text-gray-500 dark:text-gray-400 mt-0.5">
            ID: {provider.id}
          </p>
        </div>
      </div>
      <div class="flex items-center gap-3">
        <button
          type="button"
          onclick={() => (formData.is_enabled = !formData.is_enabled)}
          class="flex items-center gap-2 cursor-pointer"
        >
          <span
            class="text-sm font-medium {formData.is_enabled
              ? 'text-green-600 dark:text-green-400'
              : 'text-gray-400'}"
          >
            {formData.is_enabled ? 'Enabled' : 'Disabled'}
          </span>
          <div
            class="relative inline-flex h-5 w-9 items-center rounded-full transition-colors {formData.is_enabled
              ? 'bg-green-500'
              : 'bg-gray-300 dark:bg-gray-600'}"
          >
            <span
              class="inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform {formData.is_enabled
                ? 'translate-x-4.5'
                : 'translate-x-0.76'}"
            />
          </div>
        </button>
        <button
          type="button"
          onclick={() => (deleteConfirm = true)}
          class="btn-icon text-red-500 hover:text-red-700 hover:bg-red-50 dark:hover:bg-red-900/20"
          title="Delete provider"
        >
          <Trash2 class="w-4 h-4" />
        </button>
      </div>
    </div>

    <!-- Error Display -->
    {#if $errors.providers}
      <div class="alert-error">
        <div class="flex">
          <div class="shrink-0">
            <AlertCircle class="h-5 w-5 text-red-400" />
          </div>
          <div class="ml-3">
            <p class="text-sm text-red-700">{$errors.providers}</p>
          </div>
          <div class="ml-auto pl-3">
            <button
              type="button"
              onclick={() => actions.clearError('providers')}
              class="text-red-400 hover:text-red-600"
            >
              <X class="h-5 w-5" />
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- Configuration Section -->
    <div class="card">
      <div class="card-header">
        <h2 class="card-title">Configuration</h2>
      </div>
      <div class="card-body space-y-4">
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <label for="provider_key" class="label">Provider Key</label>
            <input
              id="provider_key"
              type="text"
              value={formData.provider_key}
              disabled
              class="input bg-gray-100 dark:bg-gray-800 cursor-not-allowed"
            />
          </div>
          <div>
            <label for="provider_type" class="label">Provider Type</label>
            <select
              id="provider_type"
              bind:value={formData.provider_type}
              class="input"
              required
            >
              <option value="openai">OpenAI</option>
              <option value="azure">Azure OpenAI</option>
              <option value="anthropic">Anthropic</option>
              <option value="github-copilot">GitHub Copilot</option>
              <option value="google">Google</option>
              <option value="gemini">Gemini</option>
              <option value="gcp-vertex">GCP Vertex AI</option>
              <option value="response_api">Response API</option>
              <option value="custom">Custom</option>
            </select>
          </div>
        </div>

        <div>
          <label for="api_base" class="label">API Base URL</label>
          <input
            id="api_base"
            type="url"
            bind:value={formData.api_base}
            class="input"
            placeholder={formData.provider_type === 'gcp-vertex' ||
            formData.provider_type === 'gemini'
              ? 'https://us-central1-aiplatform.googleapis.com'
              : formData.provider_type === 'github-copilot'
                ? 'https://api.githubcopilot.com'
                : 'https://api.openai.com/v1'}
            required
          />
        </div>

        <!-- GCP Vertex AI / Gemini specific fields -->
        {#if formData.provider_type === 'gcp-vertex' || formData.provider_type === 'gemini'}
          <div
            class="space-y-3 p-4 bg-gray-50 dark:bg-gray-800/50 rounded-lg border border-gray-200 dark:border-gray-700"
          >
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
              <div>
                <label for="gcp_project" class="label">
                  GCP Project ID <span class="text-red-500">*</span>
                </label>
                <input
                  id="gcp_project"
                  type="text"
                  bind:value={formData.gcp_project}
                  class="input"
                  placeholder="my-project-id"
                  required
                />
                <p class="helper-text">Your GCP project identifier</p>
              </div>
              <div>
                <label for="gcp_location" class="label">GCP Location</label>
                <input
                  id="gcp_location"
                  type="text"
                  bind:value={formData.gcp_location}
                  class="input"
                  placeholder="us-central1"
                />
                <p class="helper-text">Default: us-central1</p>
              </div>
              <div>
                <label for="gcp_publisher" class="label">GCP Publisher</label>
                <input
                  id="gcp_publisher"
                  type="text"
                  bind:value={formData.gcp_publisher}
                  class="input"
                  placeholder={formData.provider_type === 'gemini'
                    ? 'google'
                    : 'anthropic'}
                />
                <p class="helper-text">
                  Default: {formData.provider_type === 'gemini'
                    ? 'google'
                    : 'anthropic'}
                </p>
              </div>
            </div>

            {#if formData.provider_type === 'gcp-vertex'}
              <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label for="gcp_blocking_action" class="label">
                    Blocking Action
                  </label>
                  <input
                    id="gcp_blocking_action"
                    type="text"
                    bind:value={formData.gcp_blocking_action}
                    class="input"
                    placeholder="rawPredict"
                  />
                  <p class="helper-text">
                    Default: rawPredict (Gemini: generateContent)
                  </p>
                </div>
                <div>
                  <label for="gcp_streaming_action" class="label">
                    Streaming Action
                  </label>
                  <input
                    id="gcp_streaming_action"
                    type="text"
                    bind:value={formData.gcp_streaming_action}
                    class="input"
                    placeholder="streamRawPredict"
                  />
                  <p class="helper-text">
                    Default: streamRawPredict (Gemini: streamGenerateContent)
                  </p>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- Authentication Section -->
    <div class="card">
      <div class="card-header">
        <h2 class="card-title">Authentication</h2>
      </div>
      <div class="card-body space-y-4">
        <div>
          <label for="api_key" class="label">API Key</label>
          <div class="flex space-x-2">
            <div class="relative flex-1">
              <input
                id="api_key"
                type={showApiKey ? 'text' : 'password'}
                bind:value={formData.api_key}
                class="input pr-10"
                placeholder="Enter new API key to update..."
              />
              <button
                type="button"
                onclick={() => (showApiKey = !showApiKey)}
                class="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                aria-label={showApiKey ? 'Hide API key' : 'Show API key'}
              >
                {#if showApiKey}
                  <EyeOff class="w-4 h-4" />
                {:else}
                  <Eye class="w-4 h-4" />
                {/if}
              </button>
            </div>
            <button
              type="button"
              onclick={generateRandomKey}
              class="btn btn-secondary"
              title="Generate random key"
            >
              <Shuffle class="w-4 h-4" />
            </button>
          </div>
          <p class="helper-text">
            Leave empty to keep the current key unchanged
          </p>
        </div>
      </div>
    </div>

    <!-- Model Mapping Section (collapsible) -->
    <div class="card">
      <button
        type="button"
        class="card-header flex items-center justify-between w-full cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
        onclick={() => (sectionOpen.modelMapping = !sectionOpen.modelMapping)}
      >
        <div class="flex items-center gap-2">
          {#if sectionOpen.modelMapping}
            <ChevronDown class="w-4 h-4 text-gray-400" />
          {:else}
            <ChevronRight class="w-4 h-4 text-gray-400" />
          {/if}
          <h2 class="card-title">Model Mapping</h2>
          <span
            class="text-xs px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
          >
            optional
          </span>
        </div>
      </button>
      {#if sectionOpen.modelMapping}
        <div class="card-body space-y-4">
          <JsonEditor
            id="model_mapping"
            label="Mapping Rules"
            value={formData.model_mapping}
            onChange={next => (formData.model_mapping = next)}
            onErrorChange={err => (modelMappingError = err)}
            rows={16}
            placeholder={`{\n  "gpt-4": "gpt-4-turbo",\n  "gpt-3.5-turbo": "gpt-3.5-turbo-16k"\n}`}
            helperText={'JSON object in format: {"source_model":"target_model"}'}
          />
        </div>
      {/if}
    </div>

    <!-- Custom Headers Section (collapsible) -->
    <div class="card">
      <button
        type="button"
        class="card-header flex items-center justify-between w-full cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
        onclick={() => (sectionOpen.headers = !sectionOpen.headers)}
      >
        <div class="flex items-center gap-2">
          {#if sectionOpen.headers}
            <ChevronDown class="w-4 h-4 text-gray-400" />
          {:else}
            <ChevronRight class="w-4 h-4 text-gray-400" />
          {/if}
          <h2 class="card-title">Custom Headers</h2>
          <span
            class="text-xs px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
          >
            optional
          </span>
        </div>
      </button>
      {#if sectionOpen.headers}
        <div class="card-body space-y-4">
          <div>
            <div class="flex items-center justify-between mb-2">
              <p class="text-sm text-gray-500 dark:text-gray-400">
                Additional HTTP headers sent with upstream requests
              </p>
              {#if formData.provider_type === 'github-copilot' && Object.keys(formData.custom_headers).length === 0}
                <button
                  type="button"
                  onclick={() => {
                    formData.custom_headers = { ...DEFAULT_COPILOT_HEADERS };
                  }}
                  class="text-xs text-primary-600 hover:text-primary-800 dark:text-primary-400"
                >
                  Load Copilot defaults
                </button>
              {/if}
            </div>
            {#if Object.keys(formData.custom_headers).length > 0}
              <div class="space-y-2 mb-2">
                {#each Object.entries(formData.custom_headers) as [key, _value] (key)}
                  <div class="flex items-center gap-2">
                    <input
                      type="text"
                      value={key}
                      disabled
                      class="input flex-1 text-sm font-mono"
                      placeholder="Header name"
                    />
                    <span class="text-gray-400">:</span>
                    <input
                      type="text"
                      value={formData.custom_headers[key]}
                      oninput={e => {
                        formData.custom_headers = {
                          ...formData.custom_headers,
                          [key]: (e.target as HTMLInputElement).value,
                        };
                      }}
                      class="input flex-1 text-sm font-mono"
                      placeholder="Header value"
                    />
                    <button
                      type="button"
                      onclick={() => {
                        const { [key]: _, ...rest } = formData.custom_headers;
                        formData.custom_headers = rest;
                      }}
                      class="btn-icon text-red-500 hover:text-red-700"
                      title="Remove header"
                    >
                      <X class="w-4 h-4" />
                    </button>
                  </div>
                {/each}
              </div>
            {:else}
              <div
                class="text-center py-4 text-sm text-gray-400 dark:text-gray-500"
              >
                No custom headers configured
              </div>
            {/if}
            <div class="flex items-center gap-2">
              <input
                type="text"
                bind:value={newHeaderKey}
                class="input flex-1 text-sm font-mono"
                placeholder="Header name"
              />
              <span class="text-gray-400">:</span>
              <input
                type="text"
                bind:value={newHeaderValue}
                class="input flex-1 text-sm font-mono"
                placeholder="Header value"
              />
              <button
                type="button"
                onclick={() => {
                  if (newHeaderKey.trim() && newHeaderValue.trim()) {
                    formData.custom_headers = {
                      ...formData.custom_headers,
                      [newHeaderKey.trim()]: newHeaderValue.trim(),
                    };
                    newHeaderKey = '';
                    newHeaderValue = '';
                  }
                }}
                class="btn btn-secondary text-sm"
                disabled={!newHeaderKey.trim() || !newHeaderValue.trim()}
              >
                Add
              </button>
            </div>
          </div>
        </div>
      {/if}
    </div>

    <!-- Lua Script Section (collapsible) -->
    <div class="card">
      <button
        type="button"
        class="card-header flex items-center justify-between w-full cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors"
        onclick={() => (sectionOpen.lua = !sectionOpen.lua)}
      >
        <div class="flex items-center gap-2">
          {#if sectionOpen.lua}
            <ChevronDown class="w-4 h-4 text-gray-400" />
          {:else}
            <ChevronRight class="w-4 h-4 text-gray-400" />
          {/if}
          <h2 class="card-title">Lua Script</h2>
          <span
            class="text-xs px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400"
          >
            optional
          </span>
        </div>
      </button>
      {#if sectionOpen.lua}
        <div class="card-body space-y-4">
          <LuaEditor
            id="lua_script"
            label="Script"
            value={formData.lua_script}
            onChange={next => (formData.lua_script = next)}
            rows={20}
            providerId={provider.id}
          />
        </div>
      {/if}
    </div>

    <!-- Sticky Action Bar -->
    <div
      class="sticky bottom-0 -mx-6 px-6 py-4 bg-white/80 dark:bg-gray-900/80 backdrop-blur border-t border-gray-200 dark:border-gray-700 flex items-center justify-end gap-3 z-10"
    >
      <button
        type="button"
        onclick={() => goto('/providers')}
        class="btn btn-secondary"
      >
        Cancel
      </button>
      <button
        type="submit"
        class="btn btn-primary flex items-center space-x-2"
        disabled={saving || $loading.providers || !!modelMappingError}
      >
        {#if saving || $loading.providers}
          <Loader2 class="w-4 h-4 animate-spin" />
        {:else}
          <Save class="w-4 h-4" />
        {/if}
        <span>Save Changes</span>
      </button>
    </div>
  </form>

  <!-- Delete Confirmation Modal -->
  {#if deleteConfirm}
    <div
      class="modal-overlay animate-fade-in"
      onclick={() => (deleteConfirm = false)}
      onkeydown={e => e.key === 'Escape' && (deleteConfirm = false)}
      role="button"
      tabindex="0"
      aria-label="Close modal"
    >
      <div
        class="modal animate-modal-enter"
        onclick={e => e.stopPropagation()}
        onkeydown={e => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        tabindex="-1"
      >
        <div class="modal-header">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Delete Provider
          </h3>
          <button
            type="button"
            onclick={() => (deleteConfirm = false)}
            class="btn-icon"
          >
            <X class="w-5 h-5" />
          </button>
        </div>
        <div class="modal-body">
          <p class="text-sm text-gray-600 dark:text-gray-400">
            Are you sure you want to delete provider
            <strong>{provider?.provider_key}</strong>? This action cannot be
            undone.
          </p>
        </div>
        <div class="modal-footer">
          <button
            type="button"
            onclick={() => (deleteConfirm = false)}
            class="btn btn-secondary"
          >
            Cancel
          </button>
          <button
            type="button"
            onclick={handleDelete}
            class="btn btn-danger flex items-center space-x-2"
            disabled={$loading.providers}
          >
            {#if $loading.providers}
              <Loader2 class="w-4 h-4 animate-spin" />
            {/if}
            <span>Delete</span>
          </button>
        </div>
      </div>
    </div>
  {/if}
{/if}
