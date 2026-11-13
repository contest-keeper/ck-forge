<script lang="ts">
  import Field from '../components/Field.svelte';
  import { optionalText, type Settings } from '../core/profile';

  type Props = { settings: Settings };
  let { settings = $bindable() }: Props = $props();

  function addBookmark() {
    settings.branding.bookmarks.push({ title: '', url: '' });
  }

  function removeBookmark(index: number) {
    settings.branding.bookmarks.splice(index, 1);
  }
</script>

<section>
  <h2>What contestants see</h2>
  <div class="grid">
    <Field label="Display name">
      <input bind:value={settings.branding.display_name} />
    </Field>
    <Field label="Computer name" hint="Placeholders: {'{team}'} {'{site}'} {'{n}'}">
      <input bind:value={settings.branding.computer_name_pattern} />
    </Field>
    <Field label="USB label" hint="At most 11 characters.">
      <input bind:value={settings.branding.usb_label} maxlength="11" />
    </Field>
    <Field label="Wallpaper file name" hint="A picture in the same folder as the profile.">
      <input
        bind:value={
          () => settings.branding.wallpaper ?? '',
          (v) => (settings.branding.wallpaper = optionalText(v))
        }
      />
    </Field>
    <Field label="Home page">
      <input
        bind:value={
          () => settings.branding.home_page ?? '',
          (v) => (settings.branding.home_page = optionalText(v))
        }
      />
    </Field>
  </div>
  <Field label="Welcome text" hint="Shown at login.">
    <textarea rows="3" bind:value={settings.branding.welcome_text}></textarea>
  </Field>

  <h3>Bookmarks</h3>
  {#each settings.branding.bookmarks as bookmark, index (index)}
    <div class="bookmark">
      <input aria-label="Bookmark title" placeholder="Title" bind:value={bookmark.title} />
      <input aria-label="Bookmark address" placeholder="https://…" bind:value={bookmark.url} />
      <button type="button" onclick={() => removeBookmark(index)}>Remove</button>
    </div>
  {/each}
  <div><button type="button" onclick={addBookmark}>Add a bookmark</button></div>
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
  }
  h2,
  h3 {
    margin: 0;
    font-size: 1.05rem;
  }
  h3 {
    font-size: 0.95rem;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--gap);
  }
  .bookmark {
    display: grid;
    grid-template-columns: 1fr 2fr auto;
    gap: var(--gap);
  }
</style>
