<script lang="ts">
  import { m } from "$lib/i18n";

  // Versioned key — bumping the suffix re-shows the notice for users
  // who already dismissed an earlier announcement. v1 = the v0.4.0
  // "we now support translation" intro.
  const STORAGE_KEY = "sc-langpatch:language-notice-dismissed:v1";

  let visible = $state(localStorage.getItem(STORAGE_KEY) === null);

  function dismiss() {
    localStorage.setItem(STORAGE_KEY, "1");
    visible = false;
  }
</script>

{#if visible}
  <div class="notice" role="status">
    <span class="text">{m.language_notice()}</span>
    <button
      type="button"
      class="dismiss"
      onclick={dismiss}
      aria-label={m.language_notice_dismiss()}
      title={m.language_notice_dismiss()}
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
  </div>
{/if}

<style>
  .notice {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 16px;
    padding: 8px 8px 8px 12px;
    background: #4361ee18;
    border: 1px solid #4361ee44;
    border-radius: 6px;
    color: #c8d4e6;
    font-size: 0.85rem;
  }

  .text {
    flex: 1;
  }

  .dismiss {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: #888;
    cursor: pointer;
    transition: color 0.15s, background 0.15s;
  }

  .dismiss:hover {
    background: #ffffff10;
    color: #e0e0e0;
  }
</style>
