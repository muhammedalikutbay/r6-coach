<script lang="ts">
  import { onMount } from 'svelte';
  import { operatorTable } from '$lib/api';
  import type { OperatorRow, Side } from '$lib/types';

  let rows = $state<OperatorRow[] | null>(null);
  let error = $state('');
  let side = $state<'all' | Side>('all');

  onMount(async () => {
    try { rows = await operatorTable(); }
    catch (e) { console.error(e); error = String(e instanceof Error ? e.message : e); }
  });

  const shown = $derived(
    (rows ?? []).filter((r) => side === 'all' || r.side === side).sort((a, b) => b.rounds - a.rounds)
  );
  const arrow = { up: '▲', down: '▼', flat: '▬' } as const;
  const maxRounds = $derived(Math.max(1, ...shown.map((r) => r.rounds)));
  const filters: { v: 'all' | Side; l: string }[] = [
    { v: 'all', l: 'All' }, { v: 'attack', l: 'Attack' }, { v: 'defense', l: 'Defense' }
  ];
</script>

<div class="top">
  <h1>Operators</h1>
  <div class="seg">
    {#each filters as f}
      <button class:on={side === f.v} onclick={() => (side = f.v)}>{f.l}</button>
    {/each}
  </div>
</div>

{#if error}
  <div class="card state error">Could not load operators: {error}</div>
{:else if !rows}
  <div class="state"><span class="spinner"></span> Loading…</div>
{:else if shown.length === 0}
  <div class="card state">No operator data yet. Press Update on the Home screen.</div>
{:else}
  <div class="card tbl">
    <table>
      <thead><tr><th>Operator</th><th>Side</th><th>Rounds</th><th>Win %</th><th>K/D</th><th>HS %</th><th>Trend</th></tr></thead>
      <tbody>
        {#each shown as r (r.name + r.side)}
          <tr>
            <td class="name">{r.name}</td>
            <td class="muted">{r.side === 'attack' ? 'Attack' : 'Defense'}</td>
            <td><div class="bar"><i style="width:{(r.rounds / maxRounds) * 100}%"></i><span>{r.rounds}</span></div></td>
            <td>{Math.round(r.winPct)}%</td>
            <td>{r.kd.toFixed(2)}</td>
            <td>{Math.round(r.hsPct)}%</td>
            <td class="t {r.trend}">{arrow[r.trend]}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .top { display: flex; justify-content: space-between; align-items: center; margin-bottom: 14px; gap: 12px; }
  .seg { display: flex; border: 1px solid var(--line); border-radius: 8px; overflow: hidden; }
  .seg button { background: var(--panel); border: 0; padding: 6px 14px; color: var(--muted); }
  .seg button.on { background: var(--accent); color: #fff; }
  .tbl { padding: 4px 8px; overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; }
  th { text-align: left; font-weight: 500; color: var(--muted); font-size: 12px; padding: 8px; }
  td { padding: 9px 8px; border-top: 1px solid var(--line); }
  .name { font-weight: 600; }
  .bar { position: relative; min-width: 80px; height: 20px; background: var(--panel2); border-radius: 4px; }
  .bar i { position: absolute; inset: 0 auto 0 0; background: #2f4a80; border-radius: 4px; }
  .bar span { position: relative; padding-left: 8px; line-height: 20px; font-size: 12px; }
  .t { font-size: 12px; } .up { color: var(--green); } .down { color: var(--red); } .flat { color: var(--gray); }
</style>
