<script lang="ts">
  // Enemy operators with a count, each linking to that operator's screen.
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import type { DetailRival } from '$lib/v2/types';

  let { rows, unit, empty = 'Nothing yet.' }: { rows: DetailRival[]; unit: string; empty?: string } = $props();
  const max = $derived(Math.max(1, ...rows.map((r) => r.count)));
</script>

{#if rows.length === 0}
  <p class="text-sm text-muted">{empty}</p>
{:else}
  <ul class="flex flex-col gap-1.5">
    {#each rows as r (r.operatorKey)}
      <li>
        <a href="/operators/{encodeURIComponent(r.operatorKey)}" class="flex items-center gap-2.5 rounded-lg px-1.5 py-1 transition-colors hover:bg-surface-2">
          <OperatorBadge opKey={r.operatorKey} name={r.name} size={28} />
          <span class="w-24 truncate text-sm">{r.name}</span>
          <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-surface-3">
            <div class="h-full rounded-full bg-accent" style="width:{(r.count / max) * 100}%"></div>
          </div>
          <span class="w-16 text-right text-xs text-muted tabular-nums">{r.count} {unit}</span>
        </a>
      </li>
    {/each}
  </ul>
{/if}
