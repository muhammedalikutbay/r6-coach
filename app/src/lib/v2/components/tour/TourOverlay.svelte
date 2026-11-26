<script lang="ts">
  // First-run guided tour: dims the window, spotlights one sidebar link at a time and shows a card
  // next to it. Sits below the custom title bar (--titlebar-h) like the welcome screen.
  import { tick } from 'svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import { tour, closeTour, TOUR_STEPS } from './tour.svelte';

  const PAD = 4; // spotlight padding around the target
  const GAP = 14; // distance between the spotlight and the card

  let root: HTMLDivElement | undefined = $state();
  let card: HTMLDivElement | undefined = $state();
  let nextBtn: HTMLButtonElement | null = $state(null);
  let rect = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  let box = $state({ w: 0, h: 0 });
  let cardH = $state(0);

  const step = $derived(TOUR_STEPS[tour.step]);
  const last = $derived(tour.step === TOUR_STEPS.length - 1);
  const cardW = $derived(Math.min(340, Math.max(220, box.w - 24)));

  /** Where the target is, relative to the overlay (which starts below the title bar). */
  function measure() {
    if (!root) return;
    const o = root.getBoundingClientRect();
    box = { w: o.width, h: o.height };
    const el = step.target ? document.querySelector<HTMLElement>(`[data-tour="${step.target}"]`) : null;
    if (!el) {
      rect = null;
      return;
    }
    el.scrollIntoView({ block: 'nearest' });
    const r = el.getBoundingClientRect();
    rect = { x: r.left - o.left, y: r.top - o.top, w: r.width, h: r.height };
  }

  // Re-measure and move focus into the card whenever the tour opens or the step changes.
  $effect(() => {
    if (!tour.open) return;
    void tour.step;
    tick().then(() => {
      measure();
      nextBtn?.focus();
    });
  });

  const left = $derived(rect ? Math.min(rect.x + rect.w + PAD + GAP, Math.max(8, box.w - cardW - 8)) : Math.max(8, (box.w - cardW) / 2));
  const top = $derived(
    rect ? Math.min(Math.max(8, rect.y + rect.h / 2 - cardH / 2), Math.max(8, box.h - cardH - 8)) : Math.max(8, (box.h - cardH) / 2)
  );
  // The little pointer sits at the height of the target's center, kept inside the card's rounded edge.
  const pointerTop = $derived(rect ? Math.min(Math.max(14, rect.y + rect.h / 2 - top - 6), Math.max(14, cardH - 26)) : 0);

  const next = () => (last ? closeTour() : (tour.step += 1));
  const back = () => {
    if (tour.step > 0) tour.step -= 1;
  };

  function onkeydown(e: KeyboardEvent) {
    if (!tour.open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeTour();
    } else if (e.key === 'Tab' && card) {
      // Keep keyboard focus inside the tour card.
      const items = [...card.querySelectorAll<HTMLElement>('button:not([disabled])')];
      if (items.length === 0) return;
      const first = items[0];
      const lastEl = items[items.length - 1];
      const active = document.activeElement;
      if (!card.contains(active)) {
        e.preventDefault();
        first.focus();
      } else if (e.shiftKey && active === first) {
        e.preventDefault();
        lastEl.focus();
      } else if (!e.shiftKey && active === lastEl) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window {onkeydown} onresize={() => tour.open && measure()} />

{#if tour.open}
  <div bind:this={root} class="fixed inset-x-0 top-[var(--titlebar-h)] bottom-0 z-[110] overflow-hidden">
    {#if rect}
      <div
        class="pointer-events-none absolute rounded-xl ring-2 ring-accent transition-all duration-200"
        style="left:{rect.x - PAD}px;top:{rect.y - PAD}px;width:{rect.w + PAD * 2}px;height:{rect.h + PAD * 2}px;box-shadow:0 0 0 9999px rgb(0 0 0 / 0.72)"
      ></div>
    {:else}
      <div class="absolute inset-0 bg-black/72"></div>
    {/if}

    <div
      bind:this={card}
      bind:clientHeight={cardH}
      role="dialog"
      aria-modal="true"
      aria-labelledby="tour-title"
      class="absolute rounded-2xl border border-border-strong bg-surface-1 p-4 shadow-2xl transition-[top,left] duration-200"
      style="left:{left}px;top:{top}px;width:{cardW}px"
    >
      {#if rect}
        <span
          class="absolute -left-1.5 size-3 rotate-45 border-b border-l border-border-strong bg-surface-1"
          style="top:{pointerTop}px"
          aria-hidden="true"
        ></span>
      {/if}
      <div class="mb-1 flex items-center justify-between gap-2">
        <span class="text-[11px] font-semibold tracking-wider text-accent uppercase" aria-live="polite">Step {tour.step + 1} of {TOUR_STEPS.length}</span>
        <button type="button" class="text-xs text-muted transition-colors hover:text-foreground" onclick={closeTour}>Skip</button>
      </div>
      <h2 id="tour-title" class="text-base font-bold">{step.title}</h2>
      <p class="mt-1 text-sm leading-relaxed text-muted">{step.text}</p>
      <div class="mt-4 flex items-center justify-between gap-2">
        <div class="flex gap-1" aria-hidden="true">
          {#each TOUR_STEPS as _, i (i)}
            <span class="size-1.5 rounded-full {i === tour.step ? 'bg-accent' : 'bg-surface-3'}"></span>
          {/each}
        </div>
        <div class="flex gap-2">
          <Button size="sm" onclick={back} disabled={tour.step === 0}>Back</Button>
          <Button size="sm" variant="primary" bind:ref={nextBtn} onclick={next}>{last ? 'Done' : 'Next'}</Button>
        </div>
      </div>
    </div>
  </div>
{/if}
