pub mod component_grades;
pub mod illegal_goods;
pub mod mission_enhancer;
pub mod toml_module;
pub mod weapon_enhancer;

use crate::module::Module;

/// Collect all built-in modules (code + embedded TOML).
pub fn builtin_modules() -> Vec<Box<dyn Module>> {
    let mut modules: Vec<Box<dyn Module>> = Vec::new();

    // Key fixes (priority 0 — runs first)
    modules.push(Box::new(toml_module::TomlModule::from_embedded(
        "key_fixes",
        include_str!("toml/key_fixes.toml"),
    )));

    // Embedded TOML modules
    modules.push(Box::new(toml_module::TomlModule::from_embedded(
        "label_fixes",
        include_str!("toml/label_fixes.toml"),
    )));

    // Code-derived modules
    modules.push(Box::new(component_grades::ComponentGrades));
    modules.push(Box::new(illegal_goods::IllegalGoods));
    modules.push(Box::new(mission_enhancer::MissionEnhancer));
    modules.push(Box::new(weapon_enhancer::WeaponEnhancer));

    // Test / debug modules — only included in dev builds. Disable
    // in the UI when not actively validating in-game rendering.
    #[cfg(debug_assertions)]
    modules.push(Box::new(toml_module::TomlModule::from_embedded(
        "test_em_colors",
        include_str!("toml/test_em_colors.toml"),
    )));

    modules
}

/// Load user-defined TOML modules from %APPDATA%/sc-langpatch/modules/.
pub fn user_modules() -> Vec<Box<dyn Module>> {
    let Some(app_dir) = dirs::data_dir() else {
        return Vec::new();
    };
    let modules_dir = app_dir.join("sc-langpatch").join("modules");

    if !modules_dir.exists() {
        return Vec::new();
    }

    let mut modules: Vec<Box<dyn Module>> = Vec::new();

    let entries = match std::fs::read_dir(&modules_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml") {
            match toml_module::TomlModule::from_file(&path) {
                Ok(m) => modules.push(Box::new(m)),
                Err(e) => eprintln!("Warning: failed to load {}: {e}", path.display()),
            }
        }
    }

    modules
}
