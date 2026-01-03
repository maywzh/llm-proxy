<script lang="ts">
  import { auth, actions } from '$lib/stores';
  import { ApiClient } from '$lib/api';

  const API_BASE_URL =
    import.meta.env.VITE_PUBLIC_API_BASE_URL || 'http://127.0.0.1:18000';

  let apiKey = $state('');
  let isLoading = $state(false);
  let error = $state('');

  async function handleLogin() {
    error = '';

    if (!apiKey.trim()) {
      error = 'API key is required';
      return;
    }

    isLoading = true;

    try {
      const testClient = new ApiClient(API_BASE_URL, apiKey);
      const result = await testClient.validateAdminKey();

      if (!result.valid) {
        error = result.message || 'Invalid admin key';
        return;
      }

      auth.login(apiKey);

      await Promise.all([
        actions.loadProviders(),
        actions.loadMasterKeys(),
        actions.loadConfigVersion(),
      ]);
    } catch (err) {
      error =
        err instanceof Error ? err.message : 'Failed to connect to the API';
    } finally {
      isLoading = false;
    }
  }

  function handleKeyPress(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      handleLogin();
    }
  }
</script>

<svelte:head>
  <title>Login - LLM Proxy Admin</title>
</svelte:head>

<form
  onsubmit={e => {
    e.preventDefault();
    handleLogin();
  }}
  class="space-y-6"
>
  {#if error}
    <div class="bg-red-50 border border-red-200 rounded-md p-4">
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
          <p class="text-sm text-red-700">{error}</p>
        </div>
      </div>
    </div>
  {/if}

  <div class="space-y-4">
    <div>
      <label for="apiKey" class="block text-sm font-medium text-gray-700">
        Admin API Key
      </label>
      <div class="mt-1">
        <input
          id="apiKey"
          name="apiKey"
          type="password"
          bind:value={apiKey}
          onkeypress={handleKeyPress}
          disabled={isLoading}
          class="input"
          placeholder="Enter your admin API key"
          required
        />
      </div>
      <p class="mt-1 text-xs text-gray-500">
        The admin API key configured in your server's ADMIN_KEY environment
        variable
      </p>
    </div>
  </div>

  <div>
    <button
      type="submit"
      disabled={isLoading}
      class="btn btn-primary w-full flex justify-center items-center space-x-2"
    >
      {#if isLoading}
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
        <span>Connecting...</span>
      {:else}
        <span>Sign In</span>
      {/if}
    </button>
  </div>

  <div class="text-center">
    <div class="text-sm text-gray-600">
      <p class="mb-2">Need help?</p>
      <ul class="text-xs space-y-1">
        <li>• Make sure your LLM Proxy server is running</li>
        <li>
          • Verify the ADMIN_KEY environment variable is set on the server
        </li>
        <li>
          • Check that VITE_PUBLIC_API_BASE_URL is configured in your .env file
        </li>
      </ul>
    </div>
  </div>
</form>
