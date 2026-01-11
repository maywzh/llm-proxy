<script lang="ts">
  import { onMount } from 'svelte';
  import { credentials, loading, errors, actions } from '$lib/stores';
  import { generateApiKey } from '$lib/api';
  import type { Credential, CredentialFormData } from '$lib/types';
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
  } from 'lucide-svelte';

  let searchTerm = $state('');
  let showCreateForm = $state(false);
  let editingCredential: Credential | null = $state(null);
  let deleteConfirm: Credential | null = $state(null);
  let rotateConfirm: Credential | null = $state(null);
  let newRotatedKey: string | null = $state(null);
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
    resetForm();
    showCreateForm = true;
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
      const updateData = {
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
    const success = await actions.deleteCredential(credential.id);
    if (success) {
      deleteConfirm = null;
    }
  }

  async function handleToggleStatus(credential: Credential) {
    await actions.toggleCredentialStatus(credential.id, !credential.is_enabled);
  }

  async function handleRotate(credential: Credential) {
    const newKey = await actions.rotateCredential(credential.id);
    if (newKey) {
      newRotatedKey = newKey;
      rotateConfirm = null;
    }
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
  }
</script>

<svelte:head>
  <title>Credentials - LLM Proxy Admin</title>
</svelte:head>

<div class="space-y-6">
  <!-- Header -->
  <div class="flex justify-between items-center">
    <div>
      <h1 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
        Credentials
      </h1>
      <p class="text-gray-600 dark:text-gray-400">
        Manage API credentials for client authentication
      </p>
    </div>
    <button
      onclick={handleCreate}
      class="btn btn-primary flex items-center space-x-2"
    >
      <Plus class="w-5 h-5" />
      <span>Add Credential</span>
    </button>
  </div>

  <!-- Search -->
  <div class="max-w-md">
    <input
      type="text"
      placeholder="Search credentials..."
      bind:value={searchTerm}
      class="input"
    />
  </div>

  <!-- Error Display -->
  {#if $errors.credentials}
    <div class="alert-error">
      <div class="flex">
        <div class="shrink-0">
          <AlertCircle class="h-5 w-5 text-red-400" />
        </div>
        <div class="ml-3">
          <p class="text-sm text-red-700">{$errors.credentials}</p>
        </div>
        <div class="ml-auto pl-3">
          <button
            onclick={() => actions.clearError('credentials')}
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
      class="modal-overlay"
      onclick={resetForm}
      onkeydown={e => e.key === 'Escape' && resetForm()}
      role="button"
      tabindex="0"
      aria-label="Close modal"
    >
      <div
        class="modal"
        onclick={e => e.stopPropagation()}
        onkeydown={e => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        tabindex="-1"
      >
        <div class="modal-header">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
            {editingCredential ? 'Edit Credential' : 'Add Credential'}
          </h3>
          <button onclick={resetForm} class="btn-icon">
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
              <label for="name" class="label"> Name </label>
              <input
                id="name"
                type="text"
                bind:value={formData.name}
                class="input"
                placeholder="e.g., Production Credential"
                required
              />
            </div>

            <div>
              <label for="rate_limit" class="label">
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
          </div>

          {#if !editingCredential}
            <div>
              <label for="key" class="label"> API Key </label>
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
                  class="btn btn-secondary flex items-center space-x-2"
                  title="Generate new key"
                >
                  <Shuffle class="w-4 h-4" />
                </button>
              </div>
            </div>
          {/if}

          <div>
            <label for="allowed_models" class="label">
              Allowed Models (optional)
            </label>
            <textarea
              id="allowed_models"
              bind:value={allowedModelsText}
              class="input"
              rows={3}
              placeholder="gpt-4&#10;gpt-3.5-turbo&#10;claude-3-sonnet"
            ></textarea>
            <p class="helper-text">
              One model per line. Leave empty to allow all models.
            </p>
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
              Enable this credential
            </label>
          </div>
        </form>

        <div class="modal-footer">
          <button type="button" onclick={resetForm} class="btn btn-secondary">
            Cancel
          </button>
          <button
            type="button"
            onclick={handleSubmit}
            class="btn btn-primary flex items-center space-x-2"
            disabled={$loading.credentials}
          >
            {#if $loading.credentials}
              <Loader2 class="w-4 h-4 animate-spin" />
            {/if}
            <span>
              {editingCredential ? 'Update' : 'Create'} Credential
            </span>
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Credentials List -->
  <div class="card">
    <div class="card-header flex justify-between items-center">
      <h2 class="card-title">
        Credentials ({filteredCredentials.length})
      </h2>
      {#if $loading.credentials}
        <div class="flex items-center text-gray-500 dark:text-gray-400">
          <Loader2 class="w-5 h-5 animate-spin mr-2" />
          <span class="text-sm">Loading...</span>
        </div>
      {/if}
    </div>

    <div class="card-body p-0">
      {#if filteredCredentials.length === 0}
        <div class="text-center py-12 text-gray-500 dark:text-gray-400">
          {searchTerm
            ? 'No credentials match your search.'
            : 'No credentials configured yet.'}
        </div>
      {:else}
        <div class="table-container">
          <table class="table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Key Preview</th>
                <th>Models</th>
                <th>Rate Limit</th>
                <th>Status</th>
                <th class="text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each filteredCredentials as credential (credential.id)}
                <tr>
                  <td>
                    <div
                      class="text-sm font-medium text-gray-900 dark:text-gray-100"
                    >
                      {credential.name}
                    </div>
                    <div class="text-xs text-gray-500 dark:text-gray-400">
                      ID: {credential.id}
                    </div>
                  </td>
                  <td>
                    <code
                      class="text-sm bg-gray-100 dark:bg-gray-700 px-2 py-1 rounded font-mono"
                    >
                      {credential.key_preview}
                    </code>
                  </td>
                  <td>
                    <div class="text-sm text-gray-500 dark:text-gray-400">
                      {credential.allowed_models.length === 0
                        ? 'All models'
                        : `${credential.allowed_models.length} models`}
                    </div>
                  </td>
                  <td>
                    <div class="text-sm text-gray-500 dark:text-gray-400">
                      {credential.rate_limit
                        ? `${credential.rate_limit}/s`
                        : 'No limit'}
                    </div>
                  </td>
                  <td>
                    <button
                      onclick={() => handleToggleStatus(credential)}
                      class="badge transition-colors {credential.is_enabled
                        ? 'badge-success hover:opacity-80'
                        : 'badge-danger hover:opacity-80'}"
                    >
                      {#if credential.is_enabled}
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
                        onclick={() => handleEdit(credential)}
                        class="btn-icon text-primary-600 hover:text-primary-900"
                        title="Edit credential"
                      >
                        <Pencil class="w-4 h-4" />
                      </button>
                      <button
                        onclick={() => (rotateConfirm = credential)}
                        class="btn-icon text-yellow-600 hover:text-yellow-900"
                        title="Rotate key"
                      >
                        <RefreshCw class="w-4 h-4" />
                      </button>
                      <button
                        onclick={() => (deleteConfirm = credential)}
                        class="btn-icon text-red-600 hover:text-red-900"
                        title="Delete credential"
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
      class="modal-overlay"
      onclick={() => (deleteConfirm = null)}
      onkeydown={e => e.key === 'Escape' && (deleteConfirm = null)}
      role="button"
      tabindex="0"
      aria-label="Close modal"
    >
      <div
        class="modal"
        onclick={e => e.stopPropagation()}
        onkeydown={e => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        tabindex="-1"
      >
        <div class="modal-header">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Delete Credential
          </h3>
          <button onclick={() => (deleteConfirm = null)} class="btn-icon">
            <X class="w-5 h-5" />
          </button>
        </div>
        <div class="modal-body">
          <p class="text-sm text-gray-600 dark:text-gray-400">
            Are you sure you want to delete credential
            <strong>{deleteConfirm.name}</strong>? This action cannot be undone.
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
            disabled={$loading.credentials}
          >
            {#if $loading.credentials}
              <Loader2 class="w-4 h-4 animate-spin" />
            {/if}
            <span>Delete</span>
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Rotate Confirmation Modal -->
  {#if rotateConfirm}
    <div
      class="modal-overlay"
      onclick={() => (rotateConfirm = null)}
      onkeydown={e => e.key === 'Escape' && (rotateConfirm = null)}
      role="button"
      tabindex="0"
      aria-label="Close modal"
    >
      <div
        class="modal"
        onclick={e => e.stopPropagation()}
        onkeydown={e => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        tabindex="-1"
      >
        <div class="modal-header">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
            Rotate API Key
          </h3>
          <button onclick={() => (rotateConfirm = null)} class="btn-icon">
            <X class="w-5 h-5" />
          </button>
        </div>
        <div class="modal-body">
          <p class="text-sm text-gray-600 dark:text-gray-400">
            Are you sure you want to rotate the key for
            <strong>{rotateConfirm.name}</strong>? The old key will be
            invalidated immediately.
          </p>
        </div>
        <div class="modal-footer">
          <button
            onclick={() => (rotateConfirm = null)}
            class="btn btn-secondary"
          >
            Cancel
          </button>
          <button
            onclick={() => rotateConfirm && handleRotate(rotateConfirm)}
            class="btn btn-primary flex items-center space-x-2"
            disabled={$loading.credentials}
          >
            {#if $loading.credentials}
              <Loader2 class="w-4 h-4 animate-spin" />
            {/if}
            <span>Rotate Key</span>
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- New Key Display Modal -->
  {#if newRotatedKey}
    <div
      class="modal-overlay"
      onclick={() => (newRotatedKey = null)}
      onkeydown={e => e.key === 'Escape' && (newRotatedKey = null)}
      role="button"
      tabindex="0"
      aria-label="Close modal"
    >
      <div
        class="modal"
        onclick={e => e.stopPropagation()}
        onkeydown={e => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        tabindex="-1"
      >
        <div class="modal-header">
          <h3 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
            New API Key Generated
          </h3>
          <button onclick={() => (newRotatedKey = null)} class="btn-icon">
            <X class="w-5 h-5" />
          </button>
        </div>
        <div class="modal-body">
          <div class="alert-success mb-4">
            <p class="text-sm text-green-700 dark:text-green-400 font-medium">
              Key rotated successfully!
            </p>
          </div>
          <p class="text-sm text-gray-600 dark:text-gray-400 mb-3">
            Save this key securely. It will not be shown again.
          </p>
          <div
            class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4 border border-gray-200 dark:border-gray-600"
          >
            <code class="text-sm font-mono break-all">
              {newRotatedKey}
            </code>
          </div>
        </div>
        <div class="modal-footer">
          <button
            onclick={() => newRotatedKey && copyToClipboard(newRotatedKey)}
            class="btn btn-secondary"
          >
            Copy to Clipboard
          </button>
          <button
            onclick={() => (newRotatedKey = null)}
            class="btn btn-primary"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>
