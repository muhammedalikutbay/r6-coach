<script lang="ts">
  // Quick summary of the last 20 stored matches, from local replay data only (no network, no key).
  import { Inbox } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import StatCard from '$lib/components/StatCard.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import { query } from '$lib/v2/api';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import { DEFAULT_FILTER, type QueryResult } from '$lib/v2/types';

  let data = $state<QueryResult | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let seq = 0;

  $effect(() => {
    void refreshTick.value; // re-query after Update
    const mine = ++seq;
    query({ ...DEFAULT_FILTER, operators: [], maps: [], range: 'last20' })
      .then((r) => {
        if (mine !== seq) return;
        data = r;
        failed = false;
      })
      .catch((e) => {
        if (mine !== seq) return;
        console.error('[home] local summary failed', e);
        failed = true;
      })
      .finally(() => {
        if (mine === seq) loading = false;
      });
  });

  const top = $derived([...(data?.operators ?? [])].sort((a, b) => b.rounds - a.rounds).slice(0, 5));
</script>

<section>
  <div class="mb-3">
    <h2 class="text-sm font-semibold tracking-wide uppercase">From your replays</h2>
    <p class="text-xs text-muted">
      {data && data.matchCount > 0 ? `Last ${data.matchCount} stored ${data.matchCount === 1 ? 'match' : 'matches'}` : 'Last 20 stored matches'}
      · local data, no key needed
    </p>
  </div>

  {#if loading && !data}
    <div class="grid grid-cols-2 gap-3 lg:grid-cols-3">
      {#each Array(3) as _, i (i)}<Skeleton class="h-24" />{/each}
    </div>
  {:else if failed && !data}
    <EmptyState title="Could not load your replays" text="Try Update, or open Settings to check the replay folder." icon={Inbox} />
  {:else if !data || data.matchCount === 0}
    <EmptyState
      title="No matches yet"
      text="Play a match with Match Replay recording on, then press Update. Your quick summary appears here."
      icon={Inbox}
    />
  {:else}
    <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
      <StatCard label="Matches" value={String(data.matchCount)} sub="stored replays" />
      <StatCard label="K/D" value={data.kpis.kd.toFixed(2)} sub="last {data.matchCount}" />
      <StatCard label="Win rate" value="{data.kpis.winPct.toFixed(1)}%" sub="last {data.matchCount}" />
      <StatCard label="Headshots" value="{data.kpis.hsPct.toFixed(1)}%" sub="of kills" />
    </div>

    {#if top.length > 0}
      <Card class="mt-3">
        <h3 class="mb-2 text-xs font-medium tracking-wide text-muted uppercase">Most played operators</h3>
        <ul class="flex flex-col gap-1.5">
          {#each top as o (o.key + o.side)}
            <li class="flex items-center gap-3 text-sm">
              <OperatorBadge opKey={o.key} name={o.name} size={30} />
              <span class="min-w-0 flex-1 truncate font-medium">{o.name}</span>
              <span class="text-xs text-muted">{o.side === 'attack' ? 'ATK' : 'DEF'}</span>
              <span class="w-20 text-right text-xs text-muted tabular-nums">{o.rounds} rounds</span>
              <span class="w-14 text-right tabular-nums">{o.winPct.toFixed(0)}%</span>
              <span class="w-12 text-right tabular-nums">{o.kd.toFixed(2)}</span>
            </li>
          {/each}
        </ul>
        <p class="mt-2 text-right text-xs text-muted">win % · K/D</p>
      </Card>
    {/if}
  {/if}
</section>
