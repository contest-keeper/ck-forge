<script lang="ts">
  import { onMount } from 'svelte';
  import ProgressBar from '../components/ProgressBar.svelte';
  import { describeError, eraseConfirmation } from '../core/format';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';

  let image = $state(app.imagePath ?? '');
  let disks = $state<api.DiskView[]>([]);
  let chosen = $state<number | null>(null);
  let allowVirtual = $state(false);
  let typed = $state('');
  let testFile = $state('');
  let busy = $state(false);
  let progress = $state<api.Progress | null>(null);
  let hash = $state('');
  let failure = $state('');

  const confirmed = $derived(chosen !== null && typed.trim() === eraseConfirmation(chosen));

  onMount(() => {
    const stop = api.onProgress((p) => {
      if (busy && p.phase !== 'building') progress = p;
    });
    return () => void stop.then((unlisten) => unlisten());
  });

  async function chooseImage() {
    image = (await api.chooseImageToOpen()) ?? image;
    chosen = null;
    disks = [];
  }

  async function refresh() {
    failure = '';
    chosen = null;
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

  async function write(target: { disk: number } | { file: string }) {
    busy = true;
    failure = '';
    hash = '';
    progress = null;
    try {
      hash = await api.writeStick(image, target, allowVirtual);
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
  <h2>Write a stick</h2>
  <div class="row">
    <button type="button" onclick={chooseImage} disabled={busy}>Stick image…</button>
    <span class="path">{image || 'none chosen'}</span>
  </div>

  <h3>USB stick</h3>
  <p>
    Everything on the chosen stick is erased. Windows asks for administrator permission when the
    write starts. A stick of at least 32 GB is needed.
  </p>
  <div class="row">
    <button type="button" onclick={refresh} disabled={busy || !image}>Find sticks</button>
    <label class="check">
      <input type="checkbox" bind:checked={allowVirtual} disabled={busy} />
      <span>Also show virtual disks (for testing)</span>
    </label>
  </div>

  <ul class="disks">
    {#each disks as disk (disk.number)}
      <li>
        <label class:refused={disk.refused !== null}>
          <input
            type="radio"
            name="disk"
            value={disk.number}
            bind:group={chosen}
            disabled={busy || disk.refused !== null}
          />
          <span>
            Disk {disk.number}: {disk.name || 'unnamed'}, {disk.size}, {disk.bus}
            {#if disk.serial}(serial {disk.serial}){/if}
          </span>
        </label>
        {#if disk.refused}<small>{disk.refused}</small>{/if}
      </li>
    {/each}
  </ul>

  {#if chosen !== null}
    <label class="confirm">
      <span
        >Type <strong>{eraseConfirmation(chosen)}</strong> to erase this disk and write the image:</span
      >
      <input bind:value={typed} autocomplete="off" disabled={busy} />
    </label>
    <div>
      <button
        type="button"
        class="primary"
        onclick={() => write({ disk: chosen as number })}
        disabled={busy || !confirmed}
      >
        {busy ? 'Writing…' : 'Erase and write'}
      </button>
    </div>
  {/if}

  <h3>Test without a stick</h3>
  <p>Writes to a file instead, the same way and with the same check, and needs no permission.</p>
  <div class="row">
    <button type="button" onclick={chooseTestFile} disabled={busy}>Target file…</button>
    <span class="path">{testFile || 'none chosen'}</span>
    <button
      type="button"
      onclick={() => write({ file: testFile })}
      disabled={busy || !image || !testFile}
    >
      Write the file
    </button>
  </div>

  <ProgressBar {progress} />
  {#if failure}<p class="error" role="alert">{failure}</p>{/if}
  {#if hash}
    <p class="ok">Written and read back. The check value (SHA-256) is {hash}.</p>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
  h2,
  h3 {
    margin: 0;
    font-size: 1.05rem;
  }
  h3 {
    margin-top: 12px;
    font-size: 0.95rem;
  }
  p {
    margin: 0;
  }
  .row {
    display: flex;
    gap: var(--gap);
    align-items: center;
    flex-wrap: wrap;
  }
  .check {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .path {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .disks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .disks label {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .refused {
    color: var(--muted);
  }
  small {
    display: block;
    margin-left: 26px;
    color: var(--muted);
  }
  .confirm {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 420px;
  }
  .ok {
    color: var(--ok);
    overflow-wrap: anywhere;
  }
  .error {
    color: var(--danger);
    white-space: pre-wrap;
  }
</style>
