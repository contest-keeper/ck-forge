<script lang="ts">
  import type { Settings } from '../core/profile';
  import * as api from '../lib/api';
  import { app } from '../state.svelte';

  type Props = { name: string; settings: Settings };
  let { name, settings }: Props = $props();

  let picture = $state<string | null>(null);

  const title = $derived(settings.contest.name.trim() || name.trim() || 'Your contest');
  const start = $derived(settings.branding.home_page?.replace(/^https?:\/\//, ''));

  // The saved wallpaper, when there is one; an unsaved profile has no folder to read it from.
  $effect(() => {
    const path = app.profilePath;
    const file = settings.branding.wallpaper;
    void app.wallpaperRevision; // a replaced file keeps its name
    picture = null;
    if (!path || !file) return;
    let current = true;
    api.wallpaperPreview(path, file).then(
      (url) => {
        if (current) picture = url;
      },
      () => {
        /* No picture is fine: the preview keeps its plain background. */
      },
    );
    return () => {
      current = false;
    };
  });
</script>

<section class="card" aria-labelledby="preview-title">
  <h2 id="preview-title">What a contestant will see</h2>
  <div class="screen" style:background-image={picture ? `url(${picture})` : undefined}>
    <div class="welcome">
      <strong>{title}</strong>
      <span>{settings.branding.welcome_text || 'No welcome text yet.'}</span>
    </div>
    {#if start}<span class="start">Start page: {start}</span>{/if}
  </div>
  <p class="hint">A preview. The real screen is the Keeper OS desktop.</p>
</section>

<style>
  .screen {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 150px;
    padding: 16px;
    border-radius: var(--radius);
    background-color: #0f3d3c;
    background-position: center;
    background-size: cover;
  }
  /* White on a dark panel whatever the picture is, so the text stays readable. */
  .welcome {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 380px;
    padding: 12px 14px;
    border-radius: var(--radius);
    background: rgba(8, 20, 24, 0.82);
    color: #fff;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .start {
    align-self: flex-start;
    padding: 4px 10px;
    border-radius: var(--radius);
    background: rgba(8, 20, 24, 0.82);
    color: #fff;
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }
</style>
