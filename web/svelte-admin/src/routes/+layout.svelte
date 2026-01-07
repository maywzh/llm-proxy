<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { browser } from '$app/environment';
  import '../app.css';
  import { auth, actions, configVersion, errors } from '$lib/stores';

  let { children }: { children: Snippet } = $props();

  // Initialize auth on mount
  onMount(() => {
    auth.init();
  });

  // Reactive navigation based on auth state (only in browser)
  $effect(() => {
    if (browser) {
      if (
        $auth.isAuthenticated &&
        ($page.url.pathname === '/' || $page.url.pathname === '/login')
      ) {
        goto('/providers');
      } else if (
        !$auth.isAuthenticated &&
        $page.url.pathname !== '/' &&
        $page.url.pathname !== '/login'
      ) {
        goto('/login');
      }
    }
  });

  function handleLogout() {
    auth.logout();
    goto('/login');
  }

  function handleReloadConfig() {
    actions.reloadConfig();
  }

  function clearError(type: keyof typeof $errors) {
    actions.clearError(type);
  }

  // Navigation items
  const navItems = [
    { href: '/providers', label: 'Providers', icon: '🔌' },
    { href: '/credentials', label: 'Credentials', icon: '🔑' },
  ];
</script>

<div class="min-h-screen bg-gray-50">
  {#if $auth.isAuthenticated}
    <!-- Header -->
    <header class="bg-white shadow-sm border-b border-gray-200">
      <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div class="flex justify-between items-center h-16">
          <!-- Logo and Title -->
          <div class="flex items-center">
            <h1 class="text-xl font-semibold text-gray-900">LLM Proxy Admin</h1>
          </div>

          <!-- Navigation -->
          <nav class="hidden md:flex space-x-8">
            {#each navItems as item}
              <a
                href={item.href}
                class="flex items-center space-x-2 px-3 py-2 rounded-md text-sm font-medium transition-colors duration-200
                  {$page.url.pathname === item.href
                  ? 'bg-blue-100 text-blue-700'
                  : 'text-gray-600 hover:text-gray-900 hover:bg-gray-100'}"
              >
                <span>{item.icon}</span>
                <span>{item.label}</span>
              </a>
            {/each}
          </nav>

          <!-- Config Version and Actions -->
          <div class="flex items-center space-x-4">
            {#if $configVersion}
              <div class="text-sm text-gray-500">
                v{$configVersion.version}
              </div>
            {/if}

            <button
              onclick={handleReloadConfig}
              class="btn btn-secondary text-sm"
              title="Reload Configuration"
            >
              🔄 Reload
            </button>

            <button onclick={handleLogout} class="btn btn-secondary text-sm">
              Logout
            </button>
          </div>
        </div>

        <!-- Mobile Navigation -->
        <div class="md:hidden pb-3">
          <nav class="flex space-x-4">
            {#each navItems as item}
              <a
                href={item.href}
                class="flex items-center space-x-2 px-3 py-2 rounded-md text-sm font-medium transition-colors duration-200
                  {$page.url.pathname === item.href
                  ? 'bg-blue-100 text-blue-700'
                  : 'text-gray-600 hover:text-gray-900 hover:bg-gray-100'}"
              >
                <span>{item.icon}</span>
                <span>{item.label}</span>
              </a>
            {/each}
          </nav>
        </div>
      </div>
    </header>

    <!-- Error Notifications -->
    {#if $errors.general}
      <div class="bg-red-50 border-l-4 border-red-400 p-4 mx-4 mt-4">
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
              />
            </svg>
          </div>
          <div class="ml-3">
            <p class="text-sm text-red-700">{$errors.general}</p>
          </div>
          <div class="ml-auto pl-3">
            <button
              onclick={() => clearError('general')}
              class="text-red-400 hover:text-red-600"
              aria-label="Close error message"
            >
              <svg class="h-5 w-5" viewBox="0 0 20 20" fill="currentColor">
                <path
                  fill-rule="evenodd"
                  d="M4.293 4.293a1 1 0 011.414 0L10 8.586l4.293-4.293a1 1 0 111.414 1.414L11.414 10l4.293 4.293a1 1 0 01-1.414 1.414L10 11.414l-4.293 4.293a1 1 0 01-1.414-1.414L8.586 10 4.293 5.707a1 1 0 010-1.414z"
                  clip-rule="evenodd"
                />
              </svg>
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- Main Content -->
    <main class="max-w-7xl mx-auto py-6 px-4 sm:px-6 lg:px-8">
      {@render children()}
    </main>
  {:else}
    <!-- Login Layout -->
    <div
      class="min-h-screen flex items-center justify-center bg-gray-50 py-12 px-4 sm:px-6 lg:px-8"
    >
      <div class="max-w-md w-full space-y-8">
        <div>
          <h2 class="mt-6 text-center text-3xl font-extrabold text-gray-900">
            LLM Proxy Admin
          </h2>
          <p class="mt-2 text-center text-sm text-gray-600">
            Sign in to manage your LLM proxy configuration
          </p>
        </div>

        {@render children()}
      </div>
    </div>
  {/if}
</div>
