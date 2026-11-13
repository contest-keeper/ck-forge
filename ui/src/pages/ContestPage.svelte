<script lang="ts">
  import Field from '../components/Field.svelte';
  import Messages from '../components/Messages.svelte';
  import { describeError } from '../core/format';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';
  import BrandingForm from './BrandingForm.svelte';
  import ContestForm from './ContestForm.svelte';
  import NetworkForm from './NetworkForm.svelte';

  let problems = $state<string[]>([]);
  let warnings = $state<string[]>([]);
  let status = $state('');
  let failure = $state('');

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

  const save = () =>
    run(async () => {
      if (!app.profile) return '';
      const path = app.profilePath ?? (await api.chooseProfileToSave());
      if (!path) return '';
      await api.saveProfile(path, $state.snapshot(app.profile));
      app.profilePath = path;
      return `Saved ${path}`;
    });
</script>

<div class="page">
  <div class="toolbar">
    <button type="button" class="btn" onclick={create}>New profile</button>
    <button type="button" class="btn" onclick={open}>Open…</button>
    <button type="button" class="btn pri" onclick={save} disabled={!app.profile}>Save</button>
    <span class="where">{app.profilePath ?? 'not saved yet'}</span>
  </div>

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
  .where {
    color: var(--muted);
    font-size: 0.85rem;
    overflow-wrap: anywhere;
  }
</style>
