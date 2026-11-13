<script lang="ts">
  import BuildPage from './pages/BuildPage.svelte';
  import ContestPage from './pages/ContestPage.svelte';
  import HubPage from './pages/HubPage.svelte';
  import WritePage from './pages/WritePage.svelte';
  import { app } from './state.svelte';
  import { look, setTheme } from './theme/theme.svelte';

  const tabs = [
    {
      id: 'contest',
      title: 'Contest profile',
      hint: 'Name, platform, branding',
      blurb: 'Everything a stick needs to know about this contest. Mistakes are shown as you type.',
      page: ContestPage,
    },
    {
      id: 'build',
      title: 'Make image',
      hint: 'Copy base, apply profile',
      blurb:
        'Makes the image of one stick: the base image with its own identity and the settings of the saved profile. Only the start of the image is copied, so this takes a second, and the base image must stay where it is. The base image is only read, never changed.',
      page: BuildPage,
    },
    {
      id: 'write',
      title: 'Write sticks',
      hint: 'Erase and write USB',
      blurb:
        'Choose the stick to write. Windows asks for administrator permission when the write starts.',
      page: WritePage,
    },
    {
      id: 'hub',
      title: 'Register with Hub',
      hint: 'So sticks are recognised',
      blurb:
        'The Hub only accepts heartbeats from sticks it knows. This sends it the registry that sits next to the profile.',
      page: HubPage,
    },
  ] as const;

  let index = $state(0);
  const tab = $derived(tabs[index] ?? tabs[0]);
  const Page = $derived(tab.page);
  const done = $derived([
    app.profilePath !== null,
    app.imagePath !== null,
    app.written,
    app.registered,
  ]);
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
          onclick={() => (index = i)}
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
      <b>{app.profile?.name || 'No profile yet'}</b>
      <div class="seg" role="group" aria-label="Look">
        <button
          type="button"
          aria-pressed={look.theme === 'classic'}
          onclick={() => setTheme('classic')}>Classic</button
        >
        <button
          type="button"
          aria-pressed={look.theme === 'modern'}
          onclick={() => setTheme('modern')}>Modern</button
        >
      </div>
    </div>
  </aside>

  <section class="main">
    <header class="head">
      <h1>{tab.title}</h1>
      <p>{tab.blurb}</p>
    </header>
    <main class="body">
      <Page />
    </main>
    <footer class="foot">
      {#if index > 0}
        <button type="button" class="btn" onclick={() => index--}>Back</button>
      {/if}
      <span class="grow"></span>
      {#if index < tabs.length - 1}
        <button type="button" class="btn pri" onclick={() => index++}>Continue</button>
      {/if}
    </footer>
  </section>
</div>

<style>
  .sr-only {
    position: absolute;
    left: -9999px;
  }
</style>
