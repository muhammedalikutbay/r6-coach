<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import type { ReplayDirInfo } from '$lib/types';
  import { getReplayDir, setReplayDir, inspectReplayDir, pickReplayDir, resetDatabase } from '$lib/api';
  import { showWelcomeAgain } from '$lib/welcome.svelte';
  import { toast, toastError } from '$lib/toast.svelte';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import ArenyzeKey from '$lib/v2/components/ArenyzeKey.svelte';

  let dir = $state('');
  let loading = $state(true);
  let busy = $state(false);
  let info = $state<ReplayDirInfo | null>(null);
  let confirmReset = $state(false);

  async function check(path: string) {
    try {
      info = path ? await inspectReplayDir(path) : { status: 'missing' };
    } catch (e) {
      console.error(e);
      info = null;
    }
  }

  onMount(async () => {
    try {
      dir = await getReplayDir();
      await check(dir);
    } catch (e) {
      toastError(e);
    } finally {
      loading = false;
    }
    // First launch with no folder found: ask for one right away.
    if (!dir && page.url.searchParams.get('setup')) await change();
  });

  async function change() {
    let next: string | null;
    try {
      next = await pickReplayDir(dir);
    } catch (e) {
      toastError(e);
      return;
    }
    if (!next || next === dir) return;
    busy = true;
    try {
      await setReplayDir(next);
      dir = next;
      toast('success', 'Folder saved.');
      await check(next);
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }

  async function reset() {
    busy = true;
    try {
      await resetDatabase();
      toast('success', 'Database reset. Press Update to read your replays again.');
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
      confirmReset = false;
    }
  }
</script>

<PageHeader title="Settings" />

<div class="flex max-w-2xl flex-col gap-4">
  <Card>
    <h3 class="font-semibold">Replay folder</h3>
    <p class="mt-1 mb-3 text-muted">R6 Coach reads your match replays from this folder.</p>
    {#if loading}
      <Skeleton class="mb-3 h-9" />
    {:else}
      <code class="mb-3 block rounded-lg bg-surface-2 px-3 py-2 text-xs break-all">{dir || 'Not set'}</code>
    {/if}
    {#if info && info.status !== 'ready'}
      <p class="mb-3 text-accent">
        {info.status === 'missing' ? 'This folder was not found. ' : 'No matches found in this folder. '}
        In Rainbow Six Siege, open Settings and turn on "Match Replay" recording, play a match,
        then pick the <code>MatchReplay</code> folder here.
      </p>
    {:else if info}
      <p class="mb-3 text-muted">{info.matchCount} match{info.matchCount === 1 ? '' : 'es'} found.</p>
    {/if}
    <Button onclick={change} disabled={busy || loading}>Change folder</Button>
  </Card>

  <ArenyzeKey />

  <Card>
    <h3 class="font-semibold">Welcome</h3>
    <p class="mt-1 mb-3 text-muted">The welcome screen is shown once, on first launch.</p>
    <Button onclick={showWelcomeAgain}>Show welcome again</Button>
  </Card>

  <Card>
    <h3 class="font-semibold">Database</h3>
    <p class="mt-1 mb-3 text-muted">Deletes all stored match data. Your replay files are not touched.</p>
    {#if confirmReset}
      <div class="flex flex-wrap items-center gap-2">
        <span>Are you sure?</span>
        <Button variant="danger" onclick={reset} disabled={busy}>Yes, reset</Button>
        <Button onclick={() => (confirmReset = false)} disabled={busy}>Cancel</Button>
      </div>
    {:else}
      <Button variant="danger" onclick={() => (confirmReset = true)} disabled={busy}>Reset database</Button>
    {/if}
  </Card>
</div>
