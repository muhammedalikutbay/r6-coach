<script lang="ts">
  import type { Snippet } from 'svelte';
  import DeltaPill from './DeltaPill.svelte';
  import Card from './ui/Card.svelte';
  let { label, value, sub = '', delta, deltaText, lowerIsBetter = false, children }: {
    label: string;
    value: string;
    sub?: string;
    delta?: number;
    deltaText?: string;
    lowerIsBetter?: boolean;
    children?: Snippet;
  } = $props();
</script>

<Card class="flex flex-col gap-1">
  <div class="text-xs font-medium tracking-wide text-muted uppercase">{label}</div>
  <div class="flex flex-wrap items-baseline gap-2">
    <span class="text-2xl font-bold tabular-nums">{value}</span>
    {#if delta !== undefined}<DeltaPill value={delta} text={deltaText} {lowerIsBetter} />{/if}
  </div>
  {#if sub}<div class="text-xs text-muted">{sub}</div>{/if}
  {@render children?.()}
</Card>
