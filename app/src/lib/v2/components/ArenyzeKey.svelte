<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import { toast } from '$lib/toast.svelte';
  import {
    arenyzeStatus,
    arenyzeSetKey,
    arenyzeClearKey,
    arenyzeSaveLastResponse,
    arenyzeOpenKeyPage
  } from '../arenyze';
  import type { ArenyzeStatus } from '../types';

  let status = $state<ArenyzeStatus | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let key = $state('');
  let error = $state<string | null>(null);

  const inputCls =
    'h-9 w-full rounded-lg border border-border-strong bg-surface-2 px-3 text-sm outline-none focus:border-accent';

  onMount(async () => {
    try {
      status = await arenyzeStatus();
    } catch {
      error = 'Could not read the key state.';
    } finally {
      loading = false;
    }
  });

  function errorText(e: unknown): string {
    if (typeof e === 'string') return e;
    if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message);
    return 'Something went wrong. Try again.';
  }

  /** The key is cleared right after submit, whatever happens. */
  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const value = key.trim();
    if (busy || !value) return;
    busy = true;
    error = null;
    key = '';
    try {
      status = await arenyzeSetKey(value);
      toast('success', 'Arenyze key saved.');
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function removeKey() {
    busy = true;
    error = null;
    try {
      status = await arenyzeClearKey();
      toast('success', 'Key removed.');
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function saveResponse() {
    busy = true;
    error = null;
    try {
      const path = await arenyzeSaveLastResponse();
      if (path) toast('success', 'Saved the last Arenyze response.');
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function openKeyPage() {
    try {
      await arenyzeOpenKeyPage();
    } catch (err) {
      error = errorText(err);
    }
  }

  const quotaText = $derived.by(() => {
    const q = status?.quota;
    if (!q) return null;
    if (q.remaining !== null && q.limit !== null) {
      return `${q.remaining.toLocaleString('en-GB')} of ${q.limit.toLocaleString('en-GB')} calls left this month${q.plan ? ` (${q.plan} plan)` : ''}`;
    }
    return q.plan ? `Plan: ${q.plan}` : null;
  });
</script>

<Card>
  <h3 class="font-semibold">Online stats (Arenyze)</h3>
  <p class="mt-1 mb-3 text-muted">
    Your key stays on this PC (Windows Credential Manager). The app only requests your own profile.
  </p>

  {#if loading}
    <Skeleton class="h-9" />
  {:else if status?.hasKey}
    <p class="mb-1">
      Key saved.
      {#if status.username}Showing stats for <span class="font-semibold">{status.username}</span>.{:else}Import a match so the app knows your Ubisoft name.{/if}
    </p>
    {#if quotaText}<p class="mb-3 text-xs text-muted">{quotaText}</p>{/if}
    {#if error}<p class="mb-3 text-bad" role="alert">{error}</p>{/if}
    <div class="flex flex-wrap gap-2">
      <Button onclick={removeKey} disabled={busy} variant="danger">Remove key</Button>
      <Button onclick={saveResponse} disabled={busy} title="Saves the last raw Arenyze answers (without your key) so they can be checked">
        Save last online response
      </Button>
    </div>
  {:else}
    <form onsubmit={submit} class="flex flex-col gap-3">
      <label class="flex flex-col gap-1 text-xs text-muted">
        Arenyze API key
        <input
          class={inputCls}
          type="password"
          autocomplete="off"
          spellcheck="false"
          bind:value={key}
          disabled={busy}
          placeholder="Paste your key"
        />
      </label>
      {#if error}<p class="text-bad" role="alert">{error}</p>{/if}
      <div class="flex flex-wrap items-center gap-2">
        <Button type="submit" variant="primary" disabled={busy || !key.trim()}>{busy ? 'Checking...' : 'Verify and save'}</Button>
        <Button type="button" onclick={openKeyPage} disabled={busy}>Get a free key</Button>
      </div>
      <p class="text-xs text-muted">
        Free plan: 2,000 calls per month. The app uses a few per refresh and never calls in the background.
      </p>
    </form>
  {/if}
</Card>
