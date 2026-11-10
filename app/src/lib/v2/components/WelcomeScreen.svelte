<script lang="ts">
  import { tick } from 'svelte';
  import { goto } from '$app/navigation';
  import { BarChart3, Brain, Clapperboard, Trophy, FolderOpen, KeyRound, Video, ShieldCheck } from '@lucide/svelte';
  import { welcome, dismissWelcome } from '$lib/welcome.svelte';
  import Button from '$lib/components/ui/Button.svelte';

  let button: HTMLButtonElement | null = $state(null);

  // Move focus into the dialog when it opens, so keyboard users land on the action.
  $effect(() => {
    if (welcome.open) tick().then(() => button?.focus());
  });

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
  <div class="fixed inset-x-0 top-8 bottom-0 z-[100] flex items-center justify-center overflow-y-auto bg-black/70 p-4 backdrop-blur-sm">
    <div
      class="animate-in fade-in zoom-in-95 m-auto w-[min(560px,100%)] overflow-hidden rounded-2xl border border-border-strong bg-surface-1 shadow-2xl"
      role="dialog"
      aria-modal="true"
      aria-labelledby="welcome-title"
    >
      <div class="relative border-b border-border bg-surface-2 px-7 pt-7 pb-5">
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
        <ul class="mt-4 grid grid-cols-1 gap-x-4 gap-y-2 sm:grid-cols-2">
          {#each features as f (f.text)}
            <li class="flex items-center gap-2 text-[13px]">
              <f.icon class="size-4 flex-none text-accent" />
              {f.text}
            </li>
          {/each}
        </ul>
      </div>

      <div class="px-7 py-5">
        <h3 class="mb-2.5 text-xs font-semibold tracking-wider text-muted uppercase">Set up in 3 steps</h3>
        <ol class="space-y-2">
          {#each steps as s, i (s.title)}
            <li class="flex items-center gap-3 rounded-xl border border-border bg-surface-2 px-3 py-2.5">
              <span class="flex size-7 flex-none items-center justify-center rounded-full bg-surface-3 text-xs font-bold text-accent">{i + 1}</span>
              <div class="min-w-0 flex-1">
                <div class="text-sm font-medium">{s.title}</div>
                <div class="text-xs text-muted">{s.hint}</div>
              </div>
              <Button size="sm" onclick={toSettings}><s.icon class="size-3.5" />{s.action}</Button>
            </li>
          {/each}
        </ol>

        <p class="mt-4 flex items-center gap-2 text-xs text-muted">
          <ShieldCheck class="size-4 flex-none text-good" />
          Everything stays on this PC. Replays are read locally and nothing is uploaded.
        </p>

        <div class="mt-5 flex justify-end">
          <Button variant="primary" size="lg" bind:ref={button} onclick={dismissWelcome}>Get started</Button>
        </div>
      </div>
    </div>
  </div>
{/if}
