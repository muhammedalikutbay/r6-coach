<script lang="ts">
  import '../app.css';
  import { page } from '$app/state';
  import { goto, afterNavigate } from '$app/navigation';
  import { onMount } from 'svelte';
  import { House, LayoutDashboard, Swords, Users, Crosshair, Brain, UserRound, Settings } from '@lucide/svelte';
  import { getReplayDir } from '$lib/api';
  import { cn } from '$lib/utils';
  import { onReplaysUpdated, refreshTick } from '$lib/v2/refresh.svelte';
  import { watchClipWarnings } from '$lib/v2/clips';
  import WelcomeScreen from '$lib/v2/components/WelcomeScreen.svelte';
  import TourOverlay from '$lib/v2/components/tour/TourOverlay.svelte';
  import TitleBar from '$lib/v2/components/TitleBar.svelte';
  import Toaster from '$lib/components/Toaster.svelte';
  import FilterBar from '$lib/v2/components/FilterBar.svelte';

  let { children } = $props();
  let mainEl: HTMLElement | undefined = $state();

  // The scrolling element is <main>, so SvelteKit's own scroll reset does not reach it.
  afterNavigate((nav) => {
    if (nav.type !== 'popstate') mainEl?.scrollTo(0, 0);
  });

  // First launch: when no replay folder is known (not saved, not auto-found), go to Settings to pick one.
  onMount(async () => {
    try {
      if (!(await getReplayDir())) await goto('/settings?setup=1');
    } catch (e) {
      console.error('first-run folder check failed', e);
    }
  });

  // Clip recorder warnings (e.g. exclusive fullscreen gives a black capture), shown once as a toast.
  onMount(() => {
    let stop: (() => void) | undefined;
    watchClipWarnings().then((f) => (stop = f));
    return () => stop?.();
  });

  // Auto refresh: the backend imports new replays by itself and sends `replays-updated`.
  // Imports that changed something bump the tick, so every open view re-queries.
  onMount(() => {
    let unlisten: (() => void) | undefined;
    let closed = false;
    onReplaysUpdated((r) => {
      console.info('replays-updated', r);
      if (r.newMatches + r.updatedMatches > 0) refreshTick.bump();
    })
      .then((u) => (closed ? u() : (unlisten = u)))
      .catch((e) => console.error('cannot listen for replay updates', e));
    return () => {
      closed = true;
      unlisten?.();
    };
  });

  // `id` is what the first-run tour points at (data-tour on the link).
  const tabs = [
    { id: 'home', href: '/home', label: 'Home', icon: House },
    { id: 'overview', href: '/overview', label: 'Overview', icon: LayoutDashboard },
    { id: 'matches', href: '/matches', label: 'Matches', icon: Swords },
    { id: 'operators', href: '/operators', label: 'Operators', icon: Users },
    { id: 'weapons', href: '/weapons', label: 'Weapons', icon: Crosshair },
    { id: 'coach', href: '/coach', label: 'Coach', icon: Brain },
    { id: 'profile', href: '/profile', label: 'Profile', icon: UserRound },
    { id: 'settings', href: '/settings', label: 'Settings', icon: Settings }
  ];

  // The global filter does not apply to Home, Profile (online profile) or Settings.
  const showFilter = $derived(!['/home', '/profile', '/settings'].some((p) => page.url.pathname === p || page.url.pathname.startsWith(p + '/')));
</script>

<div class="flex h-screen min-w-0 flex-col">
<TitleBar />
<div class="flex min-h-0 min-w-0 flex-1">
  <nav aria-label="Main" class="flex w-[76px] flex-none flex-col items-stretch gap-1 overflow-y-auto border-r border-border bg-surface-1 px-2 py-2">
    {#each tabs as t (t.href)}
      {@const active = page.url.pathname === t.href || page.url.pathname.startsWith(t.href + '/')}
      <a
        href={t.href}
        data-tour={t.id}
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
    {#if showFilter}
      <header class="flex flex-none items-center justify-between gap-3 border-b border-border bg-surface-1/60 px-5 py-2.5">
        <div class="min-w-0 flex-1">
          <FilterBar hideRange={page.url.pathname.startsWith('/coach')} />
        </div>
      </header>
    {/if}
    <main bind:this={mainEl} class="min-w-0 flex-1 overflow-y-auto px-5 py-5">
      {@render children()}
    </main>
  </div>
</div>
</div>
<WelcomeScreen />
<TourOverlay />
<Toaster />
