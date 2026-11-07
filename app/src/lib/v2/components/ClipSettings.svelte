<script lang="ts">
  import { onMount } from 'svelte';
  import { TriangleAlert, FolderOpen } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import { toast, toastError } from '$lib/toast.svelte';
  import { cn } from '$lib/utils';
  import * as clips from '$lib/v2/clips';
  import type { ClipKinds, ClipSettings, ClipsStatus } from '$lib/v2/types';

  let settings = $state<ClipSettings>({ ...clips.DEFAULT_CLIP_SETTINGS });
  let status = $state<ClipsStatus | null>(null);
  let loaded = $state(false);
  let busy = $state(false);
  let confirmDelete = $state(false);

  const selectCls =
    'h-9 rounded-lg border border-border-strong bg-surface-2 px-2 text-sm text-foreground outline-none focus:border-accent';
  const numberCls = cn(selectCls, 'w-20');

  const KEEP = [
    { v: 5, label: 'Last 5 matches' },
    { v: 10, label: 'Last 10 matches' },
    { v: 20, label: 'Last 20 matches' },
    { v: 0, label: 'All matches' }
  ];
  const CAPS = [
    { v: 0, label: 'No limit' },
    { v: 2, label: '2 GB' },
    { v: 5, label: '5 GB' },
    { v: 10, label: '10 GB' },
    { v: 20, label: '20 GB' },
    { v: 50, label: '50 GB' }
  ];
  const KINDS: { v: ClipKinds; label: string }[] = [
    { v: 'kills', label: 'Kills' },
    { v: 'deaths', label: 'Deaths' },
    { v: 'both', label: 'Both' }
  ];

  async function refreshStatus() {
    try {
      status = await clips.clipsStatus();
    } catch (e) {
      console.error('[clips] status failed', e);
    }
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let closed = false;
    (async () => {
      try {
        settings = await clips.getClipSettings();
        await refreshStatus();
      } catch (e) {
        toastError(e);
      } finally {
        loaded = true;
      }
    })();
    clips
      .onClipsUpdated(() => void refreshStatus())
      .then((u) => (closed ? u() : (unlisten = u)))
      .catch((e) => console.error('[clips] cannot listen for clip updates', e));
    // The game starts and stops by itself, so the recording state is polled while this page is open.
    const timer = setInterval(() => void refreshStatus(), 3000);
    return () => {
      closed = true;
      unlisten?.();
      clearInterval(timer);
    };
  });

  /** Saves a change right away; the backend clamps the values and returns what it stored. */
  async function save(patch: Partial<ClipSettings>) {
    const next = { ...settings, ...patch };
    settings = next;
    busy = true;
    try {
      settings = await clips.setClipSettings(next);
      await refreshStatus();
    } catch (e) {
      toastError(e);
      try {
        settings = await clips.getClipSettings();
      } catch {
        /* keep the local value */
      }
    } finally {
      busy = false;
    }
  }

  const num = (e: Event, fallback: number) => {
    const v = parseFloat((e.currentTarget as HTMLInputElement).value);
    return Number.isFinite(v) ? v : fallback;
  };

  async function openFolder() {
    try {
      await clips.openClipsFolder();
    } catch (e) {
      toastError(e);
    }
  }

  async function deleteAll() {
    busy = true;
    try {
      const n = await clips.deleteAllClips();
      toast('success', `${n} clip${n === 1 ? '' : 's'} deleted.`);
      await refreshStatus();
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
      confirmDelete = false;
    }
  }

  const stateText = $derived.by(() => {
    const r = status?.recording;
    if (!settings.enabled) return 'Off';
    switch (r?.state) {
      case 'recording':
        return 'Recording the game window';
      case 'waiting':
        return 'Waiting for Rainbow Six Siege to start';
      case 'minimized':
        return 'The game is minimized, nothing is recorded';
      case 'error':
        return r.message || 'Recording failed';
      default:
        return 'Starting';
    }
  });
</script>

<Card>
  <div class="flex items-start justify-between gap-3">
    <div>
      <h3 class="font-semibold">Clips</h3>
      <p class="mt-1 text-muted">
        Record the game window while you play and keep a short clip of each of your kills. Off by default. Clips are cut
        when a round ends and stay on this PC.
      </p>
    </div>
    <button
      type="button"
      role="switch"
      aria-checked={settings.enabled}
      aria-label="Record clips"
      disabled={!loaded || busy}
      onclick={() => save({ enabled: !settings.enabled })}
      class={cn(
        'relative mt-1 h-6 w-11 flex-none rounded-full border transition disabled:opacity-50',
        settings.enabled ? 'border-accent bg-accent' : 'border-border-strong bg-surface-3'
      )}
    >
      <span
        class={cn(
          'absolute top-0.5 size-4.5 rounded-full bg-white transition-all',
          settings.enabled ? 'left-[22px]' : 'left-0.5'
        )}
      ></span>
    </button>
  </div>

  <p class="mt-3 text-xs text-muted">
    Status: <span class={cn('font-medium', status?.recording.state === 'recording' ? 'text-good' : 'text-foreground')}>{stateText}</span>
  </p>

  {#if settings.enabled && status?.recording.blackFrames}
    <div class="mt-3 flex gap-2 rounded-lg border border-accent/40 bg-accent/10 p-3 text-sm">
      <TriangleAlert class="mt-0.5 size-4 flex-none text-accent" />
      <p>
        <span class="font-semibold">Use borderless or windowed mode.</span> The captured game window is black, which
        happens in exclusive fullscreen. In Siege, open Display settings and set the window mode to Borderless or Windowed.
      </p>
    </div>
  {/if}

  <div class="mt-4 space-y-4 border-t border-border pt-4">
    <div class="flex flex-wrap items-center gap-3">
      <span class="w-44 text-sm">Make clips of</span>
      <div class="inline-flex overflow-hidden rounded-lg border border-border-strong">
        {#each KINDS as k (k.v)}
          <button
            type="button"
            disabled={!loaded || busy}
            onclick={() => save({ kinds: k.v })}
            class={cn(
              'h-9 px-4 text-sm transition disabled:opacity-50',
              settings.kinds === k.v ? 'bg-accent font-semibold text-accent-foreground' : 'bg-surface-2 hover:bg-surface-3'
            )}>{k.label}</button
          >
        {/each}
      </div>
    </div>

    <div class="flex flex-wrap items-center gap-3">
      <label for="clip-keep" class="w-44 text-sm">Keep clips for</label>
      <select id="clip-keep" class={selectCls} disabled={!loaded || busy} value={settings.keepMatches} onchange={(e) => save({ keepMatches: Number(e.currentTarget.value) })}>
        {#each KEEP as o (o.v)}<option value={o.v}>{o.label}</option>{/each}
      </select>
      <span class="text-xs text-muted">Starred clips are never deleted.</span>
    </div>

    <div class="flex flex-wrap items-center gap-3">
      <label for="clip-cap" class="w-44 text-sm">Disk limit</label>
      <select id="clip-cap" class={selectCls} disabled={!loaded || busy} value={settings.diskCapGb} onchange={(e) => save({ diskCapGb: Number(e.currentTarget.value) })}>
        {#each CAPS as o (o.v)}<option value={o.v}>{o.label}</option>{/each}
        {#if !CAPS.some((o) => o.v === settings.diskCapGb)}<option value={settings.diskCapGb}>{settings.diskCapGb} GB</option>{/if}
      </select>
      <span class="text-xs text-muted">Oldest clips go first.</span>
    </div>

    <div class="flex flex-wrap items-center gap-3">
      <span class="w-44 text-sm">Clip length</span>
      <input type="number" min="2" max="20" step="1" class={numberCls} disabled={!loaded || busy} value={settings.beforeS} onchange={(e) => save({ beforeS: num(e, settings.beforeS) })} aria-label="Seconds before" />
      <span class="text-xs text-muted">s before the kill,</span>
      <input type="number" min="0" max="10" step="1" class={numberCls} disabled={!loaded || busy} value={settings.afterS} onchange={(e) => save({ afterS: num(e, settings.afterS) })} aria-label="Seconds after" />
      <span class="text-xs text-muted">s after</span>
    </div>

    <div class="flex flex-wrap items-center gap-3">
      <label for="clip-offset" class="w-44 text-sm">Clip offset (s)</label>
      <input id="clip-offset" type="number" min="-30" max="30" step="0.5" class={numberCls} disabled={!loaded || busy} value={settings.offsetS} onchange={(e) => save({ offsetS: num(e, settings.offsetS) })} />
      <span class="text-xs text-muted">Moves every clip in time. Use it if clips start too early (+) or too late (-).</span>
    </div>
  </div>

  <div class="mt-4 flex flex-wrap items-center gap-3 border-t border-border pt-4">
    <p class="mr-auto text-xs text-muted">
      {#if status}
        {status.clipsCount} clip{status.clipsCount === 1 ? '' : 's'} · {clips.fmtBytes(status.clipsBytes)}
        {#if status.starredCount > 0}· {status.starredCount} starred{/if}
        · recording buffer {clips.fmtBytes(status.bufferBytes)}
      {:else}
        &nbsp;
      {/if}
    </p>
    <Button size="sm" onclick={openFolder} disabled={!status}><FolderOpen class="size-4" />Open folder</Button>
    {#if confirmDelete}
      <span class="text-sm">Delete every clip, starred ones too?</span>
      <Button size="sm" variant="danger" onclick={deleteAll} disabled={busy}>Yes, delete</Button>
      <Button size="sm" onclick={() => (confirmDelete = false)} disabled={busy}>Cancel</Button>
    {:else}
      <Button size="sm" variant="danger" onclick={() => (confirmDelete = true)} disabled={busy || !status || status.clipsCount === 0}>Delete all clips</Button>
    {/if}
  </div>

  <p class="mt-3 text-xs text-muted">
    Needs "Match Replay" turned on in Siege, and the game in borderless or windowed mode. The game window is captured like
    OBS "Window Capture" does it: nothing is injected into the game and no game memory is read. Recording stops while the
    game is minimized or closed.
  </p>
</Card>
