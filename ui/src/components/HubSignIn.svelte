<script lang="ts">
  import { onMount } from 'svelte';
  import { describeError } from '../core/format';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';

  // The Hub gives a code for 10 minutes; the program stops asking at the same time.
  const LIFETIME = 600;

  let phase = $state<'idle' | 'asking' | 'waiting' | 'expired' | 'denied' | 'cancelled'>('idle');
  let code = $state('');
  let left = $state(LIFETIME);
  let note = $state('');
  let failure = $state('');
  let timer: ReturnType<typeof setInterval> | undefined;

  const waiting = $derived(phase === 'asking' || phase === 'waiting');
  const clock = $derived(`${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}`);
  const ended = $derived(
    {
      expired: 'This code ran out of time. Get a new code to try again.',
      denied:
        'The Hub page said this was not you, so Forge is not connected. Get a new code to try again.',
      cancelled: 'Cancelled. Forge is not connected.',
    }[phase as 'expired' | 'denied' | 'cancelled'] ?? '',
  );

  // The window may have been reloaded while the session lives on in the program.
  onMount(() => {
    api
      .hubStatus()
      .then((email) => (app.hubEmail = email))
      .catch(() => (app.hubEmail = null));
    const stop = api.onPairingCode((value) => {
      code = value;
      left = LIFETIME;
      phase = 'waiting';
      clearInterval(timer);
      timer = setInterval(() => (left = Math.max(0, left - 1)), 1000);
    });
    return () => {
      clearInterval(timer);
      void stop.then((unlisten) => unlisten());
    };
  });

  async function signIn() {
    phase = 'asking';
    code = '';
    note = '';
    failure = '';
    try {
      const done = await api.hubSignIn(app.profilePath);
      if (done.status === 'approved') {
        app.hubEmail = done.email;
        note = done.registry ? `Registry: ${done.registry}` : '';
        phase = 'idle';
      } else {
        phase = done.status;
      }
    } catch (error) {
      failure = describeError(error);
      phase = 'idle';
    } finally {
      clearInterval(timer);
      code = '';
    }
  }

  async function cancel() {
    await api.hubSignInCancel();
  }

  async function signOut() {
    await api.hubSignOut();
    app.hubEmail = null;
    note = '';
    failure = '';
    phase = 'idle';
  }
</script>

<div class="hub">
  {#if app.hubEmail}
    <span>Hub: signed in as <b>{app.hubEmail}</b></span>
    <button type="button" class="btn sm" onclick={signOut}>Sign out</button>
  {:else if waiting}
    <span>
      {#if code}
        Forge opened the Hub in your browser. Check that it shows this code, then choose "Yes,
        connect Forge".
      {:else}
        Asking the Hub for a code…
      {/if}
    </span>
    {#if code}
      <span class="code" aria-label={`Your code is ${code.split('').join(' ')}`}>{code}</span>
      <span class="left">Time left: {clock}</span>
    {/if}
    <button type="button" class="btn sm" onclick={cancel}>Cancel</button>
  {:else}
    <span>Hub: not signed in. Sticks made now keep their events on the stick.</span>
    <button type="button" class="btn sm" onclick={signIn}>
      {ended ? 'Get a new code' : 'Sign in with browser'}
    </button>
  {/if}
  <!-- Always in the page, so a screen reader announces each change of state. -->
  <span
    class="note"
    class:error={phase === 'expired' || phase === 'denied'}
    role="status"
    aria-live="polite"
  >
    {#if waiting}Waiting for you to agree in the browser…{:else if ended}{ended}{:else if note}{note}{/if}
  </span>
  {#if failure}<span class="note error" role="alert">{failure}</span>{/if}
</div>

<style>
  .hub {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: flex-start;
  }
  .code {
    font-family: var(--mono);
    font-size: 1.6rem;
    letter-spacing: 0.12em;
    padding: 6px 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--soft);
    color: var(--text);
    /* Classic keeps one weight; the code reads by size and spacing alone. */
    font-weight: 400;
  }
  .left {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
</style>
