<script lang="ts">
  import { onMount } from 'svelte';
  import { describeError } from '../core/format';
  import * as api from '../lib/api';
  import { setBasePath } from '../state.svelte';

  let { disabled = false, onMade }: { disabled?: boolean; onMade?: () => void } = $props();

  let catalog = $state<api.CatalogItem[]>([]);
  let distro = $state('');
  let out = $state('');
  let patch = $state('');
  let busy = $state(false);
  let step = $state<api.BaseProgress | null>(null);
  let failure = $state('');
  let hash = $state('');

  const fileName = (path: string) => path.split(/[/\\]/).pop() ?? path;
  const labels: Record<api.BaseProgress['step'], string> = {
    download: 'Downloading the official image',
    convert: 'Preparing the disk',
    provision: 'Installing the desktop and contest tools (this takes a while)',
    patch: 'Applying the Keeper OS patch',
    hash: 'Checking the finished image',
  };

  onMount(() => {
    api
      .listCatalog()
      .then((items) => {
        catalog = items;
        distro = items[0]?.id ?? '';
      })
      .catch((error) => (failure = describeError(error)));
    const stop = api.onBaseProgress((p) => {
      if (busy) step = p;
    });
    return () => void stop.then((unlisten) => unlisten());
  });

  async function chooseOut() {
    out = (await api.chooseImageToSave()) ?? out;
  }

  async function choosePatch() {
    patch = (await api.choosePatchToOpen()) ?? patch;
  }

  async function create() {
    busy = true;
    failure = '';
    hash = '';
    step = null;
    try {
      hash = await api.makeBaseImage(distro, out, patch || null);
      setBasePath(out);
      onMade?.();
    } catch (error) {
      failure = describeError(error);
    } finally {
      busy = false;
      step = null;
    }
  }

  const percent = $derived(
    step && step.total > 0 ? Math.round((step.done / step.total) * 100) : null,
  );
</script>

<div class="inner">
  <h3>Build a base image</h3>
  <p class="hint">
    Downloads an official image, installs a desktop and the contest tools in a virtual machine, and
    adds the Keeper OS patch. Needs about 30 GB free, QEMU, and 15 to 40 minutes. The new image is
    then used as the base image. Most admins download one instead.
  </p>
  <div class="path">
    <label class="grow system">
      <span class="n">Which system</span>
      <select bind:value={distro} disabled={busy || disabled}>
        {#each catalog as item (item.id)}
          <option value={item.id}>{item.label}</option>
        {/each}
      </select>
    </label>
  </div>
  <div class="path">
    <div class="grow">
      <div class="n">{out ? fileName(out) : 'Where to save the image'}</div>
      {#if out}<div class="p">{out}</div>{/if}
    </div>
    <button type="button" class="btn sm" onclick={chooseOut} disabled={busy || disabled}>
      Save as…
    </button>
  </div>
  <div class="path">
    <div class="grow">
      <div class="n">{patch ? fileName(patch) : 'Keeper OS patch (ck-os-patch.tar.gz)'}</div>
      <div class="p">
        Without it the image is a plain desktop and cannot report to Keeper Hub. Needs WSL.
      </div>
    </div>
    <button type="button" class="btn sm" onclick={choosePatch} disabled={busy || disabled}>
      Choose…
    </button>
  </div>
  <div class="path">
    <button
      type="button"
      class="btn pri"
      onclick={create}
      disabled={busy || disabled || !distro || !out}
    >
      {busy ? 'Making the image…' : 'Make the base image…'}
    </button>
    <div class="grow" role="status">
      {#if busy && step}
        <label class="hint" for="base-progress">
          {labels[step.step]}{percent !== null ? ` · ${percent}%` : ''}
        </label>
        <progress id="base-progress" max="100" value={percent ?? undefined}></progress>
      {:else if busy}
        <span class="hint">Starting…</span>
      {:else if !out}
        <span class="hint">Choose where to save the image first.</span>
      {:else if !distro}
        <span class="hint">No system is available to build from.</span>
      {/if}
    </div>
  </div>
  {#if failure}<p class="note error" role="alert">{failure}</p>{/if}
  {#if hash}
    <p class="note ok" role="status">
      Base image made and chosen. Its check value (SHA-256) is {hash}.
    </p>
  {/if}
</div>

<style>
  .system select {
    display: block;
    width: 100%;
    max-width: 320px;
    min-height: 40px;
    margin-top: 4px;
    padding: 0 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--input-bg);
    color: var(--text);
  }
  progress {
    display: block;
    width: 100%;
    accent-color: var(--accent);
  }
</style>
