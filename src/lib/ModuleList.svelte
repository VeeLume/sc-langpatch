<script lang="ts">
  import {
    commands,
    type ModuleInfo,
    type ModuleConfig,
    type OptionEntry,
    type OptionValue,
  } from "$lib/bindings";
  import {
    m,
    moduleName,
    moduleDescription,
    optionLabel,
    optionDescription,
    choiceLabel,
  } from "$lib/i18n";

  interface Props {
    modules: ModuleInfo[];
    onModulesChanged: (modules: ModuleInfo[]) => void;
  }

  let { modules, onModulesChanged }: Props = $props();

  let legendOpen = $state(false);

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
  <div class="heading-row">
    <h2>{m.modules_heading()}</h2>
    <button
      type="button"
      class="info-btn"
      onclick={() => (legendOpen = !legendOpen)}
      aria-expanded={legendOpen}
      aria-label={legendOpen
        ? m.modules_hide_legend()
        : m.modules_show_legend()}
      title={legendOpen ? m.modules_hide_legend() : m.modules_show_legend()}
    >
      ?
    </button>
  </div>

  {#if legendOpen}
    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
    <p class="legend">{@html m.modules_legend_html()}</p>
  {/if}

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
              <span class="module-name">{moduleName(mod.id, mod.name)}</span>
              {#if mod.needs_datacore}
                <span class="badge" title={m.modules_badge_dcb_tooltip()}>
                  {m.modules_badge_dcb()}
                </span>
              {/if}
              {#if mod.uses_replace_ops}
                <span
                  class="badge badge-warn"
                  title={m.modules_badge_replace_tooltip()}
                >
                  {m.modules_badge_replace()}
                </span>
              {/if}
            </div>
            <span class="module-desc">
              {moduleDescription(mod.id, mod.description)}
            </span>
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
                    <span class="option-bool-label">
                      {optionLabel(mod.id, opt.id, opt.label)}
                    </span>
                    {#if opt.description}
                      <span class="option-bool-desc">
                        {optionDescription(mod.id, opt.id, opt.description)}
                      </span>
                    {/if}
                  </span>
                </label>
              {:else}
                <div class="option-field">
                  <label class="option-field-label" for="{mod.id}-{opt.id}">
                    {optionLabel(mod.id, opt.id, opt.label)}
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
                        <option value={choice.value}>
                          {choiceLabel(
                            mod.id,
                            opt.id,
                            choice.value,
                            choice.label
                          )}
                        </option>
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
                    <span class="option-field-desc">
                      {optionDescription(mod.id, opt.id, opt.description)}
                    </span>
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
  .heading-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 24px 0 8px;
  }

  .heading-row :global(h2) {
    margin: 0;
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

  .legend {
    margin: 0 0 8px;
    color: #aaa;
    font-size: 0.85rem;
    line-height: 1.5;
  }

  .legend :global(strong) {
    color: #e0e0e0;
    font-weight: 600;
  }

  .badge[title],
  .badge-warn {
    cursor: help;
  }

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

  .badge-warn {
    background: #f4a26133;
    color: #f4a261;
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
