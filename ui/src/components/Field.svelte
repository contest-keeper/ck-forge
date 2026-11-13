<script lang="ts">
  import type { Snippet } from 'svelte';

  type Props = { label: string; hint?: string; children: Snippet };
  let { label, hint, children }: Props = $props();
</script>

<label class="field">
  <span class="label">{label}</span>
  {@render children()}
  {#if hint}<span class="hint">{hint}</span>{/if}
</label>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .label {
    font-weight: 600;
    font-size: 0.9rem;
  }
  .hint {
    color: var(--muted);
    font-size: 0.8rem;
  }
</style>
