<script lang="ts">
  import Field from '../components/Field.svelte';
  import {
    RECORDING_MODES,
    RECORDING_MODE_LABELS,
    linesToList,
    optionalCount,
    optionalText,
    recordingFields,
    wholeNumber,
    type RecordingUpload,
    type Settings,
  } from '../core/profile';

  type Props = { settings: Settings };
  let { settings = $bindable() }: Props = $props();

  const fields = $derived(recordingFields(settings.recording.mode));
</script>

<div class="net">
  <section aria-labelledby="net-h">
    <h3 id="net-h">Network and Hub</h3>
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
      <Field
        label="Heartbeat every (seconds)"
        hint="Sticks made while signed in to the Hub send heartbeats to it."
      >
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

  <section aria-labelledby="bt-h">
    <h3 id="bt-h">Bluetooth and Wi-Fi</h3>
    <label class="check">
      <input type="checkbox" bind:checked={settings.block_bluetooth} />
      <span>Block Bluetooth on the sticks</span>
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={settings.block_usb_storage} />
      <span>Block other USB storage devices on the sticks</span>
    </label>
    <div class="grid">
      <Field
        label="Wi-Fi name (SSID)"
        hint="Sticks join this network by themselves at every boot, with no login on the stick. It works for a network with a WPA2 password or an open one, not for a network that asks for a user name or a web page login. Empty means none. The password is asked on the Make page and is never saved in the profile."
      >
        <input
          type="text"
          maxlength="32"
          bind:value={
            () => settings.wifi?.ssid ?? '',
            (v) => (settings.wifi = optionalText(v) === undefined ? undefined : { ssid: v.trim() })
          }
        />
      </Field>
    </div>
  </section>

  <section aria-labelledby="rec-h">
    <h3 id="rec-h">Screen recording</h3>
    <div class="grid">
      <Field label="Mode" hint={RECORDING_MODE_LABELS[settings.recording.mode]}>
        <select bind:value={settings.recording.mode}>
          {#each RECORDING_MODES as mode (mode)}
            <option value={mode}>{mode.charAt(0).toUpperCase() + mode.slice(1)}</option>
          {/each}
        </select>
      </Field>
      {#if fields.timing}
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
      {/if}
      {#if fields.snapshotInterval}
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
      {/if}
      {#if fields.keeping}
        <Field label="Recording upload">
          <select
            bind:value={
              () => settings.recording.upload ?? 'keep',
              (value: RecordingUpload) => (settings.recording.upload = value)
            }
          >
            <option value="keep">Keep on the stick</option>
            <option value="after_contest">Upload after the contest</option>
            <option value="during_contest">Upload during the contest</option>
          </select>
        </Field>
        <Field
          label="Keep on the stick (days)"
          hint="Leave empty to keep recordings until you collect them."
        >
          <input
            type="number"
            min="1"
            bind:value={
              () => settings.recording.retention_days ?? '',
              (value) => (settings.recording.retention_days = optionalCount(String(value)))
            }
          />
        </Field>
      {/if}
    </div>
    {#if !fields.timing}
      <p class="hint" role="status">Nothing is recorded, so there is nothing else to set.</p>
    {/if}
  </section>
</div>

<style>
  .net {
    display: flex;
    flex-direction: column;
    gap: 22px;
  }
  section + section {
    padding-top: 18px;
    border-top: 1px solid var(--line);
  }
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
