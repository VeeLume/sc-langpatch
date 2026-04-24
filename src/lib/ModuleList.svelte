<script lang="ts">
  import {
    commands,
    type ModuleInfo,
    type ModuleConfig,
    type OptionEntry,
    type OptionValue,
  } from "$lib/bindings";

  interface Props {
    modules: ModuleInfo[];
    onModulesChanged: (modules: ModuleInfo[]) => void;
  }

  let { modules, onModulesChanged }: Props = $props();

  // Track user-chosen option values per module: moduleId -> { optionName -> OptionValue }
  let moduleOptions = $state<Record<string, Record<string, OptionValue>>>({});

  // Seed moduleOptions from each ModuleInfo. Prefers persisted `option_values`
  // from the backend; falls back to the option's declared default. Runs once
  // per module — subsequent updates come from updateOption() to avoid
  // clobbering in-flight UI state.
  $effect(() => {
    for (const mod of modules) {
      if (mod.options.length === 0) continue;
      if (moduleOptions[mod.id]) continue;
      const saved = new Map(mod.option_values.map((e) => [e.name, e.value]));
      const values: Record<string, OptionValue> = {};
      for (const opt of mod.options) {
        values[opt.id] = saved.get(opt.id) ?? defaultValue(opt);
      }
      moduleOptions[mod.id] = values;
    }
  });

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
    await commands.setModuleConfig(mod.id, buildConfig(mod, !mod.enabled));
    onModulesChanged(await commands.getModules());
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
</script>

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
              {#if opt.kind.type === "Bool"}
                {@const v = moduleOptions[mod.id]?.[opt.id]}
                <label class="option-bool">
                  <input
                    type="checkbox"
                    checked={v?.type === "Bool" ? v.value : opt.default === "true"}
                    onchange={(e) =>
                      updateOption(mod, opt.id, {
                        type: "Bool",
                        value: e.currentTarget.checked,
                      })}
                  />
                  <span class="option-bool-text">
                    <span class="option-bool-label">{opt.label}</span>
                    {#if opt.description}
                      <span class="option-bool-desc">{opt.description}</span>
                    {/if}
                  </span>
                </label>
              {:else}
                <div class="option-field">
                  <label class="option-field-label" for="{mod.id}-{opt.id}">
                    {opt.label}
                  </label>
                  {#if opt.kind.type === "Choice"}
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
                    <span class="option-field-desc">{opt.description}</span>
                  {/if}
                </div>
              {/if}
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </div>
</section>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

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
    color: #999;
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

  .module-options {
    padding: 4px 12px 8px 34px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    border-top: 1px solid #ffffff0a;
  }

  /* Bool options — checkbox + label + description, mirroring module header */
  .option-bool {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    cursor: pointer;
    font-size: 0.85rem;
  }

  .option-bool input[type="checkbox"] {
    margin-top: 2px;
  }

  .option-bool-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .option-bool-label {
    color: #ccc;
    font-weight: 500;
  }

  .option-bool-desc {
    color: #888;
    font-size: 0.75rem;
  }

  /* Choice / String options — label above, control below */
  .option-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 0.85rem;
  }

  .option-field-label {
    color: #aaa;
    font-weight: 500;
  }

  .option-field-desc {
    color: #888;
    font-size: 0.75rem;
  }

  .module-options select,
  .module-options input[type="text"] {
    background: #0f0f1e;
    color: #e0e0e0;
    border: 1px solid #333;
    border-radius: 4px;
    padding: 5px 8px;
    font-size: 0.8rem;
    font-family: inherit;
  }

  .module-options select:focus,
  .module-options input[type="text"]:focus {
    outline: none;
    border-color: #4361ee;
  }
</style>
