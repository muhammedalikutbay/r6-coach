<script lang="ts">
  // Home with no replays yet: what the coach will do, and the replay folder step.
  import { onMount } from 'svelte';
  import { Brain, Clapperboard, FolderOpen, ChartLine, Check } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import { getReplayDir, setReplayDir, pickReplayDir } from '$lib/api';
  import { toast, toastError } from '$lib/toast.svelte';

  let dir = $state('');
  let ready = $state(false);

  onMount(() => {
    getReplayDir()
      .then((d) => (dir = d ?? ''))
      .catch((e) => console.error('[home] cannot read the replay folder', e))
      .finally(() => (ready = true));
  });

  async function pick() {
    try {
      const next = await pickReplayDir(dir);
      if (!next || next === dir) return;
      await setReplayDir(next);
      dir = next;
      toast('success', 'Folder saved.');
    } catch (e) {
      toastError(e);
    }
  }

  const points = [
    { icon: ChartLine, text: 'Reads every match you play from your replay files, automatically.' },
    { icon: Brain, text: 'Compares your last matches with your own history and tells you what to fix first.' },
    { icon: Clapperboard, text: 'Keeps your best kills as clips, if you turn them on in Settings.' }
  ];
</script>

<div class="grid gap-4 lg:grid-cols-2">
  <Card>
    <h2 class="text-lg font-bold">Your coach is ready</h2>
    <p class="mt-1 text-muted">Once your first match is in, this page turns into your coaching dashboard. Here is what it will do:</p>
    <ul class="mt-3 flex flex-col gap-2.5">
      {#each points as p (p.text)}
        <li class="flex items-start gap-2.5 text-sm">
          <p.icon class="mt-0.5 size-4 flex-none text-accent" />{p.text}
        </li>
      {/each}
    </ul>
  </Card>

  <Card>
    <h2 class="flex items-center gap-2 text-lg font-bold"><FolderOpen class="size-5 text-accent" />Set up your replay folder</h2>
    <p class="mt-1 text-muted">
      In Rainbow Six Siege, turn on "Match Replay" recording, then point R6 Coach at the <code>MatchReplay</code> folder.
    </p>
    {#if ready}
      <code class="mt-3 block rounded-lg bg-surface-2 px-3 py-2 text-xs break-all">{dir || 'Not set'}</code>
    {/if}
    <div class="mt-3 flex flex-wrap items-center gap-2">
      <Button variant="primary" onclick={pick}>{dir ? 'Change folder' : 'Choose folder'}</Button>
      <a href="/settings"><Button>Open Settings</Button></a>
    </div>
    {#if dir}
      <p class="mt-3 flex items-center gap-1.5 text-xs text-muted">
        <Check class="size-4 flex-none text-good" />
        Play a match. It appears here a few seconds after it ends.
      </p>
    {/if}
  </Card>
</div>
