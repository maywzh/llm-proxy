<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { browser } from '$app/environment';

  // Redirect to login page
  onMount(() => {
    if (browser) {
      goto('/login');
    }
  });
</script>

<svelte:head>
  <title>LLM Proxy Admin</title>
</svelte:head>

<!-- This page redirects to /login -->
<div class="min-h-screen flex items-center justify-center">
  <div class="text-center">
    <div
      class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600 mx-auto"
    ></div>
    <p class="mt-2 text-gray-600">Redirecting to login...</p>
  </div>
</div>
