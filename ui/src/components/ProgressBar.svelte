<script lang="ts">
  import { formatBytes, percent } from '../core/format';
  import type { Progress } from '../lib/api';

  type Props = { progress: Progress | null };
  let { progress }: Props = $props();

  const labels = { building: 'Copying the image', writing: 'Writing', checking: 'Checking' };
</script>

{#if progress}
  <div class="progress">
    <span>
      {labels[progress.phase]}: {formatBytes(progress.done)} of {formatBytes(progress.total)}
    </span>
    <progress max="100" value={percent(progress.done, progress.total)}></progress>
  </div>
{/if}

<style>
  .progress {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  progress {
    width: 100%;
    accent-color: var(--accent);
  }
</style>
