<script lang="ts">
  import Skeleton from './Skeleton.svelte';
</script>

<div class="card">
  <div class="card-body">
    <div class="flex items-start justify-between mb-4">
      <div class="flex items-center space-x-3 flex-1">
        <Skeleton class="w-5 h-5 rounded-full" />
        <div class="flex-1 space-y-2">
          <Skeleton class="h-5 w-32" />
          <Skeleton class="h-4 w-16" />
        </div>
      </div>
    </div>
    <div class="space-y-2 mb-4">
      <div class="flex justify-between">
        <Skeleton class="h-4 w-24" />
        <Skeleton class="h-4 w-8" />
      </div>
      <div class="flex justify-between">
        <Skeleton class="h-4 w-24" />
        <Skeleton class="h-4 w-12" />
      </div>
    </div>
    <Skeleton class="h-9 w-full rounded-lg" />
  </div>
</div>
