<script lang="ts">
  import { onMount } from 'svelte';
  import { describeError } from '../core/format';
  import * as api from '../lib/api';
  import { app, saveProfile } from '../state.svelte';

  let offer = $state<api.UpdateOffer | null>(null);
  let installing = $state(false);
  let failure = $state('');
  /** The profile could not be saved: pressing the button again updates and drops the edits. */
  let unsaved = $state(false);
  let dialog = $state<HTMLDialogElement>();

  // The check can finish before this page opens or while it is open: ask once, then listen.
  onMount(() => {
    const stop = api.onUpdateAvailable((found) => (offer = found));
    // Listening first, then asking: an offer found in between is seen by one of the two.
    stop
      .then(() => api.pendingUpdate())
      .then((found) => (offer ??= found))
      .catch(() => {});
    return () => void stop.then((off) => off());
  });

  // The browser's own modal dialog: it traps focus and closes on Escape, which counts as Later.
  $effect(() => {
    if (!dialog) return;
    if (offer && !dialog.open) dialog.showModal();
    if (!offer && dialog.open) dialog.close();
  });

  function later() {
    if (offer?.mandatory || installing) return;
    offer = null;
    unsaved = false;
    failure = '';
  }

  async function update() {
    if (app.busy) return;
    installing = true;
    failure = '';
    try {
      // The restart would drop what was typed; a profile that cannot be saved asks first.
      if (app.profile && !unsaved) {
        try {
          await saveProfile();
        } catch (error) {
          unsaved = true;
          failure = `${describeError(error)} Updating now loses these edits.`;
          installing = false;
          return;
        }
      }
      await api.installUpdate();
    } catch (error) {
      failure = describeError(error);
      installing = false;
    }
  }
</script>

<dialog
  bind:this={dialog}
  aria-labelledby="update-title"
  oncancel={(event) => {
    event.preventDefault();
    later();
  }}
>
  {#if offer}
    <h2 id="update-title">Keeper Forge {offer.version} is available</h2>
    <p>
      {offer.mandatory
        ? 'This version is no longer supported. Update to continue.'
        : 'A new version is available. Would you like to relaunch with the updated version?'}
    </p>
    <p>
      <button type="button" class="link" onclick={() => api.openReleaseNotes().catch(() => {})}>
        What's new
      </button>
    </p>
    {#if failure}
      <p class="note error" role="alert">{failure}</p>
    {/if}
    <div class="buttons">
      {#if !offer.mandatory}
        <button type="button" class="btn" disabled={installing} onclick={later}>Later</button>
      {/if}
      <button type="button" class="btn pri" disabled={installing || app.busy} onclick={update}>
        {installing ? 'Updating…' : unsaved ? 'Update without saving' : 'Update and restart'}
      </button>
    </div>
  {/if}
</dialog>

<style>
  dialog {
    width: min(460px, calc(100vw - 32px));
    padding: 22px 24px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--panel);
    color: var(--text);
  }
  dialog::backdrop {
    background: rgba(0, 0, 0, 0.45);
  }
  h2 {
    margin: 0 0 10px;
    font-size: 1.05rem;
  }
  p {
    margin: 0 0 10px;
    line-height: 1.45;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 16px;
  }
</style>
