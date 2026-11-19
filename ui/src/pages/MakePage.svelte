<script lang="ts">
  import { onMount } from 'svelte';
  import BaseImageCreator from '../components/BaseImageCreator.svelte';
  import HubImagePicker from '../components/HubImagePicker.svelte';
  import ProgressBar from '../components/ProgressBar.svelte';
  import { describeError, eraseConfirmation } from '../core/format';
  import * as api from '../lib/api';
  import { app, setBasePath } from '../state.svelte';

  let disks = $state<api.DiskView[]>([]);
  let chosen = $state<number[]>([]);
  let allowVirtual = $state(false);
  let typed = $state('');
  let testFile = $state('');
  let archive = $state('');
  let sha256 = $state('');
  let importing = $state(false);
  let importFailure = $state('');
  let wifiPassword = $state('');
  let busy = $state(false);
  let progress = $state<api.Progress | null>(null);
  let hash = $state('');
  let written = $state(0);
  let failure = $state('');

  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
  const ready = $derived(app.profilePath !== null && app.basePath !== null);
  const confirmed = $derived(chosen.length > 0 && typed.trim() === eraseConfirmation(chosen));
  const names = $derived([...chosen].sort((a, b) => a - b).join(', '));

  onMount(() => {
    const stop = api.onProgress((p) => {
      if (busy) progress = p;
    });
    return () => void stop.then((unlisten) => unlisten());
  });

  async function chooseBase() {
    const path = await api.chooseImageToOpen();
    if (!path) return;
    archive = '';
    importFailure = '';
    selectBase(path);
  }

  function selectBase(path: string) {
    setBasePath(path);
    chosen = [];
    disks = [];
    typed = '';
  }

  async function chooseArchive() {
    const path = await api.chooseImageArchive();
    if (!path) return;
    archive = path;
    sha256 = '';
    importFailure = '';
  }

  async function importDownload() {
    const out = await api.chooseBaseToSave();
    if (!out) return;
    busy = true;
    importing = true;
    importFailure = '';
    progress = null;
    try {
      await api.importBaseImage(archive, out, sha256.trim());
      selectBase(out);
      archive = '';
    } catch (error) {
      importFailure = describeError(error);
    } finally {
      busy = false;
      importing = false;
      progress = null;
    }
  }

  async function downloadFromHub(id: string) {
    busy = true;
    importFailure = '';
    progress = null;
    try {
      const out = await api.chooseBaseToSave();
      if (!out) return;
      importing = true;
      await api.downloadHubImage(id, out);
      selectBase(out);
      archive = '';
    } catch (error) {
      importFailure = describeError(error);
    } finally {
      busy = false;
      importing = false;
      progress = null;
    }
  }

  async function refresh() {
    failure = '';
    chosen = [];
    typed = '';
    try {
      disks = await api.listDisks(app.basePath ?? '', allowVirtual);
    } catch (error) {
      failure = describeError(error);
    }
  }

  async function chooseTestFile() {
    testFile = (await api.chooseImageToSave()) ?? testFile;
  }

  async function write(target: { disks: number[] } | { file: string }) {
    busy = true;
    failure = '';
    hash = '';
    progress = null;
    if (!app.profilePath || !app.basePath) return;
    try {
      hash = await api.writeStick(
        app.basePath,
        app.profilePath,
        target,
        allowVirtual,
        wifiPassword,
      );
      written = 'disks' in target ? target.disks.length : 1;
      app.written = true;
    } catch (error) {
      failure = describeError(error);
    } finally {
      busy = false;
      progress = null;
      typed = '';
      wifiPassword = '';
    }
  }
</script>

<div class="page">
  {#if !app.profilePath}
    <p class="note error">Save a profile on the Contest profile page first.</p>
  {/if}
  <div class="card">
    <h2>Base image</h2>
    {#if app.hubEmail}
      <HubImagePicker disabled={busy} onDownload={downloadFromHub} />
    {:else}
      <p class="hint">Sign in to Keeper Hub to download Keeper OS here.</p>
    {/if}
    <div class="path">
      <div class="grow">
        <div class="n">{app.basePath ? fileName(app.basePath) : 'none chosen'}</div>
        {#if app.basePath}
          <div class="p">{app.basePath}</div>
          <div class="p">Keep this file in place until all sticks are written.</div>
        {/if}
      </div>
      <button type="button" class="btn sm" onclick={chooseBase} disabled={busy}>
        Choose raw image…
      </button>
      <button type="button" class="btn sm" onclick={chooseArchive} disabled={busy}>
        Import download…
      </button>
    </div>
    {#if archive}
      <div class="path">
        <label class="grow">
          <span class="n">SHA-256 for {fileName(archive)}</span>
          <input type="text" bind:value={sha256} spellcheck="false" disabled={busy} />
          <span class="p">Copy the 64 characters shown beside this download on Keeper Hub.</span>
        </label>
        <button
          type="button"
          class="btn sm"
          onclick={importDownload}
          disabled={busy || !/^[a-f0-9]{64}$/i.test(sha256.trim())}
        >
          Verify and save raw image…
        </button>
      </div>
    {/if}
    {#if importing}<ProgressBar {progress} />{/if}
    {#if importFailure}<p class="note error" role="alert">{importFailure}</p>{/if}
  </div>

  <BaseImageCreator disabled={busy} />

  <div class="card">
    <h2>Wi-Fi password</h2>
    <p class="hint">
      Only needed when the profile has a Wi-Fi name. It is written on the sticks and nowhere else:
      not in the profile, the Hub or any log. It is cleared after each write.
    </p>
    <input
      type="password"
      autocomplete="off"
      placeholder="Leave empty for an open network"
      bind:value={wifiPassword}
      disabled={busy}
    />
  </div>

  <div class="card">
    <h2>Choose the sticks to erase</h2>
    <p class="hint">
      Forge checks that each stick is large enough for the base image. Everything on the selected
      sticks is erased. You can write several at once, each with its own identity. Windows asks for
      administrator permission when the write starts.
    </p>
    <div class="row">
      <button type="button" class="btn sm" onclick={refresh} disabled={busy || !ready}>
        Find sticks
      </button>
      <label class="check">
        <input type="checkbox" bind:checked={allowVirtual} disabled={busy} />
        <span>Also show virtual disks (for testing)</span>
      </label>
    </div>
    <ul class="disks">
      {#each disks as disk (disk.number)}
        <li>
          <label
            class="disk"
            class:sel={chosen.includes(disk.number)}
            class:off={disk.refused !== null}
          >
            <input
              type="checkbox"
              value={disk.number}
              bind:group={chosen}
              disabled={busy || disk.refused !== null}
            />
            <span class="grow">
              <span class="dn">Disk {disk.number} · {disk.name || 'unnamed'}</span>
              <span class="dm" style="display: block">
                {disk.size} · {disk.bus}{disk.serial ? ` · serial ${disk.serial}` : ''}
                {#if disk.refused}· Not offered: {disk.refused}{/if}
              </span>
            </span>
            {#if disk.refused !== null}
              <span class="pill no">Protected</span>
            {:else}
              <span class="pill ok">Safe to write</span>
            {/if}
          </label>
        </li>
      {/each}
    </ul>
  </div>

  {#if chosen.length > 0}
    <div class="danger">
      <div class="grow">
        <div class="dh">
          Everything on {chosen.length === 1 ? 'disk' : 'disks'}
          {names} will be erased
        </div>
        <div class="hint">
          This cannot be undone. Type <strong>{eraseConfirmation(chosen)}</strong> to confirm.
        </div>
      </div>
      <input
        aria-label="Type {eraseConfirmation(chosen)} to confirm"
        bind:value={typed}
        autocomplete="off"
        disabled={busy}
      />
      <button
        type="button"
        class="btn dng"
        onclick={() => write({ disks: [...chosen] })}
        disabled={busy || !confirmed}
      >
        {busy
          ? 'Writing…'
          : chosen.length > 1
            ? `Erase and write ${chosen.length} sticks`
            : 'Erase and write'}
      </button>
    </div>
  {/if}

  {#if !importing}<ProgressBar {progress} />{/if}
  {#if failure}<p class="note error" role="alert">{failure}</p>{/if}
  {#if hash}
    <p class="note ok">
      Written and read back on {written === 1 ? 'the stick' : `${written} sticks`}. The check value
      (SHA-256) is {hash}.
    </p>
  {/if}

  <div class="card">
    <h2>No stick at hand?</h2>
    <p class="hint">
      Do a dry run into a file instead. Same write, same check, and no permission prompt.
    </p>
    <div class="path">
      <div class="grow">
        <div class="n">{testFile ? fileName(testFile) : 'none chosen'}</div>
        {#if testFile}<div class="p">{testFile}</div>{/if}
      </div>
      <button type="button" class="btn sm" onclick={chooseTestFile} disabled={busy}>
        Target file…
      </button>
      <button
        type="button"
        class="btn sm"
        onclick={() => write({ file: testFile })}
        disabled={busy || !ready || !testFile}
      >
        Write the file
      </button>
    </div>
  </div>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
  .row {
    display: flex;
    gap: var(--gap);
    align-items: center;
    flex-wrap: wrap;
  }
</style>
