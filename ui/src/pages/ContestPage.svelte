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
    <button type="button" onclick={create}>New profile</button>
    <button type="button" onclick={open}>Open…</button>
    <button type="button" class="primary" onclick={save} disabled={!app.profile}>Save</button>
    <span class="path">{app.profilePath ?? 'not saved yet'}</span>
  </div>

  {#if status}<p class="ok">{status}</p>{/if}
  {#if failure}<p class="error" role="alert">{failure}</p>{/if}

  {#if app.profile}
    <Messages {problems} {warnings} />
    <Field label="Profile name">
      <input bind:value={app.profile.name} />
    </Field>
    <ContestForm bind:settings={app.profile.settings} />
    <BrandingForm bind:settings={app.profile.settings} />
    <NetworkForm bind:settings={app.profile.settings} />
  {:else}
    <p>Start with a new profile, or open one you saved.</p>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .toolbar {
    display: flex;
    gap: var(--gap);
    align-items: center;
    flex-wrap: wrap;
  }
  .path {
    color: var(--muted);
    font-size: 0.85rem;
    overflow-wrap: anywhere;
  }
  .ok {
    color: var(--ok);
    margin: 0;
  }
  .error {
    color: var(--danger);
    margin: 0;
    white-space: pre-wrap;
  }
</style>
