<script lang="ts">
  import type { Snippet } from 'svelte';
  let { title, sub = '', actions }: { title: string; sub?: string; actions?: Snippet } = $props();
</script>

<div class="mb-5 flex items-center justify-between gap-3">
  <div>
    <h1 class="text-xl font-bold">{title}</h1>
    {#if sub}<p class="text-muted">{sub}</p>{/if}
  </div>
  {#if actions}<div>{@render actions()}</div>{/if}
</div>
