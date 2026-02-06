<script lang="ts">
  import { auth, actions } from '$lib/stores';
  import { ApiClient } from '$lib/api';
  import { Loader2, AlertCircle, ChevronDown, ChevronUp } from 'lucide-svelte';

  const API_BASE_URL =
    import.meta.env.VITE_PUBLIC_API_BASE_URL || 'http://127.0.0.1:18000';

  let apiKey = $state('');
  let isLoading = $state(false);
  let error = $state('');
  let showHelp = $state(false);
  let hasError = $state(false);
  let rememberMe = $state(
    typeof localStorage !== 'undefined'
      ? localStorage.getItem('llm-proxy-remember-me') === '1'
      : false
  );

  async function handleLogin() {
    error = '';
    hasError = false;

    if (!apiKey.trim()) {
      error = 'API key is required';
      hasError = true;
      return;
    }

    isLoading = true;

    try {
      const testClient = new ApiClient(API_BASE_URL, apiKey);
      const result = await testClient.validateAdminKey();

      if (!result.valid) {
        error = result.message || 'Invalid admin key';
        hasError = true;
        return;
      }

      auth.login(apiKey);

      if (rememberMe) {
        localStorage.setItem('llm-proxy-remember-me', '1');
      } else {
        localStorage.removeItem('llm-proxy-remember-me');
      }

      await Promise.all([
        actions.loadProviders(),
        actions.loadCredentials(),
        actions.loadConfigVersion(),
      ]);
    } catch (err) {
      error =
        err instanceof Error ? err.message : 'Failed to connect to the API';
      hasError = true;
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
  <title>Login - HEN Admin</title>
</svelte:head>

<div
  class="min-h-screen flex items-center justify-center bg-linear-to-br from-gray-50 to-gray-100 dark:from-gray-900 dark:to-gray-800 py-12 px-4 sm:px-6 lg:px-8 relative overflow-hidden"
>
  <!-- Background decorations -->
  <div class="absolute inset-0 -z-10 overflow-hidden">
    <div
      class="absolute -top-40 -right-40 w-80 h-80 bg-primary-500/10 rounded-full blur-3xl"
    ></div>
    <div
      class="absolute -bottom-40 -left-40 w-80 h-80 bg-primary-600/10 rounded-full blur-3xl"
    ></div>
    <div
      class="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-96 h-96 bg-primary-400/5 rounded-full blur-3xl"
    ></div>
  </div>

  <div class="max-w-md w-full">
    <div class="card p-8 animate-fade-in">
      <!-- Logo and Title -->
      <div class="text-center mb-8">
        <div
          class="inline-flex items-center justify-center w-16 h-16 bg-primary-600 rounded-2xl mb-4 animate-fade-in"
        >
          <img src="/logo.png" alt="HEN" class="w-10 h-10" draggable="false" />
        </div>
        <h2 class="text-3xl font-bold text-gray-900 dark:text-gray-100">HEN</h2>
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
          <div
            class="alert-error {hasError ? 'animate-shake' : ''}"
            role="alert"
            id="apiKey-error"
          >
            <div class="flex">
              <div class="shrink-0">
                <AlertCircle class="h-5 w-5 text-red-400" />
              </div>
              <div class="ml-3">
                <p class="text-sm text-red-700 dark:text-red-400">{error}</p>
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
              aria-describedby={error ? 'apiKey-error' : 'apiKey-hint'}
              aria-invalid={!!error}
            />
          </div>
          <p id="apiKey-hint" class="helper-text">
            The admin API key configured in your server's ADMIN_KEY environment
            variable
          </p>
        </div>

        <div class="flex items-center">
          <input
            id="remember-me"
            type="checkbox"
            bind:checked={rememberMe}
            class="h-4 w-4 text-primary-600 focus:ring-primary-500 border-gray-300 rounded cursor-pointer"
          />
          <label
            for="remember-me"
            class="ml-2 text-sm text-gray-600 dark:text-gray-400 cursor-pointer"
          >
            Remember me
          </label>
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
          <button
            type="button"
            onclick={() => (showHelp = !showHelp)}
            class="flex items-center justify-center space-x-1 text-sm text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 mx-auto transition-colors"
          >
            <span>Need help?</span>
            {#if showHelp}
              <ChevronUp class="w-4 h-4" />
            {:else}
              <ChevronDown class="w-4 h-4" />
            {/if}
          </button>
          {#if showHelp}
            <ul
              class="text-xs space-y-1 text-left bg-gray-50 dark:bg-gray-800 rounded-lg p-3 mt-2 animate-slide-up"
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
          {/if}
        </div>
      </form>
    </div>
  </div>
</div>
