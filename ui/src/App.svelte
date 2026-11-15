<script lang="ts">
  import HubSignIn from './components/HubSignIn.svelte';
  import ContestPage from './pages/ContestPage.svelte';
  import MakePage from './pages/MakePage.svelte';
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
      id: 'make',
      title: 'Make sticks',
      hint: 'Base image, erase and write USB',
      blurb:
        'Choose the base image once, tick the sticks and write. Each stick gets its own identity from the saved profile. Windows asks for administrator permission when the write starts.',
      page: MakePage,
    },
  ] as const;

  let index = $state(0);
  const tab = $derived(tabs[index] ?? tabs[0]);
  const done = $derived([app.profilePath !== null, app.written]);
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
      <HubSignIn />
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
      <!-- Every page stays in place and is only hidden, so what was typed or chosen is still there when
           the admin comes back to it. -->
      {#each tabs as step, i (step.id)}
        <div hidden={i !== index}><step.page /></div>
      {/each}
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
