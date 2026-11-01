<script lang="ts">
  import '../app.css';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';
  import { LayoutDashboard, Swords, Brain, Settings, RefreshCw } from '@lucide/svelte';
  import { getReplayDir, refresh } from '$lib/api';
  import { toast, toastError } from '$lib/toast.svelte';
  import { cn } from '$lib/utils';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import WelcomeScreen from '$lib/WelcomeScreen.svelte';
  import Toaster from '$lib/components/Toaster.svelte';
  import FilterBar from '$lib/v2/components/FilterBar.svelte';
  import Button from '$lib/components/ui/Button.svelte';

  let { children } = $props();
  let updating = $state(false);

  // First launch: when no replay folder is known (not saved, not auto-found), go to Settings to pick one.
  onMount(async () => {
    try {
      if (!(await getReplayDir())) await goto('/settings?setup=1');
    } catch (e) {
      console.error('first-run folder check failed', e);
    }
  });

  const tabs = [
    { href: '/overview', label: 'Overview', icon: LayoutDashboard },
    { href: '/matches', label: 'Matches', icon: Swords },
    { href: '/coach', label: 'Coach', icon: Brain },
    { href: '/settings', label: 'Settings', icon: Settings }
  ];

  async function update() {
    updating = true;
    try {
      const r = await refresh();
      if (r.newMatches > 0 || r.failed.length === 0) refreshTick.bump();
      if (r.failed.length > 0) toast('error', `${r.failed.length} folder(s) could not be read: ${r.failed[0].error}`);
      else if (r.newMatches > 0) toast('success', `${r.newMatches} new ${r.newMatches === 1 ? 'match' : 'matches'} added.`);
      else toast('info', 'No new matches found. Play a game, then press Update.');
    } catch (e) {
      toastError(e);
    } finally {
      updating = false;
    }
  }
</script>

<div class="flex h-screen min-w-0">
  <nav class="flex w-[76px] flex-none flex-col items-stretch gap-1 border-r border-border bg-surface-1 px-2 py-3">
    <div class="mb-3 text-center text-[11px] font-extrabold tracking-wider text-accent">R6 COACH</div>
    {#each tabs as t (t.href)}
      {@const active = page.url.pathname === t.href}
      <a
        href={t.href}
        aria-current={active ? 'page' : undefined}
        class={cn(
          'flex flex-col items-center gap-1 rounded-xl px-1 py-2 text-[11px] font-medium transition',
          active ? 'bg-surface-3 text-accent' : 'text-muted hover:bg-surface-2 hover:text-foreground'
        )}
      >
        <t.icon class="size-5" />
        {t.label}
      </a>
    {/each}
  </nav>

  <div class="flex min-w-0 flex-1 flex-col">
    <header class="flex flex-none items-center justify-between gap-3 border-b border-border bg-surface-1/60 px-5 py-2.5">
      <div class="min-w-0 flex-1"><FilterBar /></div>
      <Button variant="primary" onclick={update} disabled={updating}>
        <RefreshCw class={cn('size-4', updating && 'animate-spin')} />
        {updating ? 'Updating…' : 'Update'}
      </Button>
    </header>
    <main class="min-w-0 flex-1 overflow-y-auto px-5 py-5">
      {@render children()}
    </main>
  </div>
</div>
<WelcomeScreen />
<Toaster />
