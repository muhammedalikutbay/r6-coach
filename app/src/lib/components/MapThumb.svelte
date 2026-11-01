<script lang="ts">
  import { cn } from '$lib/utils';
  // `mapKey` is the lowercase map key, used for the image file name.
  // Without an image: gradient plus the map name.
  let { mapKey, name = '', class: cls = '' }: { mapKey: string; name?: string; class?: string } = $props();
  let failed = $state(false);
  const label = $derived(name || mapKey);
  const safeKey = $derived(mapKey.toLowerCase().replace(/[^a-z0-9_-]/g, ''));
  $effect(() => {
    safeKey;
    failed = false;
  });
</script>

<div
  class={cn('relative aspect-video overflow-hidden rounded-lg bg-gradient-to-br from-surface-3 to-surface-1', cls)}
  title={label}
>
  {#if safeKey && !failed}
    <img src="/assets/maps/{safeKey}.jpg" alt={label} class="size-full object-cover" onerror={() => (failed = true)} />
  {:else}
    <span class="absolute inset-0 flex items-center justify-center px-2 text-center text-xs font-semibold text-muted">{label}</span>
  {/if}
</div>
