<script lang="ts">
  // Date range + hour-of-day popover for the global filter bar. Presets resolve to fixed dates when clicked.
  import { Calendar, ChevronDown } from '@lucide/svelte';
  import { filterStore as fs } from '../filter.svelte';
  import { cn } from '$lib/utils';

  let open = $state(false);
  let custom = $state(false);
  let root: HTMLDivElement | undefined = $state();

  const v = $derived(fs.value);

  const pad = (n: number) => String(n).padStart(2, '0');
  const iso = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const daysAgo = (n: number) => {
    const d = new Date();
    d.setDate(d.getDate() - n);
    return iso(d);
  };

  const presets: { key: string; label: string; range: () => [string, string] }[] = [
    { key: 'today', label: 'Today', range: () => [daysAgo(0), daysAgo(0)] },
    { key: 'yesterday', label: 'Yesterday', range: () => [daysAgo(1), daysAgo(1)] },
    { key: 'week', label: 'Last 7 days', range: () => [daysAgo(6), daysAgo(0)] },
    { key: 'month', label: 'Last 30 days', range: () => [daysAgo(29), daysAgo(0)] }
  ];

  const presetKey = $derived(
    presets.find((p) => {
      const [a, b] = p.range();
      return v.dateFrom === a && v.dateTo === b;
    })?.key ?? null
  );
  const hasDates = $derived(v.dateFrom !== null || v.dateTo !== null);
  const showCustom = $derived(custom || (hasDates && presetKey === null));

  const hours = Array.from({ length: 24 }, (_, h) => h);
  const hourLabel = (h: number) => `${pad(h)}:00`;

  function pickPreset(p: (typeof presets)[number]) {
    const [dateFrom, dateTo] = p.range();
    custom = false;
    fs.set({ dateFrom, dateTo });
  }

  function setHour(which: 'hourFrom' | 'hourTo', raw: string) {
    fs.set({ [which]: raw === '' ? null : Number(raw) });
  }

  function clearAll() {
    custom = false;
    fs.set({ dateFrom: null, dateTo: null, hourFrom: null, hourTo: null });
  }

  function onWindowClick(e: MouseEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === 'Escape' && (open = false)} />

<div class="relative shrink-0" bind:this={root}>
  <button
    type="button"
    class={cn(
      'inline-flex h-8 items-center gap-1.5 whitespace-nowrap rounded-lg border border-border bg-surface-2 px-2.5 text-xs font-medium transition hover:bg-surface-3',
      fs.dateActive && 'border-accent/60 text-accent'
    )}
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <Calendar class="size-3.5" />Date
    <ChevronDown class="size-3.5" />
  </button>
  {#if open}
    <div class="absolute left-0 top-full z-30 mt-1 w-72 rounded-xl border border-border bg-surface-1 p-3 shadow-xl">
      <div class="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted">Date</div>
      <div class="flex flex-wrap gap-1.5">
        {#each presets as p (p.key)}
          <button
            type="button"
            aria-pressed={presetKey === p.key}
            onclick={() => pickPreset(p)}
            class={cn('h-7 rounded-md border border-border px-2 text-xs transition hover:bg-surface-3', presetKey === p.key && 'border-accent bg-accent text-black hover:bg-accent')}
          >{p.label}</button>
        {/each}
        <button
          type="button"
          aria-pressed={showCustom}
          onclick={() => (custom = true)}
          class={cn('h-7 rounded-md border border-border px-2 text-xs transition hover:bg-surface-3', showCustom && 'border-accent bg-accent text-black hover:bg-accent')}
        >Custom</button>
      </div>
      {#if showCustom}
        <div class="mt-2 grid grid-cols-2 gap-2 text-xs">
          <label class="flex flex-col gap-1 text-muted">From
            <input type="date" value={v.dateFrom ?? ''} max={v.dateTo ?? undefined} onchange={(e) => fs.set({ dateFrom: e.currentTarget.value || null })} class="h-8 rounded-lg border border-border bg-surface-2 px-2 text-foreground outline-none focus:border-accent" />
          </label>
          <label class="flex flex-col gap-1 text-muted">To
            <input type="date" value={v.dateTo ?? ''} min={v.dateFrom ?? undefined} onchange={(e) => fs.set({ dateTo: e.currentTarget.value || null })} class="h-8 rounded-lg border border-border bg-surface-2 px-2 text-foreground outline-none focus:border-accent" />
          </label>
        </div>
      {/if}

      <div class="mb-1.5 mt-3 text-[10px] font-semibold uppercase tracking-wider text-muted">Hour of day</div>
      <div class="grid grid-cols-2 gap-2 text-xs">
        <label class="flex flex-col gap-1 text-muted">From
          <select value={v.hourFrom ?? ''} onchange={(e) => setHour('hourFrom', e.currentTarget.value)} class="h-8 rounded-lg border border-border bg-surface-2 px-2 text-foreground outline-none focus:border-accent">
            <option value="">Any</option>
            {#each hours as h (h)}<option value={h}>{hourLabel(h)}</option>{/each}
          </select>
        </label>
        <label class="flex flex-col gap-1 text-muted">To
          <select value={v.hourTo ?? ''} onchange={(e) => setHour('hourTo', e.currentTarget.value)} class="h-8 rounded-lg border border-border bg-surface-2 px-2 text-foreground outline-none focus:border-accent">
            <option value="">Any</option>
            {#each hours as h (h)}<option value={h}>{hourLabel(h)}</option>{/each}
          </select>
        </label>
      </div>
      <p class="mt-1.5 text-[11px] text-muted">A start later than the end wraps past midnight (22:00 to 02:00).</p>

      {#if fs.dateActive}
        <button type="button" class="mt-2 h-7 text-xs text-muted hover:text-foreground" onclick={clearAll}>Clear date and hours</button>
      {/if}
    </div>
  {/if}
</div>
