<script lang="ts">
  import type { Snippet } from 'svelte';

  type Props = { label: string; hint?: string; error?: string; children: Snippet };
  let { label, hint, error, children }: Props = $props();
</script>

<label class="field" class:err={error}>
  <span class="label">{label}</span>
  {@render children()}
  {#if error}
    <!-- Inside the label, so the message is read with the field's name; role alert announces it. -->
    <span class="msg" role="alert">{error}</span>
  {:else if hint}
    <span class="hint">{hint}</span>
  {/if}
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
  .msg {
    color: var(--danger-text);
    font-size: 0.8rem;
    font-weight: 600;
  }
</style>
