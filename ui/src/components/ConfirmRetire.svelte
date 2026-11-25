<script lang="ts">
  import { stickLabel } from '../core/format';
  import type { HubStick } from '../lib/api';

  type Choice = 'retire' | 'keep' | 'cancel';
  type Props = { open: boolean; sticks: HubStick[]; onChoose: (choice: Choice) => void };
  let { open, sticks, onChoose }: Props = $props();

  let dialog = $state<HTMLDialogElement>();

  // The browser's own modal dialog: it traps focus and closes on Escape, which counts as Cancel.
  // Focus goes back to the button that opened it; the browser does not always do that itself.
  let invoker: Element | null = null;
  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      invoker = document.activeElement;
      dialog.showModal();
    }
    if (!open && dialog.open) {
      dialog.close();
      if (invoker instanceof HTMLElement) invoker.focus();
    }
  });
</script>

<dialog
  bind:this={dialog}
  aria-labelledby="retire-title"
  oncancel={(event) => {
    event.preventDefault();
    onChoose('cancel');
  }}
>
  <h2 id="retire-title">
    {sticks.length === 1 ? 'This stick was written before' : 'These sticks were written before'}
  </h2>
  <p>
    The Hub has {sticks.length === 1 ? 'a stick' : 'sticks'} from your account on
    {sticks.length === 1 ? 'the disk' : 'these disks'} you are about to erase:
  </p>
  <ul>
    {#each sticks as stick (stick.stick_id)}
      <li>{stickLabel(stick)}</li>
    {/each}
  </ul>
  <p class="hint">
    Retiring ends the old key at the Hub for good: that stick can no longer report, and its history
    stays visible. Forge does it only after the new {sticks.length === 1
      ? 'stick is'
      : 'sticks are'}
    written and checked. A failed write retires nothing.
  </p>
  <div class="buttons">
    <button type="button" class="btn" onclick={() => onChoose('cancel')}>Cancel</button>
    <button type="button" class="btn" onclick={() => onChoose('keep')}>
      Write and keep the old {sticks.length === 1 ? 'key' : 'keys'}
    </button>
    <button type="button" class="btn dng" onclick={() => onChoose('retire')}>
      Write and retire the old {sticks.length === 1 ? 'key' : 'keys'}
    </button>
  </div>
</dialog>

<style>
  dialog {
    width: min(660px, calc(100vw - 32px));
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
  ul {
    margin: 0 0 12px;
    padding-left: 20px;
    font-weight: 600;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 16px;
  }
</style>
