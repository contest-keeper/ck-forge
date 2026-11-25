<script lang="ts">
  import Field from '../components/Field.svelte';
  import { isCodeforcesId, optionalText, type Settings } from '../core/profile';

  type Props = { settings: Settings };
  let { settings = $bindable() }: Props = $props();

  // Codeforces numbers its contests with digits only, and a wrong number gives a stick that opens a page that is not there.
  const idError = $derived(
    settings.contest.platform === 'codeforces' &&
      settings.contest.contest_id.trim() !== '' &&
      !isCodeforcesId(settings.contest.contest_id)
      ? 'Use digits only, like 1985. It is the number in codeforces.com/contest/1985.'
      : '',
  );
</script>

<section class="card">
  <h2>Contest</h2>
  <div class="grid">
    <Field label="Contest name" hint="Shown on the stick. Empty uses the profile name.">
      <input bind:value={settings.contest.name} />
    </Field>
    <Field
      label="Codeforces contest ID"
      hint="The number in codeforces.com/contest/…"
      error={idError}
    >
      <input bind:value={settings.contest.contest_id} aria-invalid={idError !== ''} />
    </Field>
    <Field label="Contest end" hint="Use the stick's local time for uploads after the contest.">
      <input
        type="datetime-local"
        bind:value={
          () => settings.contest.end ?? '', (value) => (settings.contest.end = optionalText(value))
        }
      />
    </Field>
  </div>
</section>

<details class="card">
  <summary>Team and site (optional)</summary>
  <div class="grid">
    <Field label="Team" hint="Becomes {'{team}'} in the computer name.">
      <input bind:value={settings.team} />
    </Field>
    <Field label="Site" hint="Becomes {'{site}'} in the computer name.">
      <input bind:value={settings.site} />
    </Field>
  </div>
</details>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--gap);
  }
</style>
