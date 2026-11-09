<script lang="ts">
  import Card from '$lib/components/ui/Card.svelte';
  import RankIcon from './RankIcon.svelte';
  import type { PeakEntry, SeasonPeaks } from '$lib/v2/types';

  let { peaks }: { peaks: SeasonPeaks } = $props();

  const n = (v: number) => Math.round(v).toLocaleString('en-GB');
  const where = (p: PeakEntry) => `${p.shortName}${p.name ? ` · ${p.name}` : ''}`;
</script>

{#if peaks.bestRank || peaks.bestKd}
  <Card>
    <h3 class="mb-3 text-xs font-medium tracking-wide text-muted uppercase">Season peaks</h3>
    <div class="flex flex-col gap-3">
      {#if peaks.bestRank}
        {@const p = peaks.bestRank}
        <div class="flex items-center gap-3">
          <RankIcon icon={p.rankIcon} name={p.rankName ?? p.unit} size={44} />
          <div class="min-w-0">
            <div class="text-xs text-muted">{p.unit === 'MMR' ? 'Highest rating' : 'Highest rank'}</div>
            <div class="font-semibold">
              {p.rankName ?? `${n(p.value)} ${p.unit}`}
              {#if p.rankName}<span class="ml-1 font-normal text-muted tabular-nums">{n(p.value)} {p.unit}</span>{/if}
            </div>
            <div class="flex items-center gap-1.5 truncate text-xs text-muted"><span class="size-1.5 shrink-0 rounded-full" style="background:{p.color ?? 'var(--border-strong)'}"></span>{where(p)}</div>
          </div>
        </div>
      {/if}
      {#if peaks.bestKd}
        {@const p = peaks.bestKd}
        <div class="flex items-center gap-3">
          <div class="flex size-11 shrink-0 items-center justify-center rounded-xl bg-surface-2 text-sm font-bold text-accent tabular-nums">
            {p.value.toFixed(2)}
          </div>
          <div class="min-w-0">
            <div class="text-xs text-muted">Best K/D</div>
            <div class="flex items-center gap-1.5 truncate text-xs text-muted"><span class="size-1.5 shrink-0 rounded-full" style="background:{p.color ?? 'var(--border-strong)'}"></span>{where(p)}</div>
            {#if p.matches}<div class="text-xs text-muted tabular-nums">{n(p.matches)} ranked matches</div>{/if}
          </div>
        </div>
      {/if}
    </div>
  </Card>
{/if}
