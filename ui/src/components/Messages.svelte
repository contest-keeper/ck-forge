<script lang="ts">
  type Props = { problems?: string[]; warnings?: string[] };
  let { problems = [], warnings = [] }: Props = $props();
</script>

{#if problems.length > 0}
  <ul class="messages problems" role="alert">
    {#each problems as problem (problem)}<li>{problem}</li>{/each}
  </ul>
{/if}
{#if warnings.length > 0}
  <ul class="messages warnings">
    {#each warnings as warning (warning)}<li>{warning}</li>{/each}
  </ul>
{/if}

<style>
  .messages {
    margin: 0;
    padding: 10px 14px 10px 32px;
    border-radius: var(--radius);
  }
  .problems {
    background: var(--danger-bg);
    color: var(--danger-text);
    border: 1px solid var(--danger-line);
  }
  .warnings {
    background: var(--warn-bg);
    color: var(--warn);
  }
</style>
