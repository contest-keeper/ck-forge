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
      app.registered = true;
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
  {#if !app.profilePath}<p class="note error">Save a profile on the Contest page first.</p>{/if}
  <div class="card">
    <h2>Hub login</h2>
    <Field label="Hub address" hint="A password is only sent over https, or to this PC.">
      <input bind:value={hub} placeholder="https://hub.example.org" />
    </Field>
    <div class="two">
      <Field label="Admin name">
        <input bind:value={user} autocomplete="username" />
      </Field>
      <Field label="Password">
        <input type="password" bind:value={password} autocomplete="current-password" />
      </Field>
    </div>
    <p class="lock">
      Your password is used for this login only and is not saved. Sending the registry twice is
      safe.
    </p>
  </div>
  <div>
    <button
      type="button"
      class="btn pri"
      onclick={send}
      disabled={busy || !app.profilePath || !hub || !user || !password}
    >
      {busy ? 'Sending…' : 'Send the registry'}
    </button>
  </div>
  {#if result}<p class="note ok">{result}</p>{/if}
  {#if failure}<p class="note error" role="alert">{failure}</p>{/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    max-width: 620px;
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: var(--gap);
  }
</style>
