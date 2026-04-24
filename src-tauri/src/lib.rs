pub mod discovery;
pub mod merge;
pub mod module;
pub mod modules;

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
use serde::{Deserialize, Serialize};
use specta::Type;
use svarog_datacore::DataCoreDatabase;
use svarog_p4k::P4kArchive;
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
    pub warnings: Vec<String>,
    pub error: Option<String>,
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

    // Open archive and extract global.ini
    let archive = P4kArchive::open(&p4k_path)
        .with_context(|| format!("Failed to open {}", p4k_path.display()))?;

    let ini_entry = archive
        .find("Data/Localization/english/global.ini")
        .with_context(|| "global.ini not found in Data.p4k")?;

    let ini_bytes = archive
        .read(&ini_entry)
        .with_context(|| "Failed to read global.ini")?;

    // Try to extract Game2.dcb for code modules
    let dcb: Option<DataCoreDatabase> = extract_datacore(&archive, configs);

    // Drop archive before writing
    drop(archive);

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
            db: dcb.as_ref(),
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
    let mut merged_patches: HashMap<String, PatchOp> = HashMap::new();

    for module in &all_modules {
        let config = configs.get(module.id()).cloned().unwrap_or_default();
        let enabled = config.enabled.unwrap_or(module.default_enabled());
        if !enabled {
            continue;
        }

        if module.needs_datacore() && dcb.is_none() {
            continue;
        }

        let ctx = ModuleContext {
            db: dcb.as_ref(),
            ini: &ini_map,
            config: &config,
        };

        match module.generate_patches(&ctx) {
            Ok(patches) => {
                for (key, op) in patches {
                    merged_patches.insert(key, op);
                }
            }
            Err(e) => {
                warnings.push(format!("{}: {e}", module.name()));
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

fn extract_datacore(
    archive: &P4kArchive,
    configs: &HashMap<String, ModuleConfig>,
) -> Option<DataCoreDatabase> {
    // Only extract if any enabled module needs it
    let all_modules = modules::builtin_modules();
    let any_needs_dcb = all_modules.iter().any(|m| {
        if !m.needs_datacore() {
            return false;
        }
        let enabled = configs
            .get(m.id())
            .and_then(|c| c.enabled)
            .unwrap_or(m.default_enabled());
        enabled
    });

    if !any_needs_dcb {
        return None;
    }

    let entry = archive.find("Data/Game2.dcb")?;
    let bytes = archive.read(&entry).ok()?;
    DataCoreDatabase::parse(&bytes).ok()
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

fn count_applied(ini_content: &str, patches: &HashMap<String, PatchOp>) -> usize {
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
