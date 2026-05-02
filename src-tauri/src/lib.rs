pub mod discovery;
pub mod merge;
pub mod module;
pub mod modules;
pub mod formatter_helpers;
pub mod preview;

#[cfg(test)]
mod test_helpers;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use module::{ModuleConfig, ModuleContext, ModuleInfo, PatchOp};
use sc_extract::{AssetConfig, AssetData, AssetSource, Datacore, DatacoreConfig, LocaleMap};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::{Builder, collect_commands};

// ── App state ───────────────────────────────────────────────────────────────

/// Persistent state shared across Tauri commands.
struct AppState {
    /// Per-module configs (module_id → config).
    configs: HashMap<String, ModuleConfig>,
    /// Optional path to a community language pack INI that overlays the
    /// English base before our patches are applied.
    language_pack_path: Option<String>,
    /// Channels (e.g. "LIVE", "PTU") the user has selected in the UI. Empty
    /// means the frontend falls back to "all discovered installations".
    selected_channels: Vec<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            configs: HashMap::new(),
            language_pack_path: None,
            selected_channels: Vec::new(),
        }
    }
}

// ── Persistence ─────────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language_pack: Option<String>,
    #[serde(default)]
    selected_channels: Vec<String>,
    #[serde(default)]
    modules: HashMap<String, ModuleConfig>,
}

/// Location of the persistent settings file (`%APPDATA%\sc-langpatch\config.toml`
/// on Windows, `$XDG_CONFIG_HOME/sc-langpatch/config.toml` on Linux).
fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("sc-langpatch").join("config.toml"))
}

fn load_persisted() -> PersistedConfig {
    let Some(path) = config_path() else {
        return PersistedConfig::default();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return PersistedConfig::default();
    };
    match toml::from_str(&content) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("  Failed to parse {}: {e}", path.display());
            PersistedConfig::default()
        }
    }
}

fn save_persisted(state: &AppState) {
    let Some(path) = config_path() else {
        return;
    };
    let persisted = PersistedConfig {
        language_pack: state.language_pack_path.clone(),
        selected_channels: state.selected_channels.clone(),
        modules: state.configs.clone(),
    };
    let content = match toml::to_string_pretty(&persisted) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("  Failed to serialize config: {e}");
            return;
        }
    };
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("  Failed to create {}: {e}", dir.display());
            return;
        }
    }
    if let Err(e) = std::fs::write(&path, content) {
        eprintln!("  Failed to write {}: {e}", path.display());
    }
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
fn get_installations() -> Result<Vec<discovery::Installation>, String> {
    discovery::find_installations().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
#[specta::specta]
fn get_modules(state: tauri::State<'_, Mutex<AppState>>) -> Vec<ModuleInfo> {
    let state = state.lock().unwrap();
    let all_modules = modules::builtin_modules();

    all_modules
        .iter()
        .map(|m| {
            let config = state.configs.get(m.id());
            let enabled = config
                .and_then(|c| c.enabled)
                .unwrap_or(m.default_enabled());

            ModuleInfo {
                id: m.id().to_string(),
                name: m.name().to_string(),
                description: m.description().to_string(),
                default_enabled: m.default_enabled(),
                enabled,
                needs_datacore: m.needs_datacore(),
                options: m.options(),
                option_values: config.map(|c| c.options.clone()).unwrap_or_default(),
            }
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
fn set_module_config(
    state: tauri::State<'_, Mutex<AppState>>,
    module_id: String,
    config: ModuleConfig,
) {
    let mut state = state.lock().unwrap();
    state.configs.insert(module_id, config);
    save_persisted(&state);
}

#[tauri::command]
#[specta::specta]
fn get_language_pack(state: tauri::State<'_, Mutex<AppState>>) -> Option<String> {
    state.lock().unwrap().language_pack_path.clone()
}

#[tauri::command]
#[specta::specta]
fn set_language_pack(state: tauri::State<'_, Mutex<AppState>>, path: Option<String>) {
    let mut state = state.lock().unwrap();
    state.language_pack_path = path.filter(|s| !s.is_empty());
    save_persisted(&state);
}

#[tauri::command]
#[specta::specta]
fn get_selected_channels(state: tauri::State<'_, Mutex<AppState>>) -> Vec<String> {
    state.lock().unwrap().selected_channels.clone()
}

#[tauri::command]
#[specta::specta]
fn set_selected_channels(state: tauri::State<'_, Mutex<AppState>>, channels: Vec<String>) {
    let mut state = state.lock().unwrap();
    state.selected_channels = channels;
    save_persisted(&state);
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct PatchResult {
    pub channel: String,
    pub applied: u32,
    pub total: u32,
    /// Per-module patch counts and Replace-conflict details. One entry
    /// per enabled module that ran, in the order they ran (priority).
    pub module_stats: Vec<ModuleStat>,
    /// True issues only — module errors, skips for missing datacore /
    /// locale, etc. Not per-module stats (those are `module_stats`).
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// Per-module outcome for a single patch run.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModuleStat {
    pub module_id: String,
    pub module_name: String,
    /// Number of (key, op) pairs this module emitted.
    pub patches: u32,
    /// Replace ops from this module that landed on a key another,
    /// earlier-running module had already Replaced. The later Replace
    /// wins and the earlier module's value is discarded. Aggregated by
    /// the overridden module's name (→ number of overridden keys).
    pub replace_overrides: Vec<ReplaceOverride>,
}

/// Aggregated count of Replace conflicts against a single earlier module.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ReplaceOverride {
    /// Name of the earlier module whose Replace was overridden.
    pub overrode_module: String,
    pub keys: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct RemoveResult {
    pub channel: String,
    pub removed: bool,
    pub error: Option<String>,
}

#[tauri::command]
#[specta::specta]
async fn patch(
    state: tauri::State<'_, Mutex<AppState>>,
    installations: Vec<discovery::Installation>,
) -> Result<Vec<PatchResult>, String> {
    // Clone what we need so we can drop the lock before the heavy work
    let (configs, language_pack_path) = {
        let s = state.lock().unwrap();
        (s.configs.clone(), s.language_pack_path.clone())
    };

    tauri::async_runtime::spawn_blocking(move || {
        installations
            .iter()
            .map(|inst| {
                match patch_installation(inst, &configs, language_pack_path.as_deref()) {
                    Ok(result) => result,
                    Err(e) => PatchResult {
                        channel: inst.channel.clone(),
                        applied: 0,
                        total: 0,
                        module_stats: Vec::new(),
                        warnings: Vec::new(),
                        error: Some(format!("{e:#}")),
                    },
                }
            })
            .collect()
    })
    .await
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
#[specta::specta]
async fn remove_patch(
    installations: Vec<discovery::Installation>,
) -> Result<Vec<RemoveResult>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        installations
            .iter()
            .map(|inst| {
                let install_path = Path::new(&inst.path);
                let output_dir = discovery::output_dir(install_path);
                match merge::remove_output(&output_dir) {
                    Ok(removed) => RemoveResult {
                        channel: inst.channel.clone(),
                        removed,
                        error: None,
                    },
                    Err(e) => RemoveResult {
                        channel: inst.channel.clone(),
                        removed: false,
                        error: Some(format!("{e:#}")),
                    },
                }
            })
            .collect()
    })
    .await
    .map_err(|e| format!("{e:#}"))
}

// ── Core patching logic ─────────────────────────────────────────────────────

fn patch_installation(
    inst: &discovery::Installation,
    configs: &HashMap<String, ModuleConfig>,
    language_pack_path: Option<&str>,
) -> Result<PatchResult> {
    let install_path = Path::new(&inst.path);
    let p4k_path = install_path.join("Data.p4k");

    // Open archive via sc-extract — single source of truth for both the raw
    // global.ini bytes (existing patcher pipeline) and the typed
    // Datacore / LocaleMap consumed by sc-holotable-backed modules.
    let assets = AssetSource::open(&p4k_path)
        .with_context(|| format!("Failed to open {}", p4k_path.display()))?;

    let ini_bytes = assets
        .read("Data/Localization/english/global.ini")
        .with_context(|| "global.ini not found in Data.p4k")?;

    // Decide once which holotable artefacts the current module set wants.
    let needs = ModuleNeeds::collect(configs);

    // Datacore extraction goes through sc-extract's staged pipeline.
    //
    // We use `AssetConfig::standard()` (locale on) rather than `minimal()`
    // because `Datacore::parse` builds [`DisplayNameCache`] eagerly from
    // `asset_data.locale`, and that cache is what every downstream
    // resolver (BlueprintPoolRegistry item names, ShipRegistry display
    // names, etc.) reaches into for human-readable text. With
    // `minimal()` the cache comes back empty and every resolved
    // entity name turns into the empty string — the "0 with
    // display_name" failure mode we saw when modules like
    // `mission_enhancer` started leaning on these caches.
    //
    // The trade-off: `DisplayNameCache` is baked from BASE English at
    // parse time, so when a community language pack is overlaid below,
    // those baked names won't pick up the translations. Module-level
    // INI lookups *do* see the overlaid strings (we hand them the
    // post-overlay [`LocaleMap`] further down), so player-visible
    // patches still translate correctly — but cross-record name
    // resolution (e.g. blueprint pool → entity → name) stays English.
    // Acceptable until / unless we add a mechanism to rebuild the
    // cache against the post-overlay locale.
    let datacore: Option<Datacore> = if needs.datacore {
        match AssetData::extract(&assets, &AssetConfig::standard())
            .and_then(|d| Datacore::parse(&assets, &d, &DatacoreConfig::standard()))
        {
            Ok(dc) => Some(dc),
            Err(e) => {
                eprintln!("  Datacore extract/parse failed: {e:#}");
                None
            }
        }
    } else {
        None
    };

    // Drop the archive before writing patched files back to the install dir.
    drop(assets);

    // Decode INI
    let mut ini_content = merge::decode_ini(&ini_bytes)?;

    let mut warnings = Vec::new();

    // Overlay community language pack if configured
    if let Some(source) = language_pack_path {
        match fetch_language_pack(source) {
            Ok(bytes) => match merge::decode_ini_auto(&bytes) {
                Ok(pack_content) => {
                    ini_content = merge::apply_language_pack(&ini_content, &pack_content);
                }
                Err(e) => warnings.push(format!("Language pack decode failed: {e:#}")),
            },
            Err(e) => warnings.push(format!("Language pack load failed: {e:#}")),
        }
    }

    // Build the LocaleMap that holotable modules see *after* the community
    // language pack is overlaid. This way `LocaleKey` resolution by sc-contracts
    // / sc-weapons returns translated names — annotations stay consistent
    // with the player-facing strings the language pack already rewrote.
    // Rebuilt once now (post-overlay, pre-our-patches) and held for both phases.
    let locale: Option<LocaleMap> = if needs.locale {
        let map = merge::parse_ini(&ini_content);
        let mut lm = LocaleMap::new();
        for (k, v) in &map {
            lm.set(k.as_str(), v.as_str());
        }
        Some(lm)
    } else {
        None
    };
    let locale_ref: Option<&LocaleMap> = locale.as_ref();

    // Collect all enabled modules, sorted by priority
    let mut all_modules = modules::builtin_modules();
    all_modules.sort_by_key(|m| m.priority());

    // Phase 1: collect and apply key renames
    let mut all_renames = Vec::new();
    for module in &all_modules {
        let config = configs.get(module.id()).cloned().unwrap_or_default();
        let enabled = config.enabled.unwrap_or(module.default_enabled());
        if !enabled {
            continue;
        }

        let ini_map = merge::parse_ini(&ini_content);
        let ctx = ModuleContext {
            db: datacore.as_ref().map(Datacore::db),
            datacore: datacore.as_ref(),
            locale: locale_ref,
            ini: &ini_map,
            config: &config,
        };

        match module.generate_renames(&ctx) {
            Ok(renames) => all_renames.extend(renames),
            Err(e) => warnings.push(format!("{}: {e}", module.name())),
        }
    }

    let ini_content = if all_renames.is_empty() {
        ini_content
    } else {
        merge::apply_renames(&ini_content, &all_renames)
    };

    // Phase 2: collect and apply value patches (using renamed INI)
    let ini_map = merge::parse_ini(&ini_content);
    // Each key can carry a stack of ops — Prefix/Suffix from multiple
    // modules compose, and only a duplicate Replace is a true semantic
    // conflict (flagged via module_stats).
    let mut merged_patches: HashMap<String, Vec<PatchOp>> = HashMap::new();
    // Who last placed a Replace on each key — lets us name the module
    // that gets overridden when a later module Replaces the same key.
    let mut replace_owner: HashMap<String, String> = HashMap::new();
    let mut module_stats: Vec<ModuleStat> = Vec::new();

    for module in &all_modules {
        let config = configs.get(module.id()).cloned().unwrap_or_default();
        let enabled = config.enabled.unwrap_or(module.default_enabled());
        if !enabled {
            continue;
        }

        if module.needs_datacore() && datacore.is_none() {
            warnings.push(format!("{}: skipped (datacore unavailable)", module.name()));
            continue;
        }
        if module.needs_locale() && locale_ref.is_none() {
            warnings.push(format!("{}: skipped (locale unavailable)", module.name()));
            continue;
        }

        let ctx = ModuleContext {
            db: datacore.as_ref().map(Datacore::db),
            datacore: datacore.as_ref(),
            locale: locale_ref,
            ini: &ini_map,
            config: &config,
        };

        let module_name = module.name().to_string();
        let module_id = module.id().to_string();
        match module.generate_patches(&ctx) {
            Ok(patches) => {
                let produced = patches.len();
                // Inter-module Replace conflicts only: this module's
                // Replace lands on a key a *different* earlier module
                // already Replaced. Same-module duplicates (the module
                // emitting multiple Replaces on one key) are a module-
                // internal matter and not reported here.
                let mut overrides: HashMap<String, u32> = HashMap::new();
                for (key, op) in patches {
                    let is_replace = matches!(op, PatchOp::Replace(_));
                    if is_replace
                        && let Some(prev_owner) = replace_owner.get(&key)
                        && prev_owner != &module_name
                    {
                        *overrides.entry(prev_owner.clone()).or_default() += 1;
                    }
                    if is_replace {
                        replace_owner.insert(key.clone(), module_name.clone());
                    }
                    merged_patches.entry(key).or_default().push(op);
                }
                let mut override_vec: Vec<ReplaceOverride> = overrides
                    .into_iter()
                    .map(|(overrode_module, keys)| ReplaceOverride { overrode_module, keys })
                    .collect();
                override_vec.sort_by(|a, b| b.keys.cmp(&a.keys));
                module_stats.push(ModuleStat {
                    module_id,
                    module_name,
                    patches: produced as u32,
                    replace_overrides: override_vec,
                });
            }
            Err(e) => {
                warnings.push(format!("{module_name}: {e}"));
            }
        }
    }

    let total = merged_patches.len() as u32;

    // Apply value patches
    let patched = merge::apply_patches(&ini_content, &merged_patches);

    // Count how many actually matched
    let applied = count_applied(&ini_content, &merged_patches) as u32;

    // Write output
    let output_dir = discovery::output_dir(install_path);
    merge::write_output(&output_dir, &patched)?;

    // Write debug diff files to %LOCALAPPDATA%\sc-langpatch\debug\
    #[cfg(debug_assertions)]
    if let Some(debug_dir) = discovery::debug_dir() {
        let version = discovery::game_version(install_path, &inst.channel)
            .unwrap_or_else(|| inst.channel.to_lowercase());
        let hash = options_hash(configs);
        let _ = merge::write_diff(&debug_dir, &version, &hash, &ini_content, &merged_patches);
    }

    Ok(PatchResult {
        channel: inst.channel.clone(),
        applied,
        total,
        module_stats,
        warnings,
        error: None,
    })
}

/// Load the language pack bytes from either an HTTP(S) URL or a local file path.
///
/// GitHub `blob` and `raw` web-UI URLs are transparently rewritten to
/// `raw.githubusercontent.com` so that casual copy-paste works. Responses
/// with a `text/html` content-type are rejected — this catches the common
/// mistake of pasting a GitHub repo/folder page instead of a file link.
fn fetch_language_pack(source: &str) -> Result<Vec<u8>> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let url = normalize_language_pack_url(source);
        let response = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .call()
            .with_context(|| format!("HTTP request failed for {url}"))?;

        if let Some(ct) = response.header("content-type") {
            if ct.trim_start().to_ascii_lowercase().starts_with("text/html") {
                anyhow::bail!(
                    "URL returned HTML (content-type: {ct}). Use a direct file link \
                     like https://github.com/USER/REPO/blob/BRANCH/path/global.ini \
                     or a raw.githubusercontent.com URL — not the repo/folder page."
                );
            }
        }

        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(64 * 1024 * 1024) // 64 MiB cap
            .read_to_end(&mut bytes)
            .with_context(|| format!("Reading response body from {url}"))?;
        Ok(bytes)
    } else {
        std::fs::read(source).with_context(|| format!("Reading {source}"))
    }
}

/// Rewrite well-known GitHub web URLs to their raw-file equivalents.
///
/// Accepts:
///   - https://github.com/USER/REPO/blob/BRANCH/PATH
///   - https://github.com/USER/REPO/raw/BRANCH/PATH
/// Returns the corresponding `raw.githubusercontent.com` URL. Any other URL
/// is returned unchanged.
fn normalize_language_pack_url(url: &str) -> String {
    const GH: &str = "https://github.com/";
    if let Some(rest) = url.strip_prefix(GH) {
        // Expect USER/REPO/(blob|raw)/BRANCH/PATH
        let parts: Vec<&str> = rest.splitn(5, '/').collect();
        if parts.len() == 5 && (parts[2] == "blob" || parts[2] == "raw") {
            return format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                parts[0], parts[1], parts[3], parts[4]
            );
        }
    }
    url.to_string()
}

/// What heavyweight artefacts the currently enabled modules collectively need.
/// Computed once per `patch_installation` call so we pay each cost at most once.
struct ModuleNeeds {
    datacore: bool,
    locale: bool,
}

impl ModuleNeeds {
    fn collect(configs: &HashMap<String, ModuleConfig>) -> Self {
        let all = modules::builtin_modules();
        let enabled = |m: &Box<dyn module::Module>| {
            configs
                .get(m.id())
                .and_then(|c| c.enabled)
                .unwrap_or(m.default_enabled())
        };
        Self {
            datacore: all.iter().any(|m| enabled(m) && m.needs_datacore()),
            locale: all.iter().any(|m| enabled(m) && m.needs_locale()),
        }
    }
}

/// Produce a short deterministic hex hash of the active module configs.
///
/// The hash changes whenever any module is enabled/disabled or its options
/// change, making it suitable for use in debug diff filenames.
fn options_hash(configs: &HashMap<String, ModuleConfig>) -> String {
    // Serialize to a sorted, stable JSON string and run FNV-1a over it.
    let mut sorted: Vec<_> = configs.iter().collect();
    sorted.sort_by_key(|(k, _)| k.as_str());
    let serialized = serde_json::to_string(&sorted).unwrap_or_default();

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in serialized.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", hash as u32)
}

fn count_applied(ini_content: &str, patches: &HashMap<String, Vec<PatchOp>>) -> usize {
    let mut count = 0;
    for line in ini_content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let key = &line[..eq_pos];
            if patches.contains_key(key) {
                count += 1;
            }
        }
    }
    count
}

// ── App setup ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        get_installations,
        get_modules,
        set_module_config,
        get_language_pack,
        set_language_pack,
        get_selected_channels,
        set_selected_channels,
        patch,
        remove_patch,
    ]);

    #[cfg(debug_assertions)]
    builder
        .export(
            specta_typescript::Typescript::default(),
            "../src/lib/bindings.ts",
        )
        .expect("Failed to export TypeScript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(Mutex::new({
            let persisted = load_persisted();
            AppState {
                configs: persisted.modules,
                language_pack_path: persisted.language_pack,
                selected_channels: persisted.selected_channels,
            }
        }))
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
