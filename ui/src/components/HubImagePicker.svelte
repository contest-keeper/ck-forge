<script lang="ts">
  import { describeError, formatBytes } from '../core/format';
  import { listHubImages, type HubImage } from '../lib/api';

  type Props = { disabled: boolean; onDownload: (id: string) => Promise<void> };
  let { disabled, onDownload }: Props = $props();

  let images = $state<HubImage[] | null>(null);
  let imageId = $state('');
  let loading = $state(false);
  let failure = $state('');

  const selected = $derived(images?.find((image) => image.id === imageId));

  async function load() {
    loading = true;
    failure = '';
    try {
      images = await listHubImages();
      imageId = images.length === 1 ? (images[0]?.id ?? '') : '';
    } catch (error) {
      failure = describeError(error);
    } finally {
      loading = false;
    }
  }
</script>

{#if images?.length}
  <div class="path">
    <label class="grow field">
      <span class="n">Image from Keeper Hub</span>
      <select bind:value={imageId} disabled={disabled || loading}>
        <option value="" disabled>Choose a version</option>
        {#each images as image (image.id)}
          <option value={image.id}>{image.name} {image.version} ({image.profile})</option>
        {/each}
      </select>
      {#if selected}
        <span class="p"
          >{formatBytes(selected.size)}{selected.filename ? ` · ${selected.filename}` : ''}</span
        >
      {/if}
    </label>
    <button
      type="button"
      class="btn sm primary"
      disabled={disabled || loading || !selected}
      onclick={() => onDownload(imageId)}
    >
      Download and verify…
    </button>
  </div>
{:else}
  <button type="button" class="btn sm primary" onclick={load} disabled={disabled || loading}>
    {loading ? 'Finding images…' : 'Get Keeper OS from Hub'}
  </button>
{/if}
{#if images?.length === 0}<p class="hint">The Hub has no images available.</p>{/if}
{#if failure}<p class="note error" role="alert">{failure}</p>{/if}
