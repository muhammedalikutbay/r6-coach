<script lang="ts">
  // Strip of the clips that belong to a kill or death on an operator or weapon screen. A click
  // plays the clip in the shared ClipPlayer. Shows only a hint when there are no clips.
  import { Crosshair, Skull, Star, Play } from '@lucide/svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import ClipPlayer from '$lib/v2/components/ClipPlayer.svelte';
  import { mapName, operatorName } from '$lib/v2/catalog';
  import { cn } from '$lib/utils';
  import type { ClipRow, DetailClip } from '$lib/v2/types';

  let { clips }: { clips: DetailClip[] } = $props();

  let playing = $state<ClipRow | null>(null);
  // Stars changed in the player; the strip keeps showing them until the next load.
  let starOverride = $state<Record<number, boolean>>({});

  const fmtTime = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
  const isStarred = (c: ClipRow) => starOverride[c.id] ?? c.starred;

  function changed(c: ClipRow) {
    starOverride = { ...starOverride, [c.id]: c.starred };
    if (playing && playing.id === c.id) playing = c;
  }
</script>

{#if clips.length === 0}
  <p class="rounded-xl border border-dashed border-border-strong px-4 py-6 text-center text-sm text-muted">
    No clips for this yet. Turn on Clips in Settings and play; kills and deaths recorded from now on show up here.
  </p>
{:else}
  <div class="flex gap-3 overflow-x-auto pb-2">
    {#each clips as dc (dc.clip.id)}
      {@const c = dc.clip}
      <button
        type="button"
        onclick={() => (playing = c)}
        class="group w-44 shrink-0 overflow-hidden rounded-xl border border-border bg-surface-1 text-left transition-colors hover:border-accent hover:bg-surface-2"
        title="Play this {c.kind === 'kill' ? 'kill' : 'death'}"
      >
        <div class="relative">
          <MapThumb mapKey={dc.mapKey} name={mapName(dc.mapKey)} class="rounded-none" />
          <span class="absolute inset-0 flex items-center justify-center bg-black/30 opacity-0 transition-opacity group-hover:opacity-100">
            <Play class="size-8 fill-white text-white" />
          </span>
          <span
            class={cn(
              'absolute top-1.5 left-1.5 inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-semibold text-white',
              c.kind === 'kill' ? 'bg-good/90' : 'bg-bad/90'
            )}
          >
            {#if c.kind === 'kill'}<Crosshair class="size-3" />Kill{:else}<Skull class="size-3" />Death{/if}
          </span>
          {#if isStarred(c)}<Star class="absolute top-1.5 right-1.5 size-4 fill-accent text-accent" />{/if}
        </div>
        <div class="flex items-center gap-2 p-2">
          {#if dc.enemyOperatorKey}
            <OperatorBadge opKey={dc.enemyOperatorKey} name={operatorName(dc.enemyOperatorKey)} size={26} />
          {/if}
          <div class="min-w-0 flex-1">
            <div class="truncate text-xs font-semibold">
              {c.kind === 'kill' ? 'vs' : 'by'}
              {dc.enemyOperatorKey ? operatorName(dc.enemyOperatorKey) : 'unknown'}
            </div>
            <div class="truncate text-[11px] text-muted tabular-nums">
              Round {c.round} · {fmtTime(c.timeS)}{#if dc.headshot} · HS{/if}{#if dc.weapon} · {dc.weapon}{/if}
            </div>
          </div>
        </div>
      </button>
    {/each}
  </div>
{/if}

{#if playing}
  <ClipPlayer clip={playing} onclose={() => (playing = null)} onchange={changed} />
{/if}
