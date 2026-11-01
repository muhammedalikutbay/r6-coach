<script lang="ts">
  import { normKey } from '$lib/v2/catalog';
  import { cn } from '$lib/utils';
  // `opKey` is the lowercase operator key, used for the image file name.
  // Unknown operators (no image) fall back to the initials of `name` (or of the raw key).
  let { opKey, name = '', size = 32, class: cls = '' }: {
    opKey: string;
    name?: string;
    size?: number;
    class?: string;
  } = $props();
  let failed = $state(false);
  const label = $derived(name || opKey);
  const initials = $derived(label.replace(/[^A-Za-z0-9 ]/g, '').trim().slice(0, 2).toUpperCase() || '?');
  const safeKey = $derived(normKey(opKey));
  $effect(() => {
    safeKey;
    failed = false;
  });
</script>

<span
  class={cn('inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-surface-3 font-semibold text-muted', cls)}
  style="width:{size}px;height:{size}px;font-size:{size * 0.4}px"
  title={label}
>
  {#if safeKey && !failed}
    <img src="/assets/operators/{safeKey}.png" alt={label} class="size-full object-cover" onerror={() => (failed = true)} />
  {:else}
    {initials}
  {/if}
</span>
