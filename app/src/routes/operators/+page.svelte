<script lang="ts">
  import { Users, Swords, Shield } from '@lucide/svelte';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import ListTools from '$lib/v2/components/details/ListTools.svelte';
  import SortHead from '$lib/v2/components/details/SortHead.svelte';
  import { filterStore } from '$lib/v2/filter.svelte';
  import { operatorList } from '$lib/v2/api';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import { toast } from '$lib/toast.svelte';
  import type { OperatorListRow } from '$lib/v2/types';

  let rows = $state<OperatorListRow[] | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let seq = 0;

  let search = $state('');
  let sortKey = $state('rounds');
  let desc = $state(true);

  $effect(() => {
    const f = $state.snapshot(filterStore.value);
    void refreshTick.value;
    const mine = ++seq;
    loading = true;
    operatorList(f)
      .then((r) => {
        if (mine !== seq) return;
        rows = r;
        failed = false;
      })
      .catch((e) => {
        if (mine !== seq) return;
        console.error('[operators] list failed', e);
        failed = true;
        toast('error', 'Could not load the operators.');
      })
      .finally(() => {
        if (mine === seq) loading = false;
      });
  });

  function sortBy(col: string) {
    if (sortKey === col) desc = !desc;
    else {
      sortKey = col;
      desc = col !== 'name';
    }
  }

  const value = (r: OperatorListRow, col: string): number | string => {
    switch (col) {
      case 'name': return r.name.toLowerCase();
      case 'side': return r.side;
      case 'matches': return r.matches;
      case 'winPct': return r.winPct;
      case 'kd': return r.kd;
      case 'killsPerRound': return r.killsPerRound;
      case 'hsPct': return r.hsPct;
      case 'kostPct': return r.kostPct;
      default: return r.rounds;
    }
  };

  const shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const list = (rows ?? []).filter((r) => !q || r.name.toLowerCase().includes(q) || r.key.includes(q));
    const dir = desc ? -1 : 1;
    return list.sort((a, b) => {
      const x = value(a, sortKey);
      const y = value(b, sortKey);
      const c = typeof x === 'string' ? x.localeCompare(y as string) : x - (y as number);
      return c * dir || a.name.localeCompare(b.name);
    });
  });
</script>

<PageHeader title="Operators" sub={rows ? `${rows.length} ${rows.length === 1 ? 'operator' : 'operators'} played` : ''}>
  {#snippet actions()}<ListTools bind:value={search} placeholder="Search operators" />{/snippet}
</PageHeader>

{#if loading && !rows}
  <Skeleton class="h-72" />
{:else if failed && !rows}
  <EmptyState title="Could not load operators" text="Try changing the filter or press Update." icon={Users} />
{:else if rows && rows.length === 0}
  <EmptyState title="No operators for this filter" text="Loosen the filter to see your operators." icon={Users}>
    <button class="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-accent-foreground" onclick={() => filterStore.clear()}>
      Clear filter
    </button>
  </EmptyState>
{:else if rows}
  <Card class="overflow-x-auto p-0 transition-opacity {loading ? 'opacity-60' : ''}">
    {#if shown.length === 0}
      <p class="px-4 py-8 text-center text-muted">No operator matches "{search}".</p>
    {:else}
      <table class="w-full min-w-[640px] text-left text-sm">
        <thead class="border-b border-border text-xs text-muted">
          <tr>
            <SortHead label="Operator" col="name" {sortKey} {desc} onsort={sortBy} align="left" />
            <SortHead label="Matches" col="matches" {sortKey} {desc} onsort={sortBy} />
            <SortHead label="Rounds" col="rounds" {sortKey} {desc} onsort={sortBy} />
            <SortHead label="Win %" col="winPct" {sortKey} {desc} onsort={sortBy} />
            <SortHead label="K/D" col="kd" {sortKey} {desc} onsort={sortBy} />
            <SortHead label="Kills / round" col="killsPerRound" {sortKey} {desc} onsort={sortBy} />
            <SortHead label="HS %" col="hsPct" {sortKey} {desc} onsort={sortBy} />
            <SortHead label="KOST %" col="kostPct" {sortKey} {desc} onsort={sortBy} />
          </tr>
        </thead>
        <tbody class="tabular-nums">
          {#each shown as r (r.key)}
            <tr class="border-b border-border/60 transition-colors last:border-0 hover:bg-surface-2">
              <td class="py-1.5 pl-3">
                <a href="/operators/{encodeURIComponent(r.key)}" class="flex items-center gap-2.5 font-medium">
                  <OperatorBadge opKey={r.key} name={r.name} size={32} />
                  <span>{r.name}</span>
                  {#if r.side === 'attack'}<Swords class="size-3 text-muted" />{:else}<Shield class="size-3 text-muted" />{/if}
                </a>
              </td>
              <td class="py-1.5 text-right">{r.matches}</td>
              <td class="py-1.5 text-right">{r.rounds}</td>
              <td class="py-1.5 text-right">{r.winPct.toFixed(0)}%</td>
              <td class="py-1.5 text-right font-semibold">{r.kd.toFixed(2)}</td>
              <td class="py-1.5 text-right">{r.killsPerRound.toFixed(2)}</td>
              <td class="py-1.5 text-right">{r.hsPct.toFixed(0)}%</td>
              <td class="py-1.5 pr-3 text-right">{r.kostPct.toFixed(0)}%</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </Card>
{/if}
