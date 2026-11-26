<script lang="ts">
  // Form: the last 5 and last 20 matches against the player's own all-time numbers.
  import Card from '$lib/components/ui/Card.svelte';
  import DeltaPill from '$lib/components/DeltaPill.svelte';
  import { FORM_METRICS, VERDICT_TEXT, fmtDelta, fmtMetric, verdict, type Verdict } from '$lib/v2/home';
  import type { QueryResult } from '$lib/v2/types';

  let { all, l5, l20 }: { all: QueryResult; l5: QueryResult; l20: QueryResult } = $props();

  const tone: Record<Verdict, string> = { better: 'text-good', same: 'text-muted', worse: 'text-bad' };

  const rows = $derived(
    FORM_METRICS.map((m) => {
      const usual = m.read(all);
      const cell = (q: QueryResult) => {
        const v = m.read(q);
        return v === null || usual === null ? null : { v, d: v - usual, verdict: verdict(v, usual, m.noise) };
      };
      return { m, usual, five: cell(l5), twenty: cell(l20) };
    })
  );
  const thin = $derived(all.matchCount <= l5.matchCount);
</script>

<Card class="overflow-x-auto">
  <div class="grid min-w-[34rem] grid-cols-[minmax(0,1.5fr)_repeat(3,minmax(0,1fr))_minmax(0,1.3fr)] items-center gap-x-3 gap-y-2.5 text-sm">
    <div></div>
    <div class="text-right text-xs font-medium text-muted">Last {l5.matchCount}</div>
    <div class="text-right text-xs font-medium text-muted">Last {l20.matchCount}</div>
    <div class="text-right text-xs font-medium text-muted">All time ({all.matchCount})</div>
    <div class="text-xs font-medium text-muted">Last {l5.matchCount} vs usual</div>
    {#each rows as r (r.m.key)}
      <div class="font-medium">{r.m.label}</div>
      {#each [r.five, r.twenty] as c, i (i)}
        <div class="flex flex-col items-end gap-0.5">
          {#if c}
            <span class="font-semibold tabular-nums">{fmtMetric(c.v, r.m.unit)}</span>
            <DeltaPill value={c.verdict === 'same' ? 0 : c.d} text={fmtDelta(c.d, r.m.unit)} />
          {:else}<span class="text-muted">–</span>{/if}
        </div>
      {/each}
      <div class="text-right text-muted tabular-nums">{r.usual === null ? '–' : fmtMetric(r.usual, r.m.unit)}</div>
      <div class="text-xs font-medium {r.five ? tone[r.five.verdict] : 'text-muted'}">
        {r.five ? VERDICT_TEXT[r.five.verdict] : 'Not enough data'}
      </div>
    {/each}
  </div>
  {#if thin}
    <p class="mt-3 text-xs text-muted">Your history is still short, so every column looks alike. The comparison gets useful after a few more matches.</p>
  {/if}
</Card>
