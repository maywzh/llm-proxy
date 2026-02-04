<script lang="ts">
  import Skeleton from './Skeleton.svelte';
</script>

<div class="w-full space-y-4 animate-fade-in">
  <div class="flex justify-center">
    <Skeleton class="h-8 w-48" />
  </div>
  <Skeleton class="h-64 w-full rounded-lg" />
  <div class="flex justify-between gap-4">
    <Skeleton class="h-6 w-1/4" />
    <Skeleton class="h-6 w-1/4" />
    <Skeleton class="h-6 w-1/4" />
  </div>
  <Skeleton class="h-48 w-full rounded-lg" />
</div>
