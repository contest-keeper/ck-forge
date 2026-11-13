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
    padding: 8px 12px 8px 28px;
    border-radius: var(--radius);
    border: 1px solid currentColor;
  }
  .problems {
    color: var(--danger);
  }
  .warnings {
    color: var(--warn);
  }
</style>
