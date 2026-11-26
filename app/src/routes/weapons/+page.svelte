<script lang="ts">
  import { Crosshair } from '@lucide/svelte';
  import { goto } from '$app/navigation';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import { operatorName } from '$lib/v2/catalog';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import ListTools from '$lib/v2/components/details/ListTools.svelte';
  import SortHead from '$lib/v2/components/details/SortHead.svelte';
  import { filterStore } from '$lib/v2/filter.svelte';
  import { weaponList } from '$lib/v2/api';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import { toast } from '$lib/toast.svelte';
  import { slotLabel, plural } from '$lib/v2/format';
  import { cn } from '$lib/utils';
  import type { WeaponListRow } from '$lib/v2/types';

  let rows = $state<WeaponListRow[] | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let seq = 0;

  let search = $state('');
  let sortKey = $state('kills');
  let desc = $state(true);

  $effect(() => {
    const f = $state.snapshot(filterStore.value);
    void refreshTick.value;
    const mine = ++seq;
    loading = true;
    weaponList(f)
      .then((r) => {
        if (mine !== seq) return;
        rows = r;
        failed = false;
      })
      .catch((e) => {
        if (mine !== seq) return;
        console.error('[weapons] list failed', e);
        failed = true;
        toast('error', 'Could not load the weapons.');
      })
      .finally(() => {
        if (mine === seq) loading = false;
      });
  });

  const usedBy = (r: WeaponListRow, ok: string) => (r.usedOperators ?? []).includes(ok);
  const open = (key: string) => goto('/weapons/' + encodeURIComponent(key));
  /** Enter on the row opens it; Enter on an inner link or button keeps its own action. */
  function rowKey(e: KeyboardEvent, key: string) {
    if (e.key === 'Enter' && e.target === e.currentTarget) open(key);
  }

  function sortBy(col: string) {
    if (sortKey === col) desc = !desc;
    else {
      sortKey = col;
      desc = col !== 'name' && col !== 'slot';
    }
  }

  const value = (r: WeaponListRow, col: string): number | string => {
    switch (col) {
      case 'name': return r.name.toLowerCase();
      case 'slot': return r.slot ?? '';
      case 'hsPct': return r.hsPct;
      case 'operators': return r.operators;
      default: return r.kills;
    }
  };

  const shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const list = (rows ?? []).filter((r) => !q || r.name.toLowerCase().includes(q) || (r.slot ?? '').includes(q.toLowerCase()));
    const dir = desc ? -1 : 1;
    return list.sort((a, b) => {
      const x = value(a, sortKey);
      const y = value(b, sortKey);
      const c = typeof x === 'string' ? x.localeCompare(y as string) : x - (y as number);
      return c * dir || a.name.localeCompare(b.name);
    });
  });
  const totalKills = $derived(Math.max(1, (rows ?? []).reduce((s, r) => s + r.kills, 0)));
</script>

<PageHeader title="Weapons" sub={rows ? `${plural(rows.length, 'weapon')} with kills` : ''}>
  {#snippet actions()}<ListTools bind:value={search} placeholder="Search weapons" />{/snippet}
</PageHeader>

{#if loading && !rows}
  <Skeleton class="h-72" />
{:else if failed && !rows}
  <EmptyState title="Could not load weapons" text="Try changing the filter, or restart the app if it keeps happening." icon={Crosshair} />
{:else if rows && rows.length === 0}
  <EmptyState title="No weapon kills for this filter" text="Loosen the filter, or the weapon names of these kills are not known." icon={Crosshair}>
    <button class="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-accent-foreground" onclick={() => filterStore.clear()}>
      Clear filter
    </button>
  </EmptyState>
{:else if rows}
  <Card class="overflow-x-auto p-0 transition-opacity {loading ? 'opacity-60' : ''}">
    {#if shown.length === 0}
      <p class="px-4 py-8 text-center text-muted">No weapon matches "{search}".</p>
    {:else}
      <table class="w-full min-w-[720px] text-left text-sm">
        <thead class="border-b border-border text-xs text-muted">
          <tr>
            <SortHead label="Weapon" col="name" {sortKey} {desc} onsort={sortBy} align="left" />
            <SortHead label="Slot" col="slot" {sortKey} {desc} onsort={sortBy} align="left" />
            <SortHead label="Kills" col="kills" {sortKey} {desc} onsort={sortBy} />
            <SortHead label="HS %" col="hsPct" {sortKey} {desc} onsort={sortBy} />
            <SortHead label="Used by" col="operators" {sortKey} {desc} onsort={sortBy} title="How many different operators got a kill with it. The icons under Carried by show which ones" />
            <th class="px-3 py-2 text-left font-medium" title="Operators that can carry it. Full color: you got kills with it as that operator">Carried by</th>
            <th class="w-44 px-3 py-2 text-left font-medium">Share of kills</th>
          </tr>
        </thead>
        <tbody class="tabular-nums">
          {#each shown as r (r.key)}
            <tr
              class="cursor-pointer border-b border-border/60 transition-colors last:border-0 hover:bg-surface-2 focus-visible:bg-surface-2 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent"
              tabindex="0"
              onclick={() => open(r.key)}
              onkeydown={(e) => rowKey(e, r.key)}
            >
              <td class="px-3 py-2">
                <a href="/weapons/{encodeURIComponent(r.key)}" class={cn('font-medium', r.unnamed && 'font-normal text-muted italic')} onclick={(e) => e.stopPropagation()}>{r.name}</a>
              </td>
              <td class="px-3 py-2 text-muted">{slotLabel(r.slot)}</td>
              <td class="px-3 py-2 text-right font-semibold">{r.kills}</td>
              <td class="px-3 py-2 text-right">{r.hsPct.toFixed(0)}%</td>
              <td class="px-3 py-2 text-right">{r.operators}</td>
              <td class="px-3 py-2">
                <div class="flex flex-wrap gap-0.5">
                  {#each r.loadoutOperators ?? [] as ok (ok)}
                    <a
                      href="/operators/{encodeURIComponent(ok)}"
                      title={usedBy(r, ok) ? operatorName(ok) : `${operatorName(ok)} (no kills with it yet)`}
                      aria-label={operatorName(ok)}
                      class={cn('rounded-full transition hover:scale-110 hover:opacity-100 focus-visible:outline-2 focus-visible:outline-accent', !usedBy(r, ok) && 'opacity-40')}
                      onclick={(e) => e.stopPropagation()}
                    ><OperatorBadge opKey={ok} name={operatorName(ok)} size={22} /></a>
                  {/each}
                </div>
              </td>
              <td class="px-3 py-2">
                <div class="h-1.5 overflow-hidden rounded-full bg-surface-3">
                  <div class="h-full rounded-full bg-accent" style="width:{(r.kills / totalKills) * 100}%"></div>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </Card>
{/if}
