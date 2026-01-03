<script lang="ts">
  import { onMount } from 'svelte';
  import { providers, loading, errors, actions } from '$lib/stores';
  import { generateApiKey } from '$lib/api';
  import type { Provider, ProviderFormData } from '$lib/types';

  let searchTerm = $state('');
  let showCreateForm = $state(false);
  let editingProvider: Provider | null = $state(null);
  let formData: ProviderFormData = $state({
    id: '',
    provider_type: 'openai',
    api_base: '',
    api_key: '',
    model_mapping: {},
    is_enabled: true,
  });

  // Load providers on mount
  onMount(() => {
    actions.loadProviders();
  });

  // Filtered providers based on search
  const filteredProviders = $derived(
    $providers.filter(
      provider =>
        provider.id.toLowerCase().includes(searchTerm.toLowerCase()) ||
        provider.provider_type
          .toLowerCase()
          .includes(searchTerm.toLowerCase()) ||
        provider.api_base.toLowerCase().includes(searchTerm.toLowerCase())
    )
  );

  function resetForm() {
    formData = {
      id: '',
      provider_type: 'openai',
      api_base: '',
      api_key: '',
      model_mapping: {},
      is_enabled: true,
    };
    editingProvider = null;
    showCreateForm = false;
  }

  function handleCreate() {
    showCreateForm = true;
    resetForm();
  }

  function handleEdit(provider: Provider) {
    editingProvider = provider;
    formData = {
      id: provider.id,
      provider_type: provider.provider_type,
      api_base: provider.api_base,
      api_key: '', // Don't populate existing key for security
      model_mapping: provider.model_mapping,
      is_enabled: provider.is_enabled,
    };
    showCreateForm = true;
  }

  async function handleSubmit() {
    if (editingProvider) {
      // Update existing provider
      const updateData: any = {
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
    if (confirm(`Are you sure you want to delete provider "${provider.id}"?`)) {
      await actions.deleteProvider(provider.id);
    }
  }

  async function handleToggleStatus(provider: Provider) {
    await actions.toggleProviderStatus(provider.id, !provider.is_enabled);
  }

  function generateRandomKey() {
    formData.api_key = generateApiKey();
  }

  // Model mapping helpers - derived from formData.model_mapping
  const modelMappingText = $derived(
    Object.entries(formData.model_mapping)
      .map(([key, value]) => `${key}=${value}`)
      .join('\n')
  );

  function updateModelMapping(event: Event) {
    const target = event.target as HTMLTextAreaElement;
    const text = target.value;
    const mapping: Record<string, string> = {};
    text.split('\n').forEach(line => {
      const [key, value] = line.split('=').map(s => s.trim());
      if (key && value) {
        mapping[key] = value;
      }
    });
    formData.model_mapping = mapping;
  }
</script>

<svelte:head>
  <title>Providers - LLM Proxy Admin</title>
</svelte:head>

<div class="space-y-6">
  <!-- Header -->
  <div class="flex justify-between items-center">
    <div>
      <h1 class="text-2xl font-bold text-gray-900">Providers</h1>
      <p class="text-gray-600">Manage your LLM provider configurations</p>
    </div>
    <button onclick={handleCreate} class="btn btn-primary">
      + Add Provider
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
    <div class="bg-red-50 border-l-4 border-red-400 p-4">
      <div class="flex">
        <div class="shrink-0">
          <svg
            class="h-5 w-5 text-red-400"
            viewBox="0 0 20 20"
            fill="currentColor"
          >
            <path
              fill-rule="evenodd"
              d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z"
              clip-rule="evenodd"
            ></path>
          </svg>
        </div>
        <div class="ml-3">
          <p class="text-sm text-red-700">{$errors.providers}</p>
        </div>
        <div class="ml-auto pl-3">
          <button
            onclick={() => actions.clearError('providers')}
            class="text-red-400 hover:text-red-600"
            aria-label="Close error message"
          >
            <svg class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
              <path
                fill-rule="evenodd"
                d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"
                clip-rule="evenodd"
              ></path>
            </svg>
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Create/Edit Form -->
  {#if showCreateForm}
    <div class="card">
      <h2 class="text-lg font-semibold mb-4">
        {editingProvider ? 'Edit Provider' : 'Create New Provider'}
      </h2>

      <form
        onsubmit={e => {
          e.preventDefault();
          handleSubmit();
        }}
        class="space-y-4"
      >
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <label for="id" class="block text-sm font-medium text-gray-700">
              Provider ID
            </label>
            <input
              id="id"
              type="text"
              bind:value={formData.id}
              disabled={!!editingProvider}
              class="input"
              placeholder="e.g., openai-primary"
              required
            />
          </div>

          <div>
            <label
              for="provider_type"
              class="block text-sm font-medium text-gray-700"
            >
              Provider Type
            </label>
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
          <label for="api_base" class="block text-sm font-medium text-gray-700">
            API Base URL
          </label>
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
          <label for="api_key" class="block text-sm font-medium text-gray-700">
            API Key {editingProvider ? '(leave empty to keep current)' : ''}
          </label>
          <div class="flex space-x-2">
            <input
              id="api_key"
              type="password"
              bind:value={formData.api_key}
              class="input flex-1"
              placeholder={editingProvider ? 'Enter new API key...' : 'sk-...'}
              required={!editingProvider}
            />
            <button
              type="button"
              onclick={generateRandomKey}
              class="btn btn-secondary"
              title="Generate random key"
            >
              🎲
            </button>
          </div>
        </div>

        <div>
          <label
            for="model_mapping"
            class="block text-sm font-medium text-gray-700"
          >
            Model Mapping (optional)
          </label>
          <textarea
            id="model_mapping"
            value={modelMappingText}
            oninput={updateModelMapping}
            class="input"
            rows="3"
            placeholder="gpt-4=gpt-4-turbo&#10;gpt-3.5-turbo=gpt-3.5-turbo-16k"
          ></textarea>
          <p class="text-xs text-gray-500 mt-1">
            One mapping per line in format: source_model=target_model
          </p>
        </div>

        <div class="flex items-center">
          <input
            id="is_enabled"
            type="checkbox"
            bind:checked={formData.is_enabled}
            class="h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300 rounded"
          />
          <label for="is_enabled" class="ml-2 block text-sm text-gray-900">
            Enable this provider
          </label>
        </div>

        <div class="flex justify-end space-x-3">
          <button type="button" onclick={resetForm} class="btn btn-secondary">
            Cancel
          </button>
          <button
            type="submit"
            class="btn btn-primary"
            disabled={$loading.providers}
          >
            {#if $loading.providers}
              <svg
                class="animate-spin -ml-1 mr-3 h-5 w-5 text-white"
                xmlns="http://www.w3.org/2000/svg"
                fill="none"
                viewBox="0 0 24 24"
              >
                <circle
                  class="opacity-25"
                  cx="12"
                  cy="12"
                  r="10"
                  stroke="currentColor"
                  stroke-width="4"
                ></circle>
                <path
                  class="opacity-75"
                  fill="currentColor"
                  d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                ></path>
              </svg>
            {/if}
            {editingProvider ? 'Update' : 'Create'} Provider
          </button>
        </div>
      </form>
    </div>
  {/if}

  <!-- Providers List -->
  <div class="card">
    <div class="flex justify-between items-center mb-4">
      <h2 class="text-lg font-semibold">
        Providers ({filteredProviders.length})
      </h2>
      {#if $loading.providers}
        <div class="flex items-center text-gray-500">
          <svg
            class="animate-spin -ml-1 mr-3 h-5 w-5"
            xmlns="http://www.w3.org/2000/svg"
            fill="none"
            viewBox="0 0 24 24"
          >
            <circle
              class="opacity-25"
              cx="12"
              cy="12"
              r="10"
              stroke="currentColor"
              stroke-width="4"
            ></circle>
            <path
              class="opacity-75"
              fill="currentColor"
              d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
            ></path>
          </svg>
          Loading...
        </div>
      {/if}
    </div>

    {#if filteredProviders.length === 0}
      <div class="text-center py-8 text-gray-500">
        {searchTerm
          ? 'No providers match your search.'
          : 'No providers configured yet.'}
      </div>
    {:else}
      <div class="overflow-x-auto">
        <table class="min-w-full divide-y divide-gray-200">
          <thead class="bg-gray-50">
            <tr>
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                Provider
              </th>
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                Type
              </th>
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                API Base
              </th>
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                Models
              </th>
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                Status
              </th>
              <th
                class="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider"
              >
                Actions
              </th>
            </tr>
          </thead>
          <tbody class="bg-white divide-y divide-gray-200">
            {#each filteredProviders as provider}
              <tr>
                <td class="px-6 py-4 whitespace-nowrap">
                  <div class="text-sm font-medium text-gray-900">
                    {provider.id}
                  </div>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <span
                    class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-blue-100 text-blue-800"
                  >
                    {provider.provider_type}
                  </span>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <div
                    class="text-sm text-gray-900 max-w-xs truncate"
                    title={provider.api_base}
                  >
                    {provider.api_base}
                  </div>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <div class="text-sm text-gray-500">
                    {Object.keys(provider.model_mapping).length} mappings
                  </div>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <button
                    onclick={() => handleToggleStatus(provider)}
                    class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors
                      {provider.is_enabled
                      ? 'bg-green-100 text-green-800 hover:bg-green-200'
                      : 'bg-red-100 text-red-800 hover:bg-red-200'}"
                  >
                    {provider.is_enabled ? 'Enabled' : 'Disabled'}
                  </button>
                </td>
                <td
                  class="px-6 py-4 whitespace-nowrap text-right text-sm font-medium"
                >
                  <div class="flex justify-end space-x-2">
                    <button
                      onclick={() => handleEdit(provider)}
                      class="text-blue-600 hover:text-blue-900"
                      title="Edit provider"
                    >
                      ✏️
                    </button>
                    <button
                      onclick={() => handleDelete(provider)}
                      class="text-red-600 hover:text-red-900"
                      title="Delete provider"
                    >
                      🗑️
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
