<script lang="ts">
  import { ArrowUp, ArrowDown, Minus } from '@lucide/svelte';
  import { cn } from '$lib/utils';
  // `value` is the signed change. `lowerIsBetter` flips the colors (e.g. deaths).
  let { value, text, lowerIsBetter = false, class: cls = '' }: {
    value: number;
    text?: string;
    lowerIsBetter?: boolean;
    class?: string;
  } = $props();
  const dir = $derived(Math.abs(value) < 1e-9 ? 0 : value > 0 ? 1 : -1);
  const good = $derived(dir === 0 ? null : lowerIsBetter ? dir < 0 : dir > 0);
</script>

<span
  class={cn(
    'inline-flex items-center gap-0.5 rounded-full px-2 py-0.5 text-xs font-medium tabular-nums',
    good === null ? 'bg-surface-3 text-muted' : good ? 'bg-good/15 text-good' : 'bg-bad/15 text-bad',
    cls
  )}
>
  {#if dir > 0}<ArrowUp class="size-3" />{:else if dir < 0}<ArrowDown class="size-3" />{:else}<Minus class="size-3" />{/if}
  {text ?? Math.abs(value).toFixed(2)}
</span>
