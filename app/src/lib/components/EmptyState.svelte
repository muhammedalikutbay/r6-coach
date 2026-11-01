<script lang="ts">
  import type { Snippet, Component } from 'svelte';
  import { Inbox } from '@lucide/svelte';
  let { title, text = '', icon = Inbox, children }: {
    title: string;
    text?: string;
    icon?: Component<any>;
    children?: Snippet;
  } = $props();
  const Icon = $derived(icon);
</script>

<div class="flex flex-col items-center gap-2 rounded-xl border border-dashed border-border-strong px-6 py-12 text-center">
  <Icon class="size-8 text-muted" />
  <div class="text-base font-semibold">{title}</div>
  {#if text}<p class="max-w-sm text-muted">{text}</p>{/if}
  {#if children}<div class="mt-2">{@render children()}</div>{/if}
</div>
