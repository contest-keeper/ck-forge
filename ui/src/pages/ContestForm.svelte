<script lang="ts">
  import Field from '../components/Field.svelte';
  import { PLATFORMS, optionalText, type Platform, type Settings } from '../core/profile';

  type Props = { settings: Settings };
  let { settings = $bindable() }: Props = $props();
</script>

<section class="card">
  <h2>Contest</h2>
  <div class="grid">
    <Field label="Contest name" hint="Shown on the stick. Empty uses the profile name.">
      <input bind:value={settings.contest.name} />
    </Field>
    <Field label="Platform">
      <select bind:value={settings.contest.platform}>
        {#each PLATFORMS as platform (platform)}
          <option value={platform satisfies Platform}>{platform}</option>
        {/each}
      </select>
    </Field>
    <Field label="Contest ID on the platform">
      <input bind:value={settings.contest.contest_id} />
    </Field>
    <Field label="Starts">
      <input
        type="datetime-local"
        bind:value={
          () => settings.contest.start ?? '', (v) => (settings.contest.start = optionalText(v))
        }
      />
    </Field>
    <Field label="Ends">
      <input
        type="datetime-local"
        bind:value={
          () => settings.contest.end ?? '', (v) => (settings.contest.end = optionalText(v))
        }
      />
    </Field>
    <label class="check">
      <input type="checkbox" bind:checked={settings.contest.practice} />
      <span>Practice contest</span>
    </label>
  </div>
</section>

<section class="card">
  <h2>Team and site</h2>
  <div class="grid">
    <Field label="Team" hint="Becomes {'{team}'} in the computer name.">
      <input bind:value={settings.team} />
    </Field>
    <Field label="Site" hint="Becomes {'{site}'} in the computer name.">
      <input bind:value={settings.site} />
    </Field>
  </div>
</section>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--gap);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    align-self: end;
  }
</style>
