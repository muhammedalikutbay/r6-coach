<script lang="ts">
  import '../app.css';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';
  import { getReplayDir } from '$lib/api';
  import WelcomeScreen from '$lib/WelcomeScreen.svelte';
  let { children } = $props();
  // First launch: when no replay folder is known (not saved, not auto-found), go to Settings to pick one.
  onMount(async () => {
    try {
      if (!(await getReplayDir())) await goto('/settings?setup=1');
    } catch (e) { console.error('first-run folder check failed', e); }
  });
  const tabs = [
    { href: '/', label: 'Home' },
    { href: '/operators', label: 'Operators' },
    { href: '/maps', label: 'Maps' },
    { href: '/settings', label: 'Settings' }
  ];
</script>

<div class="shell">
  <nav>
    <div class="brand">R6 Coach</div>
    {#each tabs as t}
      <a href={t.href + page.url.search} class:active={page.url.pathname === t.href}>{t.label}</a>
    {/each}
  </nav>
  <main>{@render children()}</main>
</div>
<WelcomeScreen />

<style>
  .shell { display: flex; height: 100vh; min-width: 0; }
  nav { width: 150px; flex: none; background: var(--panel); border-right: 1px solid var(--line);
    padding: 16px 10px; display: flex; flex-direction: column; gap: 4px; }
  .brand { font-weight: 700; font-size: 16px; padding: 0 10px 14px; }
  a { color: var(--muted); text-decoration: none; padding: 8px 10px; border-radius: 8px; }
  a:hover { background: var(--panel2); color: var(--text); }
  a.active { background: var(--panel2); color: var(--text); box-shadow: inset 3px 0 var(--accent); }
  main { flex: 1; min-width: 0; overflow-y: auto; padding: 20px 24px; }
</style>
