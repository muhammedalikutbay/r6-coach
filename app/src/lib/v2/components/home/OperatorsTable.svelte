<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { ChevronDown, ChevronUp, LoaderCircle } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import { cn } from '$lib/utils';
  import {
    OPERATOR_MODES,
    cachedOperators,
    onlineOperators,
    operatorSeasonOptions,
    opAssetKey
  } from '$lib/v2/arenyze';
  import type { OfficialOperator, OperatorMode, SeasonEntry } from '$lib/v2/types';

  let { seasons, currentSeason, followSeason = null }: {
    seasons: SeasonEntry[];
    /** Short name of the season picked in the Seasons list (null = the current one). The table follows it. */
    followSeason?: string | null;
    /** Short name of the current season (for example Y11S3); the first table is for it. */
    currentSeason: string | null;
  } = $props();

  let season = $state('all');
  const baseOptions = $derived(operatorSeasonOptions(seasons));
  // A season picked in the Seasons list may be older than Arenyze can answer (before Y8): keep it selectable so the select shows it.
  const seasonOptions = $derived(
    baseOptions.some((o) => o.value === season)
      ? baseOptions
      : [...baseOptions, { value: season, label: `${season} (no operator data)` }]
  );
  /** Seasons before Y8 are not served by the operators call: show the empty state without a request. */
  const answerable = (s: string) => baseOptions.some((o) => o.value === s);
  let mode = $state<OperatorMode>('ranked');
  let ops = $state<OfficialOperator[] | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let seq = 0;

  type SortKey = 'name' | 'side' | 'rounds' | 'winPct' | 'kd' | 'hsPct' | 'kills' | 'firstBloods';
  let sortKey = $state<SortKey>('rounds');
  let sortDir = $state<1 | -1>(-1);
  let side = $state<'all' | 'attack' | 'defense'>('all');
  const SHOWN = 10;
  let showAll = $state(false);

  onMount(() => {
    // The first table is the current season's ranked one that came with the profile: no request.
    season = currentSeason ?? 'all';
    ops = cachedOperators(season, mode)?.operators ?? null;
  });

  // Picking a season in the Seasons list switches this table to it; the selector can still be changed afterwards.
  let followed = untrack(() => followSeason);
  $effect(() => {
    const target = followSeason;
    if (target === followed) return;
    followed = target;
    untrack(() => {
      const next = target ?? currentSeason ?? 'all';
      if (next === season) return;
      season = next;
      if (!answerable(next)) {
        seq++;
        ops = [];
        error = null;
        loading = false;
        return;
      }
      ops = cachedOperators(next, mode)?.operators ?? null;
      void load();
    });
  });

  /** Runs only from a selector or the load button, never by itself. */
  async function load() {
    const mine = ++seq;
    loading = true;
    error = null;
    try {
      const v = await onlineOperators(season, mode);
      if (mine === seq) ops = v.operators;
    } catch (e) {
      if (mine !== seq) return;
      console.error('[home] operators failed', typeof e === 'string' ? e : 'unknown error');
      error = typeof e === 'string' ? e : 'Could not load operators.';
    } finally {
      if (mine === seq) loading = false;
    }
  }

  function pickSeason(v: string) {
    season = v;
    if (!answerable(v)) {
      seq++;
      ops = [];
      error = null;
      loading = false;
      return;
    }
    void load();
  }
  function pickMode(v: OperatorMode) {
    mode = v;
    void load();
  }

  function sortBy(k: SortKey) {
    if (sortKey === k) sortDir = sortDir === 1 ? -1 : 1;
    else {
      sortKey = k;
      sortDir = k === 'name' || k === 'side' ? 1 : -1;
    }
  }

  const rows = $derived.by(() => {
    const list = (ops ?? []).filter((o) => side === 'all' || o.side === side);
    const val = (o: OfficialOperator): string | number => {
      const v = o[sortKey];
      return v === null || v === undefined ? -Infinity : v;
    };
    return [...list].sort((a, b) => {
      const x = val(a);
      const y = val(b);
      const c = typeof x === 'string' && typeof y === 'string' ? x.localeCompare(y) : Number(x) - Number(y);
      return c * sortDir || a.name.localeCompare(b.name);
    });
  });

  const visible = $derived(showAll ? rows : rows.slice(0, SHOWN));
  const num = (v: number | null | undefined) => (v === null || v === undefined ? '—' : v.toLocaleString('en-GB'));
  const pct = (v: number | null | undefined) => (v === null || v === undefined ? '—' : `${v.toFixed(1)}%`);
  const COLS: { key: SortKey; label: string; right: boolean }[] = [
    { key: 'name', label: 'Operator', right: false },
    { key: 'side', label: 'Side', right: false },
    { key: 'rounds', label: 'Rounds', right: true },
    { key: 'winPct', label: 'Win %', right: true },
    { key: 'kd', label: 'K/D', right: true },
    { key: 'hsPct', label: 'HS %', right: true },
    { key: 'kills', label: 'Kills', right: true },
    { key: 'firstBloods', label: 'First bloods', right: true }
  ];
  const selectCls = 'h-8 rounded-lg border border-border-strong bg-surface-2 px-2 text-xs';
</script>

<Card>
  <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
    <h3 class="text-xs font-medium tracking-wide text-muted uppercase">Operators</h3>
    <div class="flex flex-wrap items-center gap-2">
      {#if loading}<LoaderCircle class="size-4 animate-spin text-muted" aria-label="Loading" />{/if}
      <div class="flex gap-1" role="group" aria-label="Side">
        {#each [['all', 'Both'], ['attack', 'ATK'], ['defense', 'DEF']] as [v, label] (v)}
          <button
            type="button"
            aria-pressed={side === v}
            onclick={() => (side = v as typeof side)}
            class={cn(
              'rounded-lg px-2.5 py-1 text-xs font-medium transition-colors',
              side === v ? 'bg-surface-3 text-accent' : 'text-muted hover:bg-surface-2 hover:text-foreground'
            )}
          >
            {label}
          </button>
        {/each}
      </div>
      <select class={selectCls} aria-label="Season" value={season} onchange={(e) => pickSeason(e.currentTarget.value)} disabled={loading}>
        {#each seasonOptions as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
      </select>
      <select
        class={selectCls}
        aria-label="Playlist"
        value={mode}
        onchange={(e) => pickMode(e.currentTarget.value as OperatorMode)}
        disabled={loading}
      >
        {#each OPERATOR_MODES as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
      </select>
    </div>
  </div>

  {#if error}
    <p class="mb-2 text-xs text-accent">{error}</p>
  {/if}

  {#if loading && ops === null}
    <div class="flex items-center justify-center gap-2 py-8 text-muted"><LoaderCircle class="size-4 animate-spin" /> Loading operators…</div>
  {:else if ops === null}
    <div class="flex flex-col items-center gap-2 py-8 text-center text-muted">
      <p>Operator stats load when you ask for them (one Arenyze call, kept for 15 minutes).</p>
      <button
        type="button"
        onclick={() => void load()}
        disabled={loading}
        class="h-8 rounded-lg border border-border-strong bg-surface-2 px-3 text-xs text-foreground transition-colors hover:bg-surface-3 disabled:opacity-50"
      >
        Load operators
      </button>
    </div>
  {:else if rows.length === 0}
    <p class="py-8 text-center text-muted">
      {answerable(season) ? 'No operator numbers for this selection.' : 'Operator stats are only available from Y8 on.'}
    </p>
  {:else}
    <div class="overflow-x-auto rounded-lg border border-border">
      <table class="w-full text-sm">
        <thead class="bg-surface-1 text-xs text-muted">
          <tr class="border-b border-border">
            {#each COLS as c (c.key)}
              <th class={cn('px-3 py-2 font-medium whitespace-nowrap', c.right ? 'text-right' : 'text-left')}>
                <button type="button" onclick={() => sortBy(c.key)} class="inline-flex items-center gap-1 hover:text-foreground">
                  {c.label}
                  {#if sortKey === c.key}
                    {#if sortDir === 1}<ChevronUp class="size-3" />{:else}<ChevronDown class="size-3" />{/if}
                  {/if}
                </button>
              </th>
            {/each}
          </tr>
        </thead>
        <tbody class="tabular-nums">
          {#each visible as o (o.name + o.side)}
            <tr class="border-b border-border last:border-0 hover:bg-surface-2">
              <td class="px-3 py-1.5">
                <span class="flex items-center gap-2">
                  <OperatorBadge opKey={opAssetKey(o.name)} name={o.name} size={26} />
                  <span class="font-medium">{o.name}</span>
                </span>
              </td>
              <td class="px-3 py-1.5 text-muted">{o.side === 'attack' ? 'ATK' : o.side === 'defense' ? 'DEF' : '—'}</td>
              <td class="px-3 py-1.5 text-right">{num(o.rounds)}</td>
              <td class="px-3 py-1.5 text-right">{pct(o.winPct)}</td>
              <td class="px-3 py-1.5 text-right">{o.kd.toFixed(2)}</td>
              <td class="px-3 py-1.5 text-right">{pct(o.hsPct)}</td>
              <td class="px-3 py-1.5 text-right">{num(o.kills)}</td>
              <td class="px-3 py-1.5 text-right">{num(o.firstBloods)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if rows.length > SHOWN}
      <button
        type="button"
        onclick={() => (showAll = !showAll)}
        class="mt-2 w-full rounded-lg py-1.5 text-xs text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
      >
        {showAll ? 'Show fewer' : `Show all ${rows.length}`}
      </button>
    {/if}
  {/if}
</Card>
