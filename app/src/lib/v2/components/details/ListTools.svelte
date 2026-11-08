<script lang="ts">
  // Search box for the list screens.
  import { Search } from '@lucide/svelte';

  let { value = $bindable(''), placeholder = 'Search' }: { value?: string; placeholder?: string } = $props();
</script>

<label class="relative block w-64 max-w-full">
  <Search class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted" />
  <input
    type="search"
    bind:value
    {placeholder}
    class="h-9 w-full rounded-lg border border-border-strong bg-surface-2 pr-3 pl-8 text-sm outline-none focus:border-accent"
  />
</label>
