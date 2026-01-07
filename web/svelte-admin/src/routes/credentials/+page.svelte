<script lang="ts">
  import { onMount } from 'svelte';
  import { credentials, loading, errors, actions } from '$lib/stores';
  import { generateApiKey } from '$lib/api';
  import type { Credential, CredentialFormData } from '$lib/types';

  let searchTerm = $state('');
  let showCreateForm = $state(false);
  let editingCredential: Credential | null = $state(null);
  let formData: CredentialFormData = $state({
    key: '',
    name: '',
    allowed_models: [],
    rate_limit: null,
    is_enabled: true,
  });
  let allowedModelsText = $state('');

  onMount(() => {
    actions.loadCredentials();
  });

  const filteredCredentials = $derived(
    $credentials.filter(
      credential =>
        credential.name.toLowerCase().includes(searchTerm.toLowerCase()) ||
        credential.key_preview.toLowerCase().includes(searchTerm.toLowerCase())
    )
  );

  function resetForm() {
    formData = {
      key: '',
      name: '',
      allowed_models: [],
      rate_limit: null,
      is_enabled: true,
    };
    allowedModelsText = '';
    editingCredential = null;
    showCreateForm = false;
  }

  function handleCreate() {
    showCreateForm = true;
    resetForm();
    formData.key = generateApiKey();
  }

  function handleEdit(credential: Credential) {
    editingCredential = credential;
    formData = {
      key: '', // Don't populate for security
      name: credential.name,
      allowed_models: credential.allowed_models,
      rate_limit: credential.rate_limit,
      is_enabled: credential.is_enabled,
    };
    allowedModelsText = credential.allowed_models.join('\n');
    showCreateForm = true;
  }

  async function handleSubmit() {
    // Update allowed models from text
    formData.allowed_models = allowedModelsText
      .split('\n')
      .map(s => s.trim())
      .filter(s => s.length > 0);

    if (editingCredential) {
      const updateData: any = {
        name: formData.name,
        allowed_models: formData.allowed_models,
        rate_limit: formData.rate_limit,
        is_enabled: formData.is_enabled,
      };

      const success = await actions.updateCredential(
        editingCredential.id,
        updateData
      );
      if (success) resetForm();
    } else {
      const success = await actions.createCredential(formData);
      if (success) resetForm();
    }
  }

  async function handleDelete(credential: Credential) {
    if (
      confirm(
        `Are you sure you want to delete credential "${credential.name}"?`
      )
    ) {
      await actions.deleteCredential(credential.id);
    }
  }

  async function handleToggleStatus(credential: Credential) {
    await actions.toggleCredentialStatus(credential.id, !credential.is_enabled);
  }

  async function handleRotate(credential: Credential) {
    if (
      confirm(
        `Are you sure you want to rotate the key for "${credential.name}"? The old key will be invalidated.`
      )
    ) {
      const newKey = await actions.rotateCredential(credential.id);
      if (newKey) {
        alert(
          `New key generated: ${newKey}\n\nSave this key securely. It will not be shown again.`
        );
      }
    }
  }
</script>

<svelte:head>
  <title>Credentials - LLM Proxy Admin</title>
</svelte:head>

<div class="space-y-6">
  <div class="flex justify-between items-center">
    <div>
      <h1 class="text-2xl font-bold text-gray-900">Credentials</h1>
      <p class="text-gray-600">Manage API keys for client authentication</p>
    </div>
    <button onclick={handleCreate} class="btn btn-primary">
      + Add Credential
    </button>
  </div>

  <div class="max-w-md">
    <input
      type="text"
      placeholder="Search credentials..."
      bind:value={searchTerm}
      class="input"
    />
  </div>

  {#if $errors.credentials}
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
          <p class="text-sm text-red-700">{$errors.credentials}</p>
        </div>
        <div class="ml-auto pl-3">
          <button
            onclick={() => actions.clearError('credentials')}
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
        {editingCredential ? 'Edit Credential' : 'Create New Credential'}
      </h2>

      <form
        onsubmit={e => {
          e.preventDefault();
          handleSubmit();
        }}
        class="space-y-4"
      >
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

        {#if !editingCredential}
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
            Enable this credential
          </label>
        </div>

        <div class="flex justify-end space-x-3">
          <button type="button" onclick={resetForm} class="btn btn-secondary">
            Cancel
          </button>
          <button
            type="submit"
            class="btn btn-primary"
            disabled={$loading.credentials}
          >
            {#if $loading.credentials}
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
            {editingCredential ? 'Update' : 'Create'} Credential
          </button>
        </div>
      </form>
    </div>
  {/if}

  <div class="card">
    <div class="flex justify-between items-center mb-4">
      <h2 class="text-lg font-semibold">
        Credentials ({filteredCredentials.length})
      </h2>
      {#if $loading.credentials}
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

    {#if filteredCredentials.length === 0}
      <div class="text-center py-8 text-gray-500">
        {searchTerm
          ? 'No credentials match your search.'
          : 'No credentials configured yet.'}
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
            {#each filteredCredentials as credential}
              <tr>
                <td class="px-6 py-4 whitespace-nowrap">
                  <div class="text-sm font-medium text-gray-900">
                    {credential.name}
                  </div>
                  <div class="text-sm text-gray-500">ID: {credential.id}</div>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <code class="text-sm bg-gray-100 px-2 py-1 rounded"
                    >{credential.key_preview}</code
                  >
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <div class="text-sm text-gray-500">
                    {credential.allowed_models.length === 0
                      ? 'All models'
                      : `${credential.allowed_models.length} models`}
                  </div>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <div class="text-sm text-gray-500">
                    {credential.rate_limit
                      ? `${credential.rate_limit}/s`
                      : 'No limit'}
                  </div>
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <button
                    onclick={() => handleToggleStatus(credential)}
                    class="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium transition-colors
                      {credential.is_enabled
                      ? 'bg-green-100 text-green-800 hover:bg-green-200'
                      : 'bg-red-100 text-red-800 hover:bg-red-200'}"
                  >
                    {credential.is_enabled ? 'Enabled' : 'Disabled'}
                  </button>
                </td>
                <td
                  class="px-6 py-4 whitespace-nowrap text-right text-sm font-medium"
                >
                  <div class="flex justify-end space-x-2">
                    <button
                      onclick={() => handleEdit(credential)}
                      class="text-blue-600 hover:text-blue-900"
                      title="Edit credential"
                    >
                      ✏️
                    </button>
                    <button
                      onclick={() => handleRotate(credential)}
                      class="text-yellow-600 hover:text-yellow-900"
                      title="Rotate key"
                    >
                      🔄
                    </button>
                    <button
                      onclick={() => handleDelete(credential)}
                      class="text-red-600 hover:text-red-900"
                      title="Delete credential"
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
