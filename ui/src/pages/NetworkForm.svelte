<script lang="ts">
  import Field from '../components/Field.svelte';
  import {
    RECORDING_MODES,
    linesToList,
    optionalCount,
    wholeNumber,
    type Settings,
  } from '../core/profile';

  type Props = { settings: Settings };
  let { settings = $bindable() }: Props = $props();
</script>

<section>
  <h3>Network and Hub</h3>
  <Field
    label="Allowed sites"
    hint="One per line, such as kilonova.ro. Empty allows no extra site."
  >
    <textarea
      rows="4"
      bind:value={
        () => settings.allowed_domains.join('\n'),
        (v) => (settings.allowed_domains = linesToList(v))
      }
    ></textarea>
  </Field>
  <div class="grid">
    <Field label="Hub address" hint="https://… Sticks send their heartbeats here.">
      <input bind:value={settings.api.url} />
    </Field>
    <Field label="Heartbeat every (seconds)">
      <input
        type="number"
        min="1"
        bind:value={
          () => settings.api.heartbeat_s, (v) => (settings.api.heartbeat_s = wholeNumber(v, 1))
        }
      />
    </Field>
  </div>
</section>

<section>
  <h3>Screen recording</h3>
  <div class="grid">
    <Field label="Mode">
      <select bind:value={settings.recording.mode}>
        {#each RECORDING_MODES as mode (mode)}<option value={mode}>{mode}</option>{/each}
      </select>
    </Field>
    <Field label="Start after (seconds)" hint="Keeps boot and login fast.">
      <input
        type="number"
        min="0"
        bind:value={
          () => settings.recording.start_delay_s,
          (v) => (settings.recording.start_delay_s = wholeNumber(v, 0))
        }
      />
    </Field>
    <Field label="Snapshot every (seconds)" hint="Empty uses 10.">
      <input
        type="number"
        min="1"
        bind:value={
          () => settings.recording.snapshot_interval_s ?? '',
          (v) => (settings.recording.snapshot_interval_s = optionalCount(String(v)))
        }
      />
    </Field>
  </div>
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
  h3 {
    margin: 0;
    font-size: 0.95rem;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--gap);
  }
</style>
