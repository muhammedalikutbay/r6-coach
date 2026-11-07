<script lang="ts">
  import { RefreshCw, ShieldQuestion, TriangleAlert } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import type { RankedProfile, UbiStatus } from '../types';

  let { profile, status, loading, refreshing, onrefresh }: {
    profile: RankedProfile | null;
    status: UbiStatus | null;
    loading: boolean;
    refreshing: boolean;
    onrefresh: () => void;
  } = $props();

  let iconFailed = $state(false);
  $effect(() => {
    void profile?.rankIcon;
    iconFailed = false;
  });

  /** "just now", "3 min ago", "2 h ago", "4 d ago" */
  function ago(iso: string): string {
    const t = new Date(iso).getTime();
    if (Number.isNaN(t)) return '';
    const min = Math.max(0, Math.round((Date.now() - t) / 60_000));
    if (min < 1) return 'just now';
    if (min < 60) return `${min} min ago`;
    if (min < 1440) return `${Math.round(min / 60)} h ago`;
    return `${Math.round(min / 1440)} d ago`;
  }

  const seasonKd = $derived(profile ? (profile.deaths > 0 ? profile.kills / profile.deaths : profile.kills) : 0);
  const games = $derived(profile ? profile.wins + profile.losses : 0);
  const seasonWin = $derived(games > 0 && profile ? (profile.wins / games) * 100 : 0);
</script>

{#if loading}
  <Skeleton class="mb-6 h-24" />
{:else if !profile || !status?.signedIn}
  <a href="/settings" class="mb-6 block">
    <Card class="flex items-center gap-3 transition-colors hover:bg-surface-2">
      <ShieldQuestion class="size-8 shrink-0 text-muted" />
      <div class="min-w-0">
        <div class="font-semibold">Sign in with Ubisoft to see your rank</div>
        <div class="text-xs text-muted">Your rank, RP and official season stats appear here. Open Settings to sign in.</div>
      </div>
    </Card>
  </a>
{:else}
  <Card class="mb-6">
    <div class="flex flex-wrap items-center gap-x-6 gap-y-3">
      {#if profile.rankIcon && !iconFailed}
        <img
          src="/assets/ranks/{profile.rankIcon}.png"
          alt={profile.rankName}
          class="size-16 shrink-0 object-contain"
          onerror={() => (iconFailed = true)}
        />
      {:else}
        <div
          class="flex size-16 shrink-0 items-center justify-center rounded-xl border border-border-strong bg-surface-2 text-xl font-bold text-muted"
          aria-hidden="true"
        >
          {profile.rankName.slice(0, 1)}
        </div>
      {/if}

      <div class="min-w-0">
        <div class="text-xs font-medium tracking-wide text-muted uppercase">Rank · {profile.season}</div>
        <div class="text-xl font-bold">{profile.rankName}</div>
        <div class="text-sm tabular-nums">
          {profile.rankPoints.toLocaleString('en-GB')} RP
          {#if profile.topRankPosition !== null}
            <span class="ml-2 text-accent">Champion #{profile.topRankPosition.toLocaleString('en-GB')}</span>
          {/if}
        </div>
      </div>

      <dl class="flex flex-wrap gap-x-6 gap-y-2 text-sm">
        <div>
          <dt class="text-xs text-muted">Max rank</dt>
          <dd class="font-semibold tabular-nums">
            {profile.maxRankName} <span class="font-normal text-muted">{profile.maxRankPoints.toLocaleString('en-GB')} RP</span>
          </dd>
        </div>
        <div>
          <dt class="text-xs text-muted">Season K/D</dt>
          <dd class="font-semibold tabular-nums">{seasonKd.toFixed(2)}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted">Season win %</dt>
          <dd class="font-semibold tabular-nums">{seasonWin.toFixed(1)}%</dd>
        </div>
      </dl>

      <div class="ml-auto flex items-center gap-2 text-xs text-muted">
        <span>updated {ago(profile.fetchedAt)}</span>
        <button
          type="button"
          onclick={onrefresh}
          disabled={refreshing}
          title="Refresh rank"
          aria-label="Refresh rank"
          class="rounded-lg p-1.5 transition-colors hover:bg-surface-3 hover:text-foreground disabled:opacity-50"
        >
          <RefreshCw class="size-4 {refreshing ? 'animate-spin' : ''}" />
        </button>
      </div>
    </div>

    {#if status.matchesReplayPlayer === false}
      <p class="mt-3 flex items-center gap-1.5 text-xs text-accent">
        <TriangleAlert class="size-3.5" /> Signed-in account differs from the replay player
      </p>
    {/if}
  </Card>
{/if}
