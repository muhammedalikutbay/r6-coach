<script lang="ts">
  import type { Comparison } from '$lib/v2/types';

  // Paired horizontal bars (focus vs reference) per metric. Metrics have different units, so each
  // row is scaled to its own larger value; the numbers beside the bars carry the real values.
  let { rows, focusLabel, referenceLabel }: {
    rows: Comparison[];
    focusLabel: string;
    referenceLabel: string;
  } = $props();

  const fmt = (n: number) => (Math.abs(n) >= 10 ? n.toFixed(1) : n.toFixed(2));
  const pct = (v: number, r: Comparison) => {
    const max = Math.max(Math.abs(r.focus), Math.abs(r.reference), 1e-9);
    return Math.max(2, (Math.abs(v) / max) * 100);
  };
</script>

<div class="flex flex-col gap-3">
  <div class="flex gap-4 text-xs text-muted">
    <span class="flex items-center gap-1.5"><span class="size-2.5 rounded-sm bg-accent"></span>{focusLabel}</span>
    <span class="flex items-center gap-1.5"><span class="size-2.5 rounded-sm bg-surface-3"></span>{referenceLabel}</span>
  </div>
  {#each rows as r (r.metric)}
    <div class="grid grid-cols-[7rem_1fr] items-center gap-3">
      <div class="text-xs text-muted">{r.metric}</div>
      <div class="flex flex-col gap-1">
        {#each [{ v: r.focus, cls: r.better ? 'bg-accent' : 'bg-accent/60' }, { v: r.reference, cls: 'bg-surface-3' }] as bar, i (i)}
          <div class="flex items-center gap-2">
            <div class="h-2 flex-1 overflow-hidden rounded-full bg-surface-2">
              <div class="h-full rounded-full {bar.cls}" style="width:{pct(bar.v, r)}%"></div>
            </div>
            <span class="w-12 text-right text-xs tabular-nums {i === 0 ? 'text-foreground' : 'text-muted'}">{fmt(bar.v)}</span>
          </div>
        {/each}
      </div>
    </div>
  {/each}
</div>
