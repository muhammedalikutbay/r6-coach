<script lang="ts">
  import { RefreshCw, TriangleAlert } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import { ago } from '$lib/v2/arenyze';
  import type { OnlineProfile } from '$lib/v2/types';

  let { profile, refreshing, error, onrefresh }: {
    profile: OnlineProfile;
    refreshing: boolean;
    /** English message of the last failed refresh, if any. */
    error: string | null;
    onrefresh: () => void;
  } = $props();

  // The Ubisoft avatar URL is not loaded: the app only talks to the Arenyze API (v4 network rule).
  const initial = $derived(profile.username.trim().slice(0, 1).toUpperCase() || '?');
</script>

<Card class="mb-4">
  <div class="flex flex-wrap items-center gap-x-4 gap-y-3">
    <div
      class="flex size-14 shrink-0 items-center justify-center rounded-full border border-border-strong bg-surface-3 text-xl font-bold text-accent"
      aria-hidden="true"
    >
      {initial}
    </div>
    <div class="min-w-0">
      <h2 class="truncate text-xl font-bold">{profile.username}</h2>
      <div class="mt-1 flex flex-wrap items-center gap-2 text-xs">
        {#if profile.platform}
          <span class="rounded-md border border-border-strong bg-surface-2 px-2 py-0.5 font-medium">{profile.platform}</span>
        {/if}
        {#if profile.level !== null}
          <span class="rounded-md border border-border-strong bg-surface-2 px-2 py-0.5 font-medium tabular-nums">
            Level {profile.level.toLocaleString('en-GB')}
          </span>
        {/if}
      </div>
    </div>
    <div class="ml-auto flex items-center gap-2 text-xs text-muted">
      <span>Updated {ago(profile.fetchedAt)} · Arenyze</span>
      <button
        type="button"
        onclick={onrefresh}
        disabled={refreshing}
        title="Refresh from Arenyze"
        aria-label="Refresh from Arenyze"
        class="rounded-lg p-1.5 transition-colors hover:bg-surface-3 hover:text-foreground disabled:opacity-50"
      >
        <RefreshCw class="size-4 {refreshing ? 'animate-spin' : ''}" />
      </button>
    </div>
  </div>
  {#if error}
    <p class="mt-3 flex items-center gap-1.5 text-xs text-accent">
      <TriangleAlert class="size-3.5 shrink-0" /> {error}
    </p>
  {/if}
</Card>
