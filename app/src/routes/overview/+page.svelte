<script lang="ts">
  import { LayoutDashboard, Swords, Shield, Funnel } from '@lucide/svelte';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Section from '$lib/components/Section.svelte';
  import CollapsibleSection from '$lib/v2/components/CollapsibleSection.svelte';
  import DeepSections from '$lib/v2/components/DeepSections.svelte';
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
  import { onlineProfile, arenyzeStatus } from '$lib/v2/arenyze';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import { formatShortDate, plural } from '$lib/v2/format';
  import { toast } from '$lib/toast.svelte';
  import type { OfficialOperator, OnlineProfile, QueryResult } from '$lib/v2/types';

  let data = $state<QueryResult | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let seq = 0;

  // Official Arenyze per-operator numbers (served from the 15-minute cache). The rank header now lives on Home.
  let profile = $state<OnlineProfile | null>(null);

  async function loadOfficial() {
    try {
      if (!(await arenyzeStatus()).hasKey) {
        profile = null;
        return;
      }
      profile = await onlineProfile(false);
    } catch (e) {
      // Keep the last profile when a refresh fails; the official numbers are optional here.
      console.error('[overview] official numbers failed', e);
    }
  }

  $effect(() => {
    void refreshTick.value;
    void loadOfficial();
  });

  $effect(() => {
    const f = $state.snapshot(filterStore.value);
    void refreshTick.value; // re-query after Update
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
    data ? `Based on ${data.matchCount} stored ${data.matchCount === 1 ? 'match' : 'matches'}` : ''
  );
  const trend = $derived(
    (data?.trend ?? []).map((t) => ({
      label: `#${t.index} · ${formatShortDate(t.timestamp)} · ${t.won ? 'Win' : 'Loss'}`,
      value: t.kd
    }))
  );
  const normName = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, '');
  // Official (Arenyze) per-operator numbers of the current ranked season: shown on the season range only.
  const officialOps = $derived.by(() => {
    const map = new Map<string, OfficialOperator>();
    if (filterStore.value.range !== 'season' || !profile) return map;
    for (const o of profile.operators) map.set(`${normName(o.name)}|${o.side}`, o);
    return map;
  });
  const officialOf = (name: string, side: string) =>
    officialOps.get(`${normName(name)}|${side}`) ?? officialOps.get(`${normName(name)}|`);
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
      <StatCard label="K/D" value={data.kpis.kd.toFixed(2)} sub={plural(data.kpis.rounds, 'round')}>
        {#if data.official}<div class="text-xs text-muted tabular-nums">Official: {data.official.kd.toFixed(2)} K/D</div>{/if}
      </StatCard>
      <StatCard label="Win %" value={pct(data.kpis.winPct)} sub={plural(data.kpis.matches, 'match', 'matches')}>
        {#if data.official}<div class="text-xs text-muted tabular-nums">Official: {pct(data.official.winPct)} win</div>{/if}
      </StatCard>
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

    <CollapsibleSection id="operators" title="Operators" hint="Click an operator for its screen; the funnel adds it to the filter" summary="{data.operators.length} {data.operators.length === 1 ? 'operator' : 'operators'}">
      <div class="mb-3 flex gap-4 text-xs">
        <a href="/operators" class="text-accent hover:underline">All operators</a>
        <a href="/weapons" class="text-accent hover:underline">All weapons</a>
      </div>
      {#if data.operators.length === 0}
        <p class="text-muted">No operator data.</p>
      {:else}
        <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {#each data.operators as op (op.key + op.side)}
            {@const active = filterStore.value.operators.includes(op.key)}
            <div class="relative rounded-xl border bg-surface-1 transition-colors hover:bg-surface-2 {active ? 'border-accent' : 'border-border'}">
            <button
              type="button"
              onclick={() => filterStore.toggleOperator(op.key)}
              title={active ? 'Remove from the filter' : 'Add to the filter'}
              aria-label={active ? `Remove ${op.name} from the filter` : `Filter by ${op.name}`}
              aria-pressed={active}
              class="absolute right-2 bottom-2 z-10 rounded-md p-1 transition-colors hover:bg-surface-3 {active ? 'text-accent' : 'text-muted'}"
            >
              <Funnel class="size-3.5" />
            </button>
            <a href="/operators/{encodeURIComponent(op.key)}" class="block p-3 text-left" title="Open the {op.name} screen">
              <div class="flex items-center gap-3">
                <OperatorBadge opKey={op.key} name={op.name} size={40} />
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-1.5 truncate text-sm font-semibold">
                    {op.name}
                    {#if op.side === 'attack'}<Swords class="size-3 text-muted" />{:else}<Shield class="size-3 text-muted" />{/if}
                  </div>
                  <div class="text-xs text-muted tabular-nums">{plural(op.rounds, 'round')} · Win {op.winPct.toFixed(0)}%</div>
                  {#if officialOf(op.name, op.side)}
                    {@const off = officialOf(op.name, op.side)!}
                    <div class="text-xs text-muted tabular-nums" title="Official numbers from Arenyze, ranked, current season">
                      Official: {off.kd.toFixed(2)} K/D · {plural(off.rounds, 'round')}
                    </div>
                  {/if}
                </div>
                <div class="text-right">
                  <div class="text-sm font-bold tabular-nums">{op.kd.toFixed(2)}</div>
                  {#if op.deltaKd !== undefined}<DeltaPill value={op.deltaKd} />{/if}
                </div>
              </div>
              <div class="mt-2 mr-6 h-1 overflow-hidden rounded-full bg-surface-3">
                <div class="h-full rounded-full bg-accent" style="width:{(op.rounds / maxRounds) * 100}%"></div>
              </div>
            </a>
            </div>
          {/each}
        </div>
      {/if}
    </CollapsibleSection>

    <CollapsibleSection id="maps" title="Maps" hint="Click a map to filter by it" summary="{data.maps.length} {data.maps.length === 1 ? 'map' : 'maps'}">
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
    </CollapsibleSection>

    <DeepSections deep={data.deep} />

    <CollapsibleSection id="recent" title="Recent matches" summary={plural(data.recentMatches.length, 'match', 'matches')}>
      <div class="flex flex-col gap-2">
        {#each data.recentMatches as match (match.matchId)}
          <MatchRowItem {match} showDate />
        {/each}
      </div>
    </CollapsibleSection>
  </div>
{/if}
