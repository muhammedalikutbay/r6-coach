<script lang="ts">
  import CollapsibleSection from '$lib/v2/components/CollapsibleSection.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import { slotLabel } from '$lib/v2/format';
  import type { DeepStats, EndsView, RateView } from '$lib/v2/types';

  // Compact, collapsible Overview sections for the numbers that come only from replays:
  // round impact, weapons, timing, rivals and clutches. Counts always sit next to percentages
  // so a small sample is easy to spot.
  let { deep, only }: {
    deep: DeepStats;
    /** Section ids to show (impact, weapons, timing, rivals); all of them when omitted. */
    only?: string[];
  } = $props();
  const show = (id: string) => !only || only.includes(id);

  const pct = (r: RateView) => (r.n === 0 ? '–' : `${r.pct.toFixed(0)}%`);
  const SINGULAR: Record<string, string> = { rounds: 'round', deaths: 'death', chances: 'chance' };
  const count = (r: RateView, unit: string) => `${r.x} of ${r.n} ${r.n === 1 ? (SINGULAR[unit] ?? unit) : unit}`;
  const noun = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

  const tiles = $derived([
    { label: 'KOST', value: pct(deep.kost), sub: count(deep.kost, 'rounds'), hint: 'Kill, survival or trade' },
    { label: 'No impact', value: pct(deep.noImpact), sub: count(deep.noImpact, 'rounds'), hint: 'Died with no kill and no trade' },
    { label: 'Opening duels', value: pct(deep.entry), sub: count(deep.entry, 'won'), hint: 'First kill vs first death' },
    { label: 'On attack', value: pct(deep.entryAttack), sub: count(deep.entryAttack, 'won'), hint: 'Opening duels on attack' },
    { label: 'On defense', value: pct(deep.entryDefense), sub: count(deep.entryDefense, 'won'), hint: 'Opening duels on defense' },
    { label: 'Deaths traded', value: pct(deep.traded), sub: count(deep.traded, 'deaths'), hint: 'A teammate killed your killer within 10 s' },
    { label: 'Trades given', value: pct(deep.tradesGiven), sub: count(deep.tradesGiven, 'chances'), hint: 'Teammate deaths you avenged within 10 s' },
    { label: 'Team wins after your death', value: pct(deep.afterDeathWin), sub: count(deep.afterDeathWin, 'rounds'), hint: 'Rounds you died in' },
    { label: 'Team wins when you live', value: pct(deep.afterSurviveWin), sub: count(deep.afterSurviveWin, 'rounds'), hint: 'Rounds you survived' }
  ]);

  const ends = $derived([
    { side: 'Attack', e: deep.lostAttack, parts: attackParts(deep.lostAttack) },
    { side: 'Defense', e: deep.lostDefense, parts: defenseParts(deep.lostDefense) }
  ]);

  function attackParts(e: EndsView) {
    return [
      ['Wiped out', e.elimination],
      ['Clock ran out', e.time],
      ['Bomb defused', e.defused]
    ] as [string, number][];
  }
  function defenseParts(e: EndsView) {
    return [
      ['Wiped out', e.elimination],
      ['Lost after a plant', e.planted],
      ['Other', e.defused]
    ] as [string, number][];
  }

  const slotRows = $derived(
    (
      [
        ['Primary', deep.slotKills.primary],
        ['Secondary', deep.slotKills.secondary],
        ['Gadget', deep.slotKills.gadget],
        ['Ability', deep.slotKills.ability],
        ['Melee', deep.slotKills.melee],
        ['Explosion', deep.slotKills.explosion],
        ['Unknown', deep.slotKills.unknown]
      ] as [string, number][]
    ).filter(([, n]) => n > 0)
  );
  const totalKills = $derived(Math.max(1, slotRows.reduce((a, [, n]) => a + n, 0)));

  const timing = $derived([
    { label: 'Deaths in the first 30 s', r: deep.earlyDeath },
    { label: 'Deaths in the last 30 s', r: deep.lateDeath },
    { label: 'Kills in the first 30 s', r: deep.earlyKill },
    { label: 'Kills in the last 30 s', r: deep.lateKill }
  ]);

  const clutchTotal = $derived(deep.clutchByOpponents.reduce((a, c) => a + c.total, 0));
  const clutchRows = $derived(deep.clutchByOpponents.filter((c) => c.total > 0));
</script>

{#if deep.rounds > 0}
  {#if show('impact')}<CollapsibleSection id="impact" title="Round impact" hint="Opening duels, trades and how rounds end" summary="KOST {pct(deep.kost)} · duels {pct(deep.entry)}">
    <div class="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-5">
      {#each tiles as t (t.label)}
        <div class="rounded-xl border border-border bg-surface-1 p-3" title={t.hint}>
          <div class="truncate text-xs text-muted">{t.label}</div>
          <div class="text-lg font-bold tabular-nums">{t.value}</div>
          <div class="text-xs text-muted tabular-nums">{t.sub}</div>
        </div>
      {/each}
    </div>
    <div class="mt-3 grid grid-cols-1 gap-2 md:grid-cols-2">
      {#each ends as row (row.side)}
        <div class="rounded-xl border border-border bg-surface-1 p-3">
          <div class="mb-1.5 flex items-baseline justify-between text-xs">
            <span class="font-medium text-foreground">Lost {row.side.toLowerCase()} rounds</span>
            <span class="text-muted tabular-nums">{row.e.lost} total</span>
          </div>
          {#if row.e.lost === 0}
            <p class="text-xs text-muted">None in this filter.</p>
          {:else}
            <div class="flex flex-wrap gap-1.5">
              {#each row.parts.filter(([, n]) => n > 0) as [label, n] (label)}
                <span class="rounded-full bg-surface-3 px-2 py-0.5 text-xs tabular-nums">{label}: {n}</span>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  </CollapsibleSection>{/if}

  {#if show('weapons')}<CollapsibleSection id="weapons" title="Weapons" hint="Your kills by weapon slot and weapon" summary={deep.weapons[0] ? `${deep.weapons[0].name} · ${noun(deep.weapons[0].kills, 'kill', 'kills')}` : 'no weapon data'}>
    {#if slotRows.length === 0}
      <p class="text-muted">No kills in this filter.</p>
    {:else}
      <div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
        <div class="rounded-xl border border-border bg-surface-1 p-3">
          <div class="mb-2 text-xs text-muted">Kills by slot</div>
          <div class="flex h-2 overflow-hidden rounded-full bg-surface-3">
            {#each slotRows as [label, n], i (label)}
              <div class="h-full {['bg-accent', 'bg-good', 'bg-accent/60', 'bg-good/60', 'bg-muted', 'bg-muted/60', 'bg-surface-3'][i % 7]}" style="width:{(n / totalKills) * 100}%" title="{label}: {n}"></div>
            {/each}
          </div>
          <div class="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted tabular-nums">
            {#each slotRows as [label, n] (label)}<span>{label} {n}</span>{/each}
          </div>
        </div>
        <div class="rounded-xl border border-border bg-surface-1 p-3">
          {#if deep.weapons.length === 0}
            <p class="text-xs text-muted">Weapon names are not known for these kills.</p>
          {:else}
            <table class="w-full text-left text-sm">
              <thead>
                <tr class="text-xs text-muted">
                  <th class="pb-1 font-medium">Weapon</th>
                  <th class="pb-1 text-right font-medium">Kills</th>
                  <th class="pb-1 text-right font-medium">HS %</th>
                </tr>
              </thead>
              <tbody>
                {#each deep.weapons as w (w.name)}
                  <tr>
                    <td class="py-0.5"><a class="hover:text-accent hover:underline" href={`/weapons/${w.name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')}`}>{w.name}</a>{#if w.slot}<span class="ml-1.5 text-xs text-muted">{slotLabel(w.slot)}</span>{/if}</td>
                    <td class="py-0.5 text-right tabular-nums">{w.kills}</td>
                    <td class="py-0.5 text-right tabular-nums">{w.hsPct.toFixed(0)}%</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>
      </div>
    {/if}
  </CollapsibleSection>{/if}

  {#if show('timing')}<CollapsibleSection id="timing" title="Timing" hint="Early and late in the round" summary="early deaths {pct(deep.earlyDeath)}">
    <div class="grid grid-cols-2 gap-2 lg:grid-cols-4">
      {#each timing as t (t.label)}
        <div class="rounded-xl border border-border bg-surface-1 p-3">
          <div class="truncate text-xs text-muted">{t.label}</div>
          <div class="text-lg font-bold tabular-nums">{pct(t.r)}</div>
          <div class="text-xs text-muted tabular-nums">{t.r.x} of {t.r.n}</div>
        </div>
      {/each}
    </div>
  </CollapsibleSection>{/if}

  {#if show('rivals')}<CollapsibleSection id="rivals" title="Rivals" hint="Enemy operators that killed you, and your clutches" summary={deep.rivals[0] ? `${deep.rivals[0].name} · ${noun(deep.rivals[0].deaths, 'death', 'deaths')}` : 'no repeat killers'}>
    <div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
      <div class="rounded-xl border border-border bg-surface-1 p-3">
        <div class="mb-2 text-xs text-muted">Enemy operators that killed you at least twice</div>
        {#if deep.rivals.length === 0}
          <p class="text-xs text-muted">No operator has killed you twice yet.</p>
        {:else}
          <div class="flex flex-col gap-2">
            {#each deep.rivals as r (r.operatorKey)}
              <div class="flex items-center gap-2.5">
                <OperatorBadge opKey={r.operatorKey} name={r.name} size={28} />
                <div class="min-w-0 flex-1 truncate text-sm">{r.name}</div>
                <div class="text-right text-xs text-muted tabular-nums">
                  {noun(r.deaths, 'death', 'deaths')} in {noun(r.faced, 'round', 'rounds')} ({r.pct.toFixed(0)}%)
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
      <div class="rounded-xl border border-border bg-surface-1 p-3">
        <div class="mb-2 text-xs text-muted">Clutches by opponents alive</div>
        {#if clutchTotal === 0}
          <p class="text-xs text-muted">No clutch situations yet.</p>
        {:else}
          <div class="flex flex-wrap gap-2">
            {#each clutchRows as c (c.vs)}
              <div class="rounded-lg bg-surface-2 px-3 py-1.5 text-center">
                <div class="text-xs text-muted">1v{c.vs}{c.vs === 5 ? '+' : ''}</div>
                <div class="text-sm font-semibold tabular-nums">{c.won} / {c.total}</div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </CollapsibleSection>{/if}
{/if}
