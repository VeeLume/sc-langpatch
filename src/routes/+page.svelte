<script lang="ts">
  import {
    commands,
    type Installation,
    type ModuleInfo,
    type PatchResult,
  } from "$lib/bindings";
  import InstallationList from "$lib/InstallationList.svelte";
  import ModuleList from "$lib/ModuleList.svelte";
  import PatchResults from "$lib/PatchResults.svelte";

  let installations = $state<Installation[]>([]);
  let selectedInstalls = $state<Set<string>>(new Set());
  let modules = $state<ModuleInfo[]>([]);
  let results = $state<PatchResult[]>([]);
  let loading = $state(true);
  let patching = $state(false);
  let error = $state<string | null>(null);

  let moduleList = $state<ModuleList>();

  async function init() {
    try {
      const installResult = await commands.getInstallations();
      if (installResult.status === "error") {
        error = installResult.error;
        installations = [];
      } else {
        installations = installResult.data;
        selectedInstalls = new Set(installResult.data.map((i) => i.channel));
      }
      modules = await commands.getModules();
      moduleList?.initOptions(modules);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function toggleInstall(channel: string) {
    const next = new Set(selectedInstalls);
    if (next.has(channel)) {
      next.delete(channel);
    } else {
      next.add(channel);
    }
    selectedInstalls = next;
  }

  async function doPatch() {
    patching = true;
    results = [];
    error = null;
    try {
      const selected = installations.filter((i) =>
        selectedInstalls.has(i.channel)
      );
      const patchResult = await commands.patch(selected);
      if (patchResult.status === "ok") {
        results = patchResult.data;
      } else {
        error = patchResult.error;
      }
    } catch (e) {
      error = String(e);
    } finally {
      patching = false;
    }
  }

  init();
</script>

<main>
  {#if loading}
    <p class="status">Loading...</p>
  {:else if error && installations.length === 0}
    <div class="error-box">{error}</div>
  {:else}
    <InstallationList
      {installations}
      selected={selectedInstalls}
      onToggle={toggleInstall}
    />

    <ModuleList
      bind:this={moduleList}
      {modules}
      onModulesChanged={(m) => (modules = m)}
    />

    <section class="actions">
      <button
        class="patch-btn"
        onclick={doPatch}
        disabled={patching || selectedInstalls.size === 0}
      >
        {#if patching}
          Patching...
        {:else}
          Patch All
        {/if}
      </button>
    </section>

    <PatchResults {results} {error} />
  {/if}
</main>

<style>
  :global(body) {
    font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
    margin: 0;
    padding: 0;
    background: #1a1a2e;
    color: #e0e0e0;
  }

  main {
    max-width: 640px;
    margin: 0 auto;
    padding: 24px;
  }

  :global(h2) {
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #888;
    margin: 24px 0 8px;
  }

  :global(h2:first-child) {
    margin-top: 0;
  }

  :global(section) {
    margin-bottom: 8px;
  }

  :global(input[type="checkbox"]) {
    accent-color: #4361ee;
  }

  .actions {
    margin: 24px 0;
  }

  .patch-btn {
    width: 100%;
    padding: 12px;
    font-size: 1rem;
    font-weight: 600;
    border: none;
    border-radius: 8px;
    background: #4361ee;
    color: #fff;
    cursor: pointer;
    transition: background 0.15s;
  }

  .patch-btn:hover:not(:disabled) {
    background: #3a56d4;
  }

  .patch-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .error-box {
    padding: 12px;
    background: #ef233c22;
    border: 1px solid #ef233c44;
    border-radius: 6px;
    color: #ef233c;
    font-size: 0.9rem;
  }

  .status {
    color: #888;
  }
</style>
