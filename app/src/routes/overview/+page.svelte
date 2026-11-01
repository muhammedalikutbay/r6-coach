<script lang="ts">
  import { LayoutDashboard, Swords, Shield } from '@lucide/svelte';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Section from '$lib/components/Section.svelte';
  import StatCard from '$lib/components/StatCard.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import DeltaPill from '$lib/components/DeltaPill.svelte';
  import LineChart from '$lib/v2/components/charts/LineChart.svelte';
  import MatchRowItem from '$lib/v2/components/MatchRowItem.svelte';
  import { filterStore } from '$lib/v2/filter.svelte';
  import { query } from '$lib/v2/api';
  import { toast } from '$lib/toast.svelte';
  import type { QueryResult } from '$lib/v2/types';

  let data = $state<QueryResult | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let seq = 0;

  $effect(() => {
    const f = $state.snapshot(filterStore.value);
    const mine = ++seq;
    loading = true;
    query(f)
      .then((r) => {
        if (mine !== seq) return;
        data = r;
        failed = false;
      })
      .catch((e) => {
        if (mine !== seq) return;
        console.error('[overview] query failed', e);
        failed = true;
        toast('error', 'Could not load the overview.');
      })
      .finally(() => {
        if (mine === seq) loading = false;
      });
  });

  const sourceHint = $derived(
    !data
      ? ''
      : data.source === 'career'
        ? 'Career + replays'
        : `Based on ${data.matchCount} ${data.matchCount === 1 ? 'match' : 'matches'} from replays`
  );
  const trend = $derived(
    (data?.trend ?? []).map((t) => ({
      label: `#${t.index} · ${new Date(t.timestamp).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })} · ${t.won ? 'Win' : 'Loss'}`,
      value: t.kd
    }))
  );
  const maxRounds = $derived(Math.max(1, ...(data?.operators ?? []).map((o) => o.rounds)));
  const pct = (v: number) => `${v.toFixed(1)}%`;
  const winBars = (m: { attackWinPct: number; defenseWinPct: number }) => [
    { label: 'ATK', v: m.attackWinPct },
    { label: 'DEF', v: m.defenseWinPct }
  ];
</script>

<PageHeader title="Overview" sub={sourceHint} />

{#if loading && !data}
  <div class="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-6">
    {#each Array(6) as _, i (i)}<Skeleton class="h-24" />{/each}
  </div>
  <Skeleton class="mt-6 h-48" />
{:else if failed && !data}
  <EmptyState title="Could not load data" text="Try changing the filter or press Update." icon={LayoutDashboard} />
{:else if data && data.matchCount === 0}
  <EmptyState title="No matches for this filter" text="Loosen the filter to see your stats." icon={LayoutDashboard}>
    <button class="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-accent-foreground" onclick={() => filterStore.clear()}>
      Clear filter
    </button>
  </EmptyState>
{:else if data}
  <div class="transition-opacity {loading ? 'opacity-60' : ''}">
    <div class="mb-6 grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-6">
      <StatCard label="K/D" value={data.kpis.kd.toFixed(2)} sub="{data.kpis.rounds} rounds" />
      <StatCard label="Win %" value={pct(data.kpis.winPct)} sub="{data.kpis.matches} matches" />
      <StatCard label="HS %" value={pct(data.kpis.hsPct)} />
      <StatCard label="Kills / round" value={data.kpis.killsPerRound.toFixed(2)} />
      <StatCard label="FK / FD" value={data.kpis.fkFdRatio.toFixed(2)} />
      <StatCard label="Clutch %" value={pct(data.kpis.clutchPct)} />
    </div>

    <Section title="K/D trend" hint="Per match, oldest to newest">
      <Card>
        {#if trend.length < 2}
          <p class="py-6 text-center text-muted">Not enough matches for a trend yet.</p>
        {:else}
          <LineChart points={trend} reference={data.kpis.kd} />
        {/if}
      </Card>
    </Section>

    <Section title="Operators" hint="Click an operator to filter by it">
      {#if data.operators.length === 0}
        <p class="text-muted">No operator data.</p>
      {:else}
        <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {#each data.operators as op (op.key + op.side)}
            {@const active = filterStore.value.operators.includes(op.key)}
            <button
              type="button"
              onclick={() => filterStore.toggleOperator(op.key)}
              class="rounded-xl border bg-surface-1 p-3 text-left transition-colors hover:bg-surface-2 {active ? 'border-accent' : 'border-border'}"
            >
              <div class="flex items-center gap-3">
                <OperatorBadge opKey={op.key} name={op.name} size={40} />
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-1.5 truncate text-sm font-semibold">
                    {op.name}
                    {#if op.side === 'attack'}<Swords class="size-3 text-muted" />{:else}<Shield class="size-3 text-muted" />{/if}
                  </div>
                  <div class="text-xs text-muted tabular-nums">{op.rounds} rounds · Win {op.winPct.toFixed(0)}%</div>
                </div>
                <div class="text-right">
                  <div class="text-sm font-bold tabular-nums">{op.kd.toFixed(2)}</div>
                  {#if op.deltaKd !== undefined}<DeltaPill value={op.deltaKd} />{/if}
                </div>
              </div>
              <div class="mt-2 h-1 overflow-hidden rounded-full bg-surface-3">
                <div class="h-full rounded-full bg-accent" style="width:{(op.rounds / maxRounds) * 100}%"></div>
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </Section>

    <Section title="Maps" hint="Click a map to filter by it">
      {#if data.maps.length === 0}
        <p class="text-muted">No map data.</p>
      {:else}
        <div class="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-4">
          {#each data.maps as m (m.key)}
            {@const active = filterStore.value.maps.includes(m.key)}
            <button
              type="button"
              onclick={() => filterStore.toggleMap(m.key)}
              class="overflow-hidden rounded-xl border bg-surface-1 text-left transition-colors hover:bg-surface-2 {active ? 'border-accent' : 'border-border'}"
            >
              <div class="relative">
                <MapThumb mapKey={m.key} name={m.name} class="rounded-none" />
                <div class="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/80 to-transparent p-2">
                  <div class="text-sm font-semibold text-white">{m.name}</div>
                  <div class="text-xs text-white/70 tabular-nums">
                    {m.matches} {m.matches === 1 ? 'match' : 'matches'} · Win {m.winPct.toFixed(0)}%
                  </div>
                </div>
              </div>
              <div class="space-y-1.5 p-2.5 text-xs">
                {#each winBars(m) as bar (bar.label)}
                  <div class="flex items-center gap-2">
                    <span class="w-7 text-muted">{bar.label}</span>
                    <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-surface-3">
                      <div class="h-full rounded-full {bar.v >= 50 ? 'bg-good' : 'bg-bad'}" style="width:{Math.min(100, bar.v)}%"></div>
                    </div>
                    <span class="w-9 text-right tabular-nums">{bar.v.toFixed(0)}%</span>
                  </div>
                {/each}
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </Section>

    <Section title="Recent matches">
      <div class="flex flex-col gap-2">
        {#each data.recentMatches as match (match.matchId)}
          <MatchRowItem {match} />
        {/each}
      </div>
    </Section>
  </div>
{/if}
