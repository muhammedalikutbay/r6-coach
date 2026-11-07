<script lang="ts">
  // Global filter bar (docs/v2-spec.md §3). Reads and writes the shared filterStore.
  import { Calendar, Clock, X } from '@lucide/svelte';
  import { filterStore as fs } from '../filter.svelte';
  import { OPERATORS, MAPS, operatorName, mapName } from '../catalog';
  import type { Mode, Range, Side } from '../types';
  import { cn } from '$lib/utils';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import MultiSelect from './MultiSelect.svelte';
  import DateTimeFilter from './DateTimeFilter.svelte';

  const ranges: [Range, string][] = [['last1', 'Last match'], ['last5', 'Last 5'], ['last20', 'Last 20'], ['season', 'Season'], ['all', 'All time']];
  const modes: [Mode, string][] = [['all', 'All'], ['ranked', 'Ranked'], ['unranked', 'Unranked'], ['quick', 'Quick']];
  const sides: [Side, string][] = [['both', 'Both'], ['attack', 'Attack'], ['defense', 'Defense']];

  const v = $derived(fs.value);
  const pad = (n: number) => String(n).padStart(2, '0');
  const dateText = $derived(
    v.dateFrom || v.dateTo ? (v.dateFrom === v.dateTo ? v.dateFrom : `${v.dateFrom ?? '...'} to ${v.dateTo ?? '...'}`) : null
  );
  const hourText = $derived(
    v.hourFrom !== null || v.hourTo !== null ? `${pad(v.hourFrom ?? 0)}:00 to ${pad(v.hourTo ?? 23)}:59` : null
  );
</script>

{#snippet seg<K extends string>(opts: [K, string][], cur: K, pick: (k: K) => void, name: string)}
  <div class="inline-flex rounded-lg border border-border bg-surface-2 p-0.5" role="group" aria-label={name}>
    {#each opts as [k, text] (k)}
      <button
        type="button"
        aria-pressed={cur === k}
        onclick={() => pick(k)}
        class={cn('h-7 rounded-md px-2.5 text-xs font-medium transition', cur === k ? 'bg-accent text-black' : 'text-muted hover:text-foreground')}
      >{text}</button>
    {/each}
  </div>
{/snippet}

<div class="flex flex-col gap-2" data-slot="filter-bar">
  <div class="flex flex-wrap items-center gap-2">
    {@render seg(ranges, v.range, (range) => fs.set({ range }), 'Range')}
    {@render seg(modes, v.mode, (mode) => fs.set({ mode }), 'Mode')}
    {@render seg(sides, v.side, (side) => fs.set({ side }), 'Side')}
    <MultiSelect label="Operators" items={OPERATORS} selected={v.operators} ontoggle={(k) => fs.toggleOperator(k)} group={(o) => (o.side === 'attacker' ? 'Attackers' : 'Defenders')}>
      {#snippet row(o)}<OperatorBadge opKey={o.key} name={o.name} size={22} /><span class="truncate">{o.name}</span>{/snippet}
    </MultiSelect>
    <MultiSelect label="Maps" items={MAPS} selected={v.maps} ontoggle={(k) => fs.toggleMap(k)}>
      {#snippet row(m)}<MapThumb mapKey={m.key} name="" class="w-12 flex-none" /><span class="truncate">{m.name}</span>{/snippet}
    </MultiSelect>
    <DateTimeFilter />
    {#if fs.active}
      <button type="button" class="h-8 rounded-lg px-2 text-xs text-muted hover:text-foreground" onclick={() => fs.clear()}>Clear all</button>
    {/if}
  </div>

  {#if v.operators.length > 0 || v.maps.length > 0 || fs.dateActive}
    <div class="flex flex-wrap items-center gap-1.5" aria-label="Active filters">
      {#if dateText}
        <button type="button" title="Remove" onclick={() => fs.set({ dateFrom: null, dateTo: null })} class="inline-flex items-center gap-1.5 rounded-full border border-border bg-surface-2 px-2 py-0.5 text-xs hover:border-accent">
          <Calendar class="size-3 text-muted" />{dateText}<X class="size-3 text-muted" />
        </button>
      {/if}
      {#if hourText}
        <button type="button" title="Remove" onclick={() => fs.set({ hourFrom: null, hourTo: null })} class="inline-flex items-center gap-1.5 rounded-full border border-border bg-surface-2 px-2 py-0.5 text-xs hover:border-accent">
          <Clock class="size-3 text-muted" />{hourText}<X class="size-3 text-muted" />
        </button>
      {/if}
      {#each v.operators as k (k)}
        <button type="button" title="Remove" onclick={() => fs.toggleOperator(k)} class="inline-flex items-center gap-1.5 rounded-full border border-border bg-surface-2 py-0.5 pl-0.5 pr-2 text-xs hover:border-accent">
          <OperatorBadge opKey={k} name={operatorName(k)} size={20} />{operatorName(k)}<X class="size-3 text-muted" />
        </button>
      {/each}
      {#each v.maps as k (k)}
        <button type="button" title="Remove" onclick={() => fs.toggleMap(k)} class="inline-flex items-center gap-1.5 rounded-full border border-border bg-surface-2 py-0.5 pl-0.5 pr-2 text-xs hover:border-accent">
          <MapThumb mapKey={k} name="" class="w-8 rounded-full" />{mapName(k)}<X class="size-3 text-muted" />
        </button>
      {/each}
    </div>
  {/if}
</div>
