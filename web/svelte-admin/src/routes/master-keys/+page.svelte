<script lang="ts">
  import { onMount } from 'svelte';
  import { masterKeys, loading, errors, actions } from '$lib/stores';
  import { generateApiKey } from '$lib/api';
  import type { MasterKey, MasterKeyFormData } from '$lib/types';

  let searchTerm = $state('');
  let showCreateForm = $state(false);
  let editingKey: MasterKey | null = $state(null);
  let formData: MasterKeyFormData = $state({
    id: '',
    key: '',
    name: '',
    allowed_models: [],
    rate_limit: null,
    is_enabled: true,
  });
  let allowedModelsText = $state('');

  onMount(() => {
    actions.loadMasterKeys();
  });

  const filteredKeys = $derived(
    $masterKeys.filter(
      key =>
        key.id.toLowerCase().includes(searchTerm.toLowerCase()) ||
        key.name.toLowerCase().includes(searchTerm.toLowerCase())
    )
  );

  function resetForm() {
    formData = {
      id: '',
      key: '',
      name: '',
      allowed_models: [],
      rate_limit: null,
      is_enabled: true,
    };
    allowedModelsText = '';
    editingKey = null;
    showCreateForm = false;
  }

  function handleCreate() {
    showCreateForm = true;
    resetForm();
    formData.key = generateApiKey();
  }

  function handleEdit(key: MasterKey) {
    editingKey = key;
    formData = {
      id: key.id,
      key: '', // Don't populate for security
      name: key.name,
      allowed_models: key.allowed_models,
      rate_limit: key.rate_limit,
      is_enabled: key.is_enabled,
    };
    allowedModelsText = key.allowed_models.join('\n');
    showCreateForm = true;
  }

  async function handleSubmit() {
    // Update allowed models from text
    formData.allowed_models = allowedModelsText
      .split('\n')
      .map(s => s.trim())
      .filter(s => s.length > 0);

    if (editingKey) {
      const updateData: any = {
        name: formData.name,
        allowed_models: formData.allowed_models,
        rate_limit: formData.rate_limit,
        is_enabled: formData.is_enabled,
      };

      const success = await actions.updateMasterKey(editingKey.id, updateData);
      if (success) resetForm();
    } else {
      const success = await actions.createMasterKey(formData);
      if (success) resetForm();
    }
  }

  async function handleDelete(key: MasterKey) {
    if (confirm(`Are you sure you want to delete master key "${key.name}"?`)) {
      await actions.deleteMasterKey(key.id);
    }
  }

  async function handleToggleStatus(key: MasterKey) {
    await actions.toggleMasterKeyStatus(key.id, !key.is_enabled);
  }

  async function handleRotate(key: MasterKey) {
    if (
      confirm(
        `Are you sure you want to rotate the key for "${key.name}"? The old key will be invalidated.`
      )
    ) {
      const newKey = await actions.rotateMasterKey(key.id);
      if (newKey) {
        alert(
          `New key generated: ${newKey}\n\nSave this key securely. It will not be shown again.`
        );
      }
    }
  }
</script>

<svelte:head>
  <title>Master Keys - LLM Proxy Admin</title>
</svelte:head>

<div class="space-y-6">
  <div class="flex justify-between items-center">
    <div>
      <h1 class="text-2xl font-bold text-gray-900">Master Keys</h1>
      <p class="text-gray-600">Manage API keys for client authentication</p>
    </div>
    <button onclick={handleCreate} class="btn btn-primary">
      + Add Master Key
    </button>
  </div>

  <div class="max-w-md">
    <input
      type="text"
      placeholder="Search master keys..."
      bind:value={searchTerm}
      class="input"
    />
  </div>

  {#if $errors.masterKeys}
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
          <p class="text-sm text-red-700">{$errors.masterKeys}</p>
        </div>
        <div class="ml-auto pl-3">
          <button
            onclick={() => actions.clearError('masterKeys')}
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

  {#if showCreateForm}
    <div class="card">
      <h2 class="text-lg font-semibold mb-4">
        {editingKey ? 'Edit Master Key' : 'Create New Master Key'}
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
            <label for="id" class="block text-sm font-medium text-gray-700"
              >Key ID</label
            >
            <input
              id="id"
              type="text"
              bind:value={formData.id}
              disabled={!!editingKey}
              class="input"
              placeholder="e.g., key-1"
              required
            />
          </div>

          <div>
            <label for="name" class="block text-sm font-medium text-gray-700"
              >Name</label
            >
            <input
              id="name"
              type="text"
              bind:value={formData.name}
              class="input"
              placeholder="e.g., Production Key"
              required
            />
          </div>
        </div>

        {#if !editingKey}
          <div>
            <label for="key" class="block text-sm font-medium text-gray-700"
              >API Key</label
            >
            <div class="flex space-x-2">
              <input
                id="key"
                type="text"
                bind:value={formData.key}
                class="input flex-1 font-mono text-sm"
                required
              />
              <button
                type="button"
                onclick={() => (formData.key = generateApiKey())}
                class="btn btn-secondary"
                title="Generate new key"
              >
                🎲
              </button>
            </div>
          </div>
        {/if}

        <div>
          <label
            for="allowed_models"
            class="block text-sm font-medium text-gray-700"
          >
            Allowed Models (optional)
          </label>
          <textarea
            id="allowed_models"
            bind:value={allowedModelsText}
            class="input"
            rows="3"
            placeholder="gpt-4&#10;gpt-3.5-turbo&#10;claude-3-sonnet"
          ></textarea>
          <p class="text-xs text-gray-500 mt-1">
            One model per line. Leave empty to allow all models.
          </p>
        </div>

        <div>
          <label
            for="rate_limit"
            class="block text-sm font-medium text-gray-700"
          >
            Rate Limit (requests per second)
          </label>
          <input
            id="rate_limit"
            type="number"
            bind:value={formData.rate_limit}
            class="input"
            placeholder="100"
            min="1"
          />
        </div>

        <div class="flex items-center">
          <input
            id="is_enabled"
            type="checkbox"
            bind:checked={formData.is_enabled}
            class="h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300 rounded"
          />
          <label for="is_enabled" class="ml-2 block text-sm text-gray-900">
            Enable this master key
          </label>
        </div>

        <div class="flex justify-end space-x-3">
          <button type="button" onclick={resetForm} class="btn btn-secondary">
            Cancel
          </button>
          <button
            type="submit"
            class="btn btn-primary"
            disabled={$loading.masterKeys}
          >
            {#if $loading.masterKeys}
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
            {editingKey ? 'Update' : 'Create'} Master Key
          </button>
        </div>
      </form>
    </div>
  {/if}

  <div class="card">
    <div class="flex justify-between items-center mb-4">
      <h2 class="text-lg font-semibold">Master Keys ({filteredKeys.length})</h2>
      {#if $loading.masterKeys}
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

    {#if filteredKeys.length === 0}
      <div class="text-center py-8 text-gray-500">
        {searchTerm
          ? 'No master keys match your search.'
          : 'No master keys configured yet.'}
      </div>
    {:else}
      <div class="overflow-x-auto">
        <table class="min-w-full divide-y divide-gray-200">
          <thead class="bg-gray-50">
            <tr>
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                >Name</th
              >
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                >Key Preview</th
              >
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                >Models</th
              >
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                >Rate Limit</th
              >
              <th
                class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider"
                >Status</th
              >
              <th
                class="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider"
                >Actions</th
              >
            </tr>
          </thead>
          <tbody class="bg-white divide-y divide-gray-200">
            {#each filteredKeys as key}
              <tr>
                <td class="px-6 py-4 whitespace-nowrap">
                  <div class="text-sm font-medium text-gray-900">
                    {key.name}
                  </div>
                  <div class="text-sm text-gray-500">{key.id}</div>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <code class="text-sm bg-gray-100 px-2 py-1 rounded"
                    >{key.key_preview}</code
                  >
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <div class="text-sm text-gray-500">
                    {key.allowed_models.length === 0
                      ? 'All models'
                      : `${key.allowed_models.length} models`}
                  </div>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <div class="text-sm text-gray-500">
                    {key.rate_limit ? `${key.rate_limit}/s` : 'No limit'}
                  </div>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <button
                    onclick={() => handleToggleStatus(key)}
                    class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors
                      {key.is_enabled
                      ? 'bg-green-100 text-green-800 hover:bg-green-200'
                      : 'bg-red-100 text-red-800 hover:bg-red-200'}"
                  >
                    {key.is_enabled ? 'Enabled' : 'Disabled'}
                  </button>
                </td>
                <td
                  class="px-6 py-4 whitespace-nowrap text-right text-sm font-medium"
                >
                  <div class="flex justify-end space-x-2">
                    <button
                      onclick={() => handleEdit(key)}
                      class="text-blue-600 hover:text-blue-900"
                      title="Edit master key"
                    >
                      ✏️
                    </button>
                    <button
                      onclick={() => handleRotate(key)}
                      class="text-yellow-600 hover:text-yellow-900"
                      title="Rotate key"
                    >
                      🔄
                    </button>
                    <button
                      onclick={() => handleDelete(key)}
                      class="text-red-600 hover:text-red-900"
                      title="Delete master key"
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
