<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte';
  import LineChart from '$lib/v2/components/charts/LineChart.svelte';
  import RankIcon from './RankIcon.svelte';
  import { formatShortDate } from '$lib/v2/format';
  import type { OnlineProfile } from '$lib/v2/types';

  let { profile }: { profile: OnlineProfile } = $props();

  const n = (v: number | null | undefined) => (v === null || v === undefined ? '—' : v.toLocaleString('en-GB'));
  const winPct = $derived(
    profile.wins !== null && profile.matches ? (profile.wins / profile.matches) * 100 : null
  );
  const chart = $derived(
    profile.rankHistory.map((p) => ({
      label: `${p.label ? formatShortDate(p.label) : ''}${p.rankName ? ` · ${p.rankName}` : ''}`.replace(/^ · /, ''),
      value: p.rankPoints
    }))
  );
  const delta = $derived(chart.length > 1 ? chart[chart.length - 1].value - chart[0].value : null);
</script>

<Card>
  <div class="mb-3 flex items-center justify-between gap-2">
    <h3 class="text-xs font-medium tracking-wide text-muted uppercase">Current season</h3>
    {#if profile.season}
      <span
        class="inline-flex items-center gap-1.5 rounded-md border border-border-strong px-2 py-0.5 text-xs font-medium text-muted"
        title={profile.seasonName ?? ''}
      >
        <span class="size-1.5 rounded-full" style="background:{profile.seasonColor ?? 'var(--border-strong)'}"></span>{profile.season}{profile.seasonName ? ` · ${profile.seasonName}` : ''}
      </span>
    {/if}
  </div>

  <div class="flex items-center gap-4">
    <RankIcon icon={profile.rankIcon} name={profile.rankName ?? 'Rank'} size={72} />
    <div class="min-w-0">
      <div class="text-xl font-bold">{profile.rankName ?? 'No ranked data'}</div>
      {#if profile.rankPoints !== null}
        <div class="text-sm tabular-nums">
          {n(profile.rankPoints)} RP
          {#if profile.topRankPosition !== null}
            <span class="ml-2 text-accent">Champion #{n(profile.topRankPosition)}</span>
          {/if}
        </div>
      {/if}
      {#if delta !== null}
        <div class="text-xs tabular-nums {delta >= 0 ? 'text-good' : 'text-bad'}">
          {delta >= 0 ? '+' : ''}{n(delta)} RP over {chart.length} history points
        </div>
      {/if}
    </div>
  </div>

  {#if chart.length > 1}
    <div class="mt-3" title="Rank points history">
      <LineChart points={chart} height={84} format={(v) => Math.round(v).toLocaleString('en-GB')} />
    </div>
  {/if}

  <div class="mt-4 grid grid-cols-2 gap-3">
    <div class="rounded-lg bg-surface-2 px-3 py-2">
      <div class="text-xs text-muted">K/D</div>
      <div class="text-lg font-bold tabular-nums">{profile.seasonKd !== null ? profile.seasonKd.toFixed(2) : '—'}</div>
    </div>
    <div class="rounded-lg bg-surface-2 px-3 py-2">
      <div class="text-xs text-muted">Win rate</div>
      <div class="text-lg font-bold tabular-nums">{winPct !== null ? `${winPct.toFixed(1)}%` : '—'}</div>
    </div>
  </div>

  <table class="mt-3 w-full text-sm tabular-nums">
    <tbody>
      <tr class="border-b border-border"><td class="py-1.5 text-muted">Wins</td><td class="py-1.5 text-right font-medium text-good">{n(profile.wins)}</td></tr>
      <tr class="border-b border-border"><td class="py-1.5 text-muted">Losses</td><td class="py-1.5 text-right font-medium text-bad">{n(profile.losses)}</td></tr>
      <tr class="border-b border-border"><td class="py-1.5 text-muted">Matches</td><td class="py-1.5 text-right font-medium">{n(profile.matches)}</td></tr>
      <tr class={profile.abandons ? 'border-b border-border' : ''}>
        <td class="py-1.5 text-muted">Kills / Deaths</td>
        <td class="py-1.5 text-right font-medium">{n(profile.kills)} / {n(profile.deaths)}</td>
      </tr>
      {#if profile.abandons}
        <tr><td class="py-1.5 text-muted">Abandons</td><td class="py-1.5 text-right font-medium">{n(profile.abandons)}</td></tr>
      {/if}
    </tbody>
  </table>
</Card>
