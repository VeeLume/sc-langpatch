pub mod discovery;
pub mod merge;
pub mod module;
pub mod modules;

#[cfg(test)]
mod test_helpers;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use module::{ModuleConfig, ModuleContext, ModuleInfo, PatchOp};
use serde::Serialize;
use specta::Type;
use svarog_datacore::DataCoreDatabase;
use svarog_p4k::P4kArchive;
use tauri_specta::{Builder, collect_commands};

// ── App state ───────────────────────────────────────────────────────────────

/// Persistent state shared across Tauri commands.
struct AppState {
    /// Per-module configs (module_id → config).
    configs: HashMap<String, ModuleConfig>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            configs: HashMap::new(),
        }
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
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct PatchResult {
    pub channel: String,
    pub applied: u32,
    pub total: u32,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

#[tauri::command]
#[specta::specta]
async fn patch(
    state: tauri::State<'_, Mutex<AppState>>,
    installations: Vec<discovery::Installation>,
) -> Result<Vec<PatchResult>, String> {
    // Clone what we need so we can drop the lock before the heavy work
    let configs = state.lock().unwrap().configs.clone();

    tauri::async_runtime::spawn_blocking(move || {
        installations
            .iter()
            .map(|inst| {
                match patch_installation(inst, &configs) {
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

// ── Core patching logic ─────────────────────────────────────────────────────

fn patch_installation(
    inst: &discovery::Installation,
    configs: &HashMap<String, ModuleConfig>,
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
    let ini_content = merge::decode_ini(&ini_bytes)?;

    // Collect all enabled modules, sorted by priority
    let mut all_modules = modules::builtin_modules();
    all_modules.sort_by_key(|m| m.priority());

    let mut warnings = Vec::new();

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

    Ok(PatchResult {
        channel: inst.channel.clone(),
        applied,
        total,
        warnings,
        error: None,
    })
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
        patch,
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
        .manage(Mutex::new(AppState::default()))
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
