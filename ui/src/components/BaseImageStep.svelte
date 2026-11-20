<script lang="ts">
  import { onMount } from 'svelte';
  import { describeError } from '../core/format';
  import * as api from '../lib/api';
  import { app, setBasePath } from '../state.svelte';
  import BaseImageCreator from './BaseImageCreator.svelte';
  import HubImagePicker from './HubImagePicker.svelte';
  import ProgressBar from './ProgressBar.svelte';

  type Method = 'hub' | 'file' | 'build';
  type Props = { onBaseChanged: () => void };
  let { onBaseChanged }: Props = $props();

  let method = $state<Method | null>(null);
  let archive = $state('');
  let sha256 = $state('');
  let importing = $state(false);
  let failure = $state('');
  let progress = $state<api.Progress | null>(null);

  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
  const signedIn = $derived(app.hubEmail !== null);
  const hashOk = $derived(/^[a-f0-9]{64}$/i.test(sha256.trim()));

  onMount(() => {
    // Only the download and the unpacking belong to this step; the write has its own bar.
    const stop = api.onProgress((p) => {
      if (importing && (p.phase === 'downloading' || p.phase === 'importing')) progress = p;
    });
    return () => void stop.then((unlisten) => unlisten());
  });

  function use(path: string) {
    setBasePath(path);
    archive = '';
    sha256 = '';
    method = null;
    onBaseChanged();
  }

  /** One button for both kinds of file: a raw image is used as it is, a download is checked and
   * unpacked first. */
  async function chooseFile() {
    const path = await api.chooseImageFile();
    if (!path) return;
    failure = '';
    if (path.toLowerCase().endsWith('.zst')) {
      archive = path;
      sha256 = '';
    } else {
      use(path);
    }
  }

  async function run(work: () => Promise<string | null>) {
    app.busy = true;
    importing = true;
    failure = '';
    progress = null;
    try {
      const out = await work();
      if (out) use(out);
    } catch (error) {
      failure = describeError(error);
    } finally {
      app.busy = false;
      importing = false;
      progress = null;
    }
  }

  const importDownload = () =>
    run(async () => {
      const out = await api.chooseBaseToSave();
      if (!out) return null;
      await api.importBaseImage(archive, out, sha256.trim());
      return out;
    });

  const downloadFromHub = (id: string) =>
    run(async () => {
      const out = await api.chooseBaseToSave();
      if (!out) return null;
      await api.downloadHubImage(id, out);
      return out;
    });
</script>

<section class="card">
  <header class="stephead" class:done={app.basePath !== null}>
    <span class="num">1</span>
    <div>
      <h2>Base image</h2>
      <p class="hint">The system every stick is copied from. Choose it once; Forge remembers it.</p>
    </div>
  </header>

  {#if app.basePath && method === null}
    <div class="path">
      <div class="grow">
        <div class="n">{fileName(app.basePath)}</div>
        <div class="p">{app.basePath}</div>
        <div class="p">Keep this file in place until all sticks are written.</div>
      </div>
      <span class="pill ok">Ready</span>
      <button type="button" class="btn sm" onclick={() => (method = 'hub')} disabled={app.busy}>
        Use another image…
      </button>
    </div>
  {:else}
    <div class="choices" role="group" aria-label="Where the base image comes from">
      <button
        type="button"
        class="choice"
        aria-pressed={method === 'hub'}
        disabled={app.busy || !signedIn}
        onclick={() => (method = 'hub')}
      >
        <b>Download from Keeper Hub</b>
        <span
          >{signedIn ? 'The usual way. Forge checks the file.' : 'Sign in to the Hub first.'}</span
        >
      </button>
      <button
        type="button"
        class="choice"
        aria-pressed={method === 'file'}
        disabled={app.busy}
        onclick={() => (method = 'file')}
      >
        <b>Use a file from this PC</b>
        <span>An image you already downloaded or built.</span>
      </button>
      <button
        type="button"
        class="choice"
        aria-pressed={method === 'build'}
        disabled={app.busy}
        onclick={() => (method = 'build')}
      >
        <b>Build one</b>
        <span>Advanced. Takes 15 to 40 minutes.</span>
      </button>
    </div>

    {#if method === 'hub'}
      <div class="inner">
        <HubImagePicker disabled={app.busy} onDownload={downloadFromHub} />
      </div>
    {:else if method === 'file'}
      <div class="inner">
        <p class="hint">
          Choose a raw disk image (<span class="mono">.img</span>), or the compressed download from
          Keeper Hub (<span class="mono">.img.zst</span>). A download is checked against its SHA-256
          and unpacked into a raw image first.
        </p>
        <div class="path">
          <div class="grow">
            <div class="n">{archive ? fileName(archive) : 'No file chosen'}</div>
            {#if archive}<div class="p">{archive}</div>{/if}
          </div>
          <button type="button" class="btn sm" onclick={chooseFile} disabled={app.busy}>
            Choose image file…
          </button>
        </div>
        {#if archive}
          <label class="sha">
            <span class="n">SHA-256 of this download</span>
            <input type="text" bind:value={sha256} spellcheck="false" disabled={app.busy} />
            <span class="hint"
              >Copy the 64 characters shown beside this download on Keeper Hub.</span
            >
          </label>
          <div>
            <button
              type="button"
              class="btn sm pri"
              onclick={importDownload}
              disabled={app.busy || !hashOk}
            >
              Check and unpack…
            </button>
          </div>
        {/if}
      </div>
    {:else if method === 'build'}
      <BaseImageCreator disabled={app.busy} onMade={onBaseChanged} />
    {/if}

    {#if app.basePath && method !== null}
      <div>
        <button type="button" class="btn sm" onclick={() => (method = null)} disabled={app.busy}>
          Keep {fileName(app.basePath)}
        </button>
      </div>
    {/if}
  {/if}

  {#if importing}<ProgressBar {progress} />{/if}
  {#if failure}<p class="note error" role="alert">{failure}</p>{/if}
</section>

<style>
  .sha {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .sha input {
    width: 100%;
    min-height: 40px;
    padding: 0 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--input-bg);
    color: var(--text);
    font-family: var(--mono);
  }
</style>
