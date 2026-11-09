<script lang="ts">
  import MapThumb from '$lib/components/MapThumb.svelte';
  import { cn } from '$lib/utils';
  import { formatShortDateTime, formatTime } from '$lib/v2/format';
  import type { MatchRow } from '$lib/v2/types';

  let { match, onclick, active = false, showDate = false }: { match: MatchRow; onclick?: () => void; active?: boolean; showDate?: boolean } = $props();

  const kd = $derived(match.deaths > 0 ? match.kills / match.deaths : match.kills);
  const time = $derived(
    showDate ? formatShortDateTime(match.timestamp) : formatTime(match.timestamp)
  );
</script>

<button
  type="button"
  {onclick}
  class={cn(
    'group flex w-full items-center gap-4 rounded-lg border border-border bg-surface-1 py-2 pr-4 pl-2 text-left transition-colors hover:bg-surface-2',
    'border-l-4',
    match.won ? 'border-l-good' : 'border-l-bad',
    active && 'bg-surface-2 ring-1 ring-accent'
  )}
>
  <MapThumb mapKey={match.mapKey} name={match.mapName} class="w-28 shrink-0" />
  <div class="min-w-0 flex-1">
    <div class="truncate font-semibold">{match.mapName}</div>
    <div class="truncate text-xs text-muted">{match.mode} · {time}</div>
  </div>
  <div class="hidden min-w-0 flex-1 flex-wrap gap-1 md:flex">
    {#each match.badges as b (b)}
      <span class="rounded-md bg-surface-3 px-1.5 py-0.5 text-xs font-medium text-accent">{b}</span>
    {/each}
  </div>
  <div class="w-20 text-center">
    <div class={cn('text-base font-bold', match.won ? 'text-good' : 'text-bad')}>
      {match.scoreUs} - {match.scoreThem}
    </div>
    <div class="text-xs text-muted">{match.won ? 'Win' : 'Loss'}</div>
  </div>
  <div class="w-16 text-center">
    <div class="font-semibold">{kd.toFixed(2)}</div>
    <div class="text-xs text-muted">{match.kills}/{match.deaths}</div>
  </div>
  <div class="w-14 text-center">
    <div class="font-semibold">{match.hsPct.toFixed(0)}%</div>
    <div class="text-xs text-muted">HS</div>
  </div>
</button>
