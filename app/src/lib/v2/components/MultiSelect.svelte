<script lang="ts" generics="T extends { key: string; name: string }">
  // Searchable multi-select popover. `group` splits the list into labelled sections (e.g. attackers / defenders).
  import type { Snippet } from 'svelte';
  import { ChevronDown, Search } from '@lucide/svelte';
  import { cn } from '$lib/utils';

  let { label, items, selected, ontoggle, row, group }: {
    label: string;
    items: T[];
    selected: string[];
    ontoggle: (key: string) => void;
    row: Snippet<[T]>;
    group?: (item: T) => string;
  } = $props();

  let open = $state(false);
  let q = $state('');
  let root: HTMLDivElement | undefined = $state();

  const shown = $derived(items.filter((i) => i.name.toLowerCase().includes(q.trim().toLowerCase())));
  const groups = $derived.by(() => {
    if (!group) return [{ title: '', items: shown }];
    const m = new Map<string, T[]>();
    for (const i of shown) m.set(group(i), [...(m.get(group(i)) ?? []), i]);
    return [...m].map(([title, items]) => ({ title, items }));
  });

  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === 'Escape' && (open = false)} />

<div class="relative" bind:this={root}>
  <button
    type="button"
    class={cn(
      'inline-flex h-8 items-center gap-1.5 rounded-lg border border-border bg-surface-2 px-2.5 text-xs font-medium transition hover:bg-surface-3',
      selected.length > 0 && 'border-accent/60 text-accent'
    )}
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    {label}{selected.length > 0 ? ` Â· ${selected.length}` : ''}
    <ChevronDown class="size-3.5" />
  </button>
  {#if open}
    <div class="absolute left-0 top-full z-30 mt-1 w-64 rounded-xl border border-border bg-surface-1 p-2 shadow-xl">
      <div class="relative mb-2">
        <Search class="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted" />
        <input
          bind:value={q}
          placeholder="Search {label.toLowerCase()}â€¦"
          class="h-8 w-full rounded-lg border border-border bg-surface-2 pl-7 pr-2 text-xs outline-none focus:border-accent"
        />
      </div>
      <div class="max-h-72 overflow-y-auto">
        {#each groups as g (g.title)}
          {#if g.title}<div class="px-2 pb-1 pt-2 text-[10px] font-semibold uppercase tracking-wider text-muted">{g.title}</div>{/if}
          {#each g.items as item (item.key)}
            {@const on = selected.includes(item.key)}
            <button
              type="button"
              class={cn('flex w-full items-center gap-2 rounded-lg px-2 py-1 text-left text-xs hover:bg-surface-2', on && 'bg-surface-3 text-accent')}
              aria-pressed={on}
              onclick={() => ontoggle(item.key)}
            >
              {@render row(item)}
            </button>
          {/each}
        {/each}
        {#if shown.length === 0}<div class="px-2 py-3 text-center text-xs text-muted">No matches</div>{/if}
      </div>
    </div>
  {/if}
</div>
