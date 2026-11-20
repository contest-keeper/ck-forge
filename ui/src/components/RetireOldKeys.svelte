<script lang="ts">
  import { describeError, stickLabel } from '../core/format';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';

  /** The sticks that were on the disks just written, as the Hub knew them before the write. */
  type Props = { previous: api.HubStick[]; retired?: string[] };
  let { previous, retired = $bindable([]) }: Props = $props();

  let failure = $state('');
  let working = $state<string | null>(null);

  // For a stick the Hub could not match to a disk: the Hub's own list, by team and machine.
  let others = $state<api.HubStick[] | null>(null);
  let otherId = $state('');
  let loading = $state(false);

  const open = $derived(
    (others ?? []).filter(
      (stick) =>
        stick.retired_at === null &&
        stick.archived_at === null &&
        !retired.includes(stick.stick_id),
    ),
  );

  async function retire(stick: api.HubStick) {
    failure = '';
    working = stick.stick_id;
    try {
      await api.hubRetireStick(stick.stick_id);
      retired.push(stick.stick_id);
      if (stick.stick_id === otherId) otherId = '';
    } catch (error) {
      failure = describeError(error);
    } finally {
      working = null;
    }
  }

  async function loadOthers() {
    failure = '';
    loading = true;
    try {
      others = await api.hubListSticks();
    } catch (error) {
      failure = describeError(error);
    } finally {
      loading = false;
    }
  }

  const chosen = $derived(open.find((stick) => stick.stick_id === otherId));
</script>

{#if app.hubEmail}
  <section class="card">
    <h2>Retire old keys</h2>
    <p class="hint">
      A rewritten stick has a new key, and the old key keeps working until you retire it. Do it
      after you have checked the new stick and any uploads from the old one are done. The old
      stick's history stays in the Hub.
    </p>

    {#each previous as stick (stick.stick_id)}
      <div class="path">
        <div class="grow">
          <div class="n">{stickLabel(stick)}</div>
          <div class="p">
            The stick that was on a disk you just wrote{stick.hardware
              ? `, written ${stick.hardware.write_count} ${stick.hardware.write_count === 1 ? 'time' : 'times'} before`
              : ''}.
          </div>
        </div>
        {#if retired.includes(stick.stick_id)}
          <span class="pill ok">Key retired</span>
        {:else}
          <button
            type="button"
            class="btn sm"
            onclick={() => retire(stick)}
            disabled={app.busy || working !== null}
          >
            {working === stick.stick_id ? 'Retiring…' : 'Retire its old key'}
          </button>
        {/if}
      </div>
    {/each}

    <details>
      <summary>Retire the key of another stick</summary>
      <div class="more">
        <p class="hint">
          Forge only recognises sticks it wrote after it started telling the Hub which disk it
          writes to, and only when the disk reports a usable serial number. For any other stick,
          pick it here by team and machine.
        </p>
        {#if others === null}
          <div>
            <button type="button" class="btn sm" onclick={loadOthers} disabled={loading}>
              {loading ? 'Loading…' : 'Load the sticks from the Hub'}
            </button>
          </div>
        {:else if open.length === 0}
          <p class="hint">The Hub has no other stick with a working key.</p>
        {:else}
          <label class="pickone">
            <span class="n">Stick</span>
            <select bind:value={otherId}>
              <option value="" disabled>Choose a stick</option>
              {#each open as stick (stick.stick_id)}
                <option value={stick.stick_id}>{stickLabel(stick)}</option>
              {/each}
            </select>
          </label>
          <div>
            <button
              type="button"
              class="btn sm"
              onclick={() => chosen && retire(chosen)}
              disabled={!chosen || app.busy || working !== null}
            >
              Retire its key
            </button>
          </div>
        {/if}
      </div>
    </details>

    {#if failure}<p class="note error" role="alert">{failure}</p>{/if}
  </section>
{/if}

<style>
  .more {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 10px;
  }
  summary {
    cursor: pointer;
    font-weight: 600;
  }
  .pickone {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 420px;
  }
  .pickone select {
    min-height: 40px;
    padding: 0 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--input-bg);
    color: var(--text);
  }
</style>
