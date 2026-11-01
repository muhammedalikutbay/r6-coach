<script lang="ts">
  import { onMount } from 'svelte';
  import { mapTable } from '$lib/api';
  import type { MapRow } from '$lib/types';

  let rows = $state<MapRow[] | null>(null);
  let error = $state('');

  onMount(async () => {
    try { rows = await mapTable(); }
    catch (e) { console.error(e); error = String(e instanceof Error ? e.message : e); }
  });

  const sorted = $derived([...(rows ?? [])].sort((a, b) => b.rounds - a.rounds));
  const tone = (p: number) => (p >= 55 ? 'good' : p < 45 ? 'bad' : 'mid');
</script>

<h1>Maps</h1>
<p class="muted legend">Win rate by side. Green ≥ 55%, orange &lt; 45%.</p>

{#if error}
  <div class="card state error">Could not load maps: {error}</div>
{:else if !rows}
  <div class="state"><span class="spinner"></span> Loading…</div>
{:else if sorted.length === 0}
  <div class="card state">No map data yet. Press Update on the Home screen.</div>
{:else}
  <div class="list">
    {#each sorted as m (m.name)}
      <div class="card row">
        <div class="info">
          <div class="name">{m.name}</div>
          <div class="muted small">{m.rounds} rounds · {m.winPct}% win · K/D {m.kd.toFixed(2)}</div>
        </div>
        <div class="sides">
          {#each [{ l: 'Attack', p: m.attackWinPct }, { l: 'Defense', p: m.defenseWinPct }] as s}
            <div class="side">
              <span class="lbl">{s.l}</span>
              <div class="track"><i class={tone(s.p)} style="width:{s.p}%"></i></div>
              <span class="pct">{s.p}%</span>
            </div>
          {/each}
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .legend { margin: 4px 0 14px; font-size: 12px; }
  .list { display: flex; flex-direction: column; gap: 8px; }
  .row { display: flex; gap: 20px; align-items: center; padding: 12px 16px; }
  .info { width: 210px; flex: none; }
  .name { font-weight: 600; font-size: 15px; }
  .small { font-size: 12px; }
  .sides { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; }
  .side { display: flex; align-items: center; gap: 10px; }
  .lbl { width: 56px; font-size: 12px; color: var(--muted); }
  .track { flex: 1; height: 10px; background: var(--panel2); border-radius: 5px; overflow: hidden; }
  .track i { display: block; height: 100%; border-radius: 5px; }
  .good { background: var(--green); } .mid { background: var(--gray); } .bad { background: var(--orange); }
  .pct { width: 36px; text-align: right; font-size: 12px; }
</style>
