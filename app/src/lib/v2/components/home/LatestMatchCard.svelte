<script lang="ts">
  // Your last match: map, result, score, K/D and one line on what decided it.
  import { ArrowRight } from '@lucide/svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import { cn } from '$lib/utils';
  import { formatDateTime } from '$lib/v2/format';
  import type { MatchRow } from '$lib/v2/types';

  /** `line` is what decided the match; null while the scoreboard loads. */
  let { match, line }: { match: MatchRow; line: string | null } = $props();

  const kd = $derived(match.deaths > 0 ? match.kills / match.deaths : match.kills);
</script>

<Card class="flex h-full flex-col overflow-hidden p-0 sm:flex-row">
  <div class="relative shrink-0 sm:w-56">
    <MapThumb mapKey={match.mapKey} name={match.mapName} class="size-full min-h-32 rounded-none" />
    <span
      class={cn(
        'absolute top-2 left-2 rounded-md px-2 py-0.5 text-xs font-bold text-white',
        match.won ? 'bg-good/90' : 'bg-bad/90'
      )}>{match.won ? 'Win' : 'Loss'}</span
    >
  </div>
  <div class="flex min-w-0 flex-1 flex-col gap-2 p-4">
    <div>
      <div class="text-xs font-medium tracking-wide text-muted uppercase">Your last match</div>
      <div class="truncate text-lg font-bold">{match.mapName}</div>
      <div class="truncate text-xs text-muted">{match.mode} · {formatDateTime(match.timestamp)}</div>
    </div>
    <div class="flex flex-wrap items-baseline gap-x-5 gap-y-1">
      <div>
        <span class={cn('text-2xl font-bold tabular-nums', match.won ? 'text-good' : 'text-bad')}>{match.scoreUs} - {match.scoreThem}</span>
        <span class="ml-1 text-xs text-muted">score</span>
      </div>
      <div>
        <span class="text-2xl font-bold tabular-nums">{kd.toFixed(2)}</span>
        <span class="ml-1 text-xs text-muted tabular-nums">K/D · {match.kills}/{match.deaths}</span>
      </div>
    </div>
    <p class="min-h-5 text-sm leading-relaxed">
      {#if line}{line}{:else}<span class="text-muted">Reading the scoreboard…</span>{/if}
    </p>
    <a
      href="/matches?match={encodeURIComponent(match.matchId)}"
      class="mt-auto inline-flex items-center gap-1 text-sm font-medium text-accent hover:underline"
    >
      Open the match <ArrowRight class="size-4" />
    </a>
  </div>
</Card>
