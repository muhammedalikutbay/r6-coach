<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte';
  import RankIcon from './RankIcon.svelte';
  import { cn } from '$lib/utils';
  import { PLAYLIST_LABEL } from '$lib/v2/arenyze';
  import type { PlaylistKey, PlaylistStats, SeasonEntry } from '$lib/v2/types';

  let { season, isCurrent }: { season: SeasonEntry | undefined; isCurrent: boolean } = $props();

  /** Playlists that have any numbers; the others are hidden. */
  const tabs = $derived(
    (season?.playlists ?? []).filter((p) => (p.matches ?? 0) > 0 || p.rankPoints !== null || p.mmr !== null)
  );
  let picked = $state<PlaylistKey | null>(null);
  const active = $derived<PlaylistStats | undefined>(
    tabs.find((p) => p.playlist === picked) ?? tabs.find((p) => p.playlist === 'ranked') ?? tabs[0]
  );

  const n = (v: number | null | undefined) => (v === null || v === undefined ? '—' : v.toLocaleString('en-GB'));
  const hasRank = $derived(!!active && active.playlist === 'ranked' && (active.rankName || active.maxRankName || active.mmr || active.maxMmr));
</script>

<Card>
  <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
    <h3 class="text-xs font-medium tracking-wide text-muted uppercase">
      Season overview
      {#if season}
        <span class="ml-1 normal-case">
          {season.shortName}{season.name ? ` · ${season.name}` : ''}{isCurrent ? ' (current)' : ''}
        </span>
      {/if}
    </h3>
    {#if tabs.length > 0}
      <div class="flex gap-1" role="tablist" aria-label="Playlist">
        {#each tabs as t (t.playlist)}
          <button
            type="button"
            role="tab"
            aria-selected={active?.playlist === t.playlist}
            onclick={() => (picked = t.playlist as PlaylistKey)}
            class={cn(
              'rounded-lg px-3 py-1 text-xs font-medium transition-colors',
              active?.playlist === t.playlist ? 'bg-surface-3 text-accent' : 'text-muted hover:bg-surface-2 hover:text-foreground'
            )}
          >
            {PLAYLIST_LABEL[t.playlist]}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  {#if !active}
    <p class="py-6 text-center text-muted">Arenyze has no numbers for this season.</p>
  {:else}
    <div class="grid grid-cols-2 gap-3 md:grid-cols-4">
      <div class="rounded-lg bg-surface-2 px-3 py-2">
        <div class="text-xs text-muted">K/D</div>
        <div class="text-xl font-bold tabular-nums">{active.kd !== null ? active.kd.toFixed(2) : '—'}</div>
        <div class="text-xs text-muted tabular-nums">{n(active.kills)} / {n(active.deaths)}</div>
      </div>
      <div class="rounded-lg bg-surface-2 px-3 py-2">
        <div class="text-xs text-muted">Win rate</div>
        <div class="text-xl font-bold tabular-nums">{active.winPct !== null ? `${active.winPct.toFixed(1)}%` : '—'}</div>
        <div class="text-xs tabular-nums"><span class="text-good">{n(active.wins)}W</span> <span class="text-bad">{n(active.losses)}L</span></div>
      </div>
      <div class="rounded-lg bg-surface-2 px-3 py-2">
        <div class="text-xs text-muted">Matches</div>
        <div class="text-xl font-bold tabular-nums">{n(active.matches)}</div>
        {#if active.abandons}<div class="text-xs text-muted tabular-nums">{n(active.abandons)} abandoned</div>{/if}
      </div>
      <div class="rounded-lg bg-surface-2 px-3 py-2">
        <div class="text-xs text-muted">Time played</div>
        <div class="text-xl font-bold tabular-nums">{active.timePlayed ?? '—'}</div>
      </div>
    </div>

    {#if hasRank}
      <div class="mt-4">
        <h4 class="mb-2 text-xs font-medium tracking-wide text-muted uppercase">
          {active.rankName || active.maxRankName ? 'Rank' : 'Rating'}: {isCurrent ? 'current vs peak' : 'final vs peak'}
        </h4>
        <div class="grid grid-cols-2 gap-3">
          <div class="flex items-center gap-3 rounded-lg bg-surface-2 px-3 py-2">
            <RankIcon icon={active.rankIcon} name={active.rankName ?? 'Rank'} size={48} />
            <div class="min-w-0">
              <div class="text-xs text-muted">{isCurrent ? 'Current' : 'Final'}</div>
              <div class="font-semibold">{active.rankName ?? (active.mmr ? 'MMR' : 'Unranked')}</div>
              <div class="text-xs text-muted tabular-nums">
                {#if active.rankPoints}{n(active.rankPoints)} RP{:else if active.mmr}{n(active.mmr)} MMR{/if}
              </div>
            </div>
          </div>
          <div class="flex items-center gap-3 rounded-lg bg-surface-2 px-3 py-2">
            <RankIcon icon={active.maxRankIcon} name={active.maxRankName ?? 'Peak'} size={48} />
            <div class="min-w-0">
              <div class="text-xs text-muted">Peak</div>
              <div class="font-semibold">{active.maxRankName ?? (active.maxMmr ? 'MMR' : '—')}</div>
              <div class="text-xs text-muted tabular-nums">
                {#if active.maxRankPoints}{n(active.maxRankPoints)} RP{:else if active.maxMmr}{n(active.maxMmr)} MMR{/if}
              </div>
            </div>
          </div>
        </div>
      </div>
    {/if}
  {/if}
</Card>
