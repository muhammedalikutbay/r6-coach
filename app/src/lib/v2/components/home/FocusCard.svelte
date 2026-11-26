<script lang="ts">
  // One thing to work on in the next session, taken from the top coaching point.
  import { Target } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import type { CoachReport } from '$lib/v2/types';

  let { report }: { report: CoachReport | null } = $props();

  const top = $derived(report?.insights.find((i) => i.kind === 'improve') ?? null);
  const fallback = $derived(report?.workOn[0] ?? null);
</script>

<Card class="flex h-full flex-col gap-2 border-accent/30 bg-accent/5">
  <div class="flex items-center gap-1.5 text-xs font-semibold tracking-wide text-accent uppercase">
    <Target class="size-4" /> Focus for your next session
  </div>
  {#if !report}
    <p class="text-sm text-muted">Your coach is thinking…</p>
  {:else if top}
    <h3 class="text-lg font-bold">{top.title}</h3>
    <p class="text-sm leading-relaxed">{top.text}</p>
    {#if top.evidence}<p class="text-xs text-muted tabular-nums">{top.evidence}</p>{/if}
  {:else if fallback}
    <h3 class="text-lg font-bold">{fallback}</h3>
    <p class="text-sm leading-relaxed text-muted">Keep an eye on this one while you play.</p>
  {:else}
    <h3 class="text-lg font-bold">Keep doing what you do</h3>
    <p class="text-sm leading-relaxed text-muted">Nothing needs fixing right now. Play your game and your coach will tell you when something changes.</p>
  {/if}
</Card>
