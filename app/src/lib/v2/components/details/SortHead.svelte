<script lang="ts">
  // Clickable column header; shows an arrow on the active sort column.
  import { ArrowDown, ArrowUp } from '@lucide/svelte';
  import { cn } from '$lib/utils';

  let { label, col, sortKey, desc, onsort, align = 'right' }: {
    label: string;
    col: string;
    sortKey: string;
    desc: boolean;
    onsort: (col: string) => void;
    align?: 'left' | 'right';
  } = $props();
  const active = $derived(sortKey === col);
</script>

<th class={cn('py-2 font-medium', align === 'right' ? 'text-right' : 'text-left')} aria-sort={active ? (desc ? 'descending' : 'ascending') : 'none'}>
  <button
    type="button"
    onclick={() => onsort(col)}
    class={cn('inline-flex items-center gap-1 transition-colors hover:text-foreground', active && 'text-foreground')}
  >
    {label}
    {#if active}{#if desc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}{/if}
  </button>
</th>
