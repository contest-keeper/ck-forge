<script lang="ts">
  import { tick } from 'svelte';
  import { look, setMode, setTheme } from '../theme/theme.svelte';

  let open = $state(false);
  let opener = $state<HTMLButtonElement>();
  let panel = $state<HTMLDivElement>();

  async function show() {
    open = true;
    await tick();
    panel?.querySelector<HTMLElement>('button[aria-pressed="true"]')?.focus();
  }

  function close() {
    open = false;
    opener?.focus();
  }

  // Escape closes the panel from anywhere; Tab stays inside it while it is open.
  function keys(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== 'Tab' || !panel) return;
    const stops = [...panel.querySelectorAll<HTMLElement>('button')];
    const first = stops[0];
    const last = stops[stops.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  }
</script>

<svelte:window onkeydown={keys} />

<div class="wrap">
  <button
    type="button"
    class="btn sm opener"
    aria-haspopup="dialog"
    aria-expanded={open}
    bind:this={opener}
    onclick={() => (open ? close() : show())}
  >
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      ><circle cx="12" cy="12" r="3" /><path
        d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M4.9 19.1 7 17M17 7l2.1-2.1"
      /></svg
    >
    Settings
  </button>
  {#if open}
    <div class="panel" role="dialog" aria-label="Appearance settings" bind:this={panel}>
      <div class="top">
        <h2>Appearance</h2>
        <button type="button" class="btn sm" onclick={close}>Done</button>
      </div>
      <div class="group" role="group" aria-labelledby="set-look">
        <h3 id="set-look">Look</h3>
        <div class="opts">
          <button
            type="button"
            class="opt"
            aria-pressed={look.theme === 'modern'}
            onclick={() => setTheme('modern')}
          >
            <span class="prev m"><i></i><i></i></span>
            <b>New</b>
            <span class="sub">Clean and modern</span>
          </button>
          <button
            type="button"
            class="opt"
            aria-pressed={look.theme === 'classic'}
            onclick={() => setTheme('classic')}
          >
            <span class="prev c"><i></i><i></i></span>
            <b>Classic</b>
            <span class="sub">Like the browser extension</span>
          </button>
        </div>
      </div>
      <div class="group" role="group" aria-labelledby="set-theme">
        <h3 id="set-theme">Theme</h3>
        <div class="opts">
          <button
            type="button"
            class="opt"
            aria-pressed={look.mode === 'light'}
            onclick={() => setMode('light')}
          >
            <b>Light</b>
            <span class="sub">Bright background</span>
          </button>
          <button
            type="button"
            class="opt"
            aria-pressed={look.mode === 'dark'}
            onclick={() => setMode('dark')}
          >
            <b>Dark</b>
            <span class="sub">Easy on the eyes</span>
          </button>
        </div>
      </div>
      <p class="hint">Saved on this computer.</p>
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
  }
  .opener {
    width: 100%;
    justify-content: flex-start;
  }
  .panel {
    position: absolute;
    z-index: 30;
    left: 0;
    bottom: calc(100% + 8px);
    width: 320px;
    max-width: calc(100vw - 32px);
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px;
    border-radius: var(--radius-lg);
    background: var(--panel);
    color: var(--text);
    box-shadow:
      0 18px 48px rgba(0, 0, 0, 0.3),
      inset 0 0 0 1px var(--line);
  }
  :global([data-theme='classic']) .panel {
    padding: 12px;
    box-shadow: var(--bevel-raised);
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .top h2 {
    font-size: 1rem;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .group h3 {
    font-size: 0.85rem;
  }
  .opts {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .opt {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
    min-height: 44px;
    padding: 10px;
    border: 0;
    border-radius: var(--radius);
    background: var(--input-bg);
    color: var(--text);
    box-shadow: inset 0 0 0 1px var(--line);
    text-align: left;
    cursor: pointer;
  }
  .opt[aria-pressed='true'] {
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  :global([data-theme='classic']) .opt {
    background: var(--face);
    box-shadow: var(--bevel-raised);
  }
  :global([data-theme='classic']) .opt[aria-pressed='true'] {
    background: var(--pushed);
    box-shadow: var(--bevel-pressed);
    text-decoration: underline;
  }
  .sub {
    font-size: 0.75rem;
    color: var(--muted);
  }
  /* Small drawings of the two looks. They keep their own colours, whatever the current look is. */
  .prev {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 34px;
    padding: 0 6px;
  }
  .prev i {
    display: block;
    flex: 1;
    height: 14px;
    background: currentColor;
    opacity: 0.25;
  }
  .prev.m {
    background: #fff;
    color: #0b7c7a;
    border-radius: 8px;
    box-shadow: inset 0 0 0 1px #e1ecec;
  }
  .prev.m i {
    border-radius: 99px;
  }
  .prev.c {
    background: #c0c0c0;
    color: #000;
    box-shadow:
      inset -1px -1px #0a0a0a,
      inset 1px 1px #fff,
      inset -2px -2px #808080,
      inset 2px 2px #dfdfdf;
  }
</style>
