<script lang="ts">
  import { Skull, Bomb, ShieldCheck, Clock, Crosshair, X, CircleHelp } from '@lucide/svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import { cn } from '$lib/utils';
  import { formatDateTime } from '$lib/v2/format';
  import * as api from '$lib/v2/api';
  import type { MatchDetail, RoundDetail, ScoreboardRow } from '$lib/v2/types';

  let { matchId, onclose }: { matchId: string; onclose: () => void } = $props();

  let detail = $state<MatchDetail | null>(null);
  let error = $state('');
  let tab = $state<'scoreboard' | 'rounds'>('scoreboard');
  let selected = $state(1);

  $effect(() => {
    const id = matchId;
    detail = null;
    error = '';
    tab = 'scoreboard';
    selected = 1;
    api
      .matchDetail(id)
      .then((d) => {
        if (id === matchId) detail = d;
      })
      .catch((e) => {
        console.error('[matches] match detail failed', e);
        error = 'Could not load this match.';
      });
  });

  const round = $derived(detail?.rounds.find((r) => r.number === selected) ?? detail?.rounds[0]);

  function condIcon(c: string) {
    const s = c.toLowerCase();
    if (s.includes('defus')) return ShieldCheck;
    if (s.includes('plant') || s.includes('bomb')) return Bomb;
    if (s.includes('time')) return Clock;
    if (s.includes('elim') || s.includes('kill')) return Skull;
    return CircleHelp;
  }

  // Per-player numbers for one round, taken from its kill feed.
  function roundRows(d: MatchDetail, r: RoundDetail) {
    return d.teams.map((team) =>
      team.map((p: ScoreboardRow) => ({
        p,
        op: p.operatorsByRound[r.number - 1] ?? '',
        k: r.kills.filter((k) => k.killer === p.username).length,
        d: r.kills.filter((k) => k.victim === p.username).length
      }))
    );
  }

  const fmtTime = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
  const opName = (k: string) => k;
</script>

<div class="flex h-full flex-col">
  <div class="flex items-center justify-between border-b border-border px-5 py-3">
    <h2 class="text-base font-semibold">Match detail</h2>
    <button type="button" onclick={onclose} aria-label="Close" class="rounded-md p-1 text-muted hover:bg-surface-2 hover:text-foreground">
      <X class="size-5" />
    </button>
  </div>

  {#if error}
    <p class="p-5 text-bad">{error}</p>
  {:else if !detail}
    <div class="space-y-3 p-5">
      <Skeleton class="h-20" />
      <Skeleton class="h-8 w-48" />
      <Skeleton class="h-64" />
    </div>
  {:else}
    {@const m = detail.match}
    <div class="flex items-center gap-4 px-5 py-4">
      <MapThumb mapKey={m.mapKey} name={m.mapName} class="w-36 shrink-0" />
      <div class="min-w-0 flex-1">
        <div class="text-lg font-bold">{m.mapName}</div>
        <div class="text-xs text-muted">{m.mode} · {formatDateTime(m.timestamp)}</div>
      </div>
      <div class="text-right">
        <div class={cn('text-2xl font-bold', m.won ? 'text-good' : 'text-bad')}>{m.scoreUs} - {m.scoreThem}</div>
        <div class="text-xs text-muted">{m.won ? 'Victory' : 'Defeat'} · {m.kills}/{m.deaths} · {m.hsPct.toFixed(0)}% HS</div>
      </div>
    </div>

    <div class="flex gap-1 border-b border-border px-5">
      {#each [['scoreboard', 'Scoreboard'], ['rounds', 'Rounds']] as [id, label] (id)}
        <button
          type="button"
          onclick={() => (tab = id as 'scoreboard' | 'rounds')}
          class={cn(
            'border-b-2 px-3 py-2 text-sm font-semibold',
            tab === id ? 'border-accent text-foreground' : 'border-transparent text-muted hover:text-foreground'
          )}>{label}</button
        >
      {/each}
    </div>

    <div class="flex-1 overflow-auto p-5">
      {#if tab === 'scoreboard'}
        {#each detail.teams as team, ti (ti)}
          <div class="mb-6">
            <div class={cn('mb-2 text-sm font-semibold', ti === 0 ? 'text-good' : 'text-bad')}>
              {ti === 0 ? 'Your team' : 'Opponents'} · {ti === 0 ? m.scoreUs : m.scoreThem}
            </div>
            <div class="overflow-x-auto rounded-lg border border-border">
              <table class="w-full min-w-[760px] text-sm">
                <thead class="bg-surface-2 text-xs text-muted">
                  <tr>
                    <th class="px-3 py-2 text-left font-medium">Player</th>
                    <th class="px-2 py-2 text-left font-medium">Operators</th>
                    <th class="px-2 py-2 font-medium">K</th>
                    <th class="px-2 py-2 font-medium">D</th>
                    <th class="px-2 py-2 font-medium">HS%</th>
                    <th class="px-2 py-2 font-medium">FK</th>
                    <th class="px-2 py-2 font-medium">FD</th>
                    <th class="px-2 py-2 font-medium">1vX</th>
                    <th class="px-2 py-2 font-medium">Multi</th>
                    <th class="px-2 py-2 font-medium">KOST</th>
                  </tr>
                </thead>
                <tbody>
                  {#each team as p (p.username)}
                    <tr class={cn('border-t border-border text-center', p.isMe && 'bg-accent/10')}>
                      <td class={cn('px-3 py-2 text-left font-semibold whitespace-nowrap', p.isMe && 'text-accent')}>{p.username}</td>
                      <td class="px-2 py-2">
                        <div class="flex flex-wrap gap-0.5">
                          {#each p.operatorsByRound as op, i (i)}
                            <OperatorBadge opKey={op} name={opName(op)} size={20} />
                          {/each}
                        </div>
                      </td>
                      <td class="px-2 py-2">{p.kills}</td>
                      <td class="px-2 py-2">{p.deaths}</td>
                      <td class="px-2 py-2">{p.hsPct.toFixed(0)}</td>
                      <td class="px-2 py-2">{p.fk}</td>
                      <td class="px-2 py-2">{p.fd}</td>
                      <td class="px-2 py-2 whitespace-nowrap">{p.clutchesWon}/{p.clutchesWon + p.clutchesLost}</td>
                      <td class="px-2 py-2">{p.multiKills}</td>
                      <td class="px-2 py-2">{p.kostPct.toFixed(0)}%</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </div>
        {/each}
      {:else if round}
        <div class="mb-4 flex flex-wrap gap-1.5">
          {#each detail.rounds as r (r.number)}
            {@const Icon = condIcon(r.winCondition)}
            <button
              type="button"
              onclick={() => (selected = r.number)}
              title="Round {r.number}: {r.winCondition}"
              class={cn(
                'flex w-10 flex-col items-center gap-0.5 rounded-md border py-1 text-xs',
                r.winnerTeam === 0 ? 'border-good/40 bg-good/10 text-good' : 'border-bad/40 bg-bad/10 text-bad',
                r.number === round.number && 'ring-2 ring-accent'
              )}
            >
              <span class="font-semibold">{r.number}</span>
              <Icon class="size-3.5" />
            </button>
          {/each}
        </div>

        <div class="mb-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
          <span class="font-semibold">Round {round.number}</span>
          <span class={round.winnerTeam === 0 ? 'text-good' : 'text-bad'}>
            {round.winnerTeam === 0 ? 'Won' : 'Lost'} · {round.winCondition}
          </span>
          <span class="text-muted">{round.mySide === 'attack' ? 'Attack' : 'Defense'}{round.site ? ` · ${round.site}` : ''}</span>
        </div>

        <div class="grid gap-4 lg:grid-cols-2">
          {#each roundRows(detail, round) as rows, ti (ti)}
            <div class="overflow-hidden rounded-lg border border-border">
              <table class="w-full text-sm">
                <thead class="bg-surface-2 text-xs text-muted">
                  <tr>
                    <th class="px-3 py-1.5 text-left font-medium">{ti === 0 ? 'Your team' : 'Opponents'}</th>
                    <th class="px-2 font-medium">K</th>
                    <th class="px-2 font-medium">D</th>
                  </tr>
                </thead>
                <tbody>
                  {#each rows as r (r.p.username)}
                    <tr class={cn('border-t border-border text-center', r.p.isMe && 'bg-accent/10')}>
                      <td class="px-3 py-1.5 text-left">
                        <span class="flex items-center gap-2">
                          <OperatorBadge opKey={r.op} name={opName(r.op)} size={22} />
                          <span class={cn('truncate', r.p.isMe && 'font-semibold text-accent')}>{r.p.username}</span>
                        </span>
                      </td>
                      <td class="px-2">{r.k}</td>
                      <td class={cn('px-2', r.d > 0 && 'text-bad')}>{r.d}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/each}
        </div>

        <div class="mt-4">
          <div class="mb-2 text-sm font-semibold">Kill feed</div>
          {#if round.kills.length === 0}
            <p class="text-muted">No kills recorded in this round.</p>
          {:else}
            <ul class="divide-y divide-border rounded-lg border border-border">
              {#each round.kills as k, i (i)}
                <li class="flex items-center gap-2 px-3 py-1.5 text-sm">
                  <span class="w-10 text-xs text-muted">{fmtTime(k.timeS)}</span>
                  <OperatorBadge opKey={k.killerOperator} name={opName(k.killerOperator)} size={22} />
                  <span class={cn('font-semibold', k.killerTeam === 0 ? 'text-good' : 'text-bad')}>{k.killer}</span>
                  {#if k.headshot}<Crosshair class="size-3.5 text-accent" aria-label="Headshot" />{:else}<Skull class="size-3.5 text-muted" />{/if}
                  <OperatorBadge opKey={k.victimOperator} name={opName(k.victimOperator)} size={22} />
                  <span class={cn(k.killerTeam === 0 ? 'text-bad' : 'text-good')}>{k.victim}</span>
                  {#if k.weapon}<span class="ml-auto text-xs text-muted" title={k.weaponSlot ?? undefined}>{k.weapon}</span>{/if}
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>
