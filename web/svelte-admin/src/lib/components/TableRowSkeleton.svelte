<script lang="ts">
  import Skeleton from './Skeleton.svelte';

  interface Props {
    columns?: number;
  }

  let { columns = 6 }: Props = $props();
</script>

<tr>
  {#each Array(columns) as _, i}
    <td class="px-6 py-4">
      <Skeleton class="h-4 w-full" />
    </td>
  {/each}
</tr>
