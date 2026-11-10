<script lang="ts">
  // Small hand-written SVG line chart. Stroke uses non-scaling-stroke so the stretched viewBox stays crisp.
  import type { ChartPoint } from './chart';
  let { points, reference, height = 160, format = (v: number) => v.toFixed(2) }: {
    points: ChartPoint[];
    /** Optional horizontal dashed reference value (for example 1.0 K/D). */
    reference?: number;
    height?: number;
    format?: (v: number) => string;
  } = $props();

  const W = 600;
  const H = 100;
  const PAD = 6;
  let hover = $state<number | null>(null);

  const bounds = $derived.by(() => {
    const vals = points.map((p) => p.value);
    if (reference !== undefined) vals.push(reference);
    let lo = Math.min(...vals);
    let hi = Math.max(...vals);
    if (!isFinite(lo) || !isFinite(hi)) return { lo: 0, hi: 1 };
    if (hi - lo < 0.2) {
      lo -= 0.1;
      hi += 0.1;
    }
    return { lo, hi };
  });
  const xAt = (i: number) => (points.length <= 1 ? W / 2 : PAD + (i * (W - 2 * PAD)) / (points.length - 1));
  const yAt = (v: number) => H - PAD - ((v - bounds.lo) / (bounds.hi - bounds.lo)) * (H - 2 * PAD);
  const path = $derived(points.map((p, i) => `${i ? 'L' : 'M'}${xAt(i).toFixed(1)},${yAt(p.value).toFixed(1)}`).join(' '));
  const area = $derived(points.length ? `${path} L${xAt(points.length - 1).toFixed(1)},${H} L${xAt(0).toFixed(1)},${H} Z` : '');

  function onMove(e: PointerEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    if (!points.length || r.width === 0) return;
    const frac = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    hover = Math.round(frac * (points.length - 1));
  }
  const hp = $derived(hover !== null ? points[hover] : null);
</script>

<div class="relative pl-11" style="height:{height}px">
  <div class="relative size-full">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="size-full" onpointermove={onMove} onpointerleave={() => (hover = null)}>
    <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" class="size-full overflow-visible" role="img" aria-label="Trend chart">
      {#if reference !== undefined}
        <line x1="0" x2={W} y1={yAt(reference)} y2={yAt(reference)} stroke="var(--border-strong)" stroke-dasharray="4 4" vector-effect="non-scaling-stroke" />
      {/if}
      {#if points.length}
        <path d={area} fill="var(--accent)" opacity="0.1" />
        <path d={path} fill="none" stroke="var(--accent)" stroke-width="2" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
      {/if}
      {#if hover !== null}
        <line x1={xAt(hover)} x2={xAt(hover)} y1="0" y2={H} stroke="var(--border-strong)" vector-effect="non-scaling-stroke" />
      {/if}
    </svg>
  </div>
  {#if hp && hover !== null}
    <div
      class="pointer-events-none absolute top-0 -translate-x-1/2 rounded-md border border-border-strong bg-surface-2 px-2 py-1 text-xs whitespace-nowrap shadow"
      style="left:{points.length <= 1 ? 50 : (xAt(hover) / W) * 100}%"
    >
      <span class="font-semibold tabular-nums">{format(hp.value)}</span>
      <span class="text-muted"> · {hp.label}</span>
    </div>
  {/if}
  </div>
  <!-- Axis labels live in the left gutter so they never sit on the line. -->
  <span class="pointer-events-none absolute top-0 left-0 w-10 pr-1 text-right text-[10px] text-muted tabular-nums">{format(bounds.hi)}</span>
  <span class="pointer-events-none absolute bottom-0 left-0 w-10 pr-1 text-right text-[10px] text-muted tabular-nums">{format(bounds.lo)}</span>
</div>
