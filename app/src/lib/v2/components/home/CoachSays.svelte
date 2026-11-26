<script lang="ts">
  // "Your coach says": the top coaching points, in the coach's own importance order.
  import { ThumbsUp, TriangleAlert, ArrowRight } from '@lucide/svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import type { Insight } from '$lib/v2/types';

  let { insights }: { insights: Insight[] } = $props();
</script>

{#if insights.length === 0}
  <Card class="text-muted">Nothing stands out yet. Play a few more matches and your coach will have something to say.</Card>
{:else}
  <div class="grid grid-cols-1 gap-3 md:grid-cols-3">
    {#each insights as ins (ins.family + ins.title)}
      <Card class="flex flex-col gap-2">
        <div class="flex items-center gap-2">
          {#if ins.operatorKey}
            <OperatorBadge opKey={ins.operatorKey} size={32} />
          {:else if ins.mapKey}
            <MapThumb mapKey={ins.mapKey} class="w-14 shrink-0" />
          {:else}
            <span
              class="flex size-8 shrink-0 items-center justify-center rounded-full {ins.kind === 'improve' ? 'bg-accent/15 text-accent' : 'bg-good/15 text-good'}"
            >
              {#if ins.kind === 'improve'}<TriangleAlert class="size-4" />{:else}<ThumbsUp class="size-4" />{/if}
            </span>
          {/if}
          <h3 class="min-w-0 font-semibold">{ins.title}</h3>
        </div>
        <p class="text-sm leading-relaxed text-muted">{ins.text}</p>
        {#if ins.evidence}<p class="text-xs text-muted/80 tabular-nums">{ins.evidence}</p>{/if}
        <a href="/coach" class="mt-auto inline-flex items-center gap-1 pt-1 text-xs font-medium text-accent hover:underline">
          See it in Coach <ArrowRight class="size-3.5" />
        </a>
      </Card>
    {/each}
  </div>
{/if}
