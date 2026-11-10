<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';

  let maximized = $state(false);

  // Only available inside the Tauri window; plain browser runs (dev preview) get a bar without actions.
  const win = (() => {
    try {
      return getCurrentWindow();
    } catch (e) {
      console.warn('titlebar: no Tauri window', e);
      return null;
    }
  })();

  async function syncMaximized() {
    try {
      maximized = (await win?.isMaximized()) ?? false;
    } catch (e) {
      console.error('titlebar: isMaximized failed', e);
    }
  }

  onMount(() => {
    syncMaximized();
    let unlisten: (() => void) | undefined;
    let closed = false;
    win
      ?.onResized(() => syncMaximized())
      .then((u) => (closed ? u() : (unlisten = u)))
      .catch((e) => console.error('titlebar: cannot listen for resize', e));
    return () => {
      closed = true;
      unlisten?.();
    };
  });

  const run = (action: 'minimize' | 'toggleMaximize' | 'close') => () =>
    win?.[action]().catch((e) => console.error(`titlebar: ${action} failed`, e));

  const btn = 'flex h-full w-[46px] items-center justify-center text-muted transition-colors';
</script>

<div class="flex h-8 flex-none select-none items-stretch border-b border-border bg-surface-1" role="toolbar" aria-label="Window">
  <!-- Double-click on the drag region toggles maximize (handled by Tauri). -->
  <div data-tauri-drag-region class="flex min-w-0 flex-1 items-center gap-2 pl-3">
    <img src="/app-icon.png" alt="" class="pointer-events-none size-4 select-none" draggable="false" />
    <span class="pointer-events-none text-xs font-semibold tracking-wide text-foreground/90">R6 Coach</span>
  </div>
  <button type="button" class="{btn} hover:bg-surface-3 hover:text-foreground" aria-label="Minimize" onclick={run('minimize')}>
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" stroke="currentColor" stroke-width="1" fill="none" /></svg>
  </button>
  <button
    type="button"
    class="{btn} hover:bg-surface-3 hover:text-foreground"
    aria-label={maximized ? 'Restore' : 'Maximize'}
    onclick={run('toggleMaximize')}
  >
    {#if maximized}
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M2.5 2.5v-2h7v7h-2M0.5 2.5h7v7h-7z" stroke="currentColor" stroke-width="1" fill="none" />
      </svg>
    {:else}
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" stroke-width="1" fill="none" /></svg>
    {/if}
  </button>
  <button type="button" class="{btn} hover:bg-[#e81123] hover:text-white" aria-label="Close" onclick={run('close')}>
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" stroke-width="1" fill="none" /></svg>
  </button>
</div>
