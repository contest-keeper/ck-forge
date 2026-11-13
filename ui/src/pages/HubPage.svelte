<script lang="ts">
  import Field from '../components/Field.svelte';
  import { describeError } from '../core/format';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';

  // The address comes from the profile, where the sticks are told to report.
  let hub = $state(app.profile?.settings.api.url ?? '');
  let user = $state('');
  let password = $state('');
  let busy = $state(false);
  let result = $state('');
  let failure = $state('');

  async function send() {
    if (!app.profilePath) return;
    busy = true;
    result = '';
    failure = '';
    try {
      result = await api.pushRegistry(hub, user, password, app.profilePath);
    } catch (error) {
      failure = describeError(error);
    } finally {
      // The password is used once and not kept.
      password = '';
      busy = false;
    }
  }
</script>

<div class="page">
  <h2>Tell the Hub about the sticks</h2>
  <p>
    The Hub only accepts heartbeats from sticks it knows. This sends the registry next to the
    profile to the Hub with an admin account. Sending it twice is safe. Your password is used for
    this login only and is not saved.
  </p>
  {#if !app.profilePath}<p class="error">Save a profile on the Contest page first.</p>{/if}

  <Field label="Hub address" hint="https://… A password is only sent over https, or to this PC.">
    <input bind:value={hub} placeholder="https://hub.example.org" />
  </Field>
  <Field label="Admin name">
    <input bind:value={user} autocomplete="username" />
  </Field>
  <Field label="Password">
    <input type="password" bind:value={password} autocomplete="current-password" />
  </Field>
  <div>
    <button
      type="button"
      class="primary"
      onclick={send}
      disabled={busy || !app.profilePath || !hub || !user || !password}
    >
      {busy ? 'Sending…' : 'Send the registry'}
    </button>
  </div>
  {#if result}<p class="ok">{result}</p>{/if}
  {#if failure}<p class="error" role="alert">{failure}</p>{/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    max-width: 520px;
  }
  h2 {
    margin: 0;
    font-size: 1.05rem;
  }
  p {
    margin: 0;
  }
  .ok {
    color: var(--ok);
  }
  .error {
    color: var(--danger);
    white-space: pre-wrap;
  }
</style>
