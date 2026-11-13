<script lang="ts">
  import BuildPage from './pages/BuildPage.svelte';
  import ContestPage from './pages/ContestPage.svelte';
  import HubPage from './pages/HubPage.svelte';
  import WritePage from './pages/WritePage.svelte';

  const tabs = [
    { id: 'contest', title: '1. Contest', page: ContestPage },
    { id: 'build', title: '2. Image', page: BuildPage },
    { id: 'write', title: '3. Write', page: WritePage },
    { id: 'hub', title: '4. Hub', page: HubPage },
  ] as const;

  let current = $state<(typeof tabs)[number]['id']>('contest');
  const Page = $derived(tabs.find((tab) => tab.id === current)?.page ?? ContestPage);
</script>

<header>
  <h1>Keeper Forge</h1>
  <nav aria-label="Steps">
    {#each tabs as tab (tab.id)}
      <button
        type="button"
        class:active={tab.id === current}
        aria-current={tab.id === current ? 'page' : undefined}
        onclick={() => (current = tab.id)}
      >
        {tab.title}
      </button>
    {/each}
  </nav>
</header>

<main>
  <Page />
</main>

<style>
  header {
    display: flex;
    align-items: center;
    gap: 24px;
    padding: 10px 20px;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
    position: sticky;
    top: 0;
    z-index: 1;
    flex-wrap: wrap;
  }
  h1 {
    margin: 0;
    font-size: 1.15rem;
  }
  nav {
    display: flex;
    gap: 6px;
  }
  nav button.active {
    background: var(--accent);
    color: var(--accent-text);
    border-color: var(--accent);
  }
  main {
    padding: 20px;
    max-width: 900px;
    margin: 0 auto;
  }
</style>
