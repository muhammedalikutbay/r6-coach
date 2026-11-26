<script lang="ts">
  import PageHeader from '$lib/components/PageHeader.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import ProfileStrip from '$lib/v2/components/profile/ProfileStrip.svelte';
  import CurrentSeasonCard from '$lib/v2/components/profile/CurrentSeasonCard.svelte';
  import SeasonPeaks from '$lib/v2/components/profile/SeasonPeaks.svelte';
  import SeasonList from '$lib/v2/components/profile/SeasonList.svelte';
  import SeasonOverview from '$lib/v2/components/profile/SeasonOverview.svelte';
  import OperatorsTable from '$lib/v2/components/profile/OperatorsTable.svelte';
  import LocalSummary from '$lib/v2/components/profile/LocalSummary.svelte';
  import KeyCard from '$lib/v2/components/profile/KeyCard.svelte';
  import { arenyzeStatus, clearOperatorCache, onlineProfile, seedOperators } from '$lib/v2/arenyze';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import { toast } from '$lib/toast.svelte';
  import type { ArenyzeStatus, OnlineProfile } from '$lib/v2/types';

  let status = $state<ArenyzeStatus | null>(null);
  let profile = $state<OnlineProfile | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let refreshing = $state(false);
  /** Season shown in the overview; null means the current one. */
  let pickedSeason = $state<number | null>(null);

  /** Profile from Arenyze. Without `force` it comes from the backend's 15-minute cache. */
  async function load(force: boolean) {
    try {
      status = await arenyzeStatus();
      if (!status.hasKey) {
        profile = null;
        error = null;
        return;
      }
      if (force) clearOperatorCache();
      const p = await onlineProfile(force);
      if (p?.season && p.operators.length > 0) seedOperators(p.season, 'ranked', p.operators, p.fetchedAt);
      profile = p;
      error = null;
      status = await arenyzeStatus(); // username / quota may have changed
    } catch (e) {
      console.error('[profile] online profile failed', typeof e === 'string' ? e : 'unknown error');
      // Keep the last profile on screen when a refresh fails.
      error = typeof e === 'string' ? e : 'Could not load your online stats.';
      if (force) toast('error', error);
    }
  }

  $effect(() => {
    void refreshTick.value;
    load(false).finally(() => (loading = false));
  });

  async function refresh() {
    refreshing = true;
    await load(true);
    refreshing = false;
  }

  const seasons = $derived(profile?.seasons ?? []);
  const currentId = $derived(profile?.currentSeasonId ?? seasons[0]?.seasonId ?? null);
  const shownId = $derived(pickedSeason ?? currentId);
  const shown = $derived(seasons.find((s) => s.seasonId === shownId));
</script>

<div class="mx-auto max-w-6xl">
  <PageHeader title="Profile" sub={profile ? `${profile.username} · online stats from Arenyze` : 'Your rank, seasons and a quick look at your replays'} />

  {#if loading}
    <Skeleton class="mb-4 h-20" />
    <div class="grid gap-4 lg:grid-cols-[22rem_minmax(0,1fr)]">
      <Skeleton class="h-96" />
      <Skeleton class="h-96" />
    </div>
  {:else if !status?.hasKey}
    <div class="mb-6"><KeyCard kind="no-key" /></div>
    <LocalSummary />
  {:else if !status.username}
    <div class="mb-6"><KeyCard kind="no-player" /></div>
    <LocalSummary />
  {:else if !profile}
    <div class="mb-6"><KeyCard kind="failed" {error} {refreshing} onretry={refresh} /></div>
    <LocalSummary />
  {:else}
    <ProfileStrip {profile} {refreshing} {error} onrefresh={refresh} />
    <div class="grid items-start gap-4 lg:grid-cols-[22rem_minmax(0,1fr)]">
      <div class="flex min-w-0 flex-col gap-4">
        <CurrentSeasonCard {profile} />
        {#if profile.peaks}<SeasonPeaks peaks={profile.peaks} />{/if}
        {#if seasons.length > 0}
          <SeasonList {seasons} selectedId={shownId} onselect={(id) => (pickedSeason = id === currentId ? null : id)} />
        {/if}
      </div>
      <div class="flex min-w-0 flex-col gap-4">
        {#key shownId}
          <SeasonOverview season={shown} isCurrent={shownId === currentId} />
        {/key}
        <OperatorsTable {seasons} currentSeason={profile.season} followSeason={pickedSeason === null ? null : (shown?.shortName ?? null)} />
      </div>
    </div>
  {/if}
</div>

