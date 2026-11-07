<script lang="ts">
  import { onMount } from 'svelte';
  import Card from '$lib/components/ui/Card.svelte';
  import Button from '$lib/components/ui/Button.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import { toast } from '$lib/toast.svelte';
  import { ubiStatus, ubiLogin, ubiLogin2fa, ubiLogout } from '../ubisoft';
  import type { UbiLoginResult, UbiStatus } from '../types';

  let status = $state<UbiStatus | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let step = $state<'credentials' | '2fa'>('credentials');
  let email = $state('');
  let password = $state('');
  let code = $state('');
  let error = $state<string | null>(null);

  const inputCls =
    'h-9 w-full rounded-lg border border-border-strong bg-surface-2 px-3 text-sm outline-none focus:border-accent';

  onMount(async () => {
    try {
      status = await ubiStatus();
    } catch {
      error = 'Could not read the sign-in state.';
    } finally {
      loading = false;
    }
  });

  function errorText(e: unknown): string {
    if (typeof e === 'string') return e;
    if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message);
    return 'Something went wrong. Try again.';
  }

  /** Applies a login result. The password and code are cleared in every case. */
  function handle(r: UbiLoginResult) {
    password = '';
    code = '';
    if (r.status === 'ok') {
      status = r.user;
      step = 'credentials';
      error = null;
      toast('success', 'Signed in to Ubisoft.');
    } else if (r.status === 'needs2fa') {
      step = '2fa';
      error = r.message;
    } else {
      error = r.message ?? 'Sign-in failed.';
    }
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    const pw = password;
    password = ''; // cleared right after submit, whatever happens
    try {
      handle(await ubiLogin(email.trim(), pw));
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function submit2fa(e: SubmitEvent) {
    e.preventDefault();
    if (busy) return;
    busy = true;
    error = null;
    const c = code.trim();
    code = '';
    try {
      handle(await ubiLogin2fa(c));
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function signOut() {
    busy = true;
    try {
      await ubiLogout();
      status = { signedIn: false, username: null, profileId: null, matchesReplayPlayer: null };
      step = 'credentials';
      toast('success', 'Signed out.');
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  function cancel2fa() {
    step = 'credentials';
    error = null;
    code = '';
  }
</script>

<Card>
  <h3 class="font-semibold">Ubisoft account</h3>
  <p class="mt-1 mb-3 text-muted">
    Sign in to show your rank and official stats. Your password is sent only to Ubisoft and never stored.
    The app keeps a sign-in ticket in Windows Credential Manager.
  </p>

  {#if loading}
    <Skeleton class="h-9" />
  {:else if status?.signedIn}
    <p class="mb-3">
      Signed in as <span class="font-semibold">{status.username ?? 'unknown'}</span>.
    </p>
    {#if status.matchesReplayPlayer === false}
      <p class="mb-3 text-accent">This account differs from the player in your replays.</p>
    {/if}
    {#if error}<p class="mb-3 text-bad" role="alert">{error}</p>{/if}
    <Button onclick={signOut} disabled={busy}>Sign out</Button>
  {:else if step === '2fa'}
    <form onsubmit={submit2fa} class="flex flex-col gap-3">
      <label class="flex flex-col gap-1 text-xs text-muted">
        Two-factor code
        <input
          class={inputCls}
          type="text"
          inputmode="numeric"
          autocomplete="one-time-code"
          maxlength="8"
          bind:value={code}
          disabled={busy}
          required
        />
      </label>
      {#if error}<p class="text-bad" role="alert">{error}</p>{/if}
      <div class="flex gap-2">
        <Button type="submit" variant="primary" disabled={busy || !code.trim()}>{busy ? 'Verifying...' : 'Verify'}</Button>
        <Button type="button" onclick={cancel2fa} disabled={busy}>Back</Button>
      </div>
    </form>
  {:else}
    <form onsubmit={submit} class="flex flex-col gap-3">
      <label class="flex flex-col gap-1 text-xs text-muted">
        Email
        <input class={inputCls} type="email" autocomplete="username" bind:value={email} disabled={busy} required />
      </label>
      <label class="flex flex-col gap-1 text-xs text-muted">
        Password
        <input class={inputCls} type="password" autocomplete="current-password" bind:value={password} disabled={busy} required />
      </label>
      {#if error}<p class="text-bad" role="alert">{error}</p>{/if}
      <div>
        <Button type="submit" variant="primary" disabled={busy || !email.trim() || !password}>
          {busy ? 'Signing in...' : 'Sign in'}
        </Button>
      </div>
    </form>
  {/if}
</Card>
