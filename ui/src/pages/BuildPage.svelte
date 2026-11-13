<script lang="ts">
  import { onMount } from 'svelte';
  import ProgressBar from '../components/ProgressBar.svelte';
  import { describeError } from '../core/format';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';

  let base = $state('');
  let out = $state('');
  let busy = $state(false);
  let progress = $state<api.Progress | null>(null);
  let built = $state<api.Built | null>(null);
  let failure = $state('');

  onMount(() => {
    const stop = api.onProgress((p) => {
      if (busy && p.phase === 'building') progress = p;
    });
    return () => void stop.then((unlisten) => unlisten());
  });

  async function chooseBase() {
    base = (await api.chooseImageToOpen()) ?? base;
  }

  async function chooseOut() {
    out = (await api.chooseImageToSave()) ?? out;
  }

  async function build() {
    if (!app.profilePath) return;
    busy = true;
    built = null;
    failure = '';
    progress = null;
    try {
      built = await api.buildImage(base, out, app.profilePath);
      app.imagePath = out;
    } catch (error) {
      failure = describeError(error);
    } finally {
      busy = false;
      progress = null;
    }
  }
</script>

<div class="page">
  <h2>Make a stick image</h2>
  <p>
    Copies the base image and gives the copy its own identity and the settings of the saved profile.
    The base image is only read, never changed. The new stick is added to the registry next to the
    profile.
  </p>

  {#if !app.profilePath}
    <p class="error">Save a profile on the Contest page first.</p>
  {/if}

  <div class="row">
    <button type="button" onclick={chooseBase} disabled={busy}>Base image…</button>
    <span class="path">{base || 'none chosen'}</span>
  </div>
  <div class="row">
    <button type="button" onclick={chooseOut} disabled={busy}>New image…</button>
    <span class="path">{out || 'none chosen'}</span>
  </div>
  <div>
    <button
      type="button"
      class="primary"
      onclick={build}
      disabled={busy || !app.profilePath || !base || !out}
    >
      {busy ? 'Making…' : 'Make the image'}
    </button>
  </div>

  <ProgressBar {progress} />
  {#if failure}<p class="error" role="alert">{failure}</p>{/if}
  {#if built}
    <p class="ok">
      Made {out}. Stick {built.stick_id}, computer name {built.computer_name}, team {built.team}.
      Registered in {built.registry}.
    </p>
    <p>Next: write it to a stick on the Write page, then send the registry on the Hub page.</p>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
  h2 {
    margin: 0;
    font-size: 1.05rem;
  }
  p {
    margin: 0;
  }
  .row {
    display: flex;
    gap: var(--gap);
    align-items: center;
  }
  .path {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .ok {
    color: var(--ok);
  }
  .error {
    color: var(--danger);
    white-space: pre-wrap;
  }
</style>
