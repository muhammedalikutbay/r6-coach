<script lang="ts">
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { Crosshair } from '@lucide/svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Section from '$lib/components/Section.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import StatCard from '$lib/components/StatCard.svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import OperatorBadge from '$lib/components/OperatorBadge.svelte';
  import MapThumb from '$lib/components/MapThumb.svelte';
  import DetailHeader from '$lib/v2/components/details/DetailHeader.svelte';
  import ClipsStrip from '$lib/v2/components/details/ClipsStrip.svelte';
  import { filterStore } from '$lib/v2/filter.svelte';
  import { weaponDetail } from '$lib/v2/api';
  import { refreshTick } from '$lib/v2/refresh.svelte';
  import { formatDateTime, plural, slotLabel } from '$lib/v2/format';
  import { operatorName } from '$lib/v2/catalog';
  import { toast } from '$lib/toast.svelte';
  import type { WeaponScreen } from '$lib/v2/types';

  const key = $derived(decodeURIComponent(page.params.key ?? '').toLowerCase());

  let data = $state<WeaponScreen | null>(null);
  let loading = $state(true);
  let failed = $state(false);
  let seq = 0;

  $effect(() => {
    const k = key;
    const f = $state.snapshot(filterStore.value);
    void refreshTick.value;
    const mine = ++seq;
    loading = true;
    weaponDetail(f, k)
      .then((r) => {
        if (mine !== seq) return;
        data = r;
        failed = false;
      })
      .catch((e) => {
        if (mine !== seq) return;
        console.error('[weapon] detail failed', e);
        failed = true;
        toast('error', 'Could not load this weapon.');
      })
      .finally(() => {
        if (mine === seq) loading = false;
      });
  });

  const d = $derived(data?.detail);
  const timing = $derived(
    d
      ? [
          { label: 'Early', hint: 'first 30 s of the round', n: d.timing.early },
          { label: 'Mid', hint: 'in between', n: d.timing.mid },
          { label: 'Late', hint: 'last 30 s', n: d.timing.late }
        ]
      : []
  );
  const timingTotal = $derived(Math.max(1, d ? d.timing.early + d.timing.mid + d.timing.late : 1));
  const maxMapKills = $derived(Math.max(1, ...(d?.maps ?? []).map((m) => m.kills)));
  const fmtClock = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
</script>

<DetailHeader
  backHref="/weapons"
  backLabel="All weapons"
  title={d?.name || key}
  sub={d && d.kills > 0 ? `${plural(d.kills, 'kill')}${d.slot ? ' · ' + slotLabel(d.slot) : ''}` : ''}
>
  {#snippet chips()}
    {#if (d?.loadoutOperators ?? []).length > 0}
      <div class="flex max-w-md flex-wrap items-center justify-end gap-1" aria-label="Operators that carry {d?.name}">
        {#each d?.loadoutOperators ?? [] as ok (ok)}
          <a
            href="/operators/{encodeURIComponent(ok)}"
            title={operatorName(ok)}
            aria-label={operatorName(ok)}
            class="rounded-full transition-transform hover:scale-110 focus-visible:outline-2 focus-visible:outline-accent"
          ><OperatorBadge opKey={ok} name={operatorName(ok)} size={28} /></a>
        {/each}
      </div>
    {/if}
  {/snippet}
</DetailHeader>

{#if loading && !data}
  <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
    {#each Array(4) as _, i (i)}<Skeleton class="h-24" />{/each}
  </div>
  <Skeleton class="mt-6 h-48" />
{:else if failed && !data}
  <EmptyState title="Could not load data" text="Try changing the filter or press Update." icon={Crosshair} />
{:else if d}
  {#if d.kills === 0}
    <EmptyState title="No kills with this weapon for this filter" text="Loosen the filter to see this weapon's stats." icon={Crosshair}>
      <button class="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-accent-foreground" onclick={() => filterStore.clear()}>
        Clear filter
      </button>
    </EmptyState>
  {:else}
    <div class="transition-opacity {loading ? 'opacity-60' : ''}">
      <div class="mb-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
        <StatCard label="Kills" value={String(d.kills)} />
        <StatCard label="HS %" value="{d.hsPct.toFixed(1)}%" sub={plural(d.headshots, 'headshot')} />
        <StatCard label="Kills / round" value={d.killsPerRound.toFixed(2)} sub="{plural(d.rounds, 'round')} with its operators" />
        <StatCard label="Slot" value={d.slot ? slotLabel(d.slot) : 'Unknown'} />
      </div>

      <Section title="Clips" hint="Your kills with {d.name}. Click to play">
        <ClipsStrip clips={data?.clips ?? []} />
      </Section>

      <div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <Section title="Operators" hint="Who got the kills with it. Click for the operator screen">
          <Card class="p-0">
            <table class="w-full text-left text-sm">
              <thead class="border-b border-border text-xs text-muted">
                <tr>
                  <th class="py-2 pl-3 font-medium">Operator</th>
                  <th class="py-2 text-right font-medium">Kills</th>
                  <th class="py-2 pr-3 text-right font-medium">HS %</th>
                </tr>
              </thead>
              <tbody class="tabular-nums">
                {#each d.operators as o (o.key)}
                  <tr class="border-b border-border/60 last:border-0">
                    <td class="py-1.5 pl-3">
                      <a href="/operators/{encodeURIComponent(o.key)}" class="flex items-center gap-2.5 hover:text-accent">
                        <OperatorBadge opKey={o.key} name={o.name} size={28} />{o.name}
                      </a>
                    </td>
                    <td class="py-1.5 text-right font-semibold">{o.kills}</td>
                    <td class="py-1.5 pr-3 text-right">{o.hsPct.toFixed(0)}%</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </Card>
        </Section>

        <Section title="Timing" hint="When in the round the kills came">
          <Card class="flex flex-col gap-3">
            <div class="flex h-2.5 overflow-hidden rounded-full bg-surface-3">
              {#each timing as t, i (t.label)}
                <div class="h-full {['bg-accent', 'bg-accent/60', 'bg-good'][i]}" style="width:{(t.n / timingTotal) * 100}%" title="{t.label}: {t.n}"></div>
              {/each}
            </div>
            <div class="grid grid-cols-3 gap-2">
              {#each timing as t (t.label)}
                <div>
                  <div class="text-xs text-muted">{t.label}</div>
                  <div class="text-lg font-bold tabular-nums">{t.n}</div>
                  <div class="text-[11px] text-muted">{t.hint}</div>
                </div>
              {/each}
            </div>
          </Card>
        </Section>
      </div>

      <Section title="Maps" hint="Kills with {d.name} per map">
        <Card class="flex flex-col gap-2">
          {#each d.maps as m (m.key)}
            <div class="flex items-center gap-3">
              <MapThumb mapKey={m.key} name={m.name} class="w-20 shrink-0" />
              <span class="w-32 truncate text-sm">{m.name}</span>
              <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-surface-3">
                <div class="h-full rounded-full bg-accent" style="width:{(m.kills / maxMapKills) * 100}%"></div>
              </div>
              <span class="w-16 text-right text-xs text-muted tabular-nums">{plural(m.kills, 'kill')}</span>
            </div>
          {/each}
        </Card>
      </Section>

      <Section title="Latest kills" hint="Most recent first">
        <Card class="p-0">
          <table class="w-full text-left text-sm">
            <thead class="border-b border-border text-xs text-muted">
              <tr>
                <th class="py-2 pl-3 font-medium">When</th>
                <th class="py-2 font-medium">Map</th>
                <th class="py-2 font-medium">Round</th>
                <th class="py-2 font-medium">You</th>
                <th class="py-2 font-medium">Victim</th>
                <th class="py-2 pr-3 text-right font-medium">Clock</th>
              </tr>
            </thead>
            <tbody class="tabular-nums">
              {#each d.recentKills as k (k.matchId + k.round + ':' + k.timeS)}
                <tr
                  class="cursor-pointer border-b border-border/60 transition-colors last:border-0 hover:bg-surface-2"
                  onclick={() => goto('/matches?match=' + encodeURIComponent(k.matchId))}
                >
                  <td class="py-1.5 pl-3 text-muted"><a href="/matches?match={encodeURIComponent(k.matchId)}" class="hover:text-accent" onclick={(e) => e.stopPropagation()}>{formatDateTime(k.timestamp)}</a></td>
                  <td class="py-1.5">{k.mapName}</td>
                  <td class="py-1.5">{k.round}</td>
                  <td class="py-1.5">
                    <span class="inline-flex items-center gap-1.5"><OperatorBadge opKey={k.operatorKey} name={k.operatorName} size={22} />{k.operatorName}</span>
                  </td>
                  <td class="py-1.5">
                    {#if k.victimOperatorKey}
                      <span class="inline-flex items-center gap-1.5"><OperatorBadge opKey={k.victimOperatorKey} name={k.victimOperatorName} size={22} />{k.victimOperatorName}</span>
                    {:else}
                      <span class="text-muted">unknown</span>
                    {/if}
                    {#if k.headshot}<span class="ml-1.5 rounded bg-surface-3 px-1 align-middle text-[11px] font-medium text-accent">HS</span>{/if}
                  </td>
                  <td class="py-1.5 pr-3 text-right">{fmtClock(k.timeS)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </Card>
      </Section>
    </div>
  {/if}
{/if}
