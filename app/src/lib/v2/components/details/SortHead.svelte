<script lang="ts">
  // Clickable column header; shows an arrow on the active sort column.
  import { ArrowDown, ArrowUp } from '@lucide/svelte';
  import { cn } from '$lib/utils';

  let { label, col, sortKey, desc, onsort, align = 'right', title }: {
    label: string;
    col: string;
    sortKey: string;
    desc: boolean;
    onsort: (col: string) => void;
    align?: 'left' | 'right';
    /** Tooltip that explains the column. */
    title?: string;
  } = $props();
  const active = $derived(sortKey === col);
</script>

<th class={cn('px-3 py-2 font-medium whitespace-nowrap', align === 'right' ? 'text-right' : 'text-left')} aria-sort={active ? (desc ? 'descending' : 'ascending') : 'none'}>
  <button
    type="button"
    onclick={() => onsort(col)}
    {title}
    class={cn('inline-flex items-center gap-1 transition-colors hover:text-foreground', active && 'text-foreground')}
  >
    {label}
    {#if active}{#if desc}<ArrowDown class="size-3" />{:else}<ArrowUp class="size-3" />{/if}{/if}
  </button>
</th>
