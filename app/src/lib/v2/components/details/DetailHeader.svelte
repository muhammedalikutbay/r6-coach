<script lang="ts">
  // Back link, title and a few chips at the top of an operator or weapon screen.
  import type { Snippet } from 'svelte';
  import { ArrowLeft } from '@lucide/svelte';

  let { backHref, backLabel, title, sub = '', lead, chips }: {
    backHref: string;
    backLabel: string;
    title: string;
    sub?: string;
    lead?: Snippet;
    chips?: Snippet;
  } = $props();
</script>

<a href={backHref} class="mb-3 inline-flex items-center gap-1.5 text-xs text-muted transition-colors hover:text-foreground">
  <ArrowLeft class="size-3.5" />{backLabel}
</a>
<div class="mb-5 flex items-center gap-4">
  {#if lead}{@render lead()}{/if}
  <div class="min-w-0 flex-1">
    <h1 class="truncate text-xl font-bold">{title}</h1>
    {#if sub}<p class="text-muted">{sub}</p>{/if}
  </div>
  {#if chips}<div class="flex flex-wrap items-center justify-end gap-2">{@render chips()}</div>{/if}
</div>
