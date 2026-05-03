//! Mission enhancer — pool-first INI patching driven by sc-contracts v0.2.0.
//!
//! Two independent passes:
//! - **Title pool** — for every `(title_key, [Mission ids])` group in
//!   [`MissionIndex::pools`], render trailing tags and emit one
//!   [`PatchOp::Suffix`] on the title key.
//! - **Description pool** — same shape on `description_key`, rendering
//!   the mission-info / blueprint / encounter / region blocks plus an
//!   optional Variants section when pool members diverge.
//!
//! Title and description pools are not aligned — two contracts can
//! share a title and not a description, or vice versa. Each pass
//! reasons about its own pool independently.

mod crimestat;
mod description;
mod encounters;
mod format;
mod pool;
mod title;
mod variants;

use anyhow::Result;
use sc_contracts::MissionIndex;

use crate::module::{
    ChoiceOption, Module, ModuleContext, ModuleOption, OptionKind, PatchOp,
};

use self::format::build_manufacturer_prefixes;

// ── Re-exports for the headless preview tooling ────────────────────────────
//
// `crate::preview` builds [`MissionIndex`], walks the same pools, and
// reuses these renderers verbatim so the preview matches the patcher
// 1:1. Internal call sites in this module continue using the
// `self::...` paths.

pub use self::description::{render as render_description, DescOptions};
pub use self::pool::PoolFacts;
pub use self::title::{render as render_title, CrimestatTagMode, TitleOptions};

/// Stable namespace for preview-facing internals that don't merit a
/// top-level re-export. Keeps the preview crate's import list small.
pub struct MissionEnhancerInternals;

impl MissionEnhancerInternals {
    pub fn build_manufacturer_prefixes(
        datacore: &sc_extract::Datacore,
        locale: &sc_extract::LocaleMap,
    ) -> Vec<String> {
        build_manufacturer_prefixes(datacore, locale)
    }
}

pub struct MissionEnhancer;

impl Module for MissionEnhancer {
    fn id(&self) -> &str {
        "mission_enhancer"
    }

    fn name(&self) -> &str {
        "Mission Enhancer"
    }

    fn description(&self) -> &str {
        "Enrich mission titles and descriptions with blueprint rewards, cooldowns, and more"
    }

    fn default_enabled(&self) -> bool {
        true
    }

    fn needs_datacore(&self) -> bool {
        true
    }

    fn needs_locale(&self) -> bool {
        true
    }

    fn options(&self) -> Vec<ModuleOption> {
        vec![
            ModuleOption {
                id: "blueprint_tag".into(),
                label: "Blueprint Tag".into(),
                description: "Add [BP] to titles of missions that reward blueprints".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "blueprint_list".into(),
                label: "Blueprint List".into(),
                description: "Append blueprint item list to mission descriptions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "solo_tag".into(),
                label: "Solo Tag".into(),
                description: "Add [Solo] to titles of solo-only missions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "once_tag".into(),
                label: "One-Time Tag".into(),
                description: "Add [Uniq] to titles of one-time-only missions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "illegal_tag".into(),
                label: "Illegal Tag".into(),
                description: "Add [Illegal] to titles of illegal missions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "mission_info".into(),
                label: "Mission Info".into(),
                description: "Append cooldown, rep reward, and scrip to descriptions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "crimestat_tag".into(),
                label: "Crimestat Risk Tag".into(),
                description: "Mark missions where killing friendly NPCs gives crimestat".into(),
                kind: OptionKind::Choice {
                    choices: vec![
                        ChoiceOption { value: "off".into(), label: "Off".into() },
                        ChoiceOption { value: "simple".into(), label: "Simple [CS Risk]".into() },
                        ChoiceOption {
                            value: "colored".into(),
                            label: "Colored (yellow/red)".into(),
                        },
                    ],
                },
                default: "colored".into(),
            },
            ModuleOption {
                id: "ship_encounters".into(),
                label: "Ship Encounters".into(),
                description: "Show hostile and allied ship types in mission descriptions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "cargo_info".into(),
                label: "Cargo Info".into(),
                description:
                    "Show cargo descriptors (Full/Half/Scraps, HighValue/LowValue) on hostile ships"
                        .into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "region_info".into(),
                label: "Region Info".into(),
                description: "Append the region / body where the mission is offered".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
        ]
    }

    fn generate_patches(&self, ctx: &ModuleContext) -> Result<Vec<(String, PatchOp)>> {
        let (Some(datacore), Some(locale), Some(db)) = (ctx.datacore, ctx.locale, ctx.db) else {
            return Ok(Vec::new());
        };

        let title_opts = TitleOptions {
            blueprint: ctx.config.get_bool("blueprint_tag").unwrap_or(true),
            solo: ctx.config.get_bool("solo_tag").unwrap_or(true),
            once: ctx.config.get_bool("once_tag").unwrap_or(true),
            illegal: ctx.config.get_bool("illegal_tag").unwrap_or(true),
            crimestat: CrimestatTagMode::from_str(
                ctx.config.get_str("crimestat_tag").unwrap_or("colored"),
            ),
        };
        let desc_opts = DescOptions {
            blueprint_list: ctx.config.get_bool("blueprint_list").unwrap_or(true),
            mission_info: ctx.config.get_bool("mission_info").unwrap_or(true),
            ship_encounters: ctx.config.get_bool("ship_encounters").unwrap_or(true),
            cargo_info: ctx.config.get_bool("cargo_info").unwrap_or(true),
            region_info: ctx.config.get_bool("region_info").unwrap_or(true),
            // One-shot patch run — fallback diagnostics surface useful
            // outliers in stderr exactly once.
            diagnostics: true,
        };

        let index = MissionIndex::build(datacore);
        let cache = &datacore.snapshot().localized_items;
        let manufacturer_prefixes = build_manufacturer_prefixes(datacore, locale);

        // Registry-population diagnostic. If any of these are zero,
        // the corresponding sc-extract feature flag (entityclassdefinition,
        // contracts, servicebeacon) didn't propagate into the build —
        // the registries silently come back empty when the underlying
        // record types weren't decoded at parse time.
        let ship_count = index.ships.len();
        let blueprint_pool_count = index.blueprints.len();
        let locality_count = index.localities.len();
        let total_bp_items: usize = index
            .blueprints
            .iter()
            .map(|p| p.items.len())
            .sum();
        let resolved_bp_names: usize = index
            .blueprints
            .iter()
            .flat_map(|p| p.items.iter())
            .filter(|i| i.display_name(cache, locale).is_some())
            .count();
        eprintln!(
            "  [MissionEnhancer] registries — manufacturers={}, ships={}, blueprint_pools={} ({} items, {} with display_name), localities={}, missions={}",
            manufacturer_prefixes.len(),
            ship_count,
            blueprint_pool_count,
            total_bp_items,
            resolved_bp_names,
            locality_count,
            index.contracts.len(),
        );

        let mut patches: Vec<(String, PatchOp)> = Vec::new();
        let mut title_hits = 0usize;
        let mut title_misses = 0usize;
        let mut desc_hits = 0usize;
        let mut desc_misses = 0usize;

        // ── Title pool pass ─────────────────────────────────────────
        for (title_key, ids) in &index.pools.title_key {
            let key = title_key.stripped();
            if key.is_empty() || !ctx.ini.contains_key(key) {
                title_misses += 1;
                continue;
            }
            let facts = PoolFacts::build(&index, ids, db, &index.localities, locale);
            let tags = title::render(&facts, title_opts);
            if tags.is_empty() {
                continue;
            }
            patches.push((key.to_string(), PatchOp::Suffix(format!(" {tags}"))));
            title_hits += 1;
        }

        // ── Description pool pass ──────────────────────────────────
        for (desc_key, ids) in &index.pools.description_key {
            let key = desc_key.stripped();
            if key.is_empty() || !ctx.ini.contains_key(key) {
                desc_misses += 1;
                continue;
            }
            let facts = PoolFacts::build(&index, ids, db, &index.localities, locale);
            let suffix = description::render(
                &facts,
                &index,
                db,
                cache,
                locale,
                &manufacturer_prefixes,
                key,
                desc_opts,
            );
            if suffix.is_empty() {
                continue;
            }
            patches.push((key.to_string(), PatchOp::Suffix(suffix)));
            desc_hits += 1;
        }

        eprintln!(
            "  [MissionEnhancer] {title_hits} title pools, {desc_hits} desc pools patched \
             (title_keys_missing_in_ini={title_misses}, desc_keys_missing_in_ini={desc_misses}, \
             total_missions={})",
            index.contracts.len()
        );

        Ok(patches)
    }
}
