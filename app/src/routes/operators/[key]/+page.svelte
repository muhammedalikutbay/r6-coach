<script lang="ts">
  import { page } from '$app/state';
  import { Users, Swords, Shield, Funnel } from '@lucide/svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Section from '$lib/components/Section.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import StatCard from '$lib/components/StatCard.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import LineChart from '$lib/v2/components/charts/LineChart.svelte';
  import DetailHeader from '$lib/v2/components/details/DetailHeader.svelte';
  import ClipsStrip from '$lib/v2/components/details/ClipsStrip.svelte';
  import RivalList from '$lib/v2/components/details/RivalList.svelte';
  import { operatorName } from '$lib/v2/catalog';
  import { filterStore } from '$lib/v2/filter.svelte';
  import { operatorDetail } from '$lib/v2/api';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import { formatShortDate, formatDateTime, plural, slotLabel } from '$lib/v2/format';
  import { toast } from '$lib/toast.svelte';
  import { cn } from '$lib/utils';
  import type { OperatorScreen } from '$lib/v2/types';

  const key = $derived(decodeURIComponent(page.params.key ?? '').toLowerCase());

  let data = $state<OperatorScreen | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let seq = 0;

  $effect(() => {
    const k = key;
    const f = $state.snapshot(filterStore.value);
    void refreshTick.value;
    const mine = ++seq;
    loading = true;
    operatorDetail(f, k)
      .then((r) => {
        if (mine !== seq) return;
        data = r;
        failed = false;
      })
      .catch((e) => {
        if (mine !== seq) return;
        console.error('[operator] detail failed', e);
        failed = true;
        toast('error', 'Could not load this operator.');
      })
      .finally(() => {
        if (mine === seq) loading = false;
      });
  });

  const d = $derived(data?.detail);
  const s = $derived(d?.stats);
  const title = $derived(d?.name || operatorName(key));
  const trend = $derived(
    (d?.trend ?? []).map((t) => ({
      label: `#${t.index} · ${formatShortDate(t.timestamp)} · ${t.won ? 'Win' : 'Loss'}`,
      value: t.kd
    }))
  );
  const filtered = $derived(filterStore.value.operators.includes(key));
  const pct = (v: number) => `${v.toFixed(1)}%`;
</script>

<DetailHeader
  backHref="/operators"
  backLabel="All operators"
  {title}
  sub={s && s.rounds > 0 ? `${plural(s.rounds, 'round')} in ${plural(s.matches, 'match', 'matches')}` : ''}
>
  {#snippet lead()}<OperatorBadge opKey={key} name={title} size={56} />{/snippet}
  {#snippet chips()}
    {#if d?.side}
      <span class="inline-flex items-center gap-1.5 rounded-lg bg-surface-2 px-2.5 py-1 text-xs font-medium">
        {#if d.side === 'attack'}<Swords class="size-3.5" />Attacker{:else}<Shield class="size-3.5" />Defender{/if}
      </span>
    {/if}
    <button
      type="button"
      onclick={() => filterStore.toggleOperator(key)}
      title={filtered ? 'Remove this operator from the global filter' : 'Add this operator to the global filter'}
      class={cn(
        'inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-xs font-medium transition-colors hover:bg-surface-2',
        filtered ? 'border-accent text-accent' : 'border-border text-muted'
      )}
    >
      <Funnel class="size-3.5" />{filtered ? 'In filter' : 'Filter by this'}
    </button>
  {/snippet}
</DetailHeader>

{#if loading && !data}
  <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
    {#each Array(8) as _, i (i)}<Skeleton class="h-24" />{/each}
  </div>
  <Skeleton class="mt-6 h-48" />
{:else if failed && !data}
  <EmptyState title="Could not load data" text="Try changing the filter, or restart the app if it keeps happening." icon={Users} />
{:else if d && s}
  {#if s.rounds === 0}
    <EmptyState
      title={operatorName(key) === key ? 'Unknown operator' : `No rounds with ${title} for this filter`}
      text={operatorName(key) === key ? 'There is no operator with this name.' : "Loosen the filter to see this operator's stats."}
      icon={Users}
    >
      {#if operatorName(key) === key}
        <a href="/operators" class="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-accent-foreground">All operators</a>
      {:else}
      <button class="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-accent-foreground" onclick={() => filterStore.clear()}>
        Clear filter
      </button>
      {/if}
    </EmptyState>
  {:else}
    <div class="transition-opacity {loading ? 'opacity-60' : ''}">
      <div class="mb-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
        <StatCard label="K/D" value={s.kd.toFixed(2)} sub="{plural(s.kills, 'kill')} · {plural(s.deaths, 'death')}" />
        <StatCard label="Round win %" value={pct(s.winPct)} sub="{s.roundsWon} of {plural(s.rounds, 'round')}" />
        <StatCard label="Kills / round" value={s.killsPerRound.toFixed(2)} />
        <StatCard label="HS %" value={pct(s.hsPct)} />
        <StatCard label="First kill / death" value="{s.firstKills} / {s.firstDeaths}" sub="opening duels" />
        <StatCard label="KOST" value={pct(s.kostPct)} sub="kill, objective, survive or traded" />
        <StatCard label="Survival" value={pct(s.survivalPct)} />
        <StatCard label="Clutches" value="{s.clutchWon} / {s.clutchWon + s.clutchLost}" sub={s.clutchWon + s.clutchLost > 0 ? pct(s.clutchPct) + ' won' : 'none played'} />
      </div>

      <Section title="Clips" hint="Your kills and deaths with {title}. Click to play">
        <ClipsStrip clips={data?.clips ?? []} />
      </Section>

      <Section title="K/D trend" hint="Rolling over 5 matches with {title}, oldest to newest">
        <Card>
          {#if trend.length < 2}
            <p class="py-6 text-center text-muted">Not enough matches for a trend yet.</p>
          {:else}
            <LineChart points={trend} reference={s.kd} />
          {/if}
        </Card>
      </Section>

      <div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <Section title="Side" hint="Where you played {title}">
          <Card class="p-0">
            <table class="w-full text-left text-sm">
              <thead class="border-b border-border text-xs text-muted">
                <tr>
                  <th class="py-2 pl-3 font-medium">Side</th>
                  <th class="py-2 text-right font-medium">Rounds</th>
                  <th class="py-2 text-right font-medium">Win %</th>
                  <th class="py-2 pr-3 text-right font-medium">K/D</th>
                </tr>
              </thead>
              <tbody class="tabular-nums">
                {#each d.sides as r (r.side)}
                  <tr class="border-b border-border/60 last:border-0">
                    <td class="py-1.5 pl-3 capitalize">{r.side}</td>
                    <td class="py-1.5 text-right">{r.rounds}</td>
                    <td class="py-1.5 text-right">{r.winPct.toFixed(0)}%</td>
                    <td class="py-1.5 pr-3 text-right font-semibold">{r.kd.toFixed(2)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </Card>
        </Section>

        <Section title="Weapons" hint="Kills with each weapon. Click for the weapon screen">
          <Card>
            {#if d.weapons.length === 0}
              <p class="text-sm text-muted">Weapon names are not known for these kills.</p>
            {:else}
              <table class="w-full text-left text-sm">
                <thead class="text-xs text-muted">
                  <tr>
                    <th class="pb-1 font-medium">Weapon</th>
                    <th class="pb-1 text-right font-medium">Kills</th>
                    <th class="pb-1 text-right font-medium">HS %</th>
                  </tr>
                </thead>
                <tbody class="tabular-nums">
                  {#each d.weapons as w (w.key)}
                    <tr>
                      <td class="py-0.5">
                        <a href="/weapons/{encodeURIComponent(w.key)}" class={cn('hover:text-accent', w.unnamed && 'text-muted italic')}>{w.name}</a>
                        {#if w.slot}<span class="ml-1.5 text-xs text-muted">{slotLabel(w.slot)}</span>{/if}
                      </td>
                      <td class="py-0.5 text-right">{w.kills}</td>
                      <td class="py-0.5 text-right">{w.hsPct.toFixed(0)}%</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </Card>
        </Section>
      </div>

      <Section title="Maps" hint="Rounds with {title} per map">
        <div class="grid grid-cols-2 gap-3 lg:grid-cols-3 xl:grid-cols-4">
          {#each d.maps as m (m.key)}
            <div class="overflow-hidden rounded-xl border border-border bg-surface-1">
              <div class="relative">
                <MapThumb mapKey={m.key} name={m.name} class="rounded-none" />
                <div class="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/80 to-transparent p-2">
                  <div class="text-sm font-semibold text-white">{m.name}</div>
                  <div class="text-xs text-white/70 tabular-nums">{plural(m.rounds, 'round')}</div>
                </div>
              </div>
              <div class="flex items-center justify-between p-2.5 text-xs tabular-nums">
                <span class={m.winPct >= 50 ? 'text-good' : 'text-bad'}>Win {m.winPct.toFixed(0)}%</span>
                <span class="text-muted">{m.kills}/{m.deaths}</span>
                <span class="font-semibold">K/D {m.kd.toFixed(2)}</span>
              </div>
            </div>
          {/each}
        </div>
      </Section>

      <div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <Section title="Most killed" hint="Enemy operators {title} took down most">
          <Card><RivalList rows={d.mostKilled} unit="kill" empty="No kills with known enemy operators." /></Card>
        </Section>
        <Section title="Killed by" hint="Enemy operators that took {title} down most">
          <Card><RivalList rows={d.mostKilledBy} unit="death" empty="No deaths to an enemy operator." /></Card>
        </Section>
      </div>

      <Section title="Matches" hint="Matches played with {title}, newest first">
        <div class="flex flex-col gap-2">
          {#each d.matches as m (m.matchId)}
            <a
              href="/matches?match={encodeURIComponent(m.matchId)}"
              title="Open this match"
              class={cn(
                'flex items-center gap-4 rounded-lg border border-l-4 border-border bg-surface-1 py-2 pr-4 pl-2 transition-colors hover:bg-surface-2',
                m.won ? 'border-l-good' : 'border-l-bad'
              )}
            >
              <MapThumb mapKey={m.mapKey} name={m.mapName} class="w-24 shrink-0" />
              <div class="min-w-0 flex-1">
                <div class="truncate font-semibold">{m.mapName}</div>
                <div class="truncate text-xs text-muted">{formatDateTime(m.timestamp)}</div>
              </div>
              <div class="w-20 text-center">
                <div class={cn('text-base font-bold', m.won ? 'text-good' : 'text-bad')}>{m.scoreUs} - {m.scoreThem}</div>
                <div class="text-xs text-muted">{m.won ? 'Win' : 'Loss'}</div>
              </div>
              <div class="w-20 text-center">
                <div class="font-semibold tabular-nums">{m.kills}/{m.deaths}</div>
                <div class="text-xs text-muted">{plural(m.rounds, 'round')}</div>
              </div>
            </a>
          {/each}
        </div>
      </Section>
    </div>
  {/if}
{/if}
