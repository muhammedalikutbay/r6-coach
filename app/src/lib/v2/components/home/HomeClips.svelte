<script lang="ts" module>
  import type { ClipRow } from '$lib/v2/types';

  export interface HomeClip {
    clip: ClipRow;
    mapKey: string;
    mapName: string;
  }
</script>

<script lang="ts">
  // Latest clips across your newest matches. A click plays the clip in the shared ClipPlayer.
  import { Crosshair, Skull, Star, Play } from '@lucide/svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import ClipPlayer from '$lib/v2/components/ClipPlayer.svelte';
  import { cn } from '$lib/utils';

  let { clips }: { clips: HomeClip[] } = $props();

  let playing = $state<ClipRow | null>(null);
  let starOverride = $state<Record<number, boolean>>({});

  const fmtTime = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
  const isStarred = (c: ClipRow) => starOverride[c.id] ?? c.starred;
  const playList = $derived(clips.map((x) => ({ ...x.clip, starred: isStarred(x.clip) })));
  const other = (c: ClipRow) => (c.kind === 'kill' ? c.victim : c.killer);

  function changed(c: ClipRow) {
    starOverride = { ...starOverride, [c.id]: c.starred };
    if (playing && playing.id === c.id) playing = c;
  }
</script>

<div class="flex gap-3 overflow-x-auto pb-2">
  {#each clips as x (x.clip.id)}
    {@const c = x.clip}
    <button
      type="button"
      onclick={() => (playing = c)}
      class="group w-44 shrink-0 overflow-hidden rounded-xl border border-border bg-surface-1 text-left transition-colors hover:border-accent hover:bg-surface-2"
      title="Play this {c.kind === 'kill' ? 'kill' : 'death'}"
    >
      <div class="relative">
        <MapThumb mapKey={x.mapKey} name={x.mapName} class="rounded-none" />
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
      <div class="p-2">
        <div class="truncate text-xs font-semibold">{x.mapName}</div>
        <div class="truncate text-[11px] text-muted tabular-nums">
          Round {c.round} · {fmtTime(c.timeS)}{#if other(c)} · {c.kind === 'kill' ? 'vs' : 'by'} {other(c)}{/if}
        </div>
      </div>
    </button>
  {/each}
</div>

{#if playing}
  <ClipPlayer clip={playing} list={playList} onclose={() => (playing = null)} onchange={changed} onselect={(c) => (playing = c)} />
{/if}
