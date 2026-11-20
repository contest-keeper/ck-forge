<script lang="ts">
  import Field from '../components/Field.svelte';
  import Messages from '../components/Messages.svelte';
  import { describeError } from '../core/format';
  import {
    defaultKeyWindow,
    localInputToUtc,
    utcToLocalInput,
    type StickKeyWindow,
  } from '../core/profile';
  import * as api from '../lib/api';
  import { app, saveProfile } from '../state.svelte';
  import BrandingForm from './BrandingForm.svelte';
  import ContestForm from './ContestForm.svelte';
  import NetworkForm from './NetworkForm.svelte';

  let problems = $state<string[]>([]);
  let warnings = $state<string[]>([]);
  let status = $state('');
  let failure = $state('');

  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const limited = $derived(app.profile?.stick_key_window !== undefined);

  /** Turning the limit on starts from now to a day later, so there is something to change. */
  function limitKey(on: boolean) {
    const profile = app.profile;
    if (!profile) return;
    if (!on) {
      profile.stick_key_window = undefined;
      return;
    }
    profile.stick_key_window = defaultKeyWindow(Date.now());
  }

  function setKeyTime(field: keyof StickKeyWindow, picked: string) {
    const window = app.profile?.stick_key_window;
    if (window) window[field] = localInputToUtc(picked);
  }

  // Check the profile as it is edited, so a mistake is shown next to the form and not at build time.
  $effect(() => {
    if (!app.profile) return;
    const snapshot = $state.snapshot(app.profile);
    api.checkProfile(snapshot).then(
      (checked) => {
        problems = checked.problems;
        warnings = checked.warnings;
      },
      (error: unknown) => (failure = describeError(error)),
    );
  });

  async function run(action: () => Promise<string>) {
    status = '';
    failure = '';
    try {
      status = await action();
    } catch (error) {
      failure = describeError(error);
    }
  }

  const create = () =>
    run(async () => {
      app.profile = await api.newProfile();
      app.profilePath = null;
      return 'A new profile. Save it to build sticks.';
    });

  const open = () =>
    run(async () => {
      const path = await api.chooseProfileToOpen();
      if (!path) return '';
      app.profile = await api.openProfile(path);
      app.profilePath = path;
      return `Opened ${path}`;
    });

  let hubProfiles = $state<api.HubProfileItem[] | null>(null);

  const listHub = () =>
    run(async () => {
      hubProfiles = await api.hubListProfiles();
      return hubProfiles.length === 0 ? 'The Hub has no profiles yet. Save one to it.' : '';
    });

  const loadFromHub = (id: string) =>
    run(async () => {
      const loaded = await api.hubLoadProfile(id);
      app.profile = loaded.profile;
      app.profilePath = loaded.path;
      hubProfiles = null;
      return `Loaded ${loaded.profile.name} from the Hub`;
    });

  const saveToHub = () =>
    run(async () => {
      if (!app.profile) return '';
      const path = await saveProfile();
      app.profile = await api.hubSaveProfile(path, $state.snapshot(app.profile));
      return `Saved ${app.profile.name} to the Hub`;
    });

  const save = () => run(async () => `Saved ${await saveProfile()}`);
</script>

<div class="page">
  <div class="toolbar">
    <button type="button" class="btn" onclick={create}>New profile</button>
    <button type="button" class="btn" onclick={open}>Open…</button>
    <button type="button" class="btn pri" onclick={save} disabled={!app.profile}>Save</button>
    {#if app.hubEmail}
      <button type="button" class="btn" onclick={listHub}>Load from Hub…</button>
      <button type="button" class="btn" onclick={saveToHub} disabled={!app.profile}>
        Save to Hub
      </button>
    {/if}
    <span class="where">
      {app.profilePath ?? 'Not saved yet. Continue saves it in the Keeper Forge folder.'}
    </span>
  </div>

  {#if hubProfiles && hubProfiles.length > 0}
    <div class="card">
      <h2>Profiles on the Hub</h2>
      <ul class="hubs">
        {#each hubProfiles as item (item.id)}
          <li>
            <span class="grow">{item.name}</span>
            <button type="button" class="btn sm" onclick={() => loadFromHub(item.id)}>Open</button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if status}<p class="note ok">{status}</p>{/if}
  {#if failure}<p class="note error" role="alert">{failure}</p>{/if}

  {#if app.profile}
    <Messages {problems} {warnings} />
    <div class="card">
      <Field label="Profile name" hint="Only for you. Contestants never see it.">
        <input bind:value={app.profile.name} />
      </Field>
    </div>
    <ContestForm bind:settings={app.profile.settings} />
    <div class="card">
      <label class="check">
        <input
          type="checkbox"
          checked={limited}
          onchange={(event) => limitKey(event.currentTarget.checked)}
        />
        <span><b>Limit when the stick key works</b> (optional)</span>
      </label>
      <p class="hint">
        Off: the key of each stick works until you retire it. On: it only works between the two
        times below, which needs the contest ID above.
      </p>
      {#if app.profile.stick_key_window}
        <div class="keys">
          <Field label="Valid from">
            <input
              type="datetime-local"
              value={utcToLocalInput(app.profile.stick_key_window.valid_from)}
              oninput={(event) => setKeyTime('valid_from', event.currentTarget.value)}
            />
          </Field>
          <Field label="Valid until">
            <input
              type="datetime-local"
              value={utcToLocalInput(app.profile.stick_key_window.valid_until)}
              oninput={(event) => setKeyTime('valid_until', event.currentTarget.value)}
            />
          </Field>
        </div>
        <p class="hint">
          Times are on this PC's clock ({timeZone}). The key stores them as UTC:
          <span class="mono"
            >{app.profile.stick_key_window.valid_from || '…'} to
            {app.profile.stick_key_window.valid_until || '…'}</span
          >.
        </p>
      {/if}
      <p class="hint">To re-key a stick, make a new stick with the new contest profile.</p>
    </div>
    <details class="card">
      <summary>What contestants see</summary>
      <BrandingForm bind:settings={app.profile.settings} />
    </details>
    <details class="card">
      <summary>Network, Hub and screen recording</summary>
      <NetworkForm bind:settings={app.profile.settings} />
    </details>
  {:else}
    <p class="card">Start with a new profile, or open one you saved.</p>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
  .toolbar {
    display: flex;
    gap: var(--gap);
    align-items: center;
    flex-wrap: wrap;
  }
  .hubs {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .hubs li {
    display: flex;
    align-items: center;
    gap: var(--gap);
  }
  .keys {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--gap);
  }
  .where {
    color: var(--muted);
    font-size: 0.85rem;
    overflow-wrap: anywhere;
  }
</style>
