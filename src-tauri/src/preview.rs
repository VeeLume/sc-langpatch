//! Headless preview core for the mission-enhancer rendering.
//!
//! Loads everything the patcher does — installs, assets, datacore,
//! locale, parsed `global.ini` — then exposes pool iteration with
//! pre-rendered titles and descriptions so debugging tools can see
//! exactly what the player would see in-game without writing any
//! files. Two thin drivers consume this:
//!
//! - `src/bin/preview_cli.rs` — text-based output for AI agents and
//!   shell pipelines (grep / less / diff).
//! - `src/bin/preview_tui.rs` — interactive browser using
//!   superlighttui, lets the user validate fixes without restarting
//!   the game.

use std::collections::HashMap;

use anyhow::{Context, Result};
use sc_contracts::MissionIndex;
use sc_extract::{
    AssetConfig, AssetData, AssetSource, Datacore, DatacoreConfig, LocaleMap,
};
use sc_installs::Installation;
use svarog_datacore::DataCoreDatabase;

use crate::formatter_helpers::Color;
use crate::merge;
use crate::modules::mission_enhancer::{
    DescOptions, MissionEnhancerInternals, PoolFacts, TitleOptions,
};

/// One fully-loaded session — install, assets, datacore, locale,
/// parsed INI, and the built [`MissionIndex`]. Owns everything the
/// renderer needs; consumers iterate and reformat.
pub struct PreviewSession {
    pub install: Installation,
    pub asset_data: AssetData,
    pub datacore: Datacore,
    pub locale: LocaleMap,
    pub ini: HashMap<String, String>,
    pub index: MissionIndex,
    pub manufacturer_prefixes: Vec<String>,
}

impl PreviewSession {
    /// Discover the primary SC install and load everything. ~5–10s
    /// cold (DCB parse dominates); near-instant on subsequent runs
    /// once the OS has cached the P4K.
    ///
    /// The DCB parser walks generated match statements with thousands
    /// of arms; on Windows the default 1 MB main-thread stack
    /// overflows in dev builds. We always run the load on a worker
    /// with an 8 MB stack, which is plenty for the deepest call chain
    /// and avoids forcing every consumer to remember the workaround.
    pub fn load() -> Result<Self> {
        load_on_big_stack(|| {
            let install = sc_installs::discover_primary()
                .context("Failed to discover Star Citizen installation")?;
            Self::load_for_inline(install)
        })
    }

    pub fn load_for(install: Installation) -> Result<Self> {
        load_on_big_stack(move || Self::load_for_inline(install))
    }

    fn load_for_inline(install: Installation) -> Result<Self> {
        let assets = AssetSource::from_install(&install)
            .context("Failed to open Data.p4k")?;

        // Read raw INI bytes before extracting other assets — same path
        // the patcher uses; keeps the post-overlay locale consistent
        // with what the player actually sees.
        let ini_bytes = assets
            .read("Data/Localization/english/global.ini")
            .context("global.ini not found in Data.p4k")?;

        let asset_data = AssetData::extract(&assets, &AssetConfig::standard())
            .context("Failed to extract asset bundle")?;

        let datacore = Datacore::parse(&assets, &asset_data, &DatacoreConfig::standard())
            .context("Failed to parse Datacore")?;

        let ini_content = merge::decode_ini(&ini_bytes)
            .context("Failed to decode global.ini")?;
        let ini = merge::parse_ini(&ini_content);

        // LocaleMap from the post-decode INI — same shape the patcher
        // builds during phase 2.
        let mut locale = LocaleMap::new();
        for (k, v) in &ini {
            locale.set(k.as_str(), v.as_str());
        }

        let index = MissionIndex::build(&datacore, &locale);
        let manufacturer_prefixes = MissionEnhancerInternals::build_manufacturer_prefixes(
            &datacore, &locale,
        );

        Ok(Self {
            install,
            asset_data,
            datacore,
            locale,
            ini,
            index,
            manufacturer_prefixes,
        })
    }

    pub fn db(&self) -> &DataCoreDatabase {
        self.datacore.db()
    }

    /// Snapshot of registry sizes for diagnostic output.
    pub fn registry_summary(&self) -> RegistrySummary {
        let total_bp_items: usize = self.index.blueprints.iter().map(|p| p.items.len()).sum();
        let resolved_bp_names: usize = self
            .index
            .blueprints
            .iter()
            .flat_map(|p| p.items.iter())
            .filter(|i| !i.display_name.is_empty())
            .count();
        RegistrySummary {
            manufacturers: self.manufacturer_prefixes.len(),
            ships: self.index.ships.len(),
            blueprint_pools: self.index.blueprints.len(),
            blueprint_items: total_bp_items,
            blueprint_items_with_name: resolved_bp_names,
            localities: self.index.localities.len(),
            missions: self.index.contracts.len(),
        }
    }

    /// Iterate `(title_key, ids)` for every title pool whose key is
    /// present in `global.ini`. Returns canonical (stripped) keys.
    pub fn title_pools(&self) -> Vec<(String, &[sc_extract::Guid])> {
        let mut out: Vec<(String, &[sc_extract::Guid])> = self
            .index
            .pools
            .title_key
            .iter()
            .map(|(k, ids)| (k.stripped().to_string(), ids.as_slice()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Iterate `(desc_key, ids)` for every description pool. Same
    /// canonicalisation as [`Self::title_pools`].
    pub fn description_pools(&self) -> Vec<(String, &[sc_extract::Guid])> {
        let mut out: Vec<(String, &[sc_extract::Guid])> = self
            .index
            .pools
            .description_key
            .iter()
            .map(|(k, ids)| (k.stripped().to_string(), ids.as_slice()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Render one description pool as the player would see it: base
    /// description from `global.ini` + the patch suffix the
    /// mission-enhancer would emit. Returns `None` when the key isn't
    /// in `global.ini`.
    pub fn render_description(
        &self,
        desc_key: &str,
        ids: &[sc_extract::Guid],
        opts: DescOptions,
    ) -> Option<String> {
        let base = self.ini.get(desc_key)?;
        let facts = PoolFacts::build(&self.index, ids, self.db());
        let suffix = crate::modules::mission_enhancer::render_description(
            &facts,
            &self.index,
            self.db(),
            &self.manufacturer_prefixes,
            desc_key,
            opts,
        );
        Some(format!("{base}{suffix}"))
    }

    /// Render the title (with applied tags) for a title pool.
    pub fn render_title(
        &self,
        title_key: &str,
        ids: &[sc_extract::Guid],
        opts: TitleOptions,
    ) -> Option<String> {
        let base = self.ini.get(title_key)?;
        let facts = PoolFacts::build(&self.index, ids, self.db());
        let tags = crate::modules::mission_enhancer::render_title(&facts, opts);
        if tags.is_empty() {
            Some(base.clone())
        } else {
            Some(format!("{base} {tags}"))
        }
    }
}

/// Run `f` on a worker thread with an 8 MB stack. Needed because the
/// generated `RecordStore` decoder has match arms deep enough to blow
/// Windows' default 1 MB main-thread stack in dev builds. The closure
/// runs to completion (we `join` immediately) so callers see no
/// concurrency — it's just a stack-size workaround.
fn load_on_big_stack<F>(f: F) -> Result<PreviewSession>
where
    F: FnOnce() -> Result<PreviewSession> + Send + 'static,
{
    std::thread::Builder::new()
        .name("sc-langpatch-preview-loader".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(f)
        .context("Failed to spawn loader thread")?
        .join()
        .map_err(|panic| {
            anyhow::anyhow!(
                "loader thread panicked: {}",
                panic
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| panic.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("(no message)")
            )
        })?
}

/// Snapshot of registry sizes — drives the "are features enabled?"
/// sanity check in the CLI's `--registries` output and the TUI's
/// status line.
#[derive(Debug, Clone, Copy)]
pub struct RegistrySummary {
    pub manufacturers: usize,
    pub ships: usize,
    pub blueprint_pools: usize,
    pub blueprint_items: usize,
    pub blueprint_items_with_name: usize,
    pub localities: usize,
    pub missions: usize,
}

// ── Markup translation ─────────────────────────────────────────────────────

/// Translate the SC HUD markup (`<EMx>` color tags + `\n` literal
/// escapes) into ANSI escape codes for terminal output. Strips
/// unknown tags rather than mangling them.
pub fn translate_to_ansi(s: &str) -> String {
    let s = s.replace("\\n", "\n");
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            // Look ahead for `EMn>` or `/EMn>` and translate.
            let mut tag = String::new();
            for tc in chars.by_ref() {
                if tc == '>' {
                    break;
                }
                tag.push(tc);
            }
            out.push_str(translate_tag(&tag));
        } else {
            out.push(c);
        }
    }
    out
}

fn translate_tag(tag: &str) -> &'static str {
    match tag {
        "EM0" | "EM1" | "EM2" | "EM3" | "EM4" => match tag {
            "EM0" => "\x1b[37m", // white
            "EM1" => "\x1b[36m", // cyan
            "EM2" => "\x1b[32m", // green
            "EM3" => "\x1b[33m", // yellow
            "EM4" => "\x1b[31m", // red
            _ => unreachable!(),
        },
        "/EM0" | "/EM1" | "/EM2" | "/EM3" | "/EM4" => "\x1b[0m",
        _ => "",
    }
}

/// Translate `<EMx>` markup into a sequence of `(Color, &str)` runs
/// for the TUI renderer. The default emphasis (no enclosing tag) is
/// `Color::Plain`. `\n` literals are turned into real newlines.
pub fn parse_styled_runs(s: &str) -> Vec<(Color, String)> {
    let s = s.replace("\\n", "\n");
    let mut runs: Vec<(Color, String)> = Vec::new();
    let mut current_color = Color::Plain;
    let mut buf = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            for tc in chars.by_ref() {
                if tc == '>' {
                    break;
                }
                tag.push(tc);
            }
            // Flush the buffer in the current color before switching.
            if !buf.is_empty() {
                runs.push((current_color, std::mem::take(&mut buf)));
            }
            current_color = match tag.as_str() {
                "EM0" => Color::Plain,
                "EM1" => Color::Faint,
                "EM2" => Color::Soft,
                "EM3" => Color::Underline,
                "EM4" => Color::Highlight,
                _ if tag.starts_with('/') => Color::Plain,
                _ => current_color,
            };
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        runs.push((current_color, buf));
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_newline_literals() {
        assert_eq!(translate_to_ansi("a\\nb"), "a\nb");
    }

    #[test]
    fn translates_em4_to_red_ansi() {
        assert_eq!(translate_to_ansi("<EM4>x</EM4>"), "\x1b[31mx\x1b[0m");
    }

    #[test]
    fn strips_unknown_tags() {
        assert_eq!(translate_to_ansi("<unknown>x"), "x");
    }

    #[test]
    fn styled_runs_split_on_color_change() {
        let runs = parse_styled_runs("plain<EM4>red</EM4>after");
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0], (Color::Plain, "plain".to_string()));
        assert_eq!(runs[1], (Color::Highlight, "red".to_string()));
        assert_eq!(runs[2], (Color::Plain, "after".to_string()));
    }

    #[test]
    fn styled_runs_translate_newlines() {
        let runs = parse_styled_runs("a\\nb");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].1, "a\nb");
    }
}
