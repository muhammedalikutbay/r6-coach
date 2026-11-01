<script lang="ts">
  // Placeholder: the real global filter bar (range, mode, side, operators, maps) replaces this component.
  import { SlidersHorizontal } from '@lucide/svelte';
</script>

<div class="flex h-9 items-center gap-2 text-xs text-muted" data-slot="filter-bar">
  <SlidersHorizontal class="size-4" />
  <span>Filters will appear here</span>
</div>
