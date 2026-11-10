<script lang="ts">
  import { tick } from 'svelte';
  import { goto } from '$app/navigation';
  import { BarChart3, Brain, Clapperboard, Trophy, FolderOpen, KeyRound, Video, ShieldCheck, Check } from '@lucide/svelte';
  import { welcome, dismissWelcome } from '$lib/welcome.svelte';
  import { getReplayDir, setReplayDir, pickReplayDir } from '$lib/api';
  import { toast, toastError } from '$lib/toast.svelte';
  import Button from '$lib/components/ui/Button.svelte';

  let button: HTMLButtonElement | null = $state(null);

  // Move focus into the dialog when it opens, so keyboard users land on the action.
  // Depends on `button` too, so it also works on the very first open, when the button mounts after this runs.
  $effect(() => {
    if (!welcome.open || !button) return;
    tick().then(() => button?.focus());
    // On the very first open the webview may still be taking focus itself; retry once it settled.
    const retry = setTimeout(() => {
      if (welcome.open && (document.activeElement === document.body || !document.activeElement)) button?.focus();
    }, 400);
    return () => clearTimeout(retry);
  });

  // Step 1 shows the replay folder that is already set.
  let replayDir = $state('');
  $effect(() => {
    if (!welcome.open) return;
    getReplayDir()
      .then((d) => (replayDir = d ?? ''))
      .catch((e) => console.error('welcome: cannot read the replay folder', e));
  });
  /** Last two path segments, enough to recognise the folder. */
  const shortDir = $derived(replayDir.split(/[\\/]+/).filter(Boolean).slice(-2).join('\\'));

  async function pickFolder() {
    try {
      const next = await pickReplayDir(replayDir);
      if (!next || next === replayDir) return;
      await setReplayDir(next);
      replayDir = next;
      toast('success', 'Folder saved.');
    } catch (e) {
      toastError(e);
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (welcome.open && e.key === 'Escape') dismissWelcome();
  }

  // Closes the screen and opens Settings, where the setup steps live.
  function toSettings() {
    dismissWelcome();
    goto('/settings').catch((e) => console.error('welcome: cannot open settings', e));
  }

  const features = [
    { icon: BarChart3, text: 'Stats from your own replay files' },
    { icon: Brain, text: 'A coach that compares you with yourself' },
    { icon: Clapperboard, text: 'Kill clips of your best moments' },
    { icon: Trophy, text: 'Your rank, via your own Arenyze key' }
  ];
  const steps = [
    { icon: FolderOpen, title: 'Pick your replay folder', hint: 'Where Siege saves MatchReplay files.', action: 'Choose folder' },
    { icon: KeyRound, title: 'Add an Arenyze API key', hint: 'Optional. Shows your rank and seasons.', action: 'Add key' },
    { icon: Video, title: 'Turn on clips', hint: 'Optional. Records the game window for kill clips.', action: 'Clip settings' }
  ];
  const operators = ['ash', 'alibi', 'amaru', 'aruni', 'ace'];
</script>

<svelte:window {onkeydown} />

{#if welcome.open}
  <div class="fixed inset-x-0 top-[var(--titlebar-h)] bottom-0 z-[100] flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
    <div
      class="animate-in fade-in zoom-in-95 flex max-h-full w-[min(560px,100%)] flex-col overflow-hidden rounded-2xl border border-border-strong bg-surface-1 shadow-2xl"
      role="dialog"
      aria-modal="true"
      aria-labelledby="welcome-title"
    >
      <div class="relative flex-none border-b border-border bg-surface-2 px-7 pt-7 pb-5 [@media(max-height:680px)]:pt-5 [@media(max-height:680px)]:pb-4">
        <div class="absolute inset-x-0 top-0 h-0.5 bg-accent"></div>
        <div class="mb-4 flex items-center gap-1.5" aria-hidden="true">
          {#each operators as op (op)}
            <img src="/assets/operators/{op}.png" alt="" class="size-9 rounded-lg bg-surface-3 p-0.5" />
          {/each}
        </div>
        <h2 id="welcome-title" class="text-2xl font-bold">Welcome to <span class="text-accent">R6 Coach</span></h2>
        <p class="mt-1.5 leading-relaxed text-muted">
          Get better at Rainbow Six Siege by learning from your own matches.
        </p>
        <ul class="mt-4 grid grid-cols-1 gap-x-4 gap-y-2 sm:grid-cols-2 [@media(max-height:680px)]:hidden">
          {#each features as f (f.text)}
            <li class="flex items-center gap-2 text-[13px]">
              <f.icon class="size-4 flex-none text-accent" />
              {f.text}
            </li>
          {/each}
        </ul>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto px-7 pt-5 pb-3">
        <h3 class="mb-2.5 text-xs font-semibold tracking-wider text-muted uppercase">Set up in 3 steps</h3>
        <ol class="space-y-2">
          {#each steps as s, i (s.title)}
            <li class="flex items-center gap-3 rounded-xl border border-border bg-surface-2 px-3 py-2.5">
              <span class="flex size-7 flex-none items-center justify-center rounded-full bg-surface-3 text-xs font-bold text-accent">{i + 1}</span>
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-1.5 text-sm font-medium">
                  {#if i === 0 && replayDir}<Check class="size-4 flex-none text-good" aria-label="Folder set" />{/if}
                  {i === 0 && replayDir ? 'Replay folder set' : s.title}
                </div>
                <div class="truncate text-xs text-muted" title={i === 0 && replayDir ? replayDir : undefined}>{i === 0 && replayDir ? shortDir : s.hint}</div>
              </div>
              {#if i === 0}
                <Button size="sm" onclick={pickFolder}><s.icon class="size-3.5" />{replayDir ? 'Change' : s.action}</Button>
              {:else}
                <Button size="sm" onclick={toSettings}><s.icon class="size-3.5" />{s.action}</Button>
              {/if}
            </li>
          {/each}
        </ol>
      </div>

      <!-- Footer stays visible however small the window is; only the body above scrolls. -->
      <div class="flex flex-none items-center justify-between gap-4 border-t border-border px-7 py-4">
        <p class="flex min-w-0 items-center gap-2 text-xs text-muted">
          <ShieldCheck class="size-4 flex-none text-good" />
          Everything stays on this PC. Replays are read locally and nothing is uploaded.
        </p>
        <Button variant="primary" size="lg" class="flex-none" bind:ref={button} onclick={dismissWelcome}>Get started</Button>
      </div>
    </div>
  </div>
{/if}
