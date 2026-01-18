<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { browser } from '$app/environment';
  import '../app.css';
  import { auth, actions, configVersion, errors } from '$lib/stores';
  import { theme } from '$lib/theme';
  import {
    Plug,
    Key,
    RefreshCw,
    LogOut,
    Menu,
    X,
    LayoutDashboard,
    Sun,
    Moon,
    Monitor,
    MessageSquare,
    ChevronLeft,
    ChevronRight,
    Activity,
  } from 'lucide-svelte';

  let { children }: { children: Snippet } = $props();

  const SIDEBAR_COLLAPSED_STORAGE_KEY = 'llm_proxy_sidebar_collapsed';

  let isMobileMenuOpen = $state(false);
  let isReloading = $state(false);
  let showThemeMenu = $state(false);
  let isSidebarCollapsed = $state(false);

  $effect(() => {
    if (showThemeMenu && browser) {
      const handleClick = (e: MouseEvent) => {
        const target = e.target as HTMLElement;
        if (!target.closest('.theme-menu-container')) {
          showThemeMenu = false;
        }
      };
      document.addEventListener('click', handleClick);
      return () => document.removeEventListener('click', handleClick);
    }
  });

  onMount(() => {
    auth.init();
    theme.init();
    if (browser) {
      isSidebarCollapsed =
        localStorage.getItem(SIDEBAR_COLLAPSED_STORAGE_KEY) === '1';
    }
  });

  $effect(() => {
    if (browser) {
      localStorage.setItem(
        SIDEBAR_COLLAPSED_STORAGE_KEY,
        isSidebarCollapsed ? '1' : '0'
      );
    }
  });

  $effect(() => {
    if (browser) {
      if (
        $auth.isAuthenticated &&
        ($page.url.pathname === '/' || $page.url.pathname === '/login')
      ) {
        goto('/dashboard');
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

  async function handleReloadConfig() {
    isReloading = true;
    try {
      await actions.reloadConfig();
    } finally {
      isReloading = false;
    }
  }

  function clearError(type: keyof typeof $errors) {
    actions.clearError(type);
  }

  function setTheme(newTheme: 'light' | 'dark' | 'system') {
    theme.set(newTheme);
    showThemeMenu = false;
  }

  function toggleSidebarCollapsed() {
    isSidebarCollapsed = !isSidebarCollapsed;
  }

  const navItems = [
    { href: '/dashboard', label: 'Dashboard', icon: LayoutDashboard },
    { href: '/providers', label: 'Providers', icon: Plug },
    { href: '/credentials', label: 'Credentials', icon: Key },
    { href: '/health', label: 'Health Check', icon: Activity },
    { href: '/chat', label: 'Chat', icon: MessageSquare },
  ];
</script>

<div class="min-h-screen bg-gray-50 dark:bg-gray-900">
  {#if $auth.isAuthenticated}
    <aside
      class={`hidden lg:fixed lg:inset-y-0 lg:flex lg:flex-col transition-all duration-200 relative ${
        isSidebarCollapsed ? 'lg:w-16' : 'lg:w-64'
      }`}
    >
      <div class="flex flex-col grow bg-gray-900 overflow-y-auto">
        <div
          class="flex items-center shrink-0 px-4 py-5 border-b border-gray-800"
        >
          <div class="flex items-center space-x-3">
            <div
              class="w-8 h-8 rounded-lg overflow-hidden flex items-center justify-center"
            >
              <img
                src="/logo.png"
                alt="LLM Proxy"
                class="w-full h-full object-cover"
                draggable="false"
              />
            </div>
            {#if !isSidebarCollapsed}
              <span class="text-xl font-semibold text-white"> LLM Proxy </span>
            {/if}
          </div>
        </div>

        <nav class="flex-1 px-2 py-4 space-y-1">
          {#each navItems as item (item.href)}
            {@const Icon = item.icon}
            {@const isActive = $page.url.pathname === item.href}
            <a
              href={item.href}
              class={`sidebar-nav-item ${isActive ? 'active' : ''} ${
                isSidebarCollapsed ? 'justify-center px-0 space-x-0' : ''
              }`}
              title={isSidebarCollapsed ? item.label : undefined}
              aria-label={item.label}
            >
              <Icon class="w-5 h-5" />
              {#if !isSidebarCollapsed}
                <span>{item.label}</span>
              {/if}
            </a>
          {/each}
        </nav>

        <div
          class={`shrink-0 border-t border-gray-800 ${
            isSidebarCollapsed ? 'p-2' : 'p-4'
          }`}
        >
          <button
            onclick={handleLogout}
            class={`flex items-center w-full px-4 py-3 text-gray-300 dark:text-gray-400 hover:bg-gray-800 dark:hover:bg-gray-700 hover:text-white transition-colors duration-200 rounded-lg ${
              isSidebarCollapsed ? 'justify-center px-0 space-x-0' : 'space-x-3'
            }`}
            title={isSidebarCollapsed ? 'Logout' : undefined}
            aria-label="Logout"
          >
            <LogOut class="w-5 h-5" />
            {#if !isSidebarCollapsed}
              <span>Logout</span>
            {/if}
          </button>
        </div>
      </div>
      <button
        onclick={toggleSidebarCollapsed}
        class="absolute top-23 left-full -translate-x-1/2 -translate-y-1/2 w-5 h-5 rounded-lg flex items-center justify-center bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 shadow-md text-gray-700 dark:text-gray-200 hover:bg-gray-50 dark:hover:bg-gray-700 z-50"
        title={isSidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        aria-label={isSidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      >
        {#if isSidebarCollapsed}
          <ChevronRight class="w-4 h-4" />
        {:else}
          <ChevronLeft class="w-4 h-4" />
        {/if}
      </button>
    </aside>

    {#if isMobileMenuOpen}
      <div class="lg:hidden">
        <div
          class="fixed inset-0 bg-black bg-opacity-50 dark:bg-opacity-70 z-40"
          onclick={() => (isMobileMenuOpen = false)}
          onkeydown={e => e.key === 'Escape' && (isMobileMenuOpen = false)}
          role="button"
          tabindex="0"
          aria-label="Close menu"
        ></div>
        <aside
          class="fixed inset-y-0 left-0 flex flex-col w-64 bg-gray-900 z-50"
        >
          <div
            class="flex items-center justify-between px-4 py-5 border-b border-gray-800"
          >
            <div class="flex items-center space-x-3">
              <div
                class="w-8 h-8 rounded-lg overflow-hidden flex items-center justify-center"
              >
                <img
                  src="/logo.png"
                  alt="LLM Proxy"
                  class="w-full h-full object-cover"
                  draggable="false"
                />
              </div>
              <span class="text-xl font-semibold text-white"> LLM Proxy </span>
            </div>
            <button
              onclick={() => (isMobileMenuOpen = false)}
              class="text-gray-400 hover:text-white"
            >
              <X class="w-6 h-6" />
            </button>
          </div>
          <nav class="flex-1 px-2 py-4 space-y-1">
            {#each navItems as item (item.href)}
              {@const Icon = item.icon}
              {@const isActive = $page.url.pathname === item.href}
              <a
                href={item.href}
                onclick={() => (isMobileMenuOpen = false)}
                class="sidebar-nav-item {isActive ? 'active' : ''}"
              >
                <Icon class="w-5 h-5" />
                <span>{item.label}</span>
              </a>
            {/each}
          </nav>
          <div class="shrink-0 border-t border-gray-800 p-4">
            <button
              onclick={() => {
                handleLogout();
                isMobileMenuOpen = false;
              }}
              class="flex items-center space-x-3 w-full px-4 py-3 text-gray-300 dark:text-gray-400 hover:bg-gray-800 dark:hover:bg-gray-700 hover:text-white transition-colors duration-200 rounded-lg"
            >
              <LogOut class="w-5 h-5" />
              <span>Logout</span>
            </button>
          </div>
        </aside>
      </div>
    {/if}

    <div
      class={`flex flex-col flex-1 transition-all duration-200 ${
        isSidebarCollapsed ? 'lg:pl-16' : 'lg:pl-64'
      }`}
    >
      <header
        class="sticky top-0 z-30 bg-white dark:bg-gray-800 shadow-sm border-b border-gray-200 dark:border-gray-700"
      >
        <div class="px-4 sm:px-6 lg:px-8">
          <div class="flex items-center justify-between h-16">
            <button
              onclick={() => (isMobileMenuOpen = true)}
              class="lg:hidden btn-icon"
            >
              <Menu class="w-6 h-6" />
            </button>

            <div class="hidden lg:block">
              <h1
                class="text-xl font-semibold text-gray-900 dark:text-gray-100"
              >
                {navItems.find(item => item.href === $page.url.pathname)
                  ?.label || 'Admin'}
              </h1>
            </div>

            <div class="flex items-center space-x-4 ml-auto">
              {#if $configVersion}
                <span class="badge badge-info">
                  v{$configVersion.version}
                </span>
              {/if}

              <div class="relative theme-menu-container">
                <button
                  onclick={() => (showThemeMenu = !showThemeMenu)}
                  class="btn btn-secondary text-sm flex items-center space-x-2"
                  title="Theme"
                >
                  {#if $theme === 'light'}
                    <Sun class="w-4 h-4" />
                  {:else if $theme === 'dark'}
                    <Moon class="w-4 h-4" />
                  {:else}
                    <Monitor class="w-4 h-4" />
                  {/if}
                </button>

                {#if showThemeMenu}
                  <div
                    class="absolute right-0 mt-2 w-40 bg-white dark:bg-gray-800 rounded-lg shadow-lg border border-gray-200 dark:border-gray-700 py-1 z-50"
                    onclick={e => e.stopPropagation()}
                    onkeydown={e => e.stopPropagation()}
                    role="menu"
                    tabindex="-1"
                  >
                    <button
                      onclick={() => setTheme('light')}
                      class="w-full px-4 py-2 text-left text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 flex items-center space-x-2 {$theme ===
                      'light'
                        ? 'bg-gray-100 dark:bg-gray-700'
                        : ''}"
                    >
                      <Sun class="w-4 h-4" />
                      <span>Light</span>
                    </button>
                    <button
                      onclick={() => setTheme('dark')}
                      class="w-full px-4 py-2 text-left text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 flex items-center space-x-2 {$theme ===
                      'dark'
                        ? 'bg-gray-100 dark:bg-gray-700'
                        : ''}"
                    >
                      <Moon class="w-4 h-4" />
                      <span>Dark</span>
                    </button>
                    <button
                      onclick={() => setTheme('system')}
                      class="w-full px-4 py-2 text-left text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 flex items-center space-x-2 {$theme ===
                      'system'
                        ? 'bg-gray-100 dark:bg-gray-700'
                        : ''}"
                    >
                      <Monitor class="w-4 h-4" />
                      <span>System</span>
                    </button>
                  </div>
                {/if}
              </div>

              <button
                onclick={handleReloadConfig}
                disabled={isReloading}
                class="btn btn-secondary text-sm flex items-center space-x-2"
                title="Reload Configuration"
              >
                <RefreshCw
                  class="w-4 h-4 {isReloading ? 'animate-spin' : ''}"
                />
                <span class="hidden sm:inline">Reload</span>
              </button>
            </div>
          </div>
        </div>
      </header>

      {#if $errors.general}
        <div class="alert-error mx-4 mt-4">
          <div class="flex">
            <div class="shrink-0">
              <svg
                class="h-5 w-5 text-red-400 dark:text-red-500"
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
              <p class="text-sm text-red-700 dark:text-red-400">
                {$errors.general}
              </p>
            </div>
            <div class="ml-auto pl-3">
              <button
                onclick={() => clearError('general')}
                class="text-red-400 dark:text-red-500 hover:text-red-600 dark:hover:text-red-400"
                aria-label="Close error message"
              >
                <X class="h-5 w-5" />
              </button>
            </div>
          </div>
        </div>
      {/if}

      <main class="flex-1 py-6 px-4 sm:px-6 lg:px-8">
        {@render children()}
      </main>
    </div>
  {:else}
    {@render children()}
  {/if}
</div>
