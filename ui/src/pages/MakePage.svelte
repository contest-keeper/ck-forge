<script lang="ts">
  import { onMount } from 'svelte';
  import BaseImageStep from '../components/BaseImageStep.svelte';
  import ConfirmRetire from '../components/ConfirmRetire.svelte';
  import ProgressBar from '../components/ProgressBar.svelte';
  import RetireOldKeys from '../components/RetireOldKeys.svelte';
  import { describeError, eraseConfirmation, formatDuration, stickLabel } from '../core/format';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';

  let disks = $state<api.DiskView[]>([]);
  let chosen = $state<number[]>([]);
  let allowVirtual = $state(false);
  let typed = $state('');
  let testFile = $state('');
  let wifiPassword = $state('');
  let progress = $state<api.Progress | null>(null);
  let hash = $state('');
  let written = $state(0);
  let failure = $state('');
  let enrolmentSupported = $state(false);
  let firstBootEnrolment = $state(false);
  // Whether the base image has been asked yet, so "cannot enrol" is not said while it is still unknown.
  let enrolmentKnown = $state(false);
  // An image that carries public keys only accepts settings the Hub signed.
  let signatureRequired = $state(false);
  // What the Hub knows about each disk, from the sticks written to it before.
  let earlier = $state<Record<number, api.HubStick>>({});
  let lookupNote = $state('');
  // The sticks that were on the disks just written, kept for retiring their old keys.
  let rewritten = $state<api.HubStick[]>([]);
  let retired = $state<string[]>([]);
  let retireFailure = $state('');
  let asking = $state(false);
  let took = $state('');

  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
  const ready = $derived(app.profilePath !== null && app.basePath !== null);
  const confirmed = $derived(chosen.length > 0 && typed.trim() === eraseConfirmation(chosen));
  const names = $derived([...chosen].sort((a, b) => a - b).join(', '));
  const wifiName = $derived(app.profile?.settings.wifi?.ssid);
  const mustSignIn = $derived(signatureRequired && app.hubEmail === null);
  // The sticks the Hub has from this account on the chosen disks: the ones a write replaces.
  const replaced = $derived(
    chosen.map((number) => earlier[number]).filter((stick) => stick !== undefined),
  );

  onMount(() => {
    // The write is the only thing here that shows its progress; the base image step has its own.
    const stop = api.onProgress((p) => {
      if (app.busy && p.phase !== 'downloading' && p.phase !== 'importing') progress = p;
    });
    return () => void stop.then((unlisten) => unlisten());
  });

  $effect(() => {
    const base = app.basePath;
    const signedIn = app.hubEmail !== null;
    enrolmentSupported = false;
    firstBootEnrolment = false;
    enrolmentKnown = false;
    if (!base || !signedIn) return;
    let current = true;
    void api
      .enrolmentSupported(base)
      .then((supported) => {
        if (!current) return;
        enrolmentSupported = supported;
        // The recommended way is the default whenever the image can do it.
        firstBootEnrolment = supported;
        enrolmentKnown = true;
      })
      .catch(() => {
        /* A base that cannot be checked must use the existing credential flow. */
        if (current) enrolmentKnown = true;
      });
    return () => {
      current = false;
    };
  });

  $effect(() => {
    const base = app.basePath;
    signatureRequired = false;
    if (!base) return;
    let current = true;
    void api
      .signatureRequired(base)
      .then((needed) => {
        if (current) signatureRequired = needed;
      })
      .catch(() => {
        /* An image that cannot be checked here is checked again when the sticks are built. */
      });
    return () => {
      current = false;
    };
  });

  /** A new base image means the sticks found so far were checked against the old one. */
  function baseChanged() {
    chosen = [];
    disks = [];
    typed = '';
  }

  async function lookUpEarlier(found: api.DiskView[]) {
    earlier = {};
    lookupNote = '';
    if (!app.hubEmail || !found.some((disk) => disk.hardware_id !== null)) return;
    try {
      const sticks = await api.hubListSticks();
      const known: Record<number, api.HubStick> = {};
      for (const disk of found) {
        const stick = sticks.find(
          (s) =>
            disk.hardware_id !== null &&
            s.hardware?.id === disk.hardware_id &&
            s.retired_at === null &&
            s.archived_at === null,
        );
        if (stick) known[disk.number] = stick;
      }
      earlier = known;
    } catch (error) {
      lookupNote = `Forge could not check which sticks these disks held before: ${describeError(error)}`;
    }
  }

  async function refresh() {
    failure = '';
    chosen = [];
    typed = '';
    try {
      disks = await api.listDisks(app.basePath ?? '', allowVirtual);
      await lookUpEarlier(disks);
    } catch (error) {
      failure = describeError(error);
    }
  }

  async function chooseTestFile() {
    testFile = (await api.chooseImageToSave()) ?? testFile;
  }

  /** Pressing the write button: when it replaces sticks the Hub knows, ask first. */
  function begin() {
    if (replaced.length > 0) asking = true;
    else void write({ disks: [...chosen] }, false);
  }

  function decided(choice: 'retire' | 'keep' | 'cancel') {
    asking = false;
    if (choice !== 'cancel') void write({ disks: [...chosen] }, choice === 'retire');
  }

  /** Retires the old keys one by one; a key that fails is told, and the others still go. */
  async function retireOld(sticks: api.HubStick[]) {
    const failed: string[] = [];
    for (const stick of sticks) {
      try {
        await api.hubRetireStick(stick.stick_id);
        retired.push(stick.stick_id);
      } catch (error) {
        failed.push(`${stickLabel(stick)}: ${describeError(error)}`);
      }
    }
    if (failed.length > 0) {
      retireFailure = `The new sticks are written, but these old keys are still active:\n${failed.join('\n')}`;
    }
  }

  async function write(target: { disks: number[] } | { file: string }, retireAfter = false) {
    if (!app.profilePath || !app.basePath) return;
    app.busy = true;
    failure = '';
    hash = '';
    took = '';
    retired = [];
    retireFailure = '';
    progress = null;
    const started = Date.now();
    // Taken before the write: afterwards the Hub points at the new stick.
    const before =
      'disks' in target
        ? target.disks.map((number) => earlier[number]).filter((s) => s !== undefined)
        : [];
    try {
      hash = await api.writeStick(
        app.basePath,
        app.profilePath,
        target,
        allowVirtual,
        wifiPassword,
        enrolmentSupported && firstBootEnrolment,
      );
      written = 'disks' in target ? target.disks.length : 1;
      rewritten = before;
      earlier = {};
      app.written = true;
      took = formatDuration((Date.now() - started) / 1000);
      // Only now: the new sticks are written and read back, so the old keys are not needed.
      if (retireAfter) await retireOld(before);
    } catch (error) {
      failure = describeError(error);
    } finally {
      app.busy = false;
      progress = null;
      typed = '';
      wifiPassword = '';
      firstBootEnrolment = enrolmentSupported;
    }
  }
</script>

<div class="page">
  {#if !app.profilePath}
    <p class="note error">Go back to the Contest profile step and continue to save the profile.</p>
  {/if}

  <BaseImageStep onBaseChanged={baseChanged} />

  <section class="card">
    <header class="stephead" class:done={chosen.length > 0}>
      <span class="num">2</span>
      <div>
        <h2>Sticks to write</h2>
        <p class="hint">
          Plug in the sticks, then find them. Forge only offers USB sticks that are big enough for
          the base image, and never the disk Windows runs from.
        </p>
      </div>
    </header>

    {#if wifiName}
      <label class="wifi">
        <span class="n">Wi-Fi password for “{wifiName}”</span>
        <input
          type="password"
          autocomplete="off"
          placeholder="Leave empty for an open network"
          bind:value={wifiPassword}
          disabled={app.busy}
        />
        <span class="hint">
          The sticks join this network by themselves at every boot. The password is written on the
          sticks and nowhere else: not in the profile, the Hub or any log. It is cleared after each
          write.
        </span>
      </label>
    {/if}

    {#if mustSignIn}
      <p class="note error">
        This image only accepts settings the Hub signed, so you have to be signed in to the Hub to
        write sticks from it. Sign in at the bottom left.
      </p>
    {:else if signatureRequired}
      <p class="hint">
        This image only accepts settings the Hub signed. Forge gets each stick's settings signed
        when it writes, so edits made to the CKCONFIG drive on another PC are refused by the stick.
      </p>
    {/if}

    {#if enrolmentSupported}
      <label class="check">
        <input type="checkbox" bind:checked={firstBootEnrolment} disabled={app.busy} />
        <span><b>Create each stick's key at its first boot</b> (recommended)</span>
      </label>
      {#if firstBootEnrolment}
        <p class="hint">
          The key is made on the stick and is never written to the CKCONFIG drive, which any PC can
          read. Boot each stick once, online (cable, or the Wi-Fi in the profile), within 24 hours
          of writing it. After that the code on the stick is useless.
        </p>
      {:else}
        <p class="note error">
          Without this, the stick's secret key is written in the settings file on the CKCONFIG
          drive. Anyone with the stick can read it on a PC and copy it.
        </p>
      {/if}
    {:else if app.hubEmail === null}
      <p class="note error">
        Not signed in to the Hub, so the sticks cannot create their key at first boot. Each stick's
        secret key is written in the settings file on the CKCONFIG drive, where anyone with the
        stick can read it. Sign in to the Hub to avoid that.
      </p>
    {:else if enrolmentKnown && app.basePath}
      <p class="note error">
        This base image cannot create keys at first boot, so each stick's secret key is written in
        the settings file on the CKCONFIG drive. A newer Keeper OS image can.
      </p>
    {/if}

    <div class="row">
      <button type="button" class="btn sm" onclick={refresh} disabled={app.busy || !ready}>
        Find sticks
      </button>
      <label class="check">
        <input type="checkbox" bind:checked={allowVirtual} disabled={app.busy} />
        <span>Also show virtual disks (for testing)</span>
      </label>
    </div>
    {#if !ready}
      <p class="hint">
        Choose a base image first, so Forge can check that the sticks are big enough.
      </p>
    {/if}
    {#if lookupNote}<p class="hint">{lookupNote}</p>{/if}

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
              disabled={app.busy || disk.refused !== null}
            />
            <span class="grow">
              <span class="dn">Disk {disk.number} · {disk.name || 'unnamed'}</span>
              <span class="dm" style="display: block">
                {disk.size} · {disk.bus}{disk.serial ? ` · serial ${disk.serial}` : ''}
                {#if disk.refused}· Not offered: {disk.refused}{/if}
              </span>
              {#if earlier[disk.number]}
                <span class="dm" style="display: block">
                  Written before as {stickLabel(earlier[disk.number] as api.HubStick)}
                </span>
              {/if}
            </span>
            {#if disk.refused !== null}
              <span class="pill no">Protected</span>
            {:else if earlier[disk.number]}
              <span class="pill warn">Rewrite</span>
            {:else}
              <span class="pill ok">Safe to write</span>
            {/if}
          </label>
        </li>
      {/each}
    </ul>
  </section>

  <section class="card">
    <header class="stephead" class:done={hash !== ''}>
      <span class="num">3</span>
      <div>
        <h2>Erase and write</h2>
        <p class="hint">
          Everything on the chosen sticks is erased. Each stick gets its own identity from the saved
          profile. Windows asks for administrator permission when the write starts.
        </p>
      </div>
    </header>

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
          disabled={app.busy}
        />
        <button
          type="button"
          class="btn dng"
          onclick={begin}
          disabled={app.busy || !confirmed || mustSignIn}
        >
          {app.busy
            ? 'Writing…'
            : chosen.length > 1
              ? `Erase and write ${chosen.length} sticks`
              : 'Erase and write'}
        </button>
      </div>
    {:else}
      <p class="hint">Choose at least one stick above.</p>
    {/if}

    <ProgressBar {progress} />
    {#if failure}<p class="note error" role="alert">{failure}</p>{/if}
    {#if hash}
      <p class="note ok">
        Written and read back on {written === 1 ? 'the stick' : `${written} sticks`} in {took}. The
        check value (SHA-256) is {hash}.
      </p>
    {/if}
  </section>

  {#if retireFailure}<p class="note error" role="alert">{retireFailure}</p>{/if}
  <RetireOldKeys previous={rewritten} bind:retired />

  <ConfirmRetire open={asking} sticks={replaced} onChoose={decided} />

  <details class="card">
    <summary>No stick at hand? Test the write into a file</summary>
    <div class="inner">
      <p class="hint">
        A dry run: the same write and the same check into a file, with no permission prompt.
      </p>
      <div class="path">
        <div class="grow">
          <div class="n">{testFile ? fileName(testFile) : 'none chosen'}</div>
          {#if testFile}<div class="p">{testFile}</div>{/if}
        </div>
        <button type="button" class="btn sm" onclick={chooseTestFile} disabled={app.busy}>
          Target file…
        </button>
        <button
          type="button"
          class="btn sm"
          onclick={() => write({ file: testFile })}
          disabled={app.busy || !ready || !testFile || mustSignIn}
        >
          Write the file
        </button>
      </div>
    </div>
  </details>
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
  .wifi {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 520px;
  }
  .wifi input {
    min-height: 40px;
    padding: 0 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--input-bg);
    color: var(--text);
  }
  .n {
    font-weight: 600;
  }
</style>
