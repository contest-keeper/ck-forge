<script lang="ts">
  import { onMount } from 'svelte';
  import HubSignIn from './components/HubSignIn.svelte';
  import Settings from './components/Settings.svelte';
  import { describeError } from './core/format';
  import * as api from './lib/api';
  import ContestPage from './pages/ContestPage.svelte';
  import MakePage from './pages/MakePage.svelte';
  import { app, isUnsaved, saveProfile } from './state.svelte';

  const tabs = [
    {
      id: 'contest',
      title: 'Contest profile',
      hint: 'Name, platform, branding',
      blurb: 'Everything a stick needs to know about this contest. Mistakes are shown as you type.',
      page: ContestPage,
    },
    {
      id: 'make',
      title: 'Make sticks',
      hint: 'Base image, erase and write USB',
      blurb:
        'Choose the base image once, tick the sticks and write. Each stick gets its own identity from the saved profile. Windows asks for administrator permission when the write starts.',
      page: MakePage,
    },
  ] as const;

  let index = $state(0);
  let leaving = $state(false);
  let leaveFailure = $state('');
  const tab = $derived(tabs[index] ?? tabs[0]);
  // What the bar at the bottom tells about the profile: saved, changed since, or never saved.
  const saveState = $derived(
    app.profilePath === null
      ? 'Not saved yet. Continue saves it in the Keeper Forge folder.'
      : isUnsaved()
        ? 'Unsaved changes. Continue saves them.'
        : `Saved in ${app.profilePath}`,
  );
  const done = $derived([app.profilePath !== null, app.written]);

  // A profile is ready to fill in from the start; the admin does not have to ask for one first.
  onMount(() => {
    if (app.profile) return;
    api.newProfile().then(
      (profile) => {
        app.profile ??= profile;
      },
      (error: unknown) => (leaveFailure = describeError(error)),
    );
  });

  /** Moves to the next step. Leaving the profile saves it, in Forge's own folder when it has no
   * file yet; a profile with problems stays here, and the problems are shown. */
  async function next() {
    leaveFailure = '';
    if (index === 0) {
      leaving = true;
      try {
        await saveProfile();
      } catch (error) {
        leaveFailure = describeError(error);
        return;
      } finally {
        leaving = false;
      }
      // The name stays editable during the save. A stick must not be written under a name that was never saved.
      if (isUnsaved()) {
        leaveFailure = 'The profile changed while it was being saved. Check it and continue again.';
        return;
      }
    }
    index++;
  }

  /** The sidebar steps go through the same rule as Continue. */
  async function show(target: number) {
    if (target > index) {
      while (index < target && !leaveFailure) await next();
      return;
    }
    leaveFailure = '';
    index = target;
  }
</script>

<div class="win">
  <aside class="side">
    <div class="brand">
      <img class="logo" src="/contest-keeper-icon.svg" alt="" width="32" height="32" />
      Keeper Forge
    </div>
    <nav class="steps" aria-label="Steps">
      {#each tabs as step, i (step.id)}
        <button
          type="button"
          class="step"
          class:on={i === index}
          class:done={done[i]}
          aria-current={i === index ? 'step' : undefined}
          onclick={() => show(i)}
        >
          <span class="dot">
            {#if done[i]}
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="3"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"><path d="m5 12 5 5 9-10" /></svg
              >
              <span class="sr-only">done</span>
            {:else}
              {i + 1}
            {/if}
          </span>
          <span>
            <span class="st">{step.title}</span>
            <span class="ss">{step.hint}</span>
          </span>
        </button>
      {/each}
    </nav>
    <div class="side-foot">
      <HubSignIn />
      <b>{app.profile?.name || 'No profile yet'}</b>
      <Settings />
    </div>
  </aside>

  <section class="main">
    <header class="head">
      <h1>{tab.title}</h1>
      <p>{tab.blurb}</p>
    </header>
    <main class="body">
      <!-- Every page stays in place and is only hidden, so what was typed or chosen is still there when
           the admin comes back to it. -->
      {#each tabs as step, i (step.id)}
        <div hidden={i !== index}><step.page /></div>
      {/each}
    </main>
    <footer class="foot">
      {#if index > 0}
        <button type="button" class="btn" onclick={() => show(index - 1)}>Back</button>
      {/if}
      {#if leaveFailure}
        <p class="note error leave" role="alert">{leaveFailure}</p>
      {:else if index === 0}
        <span class="hint saved" role="status">{saveState}</span>
      {/if}
      <span class="grow"></span>
      {#if index < tabs.length - 1}
        <button type="button" class="btn pri" onclick={next} disabled={leaving || !app.profile}>
          {leaving ? 'Saving…' : 'Save and continue'}
        </button>
      {/if}
    </footer>
  </section>
</div>

<style>
  .sr-only {
    position: absolute;
    left: -9999px;
  }
  .saved {
    overflow-wrap: anywhere;
  }
  .leave {
    margin: 0;
    max-height: 120px;
    overflow-y: auto;
  }
</style>
