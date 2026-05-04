//! Structured error & warning types surfaced to the frontend.
//!
//! The frontend translates each variant via a Paraglide message key
//! (`error_<code>` / `warning_<code>`), so the codes here are part of
//! the public UI contract — renaming a variant is a translation drift
//! event caught by the catalog drift test.
//!
//! `AppError::Unexpected` and `AppWarning::Unexpected` are escape
//! hatches for genuinely unclassified errors: anything that comes
//! through them shows the raw English message inside a localized
//! "Unexpected error: {message}" frame, which is good enough for
//! bug-report content while keeping the predictable cases translated.

use serde::Serialize;
use specta::Type;

/// Errors that can stop a command from producing a result, or that
/// land in `PatchResult.error` / `RemoveResult.error` when a single
/// installation can't be processed.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "code", content = "data", rename_all = "snake_case")]
pub enum AppError {
    /// Discovery (RSI launcher log parsing) failed at the OS level.
    DiscoveryFailed { message: String },
    /// `tauri::async_runtime::spawn_blocking` join failed.
    TaskJoinFailed { message: String },
    /// Could not open `Data.p4k`. Path missing, locked, or corrupted.
    #[serde(rename = "p4k_open_failed")]
    P4kOpenFailed { path: String, message: String },
    /// `Data/Localization/english/global.ini` is missing inside the
    /// p4k. Game install is incomplete or shape changed.
    GlobalIniNotFound,
    /// UTF-16 decode of the base global.ini failed.
    IniDecodeFailed { message: String },
    /// Writing the patched global.ini back to the install dir failed.
    OutputWriteFailed { message: String },
    /// Removing a previously written patch (and clearing user.cfg)
    /// failed.
    OutputRemoveFailed { message: String },
    /// Any error that wasn't classified — surfaced raw inside a
    /// localized "Unexpected error: {message}" frame.
    Unexpected { message: String },
}

/// Non-fatal issues collected during a patch run. Surfaced in
/// `PatchResult.warnings`. Module-level variants carry both the id
/// (used to look up the translated module name in the frontend
/// catalog) and the english name (fallback when no translation
/// exists for that module).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "code", content = "data", rename_all = "snake_case")]
pub enum AppWarning {
    /// Could not load the community language pack from URL or path.
    LanguagePackLoadFailed { message: String },
    /// Loaded the bytes but couldn't decode them as INI.
    LanguagePackDecodeFailed { message: String },
    /// Module needs the DataCore but it wasn't extracted (e.g. the
    /// p4k extraction step failed earlier in this run).
    ModuleSkippedNoDatacore { module_id: String, module_name: String },
    /// Module needs the parsed locale map but it wasn't built.
    ModuleSkippedNoLocale { module_id: String, module_name: String },
    /// `generate_renames` returned an error.
    ModuleRenameFailed {
        module_id: String,
        module_name: String,
        message: String,
    },
    /// `generate_patches` returned an error.
    ModulePatchFailed {
        module_id: String,
        module_name: String,
        message: String,
    },
    /// Module emitted Replace ops without declaring `uses_replace_ops()`
    /// — those ops were dropped to protect the user's language pack
    /// values.
    UndeclaredReplaceDropped {
        module_id: String,
        module_name: String,
        count: u32,
    },
    /// Any non-fatal issue that wasn't classified.
    Unexpected { message: String },
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Unexpected {
            message: format!("{e:#}"),
        }
    }
}

impl AppError {
    /// Discriminant string used by the frontend to dispatch to a
    /// translated message. Kept in sync with the `serde(rename_all)`
    /// rule so `code` from the wire matches.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::DiscoveryFailed { .. } => "discovery_failed",
            AppError::TaskJoinFailed { .. } => "task_join_failed",
            AppError::P4kOpenFailed { .. } => "p4k_open_failed",
            AppError::GlobalIniNotFound => "global_ini_not_found",
            AppError::IniDecodeFailed { .. } => "ini_decode_failed",
            AppError::OutputWriteFailed { .. } => "output_write_failed",
            AppError::OutputRemoveFailed { .. } => "output_remove_failed",
            AppError::Unexpected { .. } => "unexpected",
        }
    }
}

impl AppWarning {
    pub fn code(&self) -> &'static str {
        match self {
            AppWarning::LanguagePackLoadFailed { .. } => "language_pack_load_failed",
            AppWarning::LanguagePackDecodeFailed { .. } => "language_pack_decode_failed",
            AppWarning::ModuleSkippedNoDatacore { .. } => "module_skipped_no_datacore",
            AppWarning::ModuleSkippedNoLocale { .. } => "module_skipped_no_locale",
            AppWarning::ModuleRenameFailed { .. } => "module_rename_failed",
            AppWarning::ModulePatchFailed { .. } => "module_patch_failed",
            AppWarning::UndeclaredReplaceDropped { .. } => "undeclared_replace_dropped",
            AppWarning::Unexpected { .. } => "unexpected",
        }
    }
}

/// Variants that the catalog drift test must cover. Add new ones here
/// when you add an enum variant.
#[cfg(test)]
pub const ALL_ERROR_CODES: &[&str] = &[
    "discovery_failed",
    "task_join_failed",
    "p4k_open_failed",
    "global_ini_not_found",
    "ini_decode_failed",
    "output_write_failed",
    "output_remove_failed",
    "unexpected",
];

#[cfg(test)]
pub const ALL_WARNING_CODES: &[&str] = &[
    "language_pack_load_failed",
    "language_pack_decode_failed",
    "module_skipped_no_datacore",
    "module_skipped_no_locale",
    "module_rename_failed",
    "module_patch_failed",
    "undeclared_replace_dropped",
    "unexpected",
];
