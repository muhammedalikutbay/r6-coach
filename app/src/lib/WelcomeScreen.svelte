<script lang="ts">
  import { tick } from 'svelte';
  import { welcome, dismissWelcome } from './welcome.svelte';

  let button: HTMLButtonElement | undefined = $state();

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
  <div class="backdrop">
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="welcome-title">
      <div class="cake" aria-hidden="true">🎂</div>
      <h2 id="welcome-title">Happy Birthday!</h2>
      <p>
        R6 Coach reads your new Rainbow Six Siege match replays after each game, compares your
        last match with your own history, and shows short, practical advice. Press Update on Home
        after every match to keep it current.
      </p>
      <button class="btn primary" bind:this={button} onclick={dismissWelcome}>Let's go</button>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 100; display: flex; align-items: center;
    justify-content: center; padding: 16px; background: rgba(8, 9, 12, 0.7); }
  .dialog { width: min(460px, 100%); background: var(--panel); border: 1px solid var(--line);
    border-radius: 14px; padding: 28px 26px 24px; text-align: center; box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5); }
  .cake { font-size: 40px; line-height: 1; margin-bottom: 10px; }
  h2 { font-size: 24px; margin-bottom: 12px; }
  p { color: var(--muted); margin: 0 0 22px; line-height: 1.55; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; padding: 9px 22px; }
  .btn.primary:hover:not(:disabled) { filter: brightness(1.1); }
  .btn.primary:focus-visible { outline: 2px solid #fff; outline-offset: 2px; }
</style>
