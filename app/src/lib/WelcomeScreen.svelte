<script lang="ts">
  import { tick } from 'svelte';
  import { welcome, dismissWelcome } from './welcome.svelte';
  import Button from './components/ui/Button.svelte';

  let button: HTMLButtonElement | null = $state(null);

  // Move focus into the dialog when it opens, so keyboard users land on the action.
  $effect(() => {
    if (welcome.open) tick().then(() => button?.focus());
  });

  function onkeydown(e: KeyboardEvent) {
    if (welcome.open && e.key === 'Escape') dismissWelcome();
  }
</script>

<svelte:window {onkeydown} />

{#if welcome.open}
  <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
    <div
      class="animate-in fade-in zoom-in-95 w-[min(460px,100%)] rounded-2xl border border-border-strong bg-surface-1 px-6 pt-7 pb-6 text-center shadow-2xl"
      role="dialog"
      aria-modal="true"
      aria-labelledby="welcome-title"
    >
      <div class="mb-2 text-4xl leading-none" aria-hidden="true">🎂</div>
      <h2 id="welcome-title" class="mb-3 text-2xl font-bold">Happy Birthday!</h2>
      <p class="mb-5 leading-relaxed text-muted">
        R6 Coach reads your new Rainbow Six Siege match replays after each game, compares your
        last match with your own history, and shows short, practical advice. Press Update
        after every match to keep it current.
      </p>
      <Button variant="primary" size="lg" bind:ref={button} onclick={dismissWelcome}>Let's go</Button>
    </div>
  </div>
{/if}
