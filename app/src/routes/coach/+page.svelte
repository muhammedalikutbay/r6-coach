<script lang="ts">
  import { Brain, ThumbsUp, TriangleAlert, Info, TrendingUp, Target } from '@lucide/svelte';
  import PageHeader from '$lib/components/PageHeader.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Section from '$lib/components/Section.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import DeltaPill from '$lib/components/DeltaPill.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import CompareBars from '$lib/v2/components/charts/CompareBars.svelte';
  import { filterStore } from '$lib/v2/filter.svelte';
  import { coach } from '$lib/v2/api';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import type { CoachReport, InsightKind, Range } from '$lib/v2/types';

  // The coach compares a focus set against a reference set; the focus is chosen here, the other
  // filters (mode, side, operators, maps) come from the global filter.
  let focus = $state<Extract<Range, 'last1' | 'last5'>>('last5');
  let report = $state<CoachReport | null>(null);
  let loading = $state(true);
  let error = $state('');
  let showAll = $state(false);

  /** Cards shown before "Show all"; the list arrives ranked by importance. */
  const TOP_CARDS = 8;
  /** Rows in the comparison bars; the table lists every metric. */
  const TOP_BARS = 8;

  $effect(() => {
    const filter = { ...filterStore.value, range: focus };
    void refreshTick.value; // re-query after an import
    let stale = false;
    loading = true;
    error = '';
    coach(filter)
      .then((r) => {
        if (!stale) report = r;
      })
      .catch((e) => {
        console.error('[coach] load failed', e);
        if (!stale) error = 'Could not load the coach report.';
      })
      .finally(() => {
        if (!stale) loading = false;
      });
    return () => {
      stale = true;
    };
  });

  const kindStyle: Record<InsightKind, { icon: typeof Info; cls: string }> = {
    praise: { icon: ThumbsUp, cls: 'text-good bg-good/15' },
    improve: { icon: TriangleAlert, cls: 'text-accent bg-accent/15' },
    info: { icon: Info, cls: 'text-muted bg-surface-3' }
  };

  const visible = $derived(report ? (showAll ? report.insights : report.insights.slice(0, TOP_CARDS)) : []);
  const hidden = $derived(report ? Math.max(0, report.insights.length - TOP_CARDS) : 0);
  const fmtUnit = (n: number, unit: 'pct' | 'ratio') => (unit === 'pct' ? `${n.toFixed(1)}%` : n.toFixed(2));

  const fmtDelta = (d: number) => `${d > 0 ? '+' : ''}${Math.abs(d) >= 10 ? d.toFixed(1) : d.toFixed(2)}`;
</script>

<PageHeader title="Coach" sub="Compares the match range you pick here with the rest of your history. The other filters still apply.">
  {#snippet actions()}
    <div class="inline-flex rounded-lg border border-border bg-surface-1 p-0.5" role="group" aria-label="Focus">
      {#each [['last1', 'Last match'], ['last5', 'Last 5']] as [val, label] (val)}
        <button
          type="button"
          class="rounded-md px-3 py-1 text-xs font-medium transition-colors {focus === val
            ? 'bg-accent text-accent-foreground'
            : 'text-muted hover:text-foreground'}"
          aria-pressed={focus === val}
          onclick={() => (focus = val as typeof focus)}>{label}</button
        >
      {/each}
    </div>
  {/snippet}
</PageHeader>

{#if error}
  <EmptyState title="Coach unavailable" text={error} icon={Brain} />
{:else if !report && loading}
  <div class="flex flex-col gap-4">
    <Skeleton class="h-16" />
    <div class="grid grid-cols-3 gap-4"><Skeleton class="h-32" /><Skeleton class="h-32" /><Skeleton class="h-32" /></div>
    <Skeleton class="h-56" />
  </div>
{:else if report}
  <div class="transition-opacity {loading ? 'opacity-60' : ''}">
    <p class="mb-3 text-xs text-muted">
      <span class="font-medium text-foreground">{report.focusLabel}</span> vs
      <span class="font-medium text-foreground">{report.referenceLabel}</span>
    </p>

    <Card class="mb-6 border-accent/30 bg-accent/5">
      <p class="text-base leading-relaxed">{report.summary}</p>
    </Card>

    {#if report.insights.length}
      <Section title="Insights">
        <div class="grid grid-cols-1 gap-3 lg:grid-cols-2 xl:grid-cols-3">
          {#each visible as ins (ins.family + ins.title)}
            {@const k = kindStyle[ins.kind]}
            <Card class="flex gap-3">
              {#if ins.operatorKey}
                <OperatorBadge opKey={ins.operatorKey} size={40} />
              {:else if ins.mapKey}
                <MapThumb mapKey={ins.mapKey} class="w-20 shrink-0 self-start" />
              {:else}
                <span class="flex size-10 shrink-0 items-center justify-center rounded-full {k.cls}">
                  <k.icon class="size-5" />
                </span>
              {/if}
              <div class="min-w-0">
                <div class="flex items-center gap-1.5">
                  <h3 class="font-semibold">{ins.title}</h3>
                  {#if ins.kind !== 'info'}<span class="ml-auto shrink-0 rounded-full bg-surface-3 px-1.5 text-[10px] text-muted tabular-nums" title="Importance score (effect x confidence x weight). Higher means it matters more.">{ins.score.toFixed(2)}</span>{/if}
                </div>
                <p class="mt-1 text-muted">{ins.text}</p>
                {#if ins.evidence}<p class="mt-1.5 text-xs text-muted/80 tabular-nums">{ins.evidence}</p>{/if}
              </div>
            </Card>
          {/each}
        </div>
        {#if hidden > 0}
          <button
            type="button"
            class="mt-3 rounded-lg border border-border bg-surface-1 px-3 py-1.5 text-xs font-medium text-muted transition-colors hover:text-foreground"
            aria-expanded={showAll}
            onclick={() => (showAll = !showAll)}>{showAll ? 'Show fewer' : `Show all ${report.insights.length}`}</button
          >
        {/if}
      </Section>
    {/if}

    <div class="mb-6 grid grid-cols-1 gap-3 md:grid-cols-2">
      <Card>
        <h2 class="mb-2 flex items-center gap-1.5 text-sm font-semibold tracking-wide uppercase text-good">
          <TrendingUp class="size-4" />Strengths
        </h2>
        {#if report.strengths.length}
          <ul class="flex flex-col gap-1.5">
            {#each report.strengths as s (s)}<li class="flex gap-2"><span class="mt-2 size-1.5 shrink-0 rounded-full bg-good"></span>{s}</li>{/each}
          </ul>
        {:else}<p class="text-muted">Not enough data yet.</p>{/if}
      </Card>
      <Card>
        <h2 class="mb-2 flex items-center gap-1.5 text-sm font-semibold tracking-wide uppercase text-accent">
          <Target class="size-4" />Work on
        </h2>
        {#if report.workOn.length}
          <ul class="flex flex-col gap-1.5">
            {#each report.workOn as s (s)}<li class="flex gap-2"><span class="mt-2 size-1.5 shrink-0 rounded-full bg-accent"></span>{s}</li>{/each}
          </ul>
        {:else}<p class="text-muted">Nothing stands out. Keep going.</p>{/if}
      </Card>
    </div>

    {#if report.comparisons.length}
      <Section title="Comparison" hint="Every metric, most important first · {report.focusLabel} vs {report.referenceLabel}">
        <div class="grid grid-cols-1 gap-3 xl:grid-cols-2">
          <Card class="overflow-x-auto p-0">
            <table class="w-full text-left">
              <thead>
                <tr class="border-b border-border text-xs text-muted">
                  <th class="px-4 py-2 font-medium">Metric</th>
                  <th class="px-4 py-2 text-right font-medium">Focus</th>
                  <th class="px-4 py-2 text-right font-medium">Reference</th>
                  <th class="px-4 py-2 text-right font-medium">Change</th>
                  <th class="px-4 py-2 text-right font-medium" title="Sample size behind the focus value">n</th>
                  <th class="px-4 py-2 text-right font-medium" title="Importance: effect x confidence x weight">Score</th>
                </tr>
              </thead>
              <tbody>
                {#each report.comparisons as c, i (c.metric + i)}
                  <tr class="border-b border-border last:border-0">
                    <td class="px-4 py-2">{c.metric}</td>
                    <td class="px-4 py-2 text-right font-medium tabular-nums">{fmtUnit(c.focus, c.unit)}</td>
                    <td class="px-4 py-2 text-right text-muted tabular-nums">{fmtUnit(c.reference, c.unit)}</td>
                    <td class="px-4 py-2 text-right">
                      <DeltaPill value={c.delta} text={fmtDelta(c.delta)} lowerIsBetter={c.better === c.delta < 0} />
                    </td>
                    <td class="px-4 py-2 text-right text-muted tabular-nums">{c.n}</td>
                    <td class="px-4 py-2 text-right text-muted tabular-nums">{c.score > 0 ? c.score.toFixed(2) : '–'}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </Card>
          <Card>
            <CompareBars rows={report.comparisons.slice(0, TOP_BARS)} focusLabel={report.focusLabel} referenceLabel={report.referenceLabel} />
          </Card>
        </div>
      </Section>
    {/if}
  </div>
{:else}
  <EmptyState title="Not enough matches yet" text="Play a few matches. New ones appear here a few seconds after a match ends." icon={Brain} />
{/if}
