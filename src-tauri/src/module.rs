use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use specta::Type;
use svarog_datacore::DataCoreDatabase;

// ── Patch operations ────────────────────────────────────────────────────────

/// How a single INI key should be modified.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "value")]
pub enum PatchOp {
    /// Completely replace the original value.
    Replace(String),
    /// Prepend to the original value.
    Prefix(String),
    /// Append to the original value.
    Suffix(String),
}

/// Rename an INI key (keeping its value). Applied before value patches.
#[derive(Debug, Clone)]
pub struct KeyRename {
    pub from: String,
    pub to: String,
}

// ── Module options ──────────────────────────────────────────────────────────

/// Describes a configurable option exposed by a module.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModuleOption {
    /// Machine-readable identifier (used in config and code).
    pub id: String,
    /// Human-readable label (displayed in UI).
    pub label: String,
    pub description: String,
    pub kind: OptionKind,
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ChoiceOption {
    /// Machine-readable value (used in config and code).
    pub value: String,
    /// Human-readable label (displayed in UI).
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "type")]
pub enum OptionKind {
    Bool,
    String,
    Choice { choices: Vec<ChoiceOption> },
}

/// A user-chosen value for a module option.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", content = "value")]
pub enum OptionValue {
    Bool(bool),
    String(String),
    Choice(String),
}

/// Per-module configuration (from config.toml or GUI).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct ModuleConfig {
    pub enabled: Option<bool>,
    /// User-chosen values keyed by option name.
    #[serde(default)]
    pub options: Vec<OptionEntry>,
}

/// A single option value entry.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OptionEntry {
    pub name: String,
    pub value: OptionValue,
}

impl ModuleConfig {
    pub fn get(&self, name: &str) -> Option<&OptionValue> {
        self.options.iter().find(|e| e.name == name).map(|e| &e.value)
    }

    pub fn get_str(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(OptionValue::String(s) | OptionValue::Choice(s)) => Some(s),
            _ => None,
        }
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        match self.get(name) {
            Some(OptionValue::Bool(b)) => Some(*b),
            _ => None,
        }
    }
}

// ── Module context ──────────────────────────────────────────────────────────

/// Data available to modules during patch generation.
pub struct ModuleContext<'a> {
    /// DataCore database (from Game2.dcb). None if extraction failed or skipped.
    pub db: Option<&'a DataCoreDatabase>,
    /// Parsed global.ini: key → value.
    pub ini: &'a HashMap<String, String>,
    /// User configuration for this specific module.
    pub config: &'a ModuleConfig,
}

// ── Module trait ────────────────────────────────────────────────────────────

/// A patch module that generates INI key modifications.
///
/// Code modules (component grades, weapon stats) implement this directly.
/// TOML modules implement it via the generic `TomlModule` wrapper.
pub trait Module: Send + Sync {
    /// Unique identifier (used as key in config.toml).
    fn id(&self) -> &str;

    /// Human-readable name for the GUI.
    fn name(&self) -> &str;

    /// Short description of what this module does.
    fn description(&self) -> &str;

    /// Whether this module is enabled when no config exists.
    fn default_enabled(&self) -> bool;

    /// Configurable options this module exposes.
    fn options(&self) -> Vec<ModuleOption> {
        Vec::new()
    }

    /// Whether this module needs the DataCore database.
    fn needs_datacore(&self) -> bool {
        false
    }

    /// Priority: lower runs first. Key fix modules should use 0,
    /// normal modules default to 100.
    fn priority(&self) -> u32 {
        100
    }

    /// Generate key renames. Applied before value patches so that
    /// downstream modules see the corrected keys.
    fn generate_renames(&self, _ctx: &ModuleContext) -> Result<Vec<KeyRename>> {
        Ok(Vec::new())
    }

    /// Generate value patches for the given context.
    fn generate_patches(&self, ctx: &ModuleContext) -> Result<Vec<(String, PatchOp)>>;
}

// ── Module info (for serialization to frontend) ─────────────────────────────

/// Serializable module metadata for the GUI.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_enabled: bool,
    pub enabled: bool,
    pub needs_datacore: bool,
    pub options: Vec<ModuleOption>,
}
