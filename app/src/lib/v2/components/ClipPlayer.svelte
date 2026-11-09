<script lang="ts">
  import { X, Star, FolderOpen, ExternalLink } from '@lucide/svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import { toastError } from '$lib/toast.svelte';
  import { cn } from '$lib/utils';
  import * as clips from '$lib/v2/clips';
  import type { ClipRow } from '$lib/v2/types';

  let { clip, onclose, onchange }: { clip: ClipRow; onclose: () => void; onchange?: (c: ClipRow) => void } = $props();

  /** How the video gets its bytes: the asset protocol first, a Blob read through the backend second. */
  type Source = 'asset' | 'blob';

  /** The asset URL gets this long to produce metadata before the Blob route is tried. */
  const ASSET_TIMEOUT_MS = 4000;

  let url = $state('');
  let source = $state<Source>('asset');
  let failed = $state(false);
  let detail = $state('');
  let loading = $state(true);
  let starred = $state(false);
  let video = $state<HTMLVideoElement | null>(null);

  let blobUrl = '';
  let timer: ReturnType<typeof setTimeout> | undefined;

  function dropBlob() {
    if (blobUrl) URL.revokeObjectURL(blobUrl);
    blobUrl = '';
  }

  /** Asset URL failed (or hung): read the file through the backend and play it from memory. */
  async function useBlob(c: ClipRow, why: string) {
    console.warn(`[clips] asset playback failed (${why}); trying a Blob URL for clip ${c.id}`);
    try {
      const u = await clips.clipBlobUrl(c.id);
      if (c !== clip) {
        URL.revokeObjectURL(u);
        return;
      }
      dropBlob();
      blobUrl = u;
      source = 'blob';
      url = u;
    } catch (e) {
      console.error('[clips] cannot read the clip file', e);
      giveUp(`${why}; reading the file failed too: ${e}`);
    }
  }

  function giveUp(why: string) {
    console.error(`[clips] cannot play clip ${clip.id} (${clip.path}): ${why}`);
    clearTimeout(timer);
    detail = why;
    failed = true;
    loading = false;
  }

  $effect(() => {
    const c = clip;
    clearTimeout(timer);
    dropBlob();
    url = '';
    source = 'asset';
    failed = false;
    detail = '';
    loading = true;
    starred = c.starred;
    clips
      .clipUrl(c.path)
      .then((u) => {
        if (c !== clip) return;
        url = u;
        // A source that neither loads nor errors counts as failed too.
        timer = setTimeout(() => {
          if (c === clip && source === 'asset' && loading) void useBlob(c, 'no data after ' + ASSET_TIMEOUT_MS + ' ms');
        }, ASSET_TIMEOUT_MS);
      })
      .catch((e) => {
        console.error('[clips] cannot build the clip URL', e);
        void useBlob(c, `no asset URL: ${e}`);
      });
    return () => {
      clearTimeout(timer);
      dropBlob();
    };
  });

  const fmtTime = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
  const title = $derived(`${clip.killer || 'Unknown'} killed ${clip.victim}`);

  /** The file starts at a keyframe before the wanted moment; jump to the wanted start. */
  function onLoaded() {
    clearTimeout(timer);
    loading = false;
    if (video && clip.startMs > 0) video.currentTime = clip.startMs / 1000;
  }

  function onVideoError() {
    const text = clips.videoErrorText(video?.error ?? null);
    console.error(`[clips] video error via ${source}: ${text}; src=${video?.currentSrc}`);
    if (source === 'asset') void useBlob(clip, text);
    else giveUp(text);
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

  async function external() {
    try {
      await clips.openClipExternal(clip.id);
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
      <Button size="sm" onclick={external} title="Open the clip in the default video player of this PC">
        <ExternalLink class="size-4" />Open in default player
      </Button>
      <Button size="sm" onclick={reveal}><FolderOpen class="size-4" />Show in folder</Button>
      <button type="button" onclick={onclose} aria-label="Close" class="rounded-md p-1 text-muted hover:bg-surface-2 hover:text-foreground">
        <X class="size-5" />
      </button>
    </div>
    <div class="bg-black">
      {#if failed}
        <div class="p-8 text-center">
          <p class="text-bad">This clip cannot be played in the app.</p>
          <p class="mt-2 text-sm text-muted">Use "Open in default player" above to watch it in your video player.</p>
          {#if detail}<p class="mt-3 break-words font-mono text-xs text-muted" data-testid="clip-error">{detail}</p>{/if}
        </div>
      {:else if url}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          bind:this={video}
          src={url}
          controls
          autoplay
          class="max-h-[70vh] w-full"
          data-source={source}
          onloadedmetadata={onLoaded}
          onerror={onVideoError}
        ></video>
      {:else}
        <div class="h-64"></div>
      {/if}
    </div>
  </div>
</div>
