<script lang="ts">
  import { RefreshCw, ShieldQuestion, TriangleAlert } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import type { ArenyzeStatus, OnlineProfile } from '../types';

  let { profile, status, loading, refreshing, error, onrefresh }: {
    profile: OnlineProfile | null;
    status: ArenyzeStatus | null;
    loading: boolean;
    refreshing: boolean;
    /** English message of the last failed online request, if any. */
    error: string | null;
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

  const n = (v: number) => v.toLocaleString('en-GB');
  const games = $derived(profile && profile.wins !== null && profile.losses !== null ? profile.wins + profile.losses : 0);
  const seasonWin = $derived(
    profile && profile.wins !== null && games > 0 ? (profile.wins / (profile.matches ?? games)) * 100 : null
  );

  // Rank-points sparkline (needs two points).
  const W = 120;
  const H = 36;
  const spark = $derived.by(() => {
    const pts = profile?.rankHistory ?? [];
    if (pts.length < 2) return null;
    const vals = pts.map((p) => p.rankPoints);
    const lo = Math.min(...vals);
    const hi = Math.max(...vals);
    const span = hi - lo || 1;
    const line = vals
      .map((v, i) => `${((i / (vals.length - 1)) * (W - 4) + 2).toFixed(1)},${(H - 3 - ((v - lo) / span) * (H - 6)).toFixed(1)}`)
      .join(' ');
    return { line, delta: vals[vals.length - 1] - vals[0], first: pts[0].label, last: pts[pts.length - 1].label };
  });
</script>

{#if loading}
  <Skeleton class="mb-6 h-24" />
{:else if !status?.hasKey}
  <a href="/settings" class="mb-6 block">
    <Card class="flex items-center gap-3 transition-colors hover:bg-surface-2">
      <ShieldQuestion class="size-8 shrink-0 text-muted" />
      <div class="min-w-0">
        <div class="font-semibold">Add a free Arenyze key to see your rank</div>
        <div class="text-xs text-muted">
          Your rank, RP and official season stats appear here. Everything else works without it. Open Settings to add a key.
        </div>
      </div>
    </Card>
  </a>
{:else if !status.username}
  <Card class="mb-6 flex items-center gap-3">
    <ShieldQuestion class="size-8 shrink-0 text-muted" />
    <div class="min-w-0">
      <div class="font-semibold">Import a match to see your rank</div>
      <div class="text-xs text-muted">The app reads your Ubisoft name from your newest replay.</div>
    </div>
  </Card>
{:else if !profile}
  <Card class="mb-6 flex items-center gap-3">
    <TriangleAlert class="size-8 shrink-0 text-accent" />
    <div class="min-w-0 flex-1">
      <div class="font-semibold">Could not load your rank</div>
      <div class="text-xs text-muted">{error ?? 'No answer from Arenyze yet.'}</div>
    </div>
    <button
      type="button"
      onclick={onrefresh}
      disabled={refreshing}
      title="Try again"
      aria-label="Try again"
      class="rounded-lg p-1.5 text-muted transition-colors hover:bg-surface-3 hover:text-foreground disabled:opacity-50"
    >
      <RefreshCw class="size-4 {refreshing ? 'animate-spin' : ''}" />
    </button>
  </Card>
{:else}
  <Card class="mb-6">
    <div class="flex flex-wrap items-center gap-x-6 gap-y-3">
      {#if profile.rankIcon && !iconFailed}
        <img
          src="/assets/ranks/{profile.rankIcon}.png"
          alt={profile.rankName ?? 'Rank'}
          class="size-16 shrink-0 object-contain"
          onerror={() => (iconFailed = true)}
        />
      {:else}
        <div
          class="flex size-16 shrink-0 items-center justify-center rounded-xl border border-border-strong bg-surface-2 text-xl font-bold text-muted"
          aria-hidden="true"
        >
          {(profile.rankName ?? '?').slice(0, 1)}
        </div>
      {/if}

      <div class="min-w-0">
        <div class="text-xs font-medium tracking-wide text-muted uppercase">Rank{profile.season ? ` · ${profile.season}` : ''}</div>
        <div class="text-xl font-bold">{profile.rankName ?? 'No ranked data'}</div>
        {#if profile.rankPoints !== null}
          <div class="text-sm tabular-nums">
            {n(profile.rankPoints)} RP
            {#if profile.topRankPosition !== null}
              <span class="ml-2 text-accent">Champion #{n(profile.topRankPosition)}</span>
            {/if}
          </div>
        {/if}
      </div>

      <dl class="flex flex-wrap gap-x-6 gap-y-2 text-sm">
        {#if profile.maxRankName}
          <div>
            <dt class="text-xs text-muted">Max rank</dt>
            <dd class="font-semibold tabular-nums">
              {profile.maxRankName}
              {#if profile.maxRankPoints !== null}<span class="font-normal text-muted">{n(profile.maxRankPoints)} RP</span>{/if}
            </dd>
          </div>
        {/if}
        {#if profile.seasonKd !== null}
          <div>
            <dt class="text-xs text-muted">Season K/D</dt>
            <dd class="font-semibold tabular-nums">{profile.seasonKd.toFixed(2)}</dd>
          </div>
        {/if}
        {#if seasonWin !== null}
          <div>
            <dt class="text-xs text-muted">Season win %</dt>
            <dd class="font-semibold tabular-nums">{seasonWin.toFixed(1)}%</dd>
          </div>
        {/if}
        {#if profile.matches !== null}
          <div>
            <dt class="text-xs text-muted">Season matches</dt>
            <dd class="font-semibold tabular-nums">
              {n(profile.matches)}
              {#if profile.wins !== null && profile.losses !== null}
                <span class="font-normal text-muted">{profile.wins}W {profile.losses}L</span>
              {/if}
            </dd>
          </div>
        {/if}
        {#if profile.level !== null}
          <div>
            <dt class="text-xs text-muted">Level</dt>
            <dd class="font-semibold tabular-nums">{n(profile.level)}</dd>
          </div>
        {/if}
      </dl>

      {#if spark}
        <div title="Rank points history{spark.first && spark.last ? ` (${spark.first} to ${spark.last})` : ''}">
          <svg viewBox="0 0 {W} {H}" width={W} height={H} class="text-accent" role="img" aria-label="Rank points history">
            <polyline points={spark.line} fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" />
          </svg>
          <div class="text-center text-xs tabular-nums {spark.delta >= 0 ? 'text-good' : 'text-bad'}">
            {spark.delta >= 0 ? '+' : ''}{n(spark.delta)} RP
          </div>
        </div>
      {/if}

      <div class="ml-auto flex items-center gap-2 text-xs text-muted">
        <span>Arenyze · updated {ago(profile.fetchedAt)}</span>
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

    {#if error}
      <p class="mt-3 flex items-center gap-1.5 text-xs text-accent">
        <TriangleAlert class="size-3.5" /> {error}
      </p>
    {/if}
  </Card>
{/if}
