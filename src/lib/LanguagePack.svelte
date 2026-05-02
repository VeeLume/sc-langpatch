<script lang="ts">
  import { commands } from "$lib/bindings";
  import { open } from "@tauri-apps/plugin-dialog";

  interface Props {
    path: string | null;
    onChange: (path: string | null) => void;
  }

  let { path, onChange }: Props = $props();

  let urlInput = $state("");

  async function pickFile() {
    const selected = await open({
      title: "Select community language pack (global.ini)",
      multiple: false,
      directory: false,
      filters: [
        { name: "INI files", extensions: ["ini"] },
        { name: "All files", extensions: ["*"] },
      ],
    });

    if (typeof selected === "string") {
      await save(selected);
    }
  }

  async function useUrl() {
    const trimmed = urlInput.trim();
    if (!trimmed) return;
    await save(trimmed);
    urlInput = "";
  }

  async function save(source: string) {
    await commands.setLanguagePack(source);
    onChange(source);
  }

  async function clear() {
    await commands.setLanguagePack(null);
    onChange(null);
  }

  function isUrl(s: string): boolean {
    return s.startsWith("http://") || s.startsWith("https://");
  }

  function filename(p: string): string {
    const parts = p.split(/[/\\]/);
    return parts[parts.length - 1] || p;
  }
</script>

<section>
  <h2>Community language pack</h2>
  <p class="hint">
    Optional. Overlay a translated <code>global.ini</code> (e.g. German)
    before our enhancements are applied. Accepts a local file or a URL
    pointing directly at an <code>.ini</code> file — GitHub
    <code>blob/</code> links work (the repo root page does not).
  </p>

  {#if path}
    <div class="current">
      <span class="filename" title={path}>
        {isUrl(path) ? "URL" : filename(path)}
      </span>
      <span class="full-path" title={path}>{path}</span>
      <div class="actions">
        <button class="secondary" onclick={pickFile}>Pick file…</button>
        <button class="clear" onclick={clear}>Clear</button>
      </div>
    </div>
  {/if}

  <div class="pickers" class:has-current={!!path}>
    <div class="url-row">
      <input
        type="url"
        placeholder="https://github.com/... or https://example.com/de.ini"
        bind:value={urlInput}
        onkeydown={(e) => e.key === "Enter" && useUrl()}
      />
      <button class="use-url" onclick={useUrl} disabled={!urlInput.trim()}>
        Use URL
      </button>
    </div>
    {#if !path}
      <div class="or">or</div>
      <button class="pick" onclick={pickFile}>Pick local file…</button>
    {/if}
  </div>
</section>

<style>
  .hint {
    margin: 0 0 8px;
    color: #888;
    font-size: 0.85rem;
  }

  code {
    background: #ffffff10;
    padding: 1px 4px;
    border-radius: 3px;
    font-size: 0.85em;
  }

  .current {
    padding: 10px 12px;
    background: #16213e;
    border-radius: 6px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 8px;
  }

  .filename {
    font-weight: 600;
    color: #4cc9f0;
  }

  .full-path {
    font-size: 0.75rem;
    color: #888;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .actions {
    display: flex;
    gap: 6px;
    margin-top: 6px;
  }

  .pickers {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .pickers.has-current {
    opacity: 0.85;
  }

  .url-row {
    display: flex;
    gap: 6px;
  }

  input[type="url"] {
    flex: 1;
    padding: 8px 10px;
    border: 1px solid #333;
    border-radius: 4px;
    background: #0f1a30;
    color: #e0e0e0;
    font-size: 0.85rem;
  }

  input[type="url"]:focus {
    outline: none;
    border-color: #4cc9f0;
  }

  button {
    font-size: 0.85rem;
    padding: 6px 10px;
    border-radius: 4px;
    cursor: pointer;
    border: 1px solid #555;
    background: transparent;
    color: #ccc;
    transition: border-color 0.15s, color 0.15s;
  }

  button:hover:not(:disabled) {
    border-color: #4cc9f0;
    color: #4cc9f0;
  }

  button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .or {
    text-align: center;
    color: #666;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    margin: 2px 0;
  }

  .pick {
    padding: 10px;
    font-size: 0.9rem;
  }

  .clear:hover {
    border-color: #ef233c;
    color: #ef233c;
  }
</style>
