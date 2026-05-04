<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Installation } from "$lib/bindings";
  import { m } from "$lib/i18n";

  interface Props {
    installations: Installation[];
    selected: Set<string>;
    onToggle: (channel: string) => void;
    headingAction?: Snippet;
  }

  let { installations, selected, onToggle, headingAction }: Props = $props();
</script>

<section>
  <div class="heading-row">
    <h2>{m.installations_heading()}</h2>
    {#if headingAction}{@render headingAction()}{/if}
  </div>
  {#if installations.length === 0}
    <p class="muted">{m.installations_none_found()}</p>
  {:else}
    <div class="list">
      {#each installations as inst}
        <label class="list-item">
          <input
            type="checkbox"
            checked={selected.has(inst.channel)}
            onchange={() => onToggle(inst.channel)}
          />
          <span class="channel">{inst.channel}</span>
          <span class="path">{inst.path}</span>
        </label>
      {/each}
    </div>
  {/if}
</section>

<style>
  .heading-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin: 0 0 8px;
  }

  .heading-row :global(h2) {
    margin: 0;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .list-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    background: #16213e;
    border-radius: 6px;
    cursor: pointer;
  }

  .list-item:hover {
    background: #1a2745;
  }

  .channel {
    font-weight: 600;
    min-width: 60px;
    color: #4cc9f0;
  }

  .path {
    font-size: 0.85rem;
    color: #888;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .muted {
    color: #666;
    font-size: 0.9rem;
  }
</style>
