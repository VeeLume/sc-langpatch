<script lang="ts">
  import {
    commands,
    type Installation,
    type ModuleInfo,
    type ModuleConfig,
    type OptionEntry,
    type OptionValue,
    type PatchResult,
  } from "$lib/bindings";

  let installations = $state<Installation[]>([]);
  let selectedInstalls = $state<Set<string>>(new Set());
  let modules = $state<ModuleInfo[]>([]);
  let results = $state<PatchResult[]>([]);
  let loading = $state(true);
  let patching = $state(false);
  let error = $state<string | null>(null);

  // Track user-chosen option values per module: moduleId -> { optionName -> OptionValue }
  let moduleOptions = $state<Record<string, Record<string, OptionValue>>>({});

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

      // Initialize option values from defaults
      for (const mod of modules) {
        if (mod.options.length > 0) {
          moduleOptions[mod.id] = {};
          for (const opt of mod.options) {
            moduleOptions[mod.id][opt.id] = defaultValue(opt);
          }
        }
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function defaultValue(
    opt: import("$lib/bindings").ModuleOption
  ): OptionValue {
    switch (opt.kind.type) {
      case "Bool":
        return { type: "Bool", value: opt.default === "true" };
      case "String":
        return { type: "String", value: opt.default };
      case "Choice":
        return { type: "Choice", value: opt.default };
    }
  }

  function buildConfig(mod: ModuleInfo, enabled: boolean): ModuleConfig {
    const entries: OptionEntry[] = [];
    const opts = moduleOptions[mod.id];
    if (opts) {
      for (const [name, value] of Object.entries(opts)) {
        entries.push({ name, value });
      }
    }
    return { enabled, options: entries };
  }

  async function toggleModule(mod: ModuleInfo) {
    const newEnabled = !mod.enabled;
    await commands.setModuleConfig(mod.id, buildConfig(mod, newEnabled));
    modules = await commands.getModules();
  }

  async function updateOption(
    mod: ModuleInfo,
    optName: string,
    value: OptionValue
  ) {
    if (!moduleOptions[mod.id]) moduleOptions[mod.id] = {};
    moduleOptions[mod.id][optName] = value;
    await commands.setModuleConfig(mod.id, buildConfig(mod, mod.enabled));
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
      results = await commands.patch(selected);
    } catch (e) {
      error = String(e);
    } finally {
      patching = false;
    }
  }

  init();
</script>

<main>
  <h1>SC Comp LangPack</h1>

  {#if loading}
    <p class="status">Loading...</p>
  {:else if error && installations.length === 0}
    <div class="error-box">{error}</div>
  {:else}
    <!-- Installations -->
    <section>
      <h2>Installations</h2>
      {#if installations.length === 0}
        <p class="muted">No Star Citizen installations found.</p>
      {:else}
        <div class="list">
          {#each installations as inst}
            <label class="list-item">
              <input
                type="checkbox"
                checked={selectedInstalls.has(inst.channel)}
                onchange={() => toggleInstall(inst.channel)}
              />
              <span class="channel">{inst.channel}</span>
              <span class="path">{inst.path}</span>
            </label>
          {/each}
        </div>
      {/if}
    </section>

    <!-- Modules -->
    <section>
      <h2>Modules</h2>
      <div class="list">
        {#each modules as mod}
          <div class="module-item" class:module-disabled={!mod.enabled}>
            <label class="module-header">
              <input
                type="checkbox"
                checked={mod.enabled}
                onchange={() => toggleModule(mod)}
              />
              <div class="module-info">
                <div class="module-title-row">
                  <span class="module-name">{mod.name}</span>
                  {#if mod.needs_datacore}
                    <span class="badge">DCB</span>
                  {/if}
                </div>
                <span class="module-desc">{mod.description}</span>
              </div>
            </label>

            {#if mod.enabled && mod.options.length > 0}
              <div class="module-options">
                {#each mod.options as opt}
                  <div class="option-row">
                    <label class="option-label" for="{mod.id}-{opt.id}">
                      {opt.label}
                    </label>

                    {#if opt.kind.type === "Bool"}
                      {@const v = moduleOptions[mod.id]?.[opt.id]}
                      <input
                        id="{mod.id}-{opt.id}"
                        type="checkbox"
                        checked={v?.type === "Bool" ? v.value : opt.default === "true"}
                        onchange={(e) =>
                          updateOption(mod, opt.id, {
                            type: "Bool",
                            value: e.currentTarget.checked,
                          })}
                      />
                    {:else if opt.kind.type === "Choice"}
                      <select
                        id="{mod.id}-{opt.id}"
                        value={moduleOptions[mod.id]?.[opt.id]?.type ===
                        "Choice"
                          ? moduleOptions[mod.id][opt.id].value
                          : opt.default}
                        onchange={(e) =>
                          updateOption(mod, opt.id, {
                            type: "Choice",
                            value: e.currentTarget.value,
                          })}
                      >
                        {#each opt.kind.choices as choice}
                          <option value={choice.value}>{choice.label}</option>
                        {/each}
                      </select>
                    {:else if opt.kind.type === "String"}
                      <input
                        id="{mod.id}-{opt.id}"
                        type="text"
                        value={moduleOptions[mod.id]?.[opt.id]?.type ===
                        "String"
                          ? moduleOptions[mod.id][opt.id].value
                          : opt.default}
                        onchange={(e) =>
                          updateOption(mod, opt.id, {
                            type: "String",
                            value: e.currentTarget.value,
                          })}
                      />
                    {/if}

                    {#if opt.description}
                      <span class="option-desc">{opt.description}</span>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    </section>

    <!-- Patch button -->
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

    <!-- Results -->
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
                    ({result.warnings.length} warnings)
                  {/if}
                </span>
              {/if}
            </div>
            {#each result.warnings as warning}
              <div class="warning">{warning}</div>
            {/each}
          {/each}
        </div>
      </section>
    {/if}
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

  h1 {
    font-size: 1.5rem;
    margin: 0 0 24px;
    color: #fff;
  }

  h2 {
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #888;
    margin: 24px 0 8px;
  }

  section {
    margin-bottom: 8px;
  }

  /* Shared list styles */
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

  /* Installations */
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

  /* Modules */
  .module-item {
    background: #16213e;
    border-radius: 6px;
    overflow: hidden;
  }

  .module-item:hover {
    background: #1a2745;
  }

  .module-disabled {
    opacity: 0.6;
  }

  .module-header {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 12px;
    cursor: pointer;
  }

  .module-header input[type="checkbox"] {
    margin-top: 3px;
  }

  .module-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .module-title-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .module-name {
    font-weight: 500;
    color: #e0e0e0;
  }

  .module-desc {
    font-size: 0.8rem;
    color: #777;
  }

  .badge {
    display: inline-block;
    font-size: 0.6rem;
    padding: 1px 5px;
    border-radius: 3px;
    background: #4361ee33;
    color: #4361ee;
    font-weight: 600;
  }

  /* Module options */
  .module-options {
    padding: 4px 12px 8px 34px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    border-top: 1px solid #ffffff0a;
  }

  .option-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.85rem;
  }

  .option-label {
    color: #aaa;
    min-width: 60px;
  }

  .option-desc {
    color: #666;
    font-size: 0.75rem;
  }

  .module-options select,
  .module-options input[type="text"] {
    background: #0f0f1e;
    color: #e0e0e0;
    border: 1px solid #333;
    border-radius: 4px;
    padding: 3px 8px;
    font-size: 0.8rem;
    font-family: inherit;
  }

  .module-options select:focus,
  .module-options input[type="text"]:focus {
    outline: none;
    border-color: #4361ee;
  }

  /* Actions */
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

  /* Results */
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

  .muted {
    color: #666;
    font-size: 0.9rem;
  }

  .status {
    color: #888;
  }

  input[type="checkbox"] {
    accent-color: #4361ee;
  }
</style>
