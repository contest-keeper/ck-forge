<script lang="ts">
  import { onDestroy } from 'svelte';
  import { formatBytes, formatDuration, formatRate, percent, secondsLeft } from '../core/format';
  import type { Progress } from '../lib/api';

  type Props = { progress: Progress | null };
  let { progress }: Props = $props();

  const labels = {
    downloading: 'Downloading Keeper OS',
    importing: 'Verifying and expanding the download',
    building: 'Copying the image',
    writing: 'Writing',
    checking: 'Checking',
  };

  // When a step began and, once its last byte is done, when it ended. Writing and checking go on
  // together, so each has its own clock.
  type Clock = { start: number; end: number | null };
  let main = $state<Clock | null>(null);
  let check = $state<Clock | null>(null);
  let seen = $state<Progress['phase'] | null>(null);
  let now = $state(Date.now());

  const tick = setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => clearInterval(tick));

  $effect(() => {
    const p = progress;
    const at = Date.now();
    if (!p) {
      main = null;
      check = null;
      seen = null;
      return;
    }
    // Checking after writing is the same operation; any other new kind of step starts over.
    if (seen !== p.phase && !(seen === 'writing' && p.phase === 'checking')) {
      main = { start: at, end: null };
      check = null;
    }
    seen = p.phase;
    if (main && main.end === null && p.total > 0 && p.done >= p.total) main.end = at;
    if (p.checked > 0 && check === null) check = { start: at, end: null };
    if (check && check.end === null && p.total > 0 && p.checked >= p.total) check.end = at;
  });

  /** `4:12 · 9.5 MB/s · about 6:30 left`, or `took 14:32` once the step is done. */
  function timing(clock: Clock | null, done: number, total: number): string {
    if (!clock) return '';
    const seconds = ((clock.end ?? now) - clock.start) / 1000;
    if (clock.end !== null) return `took ${formatDuration(seconds)}`;
    const parts = [formatDuration(seconds)];
    const rate = formatRate(done, seconds);
    if (rate) parts.push(rate);
    const left = secondsLeft(done, total, seconds);
    if (left !== null) parts.push(`about ${formatDuration(left)} left`);
    return parts.join(' · ');
  }
</script>

{#if progress}
  <div class="progress" role="status">
    <div class="line">
      <span>
        {labels[progress.phase]}: {formatBytes(progress.done)} of {formatBytes(progress.total)}
      </span>
      <span class="time">{timing(main, progress.done, progress.total)}</span>
    </div>
    <progress max="100" value={percent(progress.done, progress.total)}></progress>
    {#if progress.phase === 'writing' || progress.phase === 'checking'}
      <div class="line">
        <span>Checked: {formatBytes(progress.checked)} of {formatBytes(progress.total)}</span>
        <span class="time">{timing(check, progress.checked, progress.total)}</span>
      </div>
      <progress max="100" value={percent(progress.checked, progress.total)}></progress>
    {/if}
  </div>
{/if}

<style>
  .progress {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .line {
    display: flex;
    justify-content: space-between;
    gap: 14px;
    flex-wrap: wrap;
  }
  .time {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  progress {
    width: 100%;
    accent-color: var(--accent);
  }
</style>
