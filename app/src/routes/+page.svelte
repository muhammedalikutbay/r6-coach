<script lang="ts">
  import { onMount } from 'svelte';
  import { lastMatchReport, refresh } from '$lib/api';
  import type { LastMatchReport, RefreshResult } from '$lib/types';

  let report = $state<LastMatchReport | null>(null);
  let loading = $state(true);
  let updating = $state(false);
  let error = $state('');
  let result = $state<RefreshResult | null>(null);

  const msg = (e: unknown) => String(e instanceof Error ? e.message : e);

  async function load() {
    try {
      report = await lastMatchReport();
    } catch (e) {
      console.error('lastMatchReport failed', e);
      error = msg(e);
    } finally {
      loading = false;
    }
  }

  async function update() {
    updating = true; error = ''; result = null;
    try {
      result = await refresh();
      await load();
    } catch (e) {
      console.error('refresh failed', e);
      error = msg(e);
    } finally {
      updating = false;
    }
  }

  onMount(load);

  const fmt = (iso: string) =>
    new Date(iso).toLocaleString('en-GB', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
  const kd = (k: number, d: number) => (d === 0 ? k.toFixed(2) : (k / d).toFixed(2));
  const icon = { praise: '✓', improve: '▲', info: 'i' } as const;
</script>

<div class="top">
  <h1>Last match</h1>
  <button class="btn primary" onclick={update} disabled={updating}>
    {#if updating}<span class="spinner"></span> Updating…{:else}Update{/if}
  </button>
</div>

{#if error}
  <div class="card state error">Something went wrong: {error}</div>
{/if}
{#if result}
  {#if result.newMatches === 0 && result.failed.length === 0}
    <div class="notice">No new matches found. Play a game, then press Update.</div>
  {:else if result.newMatches > 0}
    <div class="notice ok">{result.newMatches} new {result.newMatches === 1 ? 'match' : 'matches'} added.</div>
  {/if}
  {#each result.failed as f}
    <div class="notice bad">Could not read <b>{f.folder}</b>: {f.error}</div>
  {/each}
{/if}

{#if loading}
  <div class="state"><span class="spinner"></span> Loading…</div>
{:else if !report}
  <div class="card state">
    <p>No matches yet.</p>
    <p>Press <b>Update</b> to read your match replays.</p>
  </div>
{:else}
  {@const m = report.match}
  <section class="card match">
    <div class="head">
      <span class="badge" class:win={m.won} class:loss={!m.won}>{m.won ? 'Win' : 'Loss'}</span>
      <span class="score">{m.scoreUs} – {m.scoreThem}</span>
      <span class="where">{m.map} · {m.matchType}</span>
      <span class="muted when">{fmt(m.timestamp)}</span>
    </div>
    <div class="stats">
      <div><b>{m.kills}/{m.deaths}</b><span>K / D ({kd(m.kills, m.deaths)})</span></div>
      <div><b>{m.headshots}</b><span>Headshots</span></div>
      <div><b>{m.firstBloods}</b><span>First bloods</span></div>
      <div><b>{m.firstDeaths}</b><span>First deaths</span></div>
    </div>
    <div class="ops muted">
      {#each m.operators as o}<span class="chip">{o.name} ×{o.rounds}</span>{/each}
    </div>
  </section>

  <h2>Advice</h2>
  <ul class="advice">
    {#each report.advice as a}
      <li class={a.kind}>
        <span class="ic">{icon[a.kind]}</span>
        <div>
          <div class="text">{a.text}</div>
          <div class="ev">{a.evidence}</div>
        </div>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .top { display: flex; justify-content: space-between; align-items: center; margin-bottom: 14px; gap: 12px; }
  .primary { background: var(--accent); border-color: var(--accent); font-size: 17px; font-weight: 600;
    padding: 12px 34px; color: #fff; }
  .primary:hover:not(:disabled) { filter: brightness(1.1); }
  .notice { background: var(--panel2); border-radius: 8px; padding: 8px 12px; margin-bottom: 10px; color: var(--muted); }
  .notice.ok { color: var(--green); } .notice.bad { color: var(--red); }
  .match { margin-bottom: 20px; }
  .head { display: flex; align-items: baseline; gap: 12px; flex-wrap: wrap; margin-bottom: 14px; }
  .badge { padding: 2px 10px; border-radius: 6px; font-weight: 700; font-size: 12px; text-transform: uppercase; }
  .win { background: #1f4d39; color: var(--green); } .loss { background: #55292a; color: var(--red); }
  .score { font-size: 26px; font-weight: 700; }
  .where { font-size: 15px; } .when { margin-left: auto; font-size: 12px; }
  .stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; }
  .stats div { background: var(--panel2); border-radius: 8px; padding: 8px 10px; display: flex; flex-direction: column; }
  .stats b { font-size: 20px; } .stats span { font-size: 12px; color: var(--muted); }
  .ops { margin-top: 12px; display: flex; gap: 6px; flex-wrap: wrap; }
  .chip { background: var(--panel2); border-radius: 12px; padding: 2px 10px; font-size: 12px; }
  h2 { font-size: 15px; margin-bottom: 10px; }
  .advice { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .advice li { display: flex; gap: 12px; background: var(--panel); border: 1px solid var(--line);
    border-left: 4px solid var(--c); border-radius: 10px; padding: 10px 14px; }
  .praise { --c: var(--green); } .improve { --c: var(--orange); } .info { --c: var(--gray); }
  .ic { color: var(--c); font-weight: 700; width: 16px; text-align: center; flex: none; }
  .ev { font-size: 12px; color: var(--muted); margin-top: 2px; }
  @media (max-width: 700px) { .stats { grid-template-columns: repeat(2, 1fr); } }
</style>
