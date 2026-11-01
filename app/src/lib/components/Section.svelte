<script lang="ts">
  import type { Snippet } from 'svelte';
  let { title, hint = '', actions, children }: {
    title: string;
    hint?: string;
    actions?: Snippet;
    children: Snippet;
  } = $props();
</script>

<section class="mb-6">
  <header class="mb-3 flex items-end justify-between gap-3">
    <div>
      <h2 class="text-sm font-semibold tracking-wide uppercase">{title}</h2>
      {#if hint}<p class="text-xs text-muted">{hint}</p>{/if}
    </div>
    {#if actions}<div class="flex items-center gap-2">{@render actions()}</div>{/if}
  </header>
  {@render children()}
</section>
