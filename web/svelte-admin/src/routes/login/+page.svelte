<script lang="ts">
  import { auth, actions } from '$lib/stores';
  import { ApiClient } from '$lib/api';
  import { Loader2, AlertCircle, Plug } from 'lucide-svelte';

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
        actions.loadCredentials(),
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

<div
  class="min-h-screen flex items-center justify-center bg-linear-to-br from-gray-50 to-gray-100 dark:from-gray-900 dark:to-gray-800 py-12 px-4 sm:px-6 lg:px-8"
>
  <div class="max-w-md w-full">
    <div class="card p-8">
      <!-- Logo and Title -->
      <div class="text-center mb-8">
        <div
          class="inline-flex items-center justify-center w-16 h-16 bg-primary-600 rounded-2xl mb-4"
        >
          <Plug class="w-8 h-8 text-white" />
        </div>
        <h2 class="text-3xl font-bold text-gray-900 dark:text-gray-100">
          LLM Proxy
        </h2>
        <p class="mt-2 text-sm text-gray-600 dark:text-gray-400">
          Sign in to manage your proxy configuration
        </p>
      </div>

      <form
        onsubmit={e => {
          e.preventDefault();
          handleLogin();
        }}
        class="space-y-6"
      >
        {#if error}
          <div class="alert-error">
            <div class="flex">
              <div class="shrink-0">
                <AlertCircle class="h-5 w-5 text-red-400" />
              </div>
              <div class="ml-3">
                <p class="text-sm text-red-700">{error}</p>
              </div>
            </div>
          </div>
        {/if}

        <div>
          <label for="apiKey" class="label"> Admin API Key </label>
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
          <p class="helper-text">
            The admin API key configured in your server's ADMIN_KEY environment
            variable
          </p>
        </div>

        <div>
          <button
            type="submit"
            disabled={isLoading}
            class="btn btn-primary w-full flex justify-center items-center space-x-2"
          >
            {#if isLoading}
              <Loader2 class="w-5 h-5 animate-spin" />
              <span>Connecting...</span>
            {:else}
              <span>Sign In</span>
            {/if}
          </button>
        </div>

        <div class="text-center">
          <div class="text-sm text-gray-600 dark:text-gray-400">
            <p class="mb-2 font-medium">Need help?</p>
            <ul
              class="text-xs space-y-1 text-left bg-gray-50 dark:bg-gray-800 rounded-lg p-3"
            >
              <li class="flex items-start">
                <span class="text-primary-600 mr-2">•</span>
                <span>Make sure your LLM Proxy server is running</span>
              </li>
              <li class="flex items-start">
                <span class="text-primary-600 mr-2">•</span>
                <span>
                  Verify the ADMIN_KEY environment variable is set on the server
                </span>
              </li>
              <li class="flex items-start">
                <span class="text-primary-600 mr-2">•</span>
                <span>
                  Check that VITE_PUBLIC_API_BASE_URL is configured in your .env
                  file
                </span>
              </li>
            </ul>
          </div>
        </div>
      </form>
    </div>
  </div>
</div>
