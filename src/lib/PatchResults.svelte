<script lang="ts">
  import type { PatchResult } from "$lib/bindings";

  interface Props {
    results: PatchResult[];
    error: string | null;
  }

  let { results, error }: Props = $props();
</script>

{#if results.length > 0 || error}
  <section>
    <h2>Status</h2>
    {#if error}
      <div class="error-box">{error}</div>
    {/if}
    <div class="results">
      {#each results as result}
        <div class="result-item" class:result-error={result.error}>
          {#if result.error}
            <span class="result-icon">x</span>
            <span>{result.channel}: {result.error}</span>
          {:else}
            <span class="result-icon success">ok</span>
            <span>
              {result.channel}: Applied {result.applied}/{result.total} patches
              {#if result.warnings.length > 0}
                — {result.warnings.length}
                {result.warnings.length === 1 ? "warning" : "warnings"}
              {/if}
            </span>
          {/if}
        </div>

        {#if result.module_stats.length > 0}
          <div class="module-stats">
            {#each result.module_stats as stat}
              <div class="module-row">
                <span class="module-name">{stat.module_name}</span>
                <span class="module-count">{stat.patches} patches</span>
              </div>
              {#each stat.replace_overrides as override}
                <div class="override-row">
                  ↳ overrides {override.keys}
                  {override.keys === 1 ? "key" : "keys"} from
                  <span class="module-name">{override.overrode_module}</span>
                </div>
              {/each}
            {/each}
          </div>
        {/if}

        {#each result.warnings as warning}
          <div class="warning">{warning}</div>
        {/each}
      {/each}
    </div>
  </section>
{/if}

<style>
  .results {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .result-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: #16213e;
    border-radius: 6px;
    font-size: 0.9rem;
  }

  .result-icon {
    font-size: 0.85rem;
    font-weight: 700;
    color: #ef233c;
  }

  .result-icon.success {
    color: #06d6a0;
  }

  .result-error {
    border-left: 3px solid #ef233c;
  }

  .module-stats {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 2px 0 6px 32px;
    padding: 6px 10px;
    background: #0f1a30;
    border-radius: 4px;
    font-size: 0.82rem;
  }

  .module-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    color: #c8d4e6;
  }

  .module-name {
    font-weight: 500;
  }

  .module-count {
    color: #94a3b8;
    font-variant-numeric: tabular-nums;
  }

  .override-row {
    margin-left: 12px;
    color: #f4a261;
    font-size: 0.78rem;
  }

  .override-row .module-name {
    color: #e9c46a;
  }

  .warning {
    font-size: 0.8rem;
    color: #f4a261;
    padding: 2px 12px 2px 32px;
  }

  .error-box {
    padding: 12px;
    background: #ef233c22;
    border: 1px solid #ef233c44;
    border-radius: 6px;
    color: #ef233c;
    font-size: 0.9rem;
  }
</style>
