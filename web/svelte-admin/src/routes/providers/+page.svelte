<script lang="ts">
  import { onMount } from 'svelte';
  import { providers, loading, errors, actions } from '$lib/stores';
  import { generateApiKey } from '$lib/api';
  import { debounce } from '$lib/debounce';
  import JsonEditor from '$lib/components/JsonEditor.svelte';
  import TableSkeleton from '$lib/components/TableSkeleton.svelte';
  import type { Provider, ProviderFormData } from '$lib/types';
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
  } from 'lucide-svelte';

  let searchTerm = $state('');
  let debouncedSearch = $state('');
  let showCreateForm = $state(false);
  let isModalClosing = $state(false);
  let editingProvider: Provider | null = $state(null);
  let deleteConfirm: Provider | null = $state(null);
  let formData: ProviderFormData = $state({
    provider_key: '',
    provider_type: 'openai',
    api_base: '',
    api_key: '',
    model_mapping: {},
    is_enabled: true,
  });
  let modelMappingError = $state<string | null>(null);

  // Debounce search input
  const updateDebouncedSearch = debounce((value: string) => {
    debouncedSearch = value;
  }, 300);

  $effect(() => {
    updateDebouncedSearch(searchTerm);
  });

  // Load providers on mount
  onMount(() => {
    actions.loadProviders();
  });

  // Filtered providers based on debounced search
  const filteredProviders = $derived(
    $providers.filter(
      provider =>
        provider &&
        provider.provider_key &&
        (provider.provider_key
          .toLowerCase()
          .includes(debouncedSearch.toLowerCase()) ||
          provider.provider_type
            .toLowerCase()
            .includes(debouncedSearch.toLowerCase()) ||
          provider.api_base
            .toLowerCase()
            .includes(debouncedSearch.toLowerCase()))
    )
  );

  function resetForm() {
    formData = {
      provider_key: '',
      provider_type: 'openai',
      api_base: '',
      api_key: '',
      model_mapping: {},
      is_enabled: true,
    };
    modelMappingError = null;
    editingProvider = null;
    showCreateForm = false;
    isModalClosing = false;
  }

  function handleCloseModal() {
    isModalClosing = true;
    setTimeout(() => {
      resetForm();
    }, 150);
  }

  function handleCreate() {
    resetForm();
    showCreateForm = true;
  }

  function handleEdit(provider: Provider) {
    editingProvider = provider;
    formData = {
      provider_key: provider.provider_key,
      provider_type: provider.provider_type,
      api_base: provider.api_base,
      api_key: '', // Don't populate existing key for security
      model_mapping: provider.model_mapping,
      is_enabled: provider.is_enabled,
    };
    modelMappingError = null;
    showCreateForm = true;
  }

  async function handleSubmit() {
    if (modelMappingError) return;

    if (editingProvider) {
      // Update existing provider
      const updateData: Record<string, unknown> = {
        provider_type: formData.provider_type,
        api_base: formData.api_base,
        model_mapping: formData.model_mapping,
        is_enabled: formData.is_enabled,
      };

      // Only include API key if it's provided
      if (formData.api_key.trim()) {
        updateData.api_key = formData.api_key;
      }

      const success = await actions.updateProvider(
        editingProvider.id,
        updateData
      );
      if (success) {
        resetForm();
      }
    } else {
      // Create new provider
      const success = await actions.createProvider(formData);
      if (success) {
        resetForm();
      }
    }
  }

  async function handleDelete(provider: Provider) {
    const success = await actions.deleteProvider(provider.id);
    if (success) {
      deleteConfirm = null;
    }
  }

  async function handleToggleStatus(provider: Provider) {
    await actions.toggleProviderStatus(provider.id, !provider.is_enabled);
  }

  function generateRandomKey() {
    formData.api_key = generateApiKey();
  }
</script>

<svelte:head>
  <title>Providers - LLM Proxy Admin</title>
</svelte:head>

<div class="space-y-6">
  <!-- Header -->
  <div class="flex justify-between items-center">
    <div>
      <h1 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
        Providers
      </h1>
      <p class="text-gray-600 dark:text-gray-400">
        Manage your LLM provider configurations
      </p>
    </div>
    <button
      onclick={handleCreate}
      class="btn btn-primary flex items-center space-x-2"
    >
      <Plus class="w-5 h-5" />
      <span>Add Provider</span>
    </button>
  </div>

  <!-- Search -->
  <div class="max-w-md">
    <input
      type="text"
      placeholder="Search providers..."
      bind:value={searchTerm}
      class="input"
    />
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
            onclick={() => actions.clearError('providers')}
            class="text-red-400 hover:text-red-600"
          >
            <X class="h-5 w-5" />
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Create/Edit Form Modal -->
  {#if showCreateForm}
    <div
      class="modal-overlay {isModalClosing
        ? 'animate-fade-out'
        : 'animate-fade-in'}"
      onclick={handleCloseModal}
      onkeydown={e => e.key === 'Escape' && handleCloseModal()}
      role="button"
      tabindex="0"
      aria-label="Close modal"
    >
      <div
        class="modal {isModalClosing
          ? 'animate-modal-exit'
          : 'animate-modal-enter'}"
        onclick={e => e.stopPropagation()}
        onkeydown={e => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        tabindex="-1"
      >
        <div class="modal-header">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
            {editingProvider ? 'Edit Provider' : 'Add Provider'}
          </h3>
          <button onclick={handleCloseModal} class="btn-icon">
            <X class="w-5 h-5" />
          </button>
        </div>

        <form
          onsubmit={e => {
            e.preventDefault();
            handleSubmit();
          }}
          class="modal-body space-y-4"
        >
          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label for="provider_key" class="label"> Provider Key </label>
              <input
                id="provider_key"
                type="text"
                bind:value={formData.provider_key}
                disabled={!!editingProvider}
                class="input"
                placeholder="e.g., openai-primary"
                required={!editingProvider}
              />
            </div>

            <div>
              <label for="provider_type" class="label"> Provider Type </label>
              <select
                id="provider_type"
                bind:value={formData.provider_type}
                class="input"
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
            <label for="api_base" class="label"> API Base URL </label>
            <input
              id="api_base"
              type="url"
              bind:value={formData.api_base}
              class="input"
              placeholder="https://api.openai.com/v1"
              required
            />
          </div>

          <div>
            <label for="api_key" class="label">
              API Key {editingProvider ? '(leave empty to keep current)' : ''}
            </label>
            <div class="flex space-x-2">
              <input
                id="api_key"
                type="password"
                bind:value={formData.api_key}
                class="input flex-1"
                placeholder={editingProvider
                  ? 'Enter new API key...'
                  : 'sk-...'}
                required={!editingProvider}
              />
              <button
                type="button"
                onclick={generateRandomKey}
                class="btn btn-secondary flex items-center space-x-2"
                title="Generate random key"
              >
                <Shuffle class="w-4 h-4" />
              </button>
            </div>
          </div>

          <div>
            <JsonEditor
              id="model_mapping"
              label="Model Mapping (optional)"
              value={formData.model_mapping}
              onChange={next => (formData.model_mapping = next)}
              onErrorChange={err => (modelMappingError = err)}
              rows={6}
              placeholder={`{\n  "gpt-4": "gpt-4-turbo",\n  "gpt-3.5-turbo": "gpt-3.5-turbo-16k"\n}`}
              helperText={'JSON object in format: {"source_model":"target_model"}'}
            />
          </div>

          <div class="flex items-center">
            <input
              id="is_enabled"
              type="checkbox"
              bind:checked={formData.is_enabled}
              class="h-4 w-4 text-primary-600 focus:ring-primary-500 border-gray-300 rounded"
            />
            <label
              for="is_enabled"
              class="ml-2 block text-sm text-gray-900 dark:text-gray-100"
            >
              Enable this provider
            </label>
          </div>
        </form>

        <div class="modal-footer">
          <button
            type="button"
            onclick={handleCloseModal}
            class="btn btn-secondary"
          >
            Cancel
          </button>
          <button
            type="button"
            onclick={handleSubmit}
            class="btn btn-primary flex items-center space-x-2"
            disabled={$loading.providers || !!modelMappingError}
          >
            {#if $loading.providers}
              <Loader2 class="w-4 h-4 animate-spin" />
            {/if}
            <span>{editingProvider ? 'Update' : 'Create'} Provider</span>
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Providers List -->
  <div class="card">
    <div class="card-header flex justify-between items-center">
      <h2 class="card-title">Providers ({filteredProviders.length})</h2>
      {#if $loading.providers && $providers.length > 0}
        <div class="flex items-center text-gray-500 dark:text-gray-400">
          <Loader2 class="w-5 h-5 animate-spin mr-2" />
          <span class="text-sm">Loading...</span>
        </div>
      {/if}
    </div>

    <div class="card-body p-0">
      {#if $loading.providers && $providers.length === 0}
        <!-- Skeleton loading state -->
        <TableSkeleton rows={5} columns={6} />
      {:else if filteredProviders.length === 0}
        <!-- Empty state -->
        <div class="text-center py-12">
          <Inbox
            class="w-12 h-12 text-gray-400 dark:text-gray-500 mx-auto mb-4"
          />
          <h3 class="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
            {debouncedSearch ? 'No providers found' : 'No providers yet'}
          </h3>
          <p class="text-gray-500 dark:text-gray-400 mb-4">
            {debouncedSearch
              ? 'Try adjusting your search criteria.'
              : 'Get started by adding your first provider.'}
          </p>
          {#if !debouncedSearch}
            <button
              onclick={handleCreate}
              class="btn btn-primary inline-flex items-center space-x-2"
            >
              <Plus class="w-5 h-5" />
              <span>Add Provider</span>
            </button>
          {/if}
        </div>
      {:else}
        <div class="table-container">
          <table class="table">
            <thead>
              <tr>
                <th>Provider</th>
                <th>Type</th>
                <th>API Base</th>
                <th>Models</th>
                <th>Status</th>
                <th class="text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each filteredProviders as provider (provider.id)}
                <tr class="animate-fade-in">
                  <td>
                    <div
                      class="text-sm font-medium text-gray-900 dark:text-gray-100"
                    >
                      {provider.provider_key}
                    </div>
                    <div class="text-xs text-gray-500 dark:text-gray-400">
                      ID: {provider.id}
                    </div>
                  </td>
                  <td>
                    <span class="badge badge-info">
                      {provider.provider_type}
                    </span>
                  </td>
                  <td>
                    <div
                      class="text-sm text-gray-900 dark:text-gray-100 max-w-xs truncate"
                      title={provider.api_base}
                    >
                      {provider.api_base}
                    </div>
                  </td>
                  <td>
                    <div class="text-sm text-gray-500 dark:text-gray-400">
                      {Object.keys(provider.model_mapping).length} mappings
                    </div>
                  </td>
                  <td>
                    <button
                      onclick={() => handleToggleStatus(provider)}
                      class="badge transition-colors {provider.is_enabled
                        ? 'badge-success hover:opacity-80'
                        : 'badge-danger hover:opacity-80'}"
                    >
                      {#if provider.is_enabled}
                        <Check class="w-3 h-3 mr-1" />
                        Enabled
                      {:else}
                        <X class="w-3 h-3 mr-1" />
                        Disabled
                      {/if}
                    </button>
                  </td>
                  <td>
                    <div class="flex justify-end space-x-2">
                      <button
                        onclick={() => handleEdit(provider)}
                        class="btn-icon text-primary-600 hover:text-primary-900"
                        title="Edit provider"
                      >
                        <Pencil class="w-4 h-4" />
                      </button>
                      <button
                        onclick={() => (deleteConfirm = provider)}
                        class="btn-icon text-red-600 hover:text-red-900"
                        title="Delete provider"
                      >
                        <Trash2 class="w-4 h-4" />
                      </button>
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  </div>

  <!-- Delete Confirmation Modal -->
  {#if deleteConfirm}
    <div
      class="modal-overlay animate-fade-in"
      onclick={() => (deleteConfirm = null)}
      onkeydown={e => e.key === 'Escape' && (deleteConfirm = null)}
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
          <button onclick={() => (deleteConfirm = null)} class="btn-icon">
            <X class="w-5 h-5" />
          </button>
        </div>
        <div class="modal-body">
          <p class="text-sm text-gray-600 dark:text-gray-400">
            Are you sure you want to delete provider
            <strong>{deleteConfirm.provider_key}</strong>? This action cannot be
            undone.
          </p>
        </div>
        <div class="modal-footer">
          <button
            onclick={() => (deleteConfirm = null)}
            class="btn btn-secondary"
          >
            Cancel
          </button>
          <button
            onclick={() => deleteConfirm && handleDelete(deleteConfirm)}
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
</div>
