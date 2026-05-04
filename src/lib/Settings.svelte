<script lang="ts">
  import {
    m,
    selectLocale,
    getLocale,
    hasExplicitLocale,
    locales,
    type Locale,
  } from "$lib/i18n";

  let open = $state(false);
  let buttonEl = $state<HTMLButtonElement>();
  let panelEl = $state<HTMLDivElement>();

  type Selection = "system" | Locale;

  let value = $state<Selection>(
    hasExplicitLocale() ? (getLocale() as Locale) : "system"
  );

  // Localized name of `loc` rendered in its own language ("Deutsch"
  // for de, "English" for en, "Français" for fr). Falls back to the
  // raw code if the runtime can't resolve a display name.
  function autonym(loc: Locale): string {
    try {
      const dn = new Intl.DisplayNames([loc], { type: "language" });
      const out = dn.of(loc);
      return out
        ? out.charAt(0).toLocaleUpperCase(loc) + out.slice(1)
        : loc;
    } catch {
      return loc;
    }
  }

  function toggle() {
    open = !open;
  }

  function onLocaleChange(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value as Selection;
    value = v;
    selectLocale(v);
  }

  function onDocumentClick(e: MouseEvent) {
    if (!open) return;
    const t = e.target as Node;
    if (panelEl?.contains(t) || buttonEl?.contains(t)) return;
    open = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") open = false;
  }
</script>

<svelte:document onclick={onDocumentClick} onkeydown={onKey} />

<div class="wrap">
  <button
    bind:this={buttonEl}
    class="trigger"
    onclick={toggle}
    aria-label={m.settings_label()}
    aria-expanded={open}
    title={m.settings_label()}
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
      <circle cx="12" cy="12" r="3" />
      <path
        d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"
      />
    </svg>
  </button>

  {#if open}
    <div class="panel" bind:this={panelEl} role="dialog">
      <label class="row">
        <span class="row-label">{m.language_picker_label()}</span>
        <select bind:value onchange={onLocaleChange}>
          <option value="system">{m.language_picker_system()}</option>
          {#each locales as loc}
            <option value={loc}>{autonym(loc)}</option>
          {/each}
        </select>
      </label>
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
  }

  .trigger {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    color: #888;
    cursor: pointer;
    transition: border-color 0.15s, color 0.15s, background 0.15s;
  }

  .trigger:hover,
  .trigger[aria-expanded="true"] {
    color: #e0e0e0;
    border-color: #333;
    background: #16213e;
  }

  .panel {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 10;
    min-width: 200px;
    padding: 10px 12px;
    background: #16213e;
    border: 1px solid #333;
    border-radius: 6px;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: 0.85rem;
  }

  .row-label {
    color: #aaa;
  }

  select {
    background: #0f1a30;
    color: #e0e0e0;
    border: 1px solid #333;
    border-radius: 4px;
    padding: 3px 6px;
    font-size: 0.8rem;
    font-family: inherit;
    cursor: pointer;
  }

  select:focus {
    outline: none;
    border-color: #4cc9f0;
  }
</style>
