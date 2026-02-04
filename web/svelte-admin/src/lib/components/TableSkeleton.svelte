<script lang="ts">
  import Skeleton from './Skeleton.svelte';
  import TableRowSkeleton from './TableRowSkeleton.svelte';

  interface Props {
    rows?: number;
    columns?: number;
  }

  let { rows = 5, columns = 6 }: Props = $props();
</script>

<div class="table-container">
  <table class="table">
    <thead>
      <tr>
        {#each Array(columns) as _, i}
          <th>
            <Skeleton class="h-3 w-20" />
          </th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#each Array(rows) as _, i}
        <TableRowSkeleton {columns} />
      {/each}
    </tbody>
  </table>
</div>
