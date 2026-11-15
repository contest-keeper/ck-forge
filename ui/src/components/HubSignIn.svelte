<script lang="ts">
  import { onMount } from 'svelte';
  import { describeError } from '../core/format';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';

  let busy = $state(false);
  let note = $state('');
  let failure = $state('');

  // The window may have been reloaded while the session lives on in the program.
  onMount(async () => {
    try {
      app.hubEmail = await api.hubStatus();
    } catch {
      app.hubEmail = null;
    }
  });

  async function signIn() {
    busy = true;
    note = 'Finish signing in in your browser…';
    failure = '';
    try {
      const done = await api.hubSignIn(app.profilePath);
      app.hubEmail = done.email;
      note = done.registry ? `Registry: ${done.registry}` : '';
    } catch (error) {
      failure = describeError(error);
      note = '';
    } finally {
      busy = false;
    }
  }

  async function signOut() {
    await api.hubSignOut();
    app.hubEmail = null;
    note = '';
    failure = '';
  }
</script>

<div class="hub">
  {#if app.hubEmail}
    <span>Hub: signed in as <b>{app.hubEmail}</b></span>
    <button type="button" class="btn sm" onclick={signOut}>Sign out</button>
  {:else}
    <span>Hub: not signed in. Sticks made now keep their events on the stick.</span>
    <button type="button" class="btn sm" onclick={signIn} disabled={busy}>
      {busy ? 'Waiting…' : 'Sign in with browser'}
    </button>
  {/if}
  {#if note}<span>{note}</span>{/if}
  {#if failure}<span class="note error" role="alert">{failure}</span>{/if}
</div>

<style>
  .hub {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: flex-start;
  }
</style>
