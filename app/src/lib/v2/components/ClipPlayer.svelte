<script lang="ts">
  import { X, Star, FolderOpen } from '@lucide/svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import { toastError } from '$lib/toast.svelte';
  import { cn } from '$lib/utils';
  import * as clips from '$lib/v2/clips';
  import type { ClipRow } from '$lib/v2/types';

  let { clip, onclose, onchange }: { clip: ClipRow; onclose: () => void; onchange?: (c: ClipRow) => void } = $props();

  let url = $state('');
  let failed = $state(false);
  let starred = $state(false);
  let video = $state<HTMLVideoElement | null>(null);

  $effect(() => {
    const c = clip;
    url = '';
    failed = false;
    starred = c.starred;
    clips
      .clipUrl(c.path)
      .then((u) => {
        if (c === clip) url = u;
      })
      .catch((e) => {
        console.error('[clips] cannot build the clip URL', e);
        failed = true;
      });
  });

  const fmtTime = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
  const title = $derived(`${clip.killer || 'Unknown'} killed ${clip.victim}`);

  /** The file starts at a keyframe before the wanted moment; jump to the wanted start. */
  function onLoaded() {
    if (video && clip.startMs > 0) video.currentTime = clip.startMs / 1000;
  }

  async function toggleStar() {
    const next = !starred;
    try {
      await clips.setClipStarred(clip.id, next);
      starred = next;
      onchange?.({ ...clip, starred: next });
    } catch (e) {
      toastError(e);
    }
  }

  async function reveal() {
    try {
      await clips.showClipInFolder(clip.id);
    } catch (e) {
      toastError(e);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-6" onclick={onclose}>
  <div
    class="flex max-h-full w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-border bg-surface-1 shadow-2xl"
    role="dialog"
    aria-modal="true"
    aria-label="Clip player"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
  >
    <div class="flex items-center gap-3 border-b border-border px-4 py-2.5">
      <div class="min-w-0 flex-1">
        <div class="truncate text-sm font-semibold">{title}</div>
        <div class="text-xs text-muted">
          Round {clip.round} · {clip.kind === 'kill' ? 'Kill' : 'Death'} at {fmtTime(clip.timeS)} · {clips.fmtBytes(clip.size)}
        </div>
      </div>
      <button
        type="button"
        onclick={toggleStar}
        aria-label={starred ? 'Remove star' : 'Star this clip'}
        aria-pressed={starred}
        title={starred ? 'Starred: this clip is never deleted' : 'Star to keep this clip forever'}
        class="rounded-md p-1.5 text-muted hover:bg-surface-2 hover:text-foreground"
      >
        <Star class={cn('size-5', starred && 'fill-accent text-accent')} />
      </button>
      <Button size="sm" onclick={reveal}><FolderOpen class="size-4" />Show in folder</Button>
      <button type="button" onclick={onclose} aria-label="Close" class="rounded-md p-1 text-muted hover:bg-surface-2 hover:text-foreground">
        <X class="size-5" />
      </button>
    </div>
    <div class="bg-black">
      {#if failed}
        <p class="p-8 text-center text-bad">This clip cannot be played.</p>
      {:else if url}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          bind:this={video}
          src={url}
          controls
          autoplay
          class="max-h-[70vh] w-full"
          onloadedmetadata={onLoaded}
          onerror={() => (failed = true)}
        ></video>
      {:else}
        <div class="h-64"></div>
      {/if}
    </div>
  </div>
</div>
