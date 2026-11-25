<script lang="ts">
  import { describeError } from '../core/format';
  import type { Settings } from '../core/profile';
  import * as api from '../lib/api';
  import { app, saveProfile } from '../state.svelte';

  type Props = { settings: Settings };
  let { settings = $bindable() }: Props = $props();

  let preview = $state<string | null>(null);
  let failure = $state('');
  let busy = $state(false);
  const name = $derived(settings.branding.wallpaper);

  $effect(() => {
    const path = app.profilePath;
    const file = name;
    // Choosing a picture of the same type replaces the file but not its name: load again.
    void app.wallpaperRevision;
    preview = null;
    if (!path || !file) return;
    let current = true;
    api.wallpaperPreview(path, file).then(
      (url) => {
        if (current) preview = url;
      },
      () => {
        /* No preview is not a problem: the name is still shown. */
      },
    );
    return () => {
      current = false;
    };
  });

  async function choose() {
    failure = '';
    busy = true;
    try {
      const source = await api.choosePicture();
      if (!source) return;
      // The picture goes next to the profile, so the profile needs a place first.
      const path = await saveProfile();
      settings.branding.wallpaper = await api.addWallpaper(path, source);
      app.wallpaperRevision += 1;
    } catch (error) {
      failure = describeError(error);
    } finally {
      busy = false;
    }
  }

  function remove() {
    settings.branding.wallpaper = undefined;
  }
</script>

<div class="wallpaper">
  <span class="label">Wallpaper</span>
  <div class="pick">
    <div class="thumb">
      {#if preview}
        <img src={preview} alt="The chosen wallpaper" />
      {:else}
        <span class="hint">{name ? 'No preview' : 'Default'}</span>
      {/if}
    </div>
    <div class="grow">
      <div class="n" role="status">{name ?? 'The Keeper OS wallpaper'}</div>
      <div class="hint">PNG, JPG, WEBP or BMP up to 16 MB. A copy is kept with the profile.</div>
    </div>
    <button type="button" class="btn sm" onclick={choose} disabled={busy}>
      {name ? 'Change picture…' : 'Choose a picture…'}
    </button>
    {#if name}
      <button type="button" class="btn sm" onclick={remove} disabled={busy}>Remove</button>
    {/if}
  </div>
  {#if failure}<p class="note error" role="alert">{failure}</p>{/if}
</div>

<style>
  .wallpaper {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .label {
    font-weight: 600;
    font-size: 0.9rem;
  }
  .pick {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 10px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    flex-wrap: wrap;
  }
  .thumb {
    width: 96px;
    height: 54px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    border-radius: 6px;
    background: var(--soft);
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .grow {
    flex: 1;
    min-width: 160px;
  }
  .n {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
</style>
