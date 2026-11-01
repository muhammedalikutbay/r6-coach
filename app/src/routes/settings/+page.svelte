<script lang="ts">
  import { onMount } from 'svelte';
  import { getReplayDir, setReplayDir, resetDatabase, parseMatch } from '$lib/api';
  import { showWelcomeAgain } from '$lib/welcome.svelte';

  let dir = $state('');
  let loading = $state(true);
  let busy = $state(false);
  let msg = $state('');
  let error = $state('');
  let confirmReset = $state(false);

  const text = (e: unknown) => String(e instanceof Error ? e.message : e);

  onMount(async () => {
    try { dir = await getReplayDir(); }
    catch (e) { console.error(e); error = text(e); }
    finally { loading = false; }
  });

  async function change() {
    // Real build: open a native folder picker first. Until then ask with a prompt.
    const next = window.prompt('Replay folder path', dir);
    if (!next || next === dir) return;
    busy = true; msg = ''; error = '';
    try { await setReplayDir(next); dir = next; msg = 'Folder saved.'; }
    catch (e) { console.error(e); error = text(e); }
    finally { busy = false; }
  }

  // TEMPORARY test button (r6k-rxr.11): parses one known match through the real backend.
  let parsed = $state('');
  async function testParse() {
    busy = true; error = ''; parsed = '';
    try {
      const s = await parseMatch(String.raw`C:\Users\Kutbay\Desktop\R6Koc-veri\MatchReplay\Match-2026-10-31_00-54-47-11844`);
      console.log('parse_match', s);
      parsed = JSON.stringify(s, null, 2);
    } catch (e) { console.error(e); error = text(e); }
    finally { busy = false; }
  }

  async function reset() {
    busy = true; msg = ''; error = '';
    try { await resetDatabase(); msg = 'Database reset. Press Update on Home to read your replays again.'; }
    catch (e) { console.error(e); error = text(e); }
    finally { busy = false; confirmReset = false; }
  }
</script>

<h1>Settings</h1>

<div class="card">
  <button onclick={testParse} disabled={busy}>Test parse_match (temporary)</button>
  {#if parsed}<pre>{parsed}</pre>{/if}
</div>

{#if error}<div class="card state error">Something went wrong: {error}</div>{/if}
{#if msg}<div class="ok">{msg}</div>{/if}

<section class="card">
  <h3>Replay folder</h3>
  <p class="muted">R6 Coach reads your match replays from this folder.</p>
  {#if loading}
    <span class="spinner"></span>
  {:else}
    <code class="path">{dir || 'Not set'}</code>
  {/if}
  <div><button class="btn" onclick={change} disabled={busy || loading}>Change folder</button></div>
</section>

<section class="card">
  <h3>Welcome</h3>
  <p class="muted">The welcome screen is shown once, on first launch.</p>
  <button class="btn" onclick={showWelcomeAgain}>Show welcome again</button>
</section>

<section class="card">
  <h3>Database</h3>
  <p class="muted">Deletes all stored match data. Your replay files are not touched.</p>
  {#if confirmReset}
    <span>Are you sure? </span>
    <button class="btn danger" onclick={reset} disabled={busy}>Yes, reset</button>
    <button class="btn" onclick={() => (confirmReset = false)} disabled={busy}>Cancel</button>
  {:else}
    <button class="btn danger" onclick={() => (confirmReset = true)} disabled={busy}>Reset database</button>
  {/if}
</section>

<style>
  h1 { margin-bottom: 14px; }
  section { margin-bottom: 14px; }
  h3 { font-size: 15px; } p { margin: 4px 0 10px; }
  .path { display: block; background: var(--panel2); border-radius: 6px; padding: 8px 10px;
    margin-bottom: 10px; word-break: break-all; font-size: 12px; }
  .ok { color: var(--green); margin-bottom: 10px; }
</style>
