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

  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

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
      app.basePath = base;
    } catch (error) {
      failure = describeError(error);
    } finally {
      busy = false;
      progress = null;
    }
  }
</script>

<div class="page">
  {#if !app.profilePath}
    <p class="note error">Save a profile on the Contest page first.</p>
  {/if}
  <div class="card">
    <h2>Files</h2>
    <div class="lbl">Base image</div>
    <div class="path">
      <div class="grow">
        <div class="n">{base ? fileName(base) : 'none chosen'}</div>
        {#if base}<div class="p">{base}</div>{/if}
      </div>
      <button type="button" class="btn sm" onclick={chooseBase} disabled={busy}>
        {base ? 'Change…' : 'Choose…'}
      </button>
    </div>
    <div class="lbl">New image</div>
    <div class="path">
      <div class="grow">
        <div class="n">{out ? fileName(out) : 'none chosen'}</div>
        {#if out}<div class="p">{out}</div>{/if}
      </div>
      <button type="button" class="btn sm" onclick={chooseOut} disabled={busy}>
        {out ? 'Change…' : 'Choose…'}
      </button>
    </div>
  </div>

  <ProgressBar {progress} />
  {#if failure}<p class="note error" role="alert">{failure}</p>{/if}
  {#if built}
    <div class="card">
      <h2>Made {fileName(out)}</h2>
      <dl class="sum">
        <dt>Stick</dt>
        <dd class="mono">{built.stick_id}</dd>
        <dt>Computer name</dt>
        <dd class="mono">{built.computer_name}</dd>
        <dt>Team</dt>
        <dd>{built.team}</dd>
        <dt>Registered in</dt>
        <dd class="mono">{built.registry}</dd>
      </dl>
      <p class="hint">Next: write it to a stick, then send the registry to the Hub.</p>
    </div>
  {/if}
  <div>
    <button
      type="button"
      class="btn pri"
      onclick={build}
      disabled={busy || !app.profilePath || !base || !out}
    >
      {busy ? 'Making…' : 'Make the image'}
    </button>
  </div>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
</style>
