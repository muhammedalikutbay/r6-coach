<script lang="ts">
  // Home: the coach greets the player. Local replay data only; the optional rank chip is the one thing
  // that uses the Arenyze key, and nothing here waits for it.
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { Inbox } from '@lucide/svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Section from '$lib/components/Section.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import MatchRowItem from '$lib/v2/components/MatchRowItem.svelte';
  import LatestMatchCard from '$lib/v2/components/home/LatestMatchCard.svelte';
  import CoachSays from '$lib/v2/components/home/CoachSays.svelte';
  import FocusCard from '$lib/v2/components/home/FocusCard.svelte';
  import FormCard from '$lib/v2/components/home/FormCard.svelte';
  import HomeClips, { type HomeClip } from '$lib/v2/components/home/HomeClips.svelte';
  import RankChip from '$lib/v2/components/home/RankChip.svelte';
  import HomeEmpty from '$lib/v2/components/home/HomeEmpty.svelte';
  import { coach, matchDetail, query } from '$lib/v2/api';
  import { arenyzeStatus, onlineProfile } from '$lib/v2/arenyze';
  import { listClips, onClipsUpdated } from '$lib/v2/clips';
  import { decidedLine, greeting } from '$lib/v2/home';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import { DEFAULT_FILTER, type CoachReport, type Filter, type OnlineProfile, type QueryResult } from '$lib/v2/types';

  interface Sets {
    all: QueryResult;
    l5: QueryResult;
    l20: QueryResult;
  }

  let sets = $state<Sets | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let report = $state<CoachReport | null>(null);
  let line = $state<string | null>(null);
  let name = $state<string | null>(null);
  let clips = $state<HomeClip[]>([]);
  let rank = $state<OnlineProfile | null>(null);
  let seq = 0;

  /** Home ignores the global filter bar: it always looks at every mode and side. */
  const base = (range: Filter['range']): Filter => ({ ...DEFAULT_FILTER, operators: [], maps: [], range });

  const latest = $derived(sets?.all.recentMatches[0] ?? null);
  const recent = $derived((sets?.all.recentMatches ?? []).slice(0, 5));
  const points = $derived((report?.insights ?? []).filter((i) => i.kind !== 'info').slice(0, 3));

  async function loadClips(all: QueryResult, mine: number) {
    try {
      const matches = all.recentMatches.slice(0, 4);
      const lists = await Promise.all(matches.map((m) => listClips(m.matchId)));
      if (mine !== seq) return;
      clips = lists
        .flatMap((rows, i) => rows.map((clip) => ({ clip, mapKey: matches[i].mapKey, mapName: matches[i].mapName })))
        .sort((a, b) => b.clip.createdMs - a.clip.createdMs)
        .slice(0, 6);
    } catch (e) {
      console.error('[home] clips failed', e);
    }
  }

  async function load() {
    const mine = ++seq;
    try {
      const [all, l5, l20] = await Promise.all([query(base('all')), query(base('last5')), query(base('last20'))]);
      if (mine !== seq) return;
      sets = { all, l5, l20 };
      failed = false;
      loading = false;
      if (all.matchCount === 0) return;

      // Everything below fills in as it arrives; none of it blocks the page.
      coach(base('last5'))
        .then((r) => mine === seq && (report = r))
        .catch((e) => console.error('[home] coach failed', e));
      const first = all.recentMatches[0];
      if (first) {
        matchDetail(first.matchId)
          .then((d) => {
            if (mine !== seq) return;
            line = decidedLine(d);
            name = d.teams[0].find((p) => p.isMe)?.username ?? null;
          })
          .catch((e) => {
            console.error('[home] match detail failed', e);
            if (mine === seq) line = first.won ? 'A win. Open the match for the full picture.' : 'A loss. Open the match for the full picture.';
          });
      }
      void loadClips(all, mine);
      arenyzeStatus()
        .then((s) => (s.hasKey ? onlineProfile(false) : null))
        .then((p) => mine === seq && (rank = p))
        .catch((e) => console.error('[home] rank chip failed', typeof e === 'string' ? e : 'unknown error'));
    } catch (e) {
      if (mine !== seq) return;
      console.error('[home] load failed', e);
      failed = !sets;
      loading = false;
    }
  }

  $effect(() => {
    void refreshTick.value; // re-query after an automatic import
    line = null;
    void load();
  });

  // New clips are cut a little after a round ends; refresh the strip when that happens.
  onMount(() => {
    let stop: (() => void) | undefined;
    let closed = false;
    onClipsUpdated(() => sets && void loadClips(sets.all, seq))
      .then((u) => (closed ? u() : (stop = u)))
      .catch((e) => console.error('[home] cannot listen for clips', e));
    return () => {
      closed = true;
      stop?.();
    };
  });

  const hello = $derived(`${greeting(new Date().getHours())}${name ? `, ${name}` : ''}`);
</script>

<div class="mx-auto max-w-6xl">
  <div class="mb-5 flex flex-wrap items-center justify-between gap-3">
    <div class="min-w-0">
      <h1 class="truncate text-xl font-bold">{hello}</h1>
      <p class="text-muted">
        {#if sets && sets.all.matchCount > 0}Here is how your last match went and what to work on next.{:else}Your coach reads your replays and tells you what to work on.{/if}
      </p>
    </div>
    {#if rank}<RankChip profile={rank} />{/if}
  </div>

  {#if loading}
    <div class="grid gap-4 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
      <Skeleton class="h-48" />
      <Skeleton class="h-48" />
    </div>
    <Skeleton class="mt-4 h-40" />
  {:else if failed || !sets}
    <EmptyState title="Could not load your replays" text="Open Settings to check the replay folder." icon={Inbox}>
      <a href="/settings" class="text-sm font-medium text-accent hover:underline">Open Settings</a>
    </EmptyState>
  {:else if sets.all.matchCount === 0 || !latest}
    <HomeEmpty />
  {:else}
    <div class="mb-6 grid items-stretch gap-4 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
      <LatestMatchCard match={latest} {line} />
      <FocusCard {report} />
    </div>

    <Section title="Your coach says" hint="The most important points from your last 5 matches, from the Coach page">
      <CoachSays insights={points} />
    </Section>

    <Section title="Your form" hint="Your last matches against your own all-time numbers">
      <FormCard all={sets.all} l5={sets.l5} l20={sets.l20} />
    </Section>

    {#if clips.length > 0}
      <Section title="Latest clips" hint="From your newest matches">
        <HomeClips {clips} />
      </Section>
    {/if}

    <Section title="Recent matches">
      {#snippet actions()}
        <a href="/matches" class="text-xs font-medium text-accent hover:underline">All matches</a>
      {/snippet}
      <div class="flex flex-col gap-2">
        {#each recent as m (m.matchId)}
          <MatchRowItem match={m} showDate onclick={() => goto(`/matches?match=${encodeURIComponent(m.matchId)}`)} />
        {/each}
      </div>
    </Section>
  {/if}
</div>
