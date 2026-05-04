<script lang="ts">
  import { commands } from "$lib/bindings";
  import { m } from "$lib/i18n";
  import { open } from "@tauri-apps/plugin-dialog";

  interface Props {
    path: string | null;
    onChange: (path: string | null) => void;
  }

  let { path, onChange }: Props = $props();

  let hintOpen = $state(false);

  // Local edit buffer — committed on Enter or blur. Lets the user
  // type a URL/path without saving on every keystroke. Re-syncs from
  // `path` whenever it changes externally (pickFile, clear, init).
  let draft = $state("");

  $effect(() => {
    draft = path ?? "";
  });

  async function commit() {
    const trimmed = draft.trim();
    const next = trimmed === "" ? null : trimmed;
    if (next === path) return;
    await commands.setLanguagePack(next);
    onChange(next);
  }

  async function pickFile() {
    const selected = await open({
      title: m.language_pack_dialog_title(),
      multiple: false,
      directory: false,
      filters: [
        { name: m.language_pack_filter_ini(), extensions: ["ini"] },
        { name: m.language_pack_filter_all(), extensions: ["*"] },
      ],
    });
    if (typeof selected === "string") {
      draft = selected;
      await commit();
    }
  }

  async function clear() {
    draft = "";
    await commit();
  }
</script>

<section>
  <div class="heading-row">
    <h2>
      {m.language_pack_heading()}
      <span class="optional">{m.language_pack_optional()}</span>
    </h2>
    <button
      type="button"
      class="info-btn"
      onclick={() => (hintOpen = !hintOpen)}
      aria-expanded={hintOpen}
      aria-label={hintOpen
        ? m.language_pack_hide_hint()
        : m.language_pack_show_hint()}
      title={hintOpen
        ? m.language_pack_hide_hint()
        : m.language_pack_show_hint()}
    >
      ?
    </button>
  </div>

  {#if hintOpen}
    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
    <p class="hint">{@html m.language_pack_hint_html()}</p>
  {/if}

  <div class="input-row">
    <input
      type="text"
      placeholder={m.language_pack_url_placeholder()}
      bind:value={draft}
      onkeydown={(e) => e.key === "Enter" && commit()}
      onblur={commit}
    />
    {#if draft}
      <button
        type="button"
        class="icon-btn clear-btn"
        onclick={clear}
        aria-label={m.language_pack_clear()}
        title={m.language_pack_clear()}
      >
        <svg
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    {/if}
    <button
      type="button"
      class="icon-btn browse-btn"
      onclick={pickFile}
      aria-label={m.language_pack_pick_file()}
      title={m.language_pack_pick_file()}
    >
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path
          d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"
        />
      </svg>
    </button>
  </div>
</section>

<style>
  .heading-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 24px 0 8px;
  }

  .heading-row :global(h2) {
    margin: 0;
  }

  .heading-row :global(h2:first-child) {
    margin-top: 0;
  }

  .optional {
    margin-left: 4px;
    color: #555;
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
    font-size: 1em;
    font-style: italic;
  }

  .info-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 1px solid #444;
    border-radius: 50%;
    background: transparent;
    color: #888;
    font-size: 0.7rem;
    font-weight: 600;
    line-height: 1;
    cursor: pointer;
    transition: color 0.15s, border-color 0.15s;
  }

  .info-btn:hover,
  .info-btn[aria-expanded="true"] {
    color: #4cc9f0;
    border-color: #4cc9f0;
  }

  .hint {
    margin: 0 0 8px;
    color: #888;
    font-size: 0.85rem;
  }

  :global(.hint code) {
    background: #ffffff10;
    padding: 1px 4px;
    border-radius: 3px;
    font-size: 0.85em;
  }

  .input-row {
    display: flex;
    align-items: center;
    gap: 6px;
    background: #16213e;
    border: 1px solid transparent;
    border-radius: 6px;
    padding: 4px 6px 4px 12px;
    transition: border-color 0.15s, background 0.15s;
  }

  .input-row:hover {
    background: #1a2745;
  }

  .input-row:focus-within {
    border-color: #4cc9f0;
    background: #16213e;
  }

  input[type="text"] {
    flex: 1;
    min-width: 0;
    padding: 4px 0;
    border: none;
    background: transparent;
    color: #e0e0e0;
    font-size: 0.9rem;
    font-family: inherit;
  }

  input[type="text"]:focus {
    outline: none;
  }

  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: #888;
    cursor: pointer;
    transition: color 0.15s, background 0.15s;
  }

  .input-row:hover .icon-btn {
    color: #aaa;
  }

  .icon-btn:hover {
    background: #ffffff10;
    color: #e0e0e0;
  }

  .clear-btn:hover {
    color: #ef233c;
  }

  .browse-btn:hover {
    color: #4cc9f0;
  }
</style>
