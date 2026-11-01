<script lang="ts">
  import { Swords } from '@lucide/svelte';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import MatchRowItem from '$lib/v2/components/MatchRowItem.svelte';
  import MatchDetailPanel from '$lib/v2/components/MatchDetailPanel.svelte';
  import { filterStore } from '$lib/v2/filter.svelte';
  import * as api from '$lib/v2/api';
  import type { MatchRow } from '$lib/v2/types';

  const PAGE_SIZE = 20;

  let matches = $state<MatchRow[]>([]);
  let loading = $state(true);
  let loadingMore = $state(false);
  let error = $state('');
  let page = $state(0);
  let hasMore = $state(false);
  let openId = $state<string | null>(null);

  // Reload from the first page whenever the global filter changes.
  $effect(() => {
    const f = $state.snapshot(filterStore.value);
    let stale = false;
    loading = true;
    error = '';
    api
      .listMatches(f, 0)
      .then((rows) => {
        if (stale) return;
        matches = rows;
        page = 0;
        hasMore = rows.length >= PAGE_SIZE;
      })
      .catch((e) => {
        if (stale) return;
        console.error('[matches] list failed', e);
        error = 'Could not load matches.';
      })
      .finally(() => {
        if (!stale) loading = false;
      });
    return () => {
      stale = true;
    };
  });

  async function loadMore() {
    loadingMore = true;
    try {
      const rows = await api.listMatches($state.snapshot(filterStore.value), page + 1);
      matches = [...matches, ...rows];
      page += 1;
      hasMore = rows.length >= PAGE_SIZE;
    } catch (e) {
      console.error('[matches] load more failed', e);
      error = 'Could not load more matches.';
    } finally {
      loadingMore = false;
    }
  }

  const dayKey = (iso: string) => {
    const d = new Date(iso);
    return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  };

  const groups = $derived.by(() => {
    const out: { key: string; label: string; rows: MatchRow[] }[] = [];
    for (const m of matches) {
      const key = dayKey(m.timestamp);
      let g = out.find((x) => x.key === key);
      if (!g) {
        g = {
          key,
          label: new Date(m.timestamp).toLocaleDateString([], {
            weekday: 'long',
            day: 'numeric',
            month: 'long'
          }),
          rows: []
        };
        out.push(g);
      }
      g.rows.push(m);
    }
    return out;
  });

  function totals(rows: MatchRow[]) {
    const wins = rows.filter((r) => r.won).length;
    const k = rows.reduce((s, r) => s + r.kills, 0);
    const d = rows.reduce((s, r) => s + r.deaths, 0);
    // Kill-weighted headshot rate, so a day total matches the per-match numbers.
    const hs = k > 0 ? rows.reduce((s, r) => s + r.hsPct * r.kills, 0) / k : 0;
    return { wins, losses: rows.length - wins, k, d, kd: d > 0 ? k / d : k, hs };
  }
</script>

<PageHeader title="Matches" sub="Your recent matches, grouped by day." />

{#if error && matches.length === 0}
  <EmptyState title="Something went wrong" text={error} icon={Swords} />
{:else if loading}
  <div class="space-y-2">
    {#each Array(6) as _, i (i)}<Skeleton class="h-16" />{/each}
  </div>
{:else if matches.length === 0}
  <EmptyState title="No matches found" text="Try widening the filter, or press Update after a match." icon={Swords} />
{:else}
  <div class="space-y-6">
    {#each groups as g (g.key)}
      {@const t = totals(g.rows)}
      <section>
        <div class="mb-2 flex flex-wrap items-baseline justify-between gap-2">
          <h2 class="text-sm font-semibold">{g.label}</h2>
          <div class="flex gap-3 text-xs text-muted">
            <span><b class="text-good">{t.wins}W</b> <b class="text-bad">{t.losses}L</b></span>
            <span>K/D <b class="text-foreground">{t.kd.toFixed(2)}</b></span>
            <span>K <b class="text-foreground">{t.k}</b></span>
            <span>D <b class="text-foreground">{t.d}</b></span>
            <span>HS <b class="text-foreground">{t.hs.toFixed(0)}%</b></span>
          </div>
        </div>
        <div class="space-y-1.5">
          {#each g.rows as m (m.matchId)}
            <MatchRowItem match={m} active={openId === m.matchId} onclick={() => (openId = m.matchId)} />
          {/each}
        </div>
      </section>
    {/each}
    {#if hasMore}
      <div class="flex justify-center">
        <Button onclick={loadMore} disabled={loadingMore}>{loadingMore ? 'Loading…' : 'Load more'}</Button>
      </div>
    {/if}
  </div>
{/if}

{#if openId}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="fixed inset-0 z-40 bg-black/50" onclick={() => (openId = null)}></div>
  <aside class="fixed top-0 right-0 z-50 h-full w-full max-w-[920px] border-l border-border-strong bg-surface-1 shadow-2xl">
    <MatchDetailPanel matchId={openId} onclose={() => (openId = null)} />
  </aside>
{/if}

<svelte:window onkeydown={(e) => e.key === 'Escape' && (openId = null)} />
