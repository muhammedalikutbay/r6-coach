<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte';
  import RankIcon from './RankIcon.svelte';
  import { cn } from '$lib/utils';
  import { playlistOf } from '$lib/v2/arenyze';
  import type { SeasonEntry } from '$lib/v2/types';

  let { seasons, selectedId, onselect }: {
    seasons: SeasonEntry[];
    selectedId: number | null;
    onselect: (seasonId: number) => void;
  } = $props();

  const SHOWN = 5;
  let showAll = $state(false);
  const rows = $derived(showAll ? seasons : seasons.slice(0, SHOWN));
  const n = (v: number) => v.toLocaleString('en-GB');
</script>

<Card>
  <h3 class="mb-2 text-xs font-medium tracking-wide text-muted uppercase">Seasons</h3>
  <ul class="flex flex-col gap-1">
    {#each rows as s (s.seasonId)}
      {@const r = playlistOf(s, 'ranked')}
      <li>
        <button
          type="button"
          onclick={() => onselect(s.seasonId)}
          aria-pressed={s.seasonId === selectedId}
          class={cn(
            'flex w-full items-center gap-3 rounded-lg border border-transparent px-2 py-1.5 text-left transition-colors hover:bg-surface-2',
            s.seasonId === selectedId && 'border-border-strong bg-surface-2'
          )}
        >
          <span class="h-8 w-1 shrink-0 rounded-full" style="background:{s.color ?? 'var(--border-strong)'}"></span>
          <span class="min-w-0 flex-1">
            <span class="block text-sm font-semibold">{s.shortName}</span>
            <span class="block truncate text-xs text-muted">{s.name ?? ''}</span>
          </span>
          {#if r}
            <span class="flex items-center gap-2 text-right">
              {#if r.maxRankName || r.rankName}
                <RankIcon icon={r.maxRankIcon ?? r.rankIcon} name={r.maxRankName ?? r.rankName ?? ''} size={26} />
              {/if}
              <span class="text-xs tabular-nums">
                {#if r.maxRankPoints ?? r.rankPoints}
                  <span class="block font-medium">{n((r.maxRankPoints ?? r.rankPoints) as number)} RP</span>
                {:else if r.maxMmr ?? r.mmr}
                  <span class="block font-medium">{n((r.maxMmr ?? r.mmr) as number)} MMR</span>
                {/if}
                {#if r.kd !== null}<span class="block text-muted">{r.kd.toFixed(2)} K/D</span>{/if}
              </span>
            </span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
  {#if seasons.length > SHOWN}
    <button
      type="button"
      onclick={() => (showAll = !showAll)}
      class="mt-2 w-full rounded-lg py-1.5 text-xs text-muted transition-colors hover:bg-surface-2 hover:text-foreground"
    >
      {showAll ? 'Show fewer' : `Show all ${seasons.length}`}
    </button>
  {/if}
</Card>
