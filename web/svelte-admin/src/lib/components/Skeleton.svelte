<script lang="ts">
  interface Props {
    class?: string;
    width?: string;
    height?: string;
  }

  let { class: className = '', width, height }: Props = $props();
</script>

<div
  class="skeleton-shimmer {className}"
  style:width
  style:height
  aria-hidden="true"
></div>
