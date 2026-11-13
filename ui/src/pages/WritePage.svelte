<script lang="ts">
  import { onMount } from 'svelte';
  import ProgressBar from '../components/ProgressBar.svelte';
  import { describeError, eraseConfirmation } from '../core/format';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';

  let image = $state(app.imagePath ?? '');
  let disks = $state<api.DiskView[]>([]);
  let chosen = $state<number[]>([]);
  let allowVirtual = $state(false);
  let typed = $state('');
  let testFile = $state('');
  let busy = $state(false);
  let progress = $state<api.Progress | null>(null);
  let hash = $state('');
  let written = $state(0);
  let failure = $state('');

  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
  const confirmed = $derived(chosen.length > 0 && typed.trim() === eraseConfirmation(chosen));
  const names = $derived([...chosen].sort((a, b) => a - b).join(', '));

  onMount(() => {
    const stop = api.onProgress((p) => {
      if (busy && p.phase !== 'building') progress = p;
    });
    return () => void stop.then((unlisten) => unlisten());
  });

  async function chooseImage() {
    image = (await api.chooseImageToOpen()) ?? image;
    chosen = [];
    disks = [];
  }

  async function refresh() {
    failure = '';
    chosen = [];
    typed = '';
    try {
      disks = await api.listDisks(image, allowVirtual);
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
    try {
      hash = await api.writeStick(image, target, allowVirtual);
      written = 'disks' in target ? target.disks.length : 1;
      app.written = true;
    } catch (error) {
      failure = describeError(error);
    } finally {
      busy = false;
      progress = null;
      typed = '';
    }
  }
</script>

<div class="page">
  <div class="card">
    <h2>Image</h2>
    <div class="path">
      <div class="grow">
        <div class="n">{image ? fileName(image) : 'none chosen'}</div>
        {#if image}<div class="p">{image}</div>{/if}
      </div>
      <button type="button" class="btn sm" onclick={chooseImage} disabled={busy}>
        {image ? 'Change…' : 'Choose…'}
      </button>
    </div>
  </div>

  <div class="card">
    <h2>Choose the sticks to erase</h2>
    <p class="hint">
      A stick of at least 32 GB is needed. Everything on it is erased. Tick several sticks to write
      them all at the same time.
    </p>
    <div class="row">
      <button type="button" class="btn sm" onclick={refresh} disabled={busy || !image}>
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

  <ProgressBar {progress} />
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
        disabled={busy || !image || !testFile}
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
