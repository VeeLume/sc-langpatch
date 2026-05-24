//! Encounter rendering — walks `Mission::encounters` and produces a
//! formatted block describing what spawns.
//!
//! The model is built around [`sc_contracts::SlotGroup`]: each group
//! is one concurrent slot in the engine where one of the `options`
//! fires per spawn (engine picks based on player profile / RNG /
//! difficulty). v0.4.0 of sc-contracts preserves this boundary, so
//! the renderer now respects it rather than flattening and trying to
//! reconstruct the meaning afterward.
//!
//! Per-group rendering rules:
//!
//! - **Singleton group** (`options.len() == 1`) → one line:
//!   `Label: Nx ship · <pool>`
//! - **Multiple options, only scaling axes vary** (skill alone, or
//!   skill + count variation in the same CombatClass) → collapse to
//!   a range: `Label: 1-3 ships · <pool>`
//! - **Surface axis varies** (hull, ship class, effect like Distortion,
//!   spawn flag like ArriveViaQT, faction, cargo, CombatClass) →
//!   render as an alternatives block:
//!   ```text
//!   Label: One of (weighted):
//!     3 ships · <pool> (Hard, Distortion)
//!     4 ships · <pool> (Hard)
//!   ```
//!
//! NPC encounters bypass per-group rendering — they collapse into a
//! single `NPCs: N` total (NPC spawn descriptions don't carry the
//! same alternatives structure as ships/entities).
//!
//! Cargo / value / faction tags aggregate across every group into a
//! single summary line at the end of the body.

use std::collections::HashSet;

use sc_contracts::{
    AxisKind, Encounter, EntityEncounter, EntitySlot, NpcEncounter, ShipEncounter, ShipRegistry,
    ShipSlot, SlotGroup, TagBag,
};
use sc_extract::{LocaleMap, LocalizedItemCache, TagTree};

use super::format::{collapse_variants, pretty_identifier};
use crate::formatter_helpers::{Color, NEWLINE, apply_color};

// ── Public entry point ─────────────────────────────────────────────────────

/// Result of rendering a mission's encounters.
///
/// Carries the formatted body block plus enemy-side spawn counts
/// so the caller can include them in the section heading.
/// Friendly slots (escort ships, allied NPCs with the
/// `mission_allied_marker` flag) are excluded from the totals — the
/// player wants to know how much they're going to fight.
#[derive(Debug, Clone, Default)]
pub struct EncounterRendering {
    pub body: String,
    /// `(min, max)` summed across enemy ship + entity groups. `min`
    /// picks the smallest concurrent in each group's alternatives;
    /// `max` the largest. `(0, 0)` when the mission has no enemy
    /// ship/entity encounters.
    pub enemy_ship_count_range: (i32, i32),
    pub enemy_npc_total: i32,
}

/// Render every encounter on a mission as a multi-line block.
pub fn render(
    encounters: &[Encounter],
    tree: &TagTree,
    ships: &ShipRegistry,
    cache: &LocalizedItemCache,
    locale: &LocaleMap,
    manufacturer_prefixes: &[String],
    include_cargo: bool,
) -> EncounterRendering {
    let mut lines: Vec<String> = Vec::new();
    let mut all_summary_tags: Vec<String> = Vec::new();
    let mut enemy_ship_min: i32 = 0;
    let mut enemy_ship_max: i32 = 0;
    let mut npc_total: i32 = 0;
    let mut enemy_npc_total: i32 = 0;

    for enc in encounters {
        match enc {
            Encounter::Ships(s) => render_ship_encounter(
                s,
                tree,
                ships,
                cache,
                locale,
                manufacturer_prefixes,
                include_cargo,
                &mut lines,
                &mut all_summary_tags,
                &mut enemy_ship_min,
                &mut enemy_ship_max,
            ),
            Encounter::Npcs(s) => {
                npc_total += count_npcs(s);
                enemy_npc_total += count_enemy_npcs(s);
            }
            Encounter::Entities(s) => render_entity_encounter(
                s,
                tree,
                include_cargo,
                &mut lines,
                &mut all_summary_tags,
                &mut enemy_ship_min,
                &mut enemy_ship_max,
            ),
            Encounter::Unknown { .. } => {}
        }
    }

    if npc_total > 0 {
        lines.push(format!("NPCs: {npc_total}"));
    }
    if let Some(summary) = aggregate_tag_summary(&all_summary_tags) {
        lines.push(summary);
    }

    EncounterRendering {
        body: lines.join(NEWLINE),
        enemy_ship_count_range: (enemy_ship_min, enemy_ship_max),
        enemy_npc_total,
    }
}

// ── Ship encounter ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn render_ship_encounter(
    enc: &ShipEncounter,
    tree: &TagTree,
    ships: &ShipRegistry,
    cache: &LocalizedItemCache,
    locale: &LocaleMap,
    manufacturer_prefixes: &[String],
    include_cargo: bool,
    out: &mut Vec<String>,
    summary_tags: &mut Vec<String>,
    enemy_ship_min: &mut i32,
    enemy_ship_max: &mut i32,
) {
    let raw_encounter = clean_encounter_label(&enc.variable_name);
    let friendly = is_friendly_label(&raw_encounter);
    for phase in &enc.phases {
        let raw_phase = clean_phase_label(&phase.name, &raw_encounter);
        let (encounter_label, phase_label) = resolve_labels(&raw_encounter, &raw_phase);
        let label = label_with_phase(&encounter_label, &phase_label);

        // Tally enemy counts across every group in this phase.
        if !friendly {
            for group in &phase.groups {
                *enemy_ship_min += group.concurrent_range.0;
                *enemy_ship_max += group.concurrent_range.1;
            }
        }

        // Render every group's body lines (label-less, no leading indent).
        // The layout is uniform: phase label on its own line, body
        // indented one level. Multi-group phases stack their bodies
        // under the same header. Inner structure inside a group (e.g.
        // a "One of:" block) carries its own additional indent — we
        // prepend the phase-level indent unconditionally.
        let bodies: Vec<Vec<String>> = phase
            .groups
            .iter()
            .map(|g| render_ship_group_body(g, ships, cache, locale, manufacturer_prefixes))
            .collect();
        if bodies.iter().any(|b| !b.is_empty()) {
            out.push(format!("{label}:"));
            for body in bodies {
                for line in body {
                    out.push(format!("  {line}"));
                }
            }
        }

        if include_cargo {
            for group in &phase.groups {
                collect_cargo_tags_from_ship_group(group, tree, summary_tags);
            }
        }
    }
}

/// Render one [`SlotGroup<ShipSlot>`] as 1–N body lines (no label
/// prefix — caller attaches the encounter / phase label or bullets).
fn render_ship_group_body(
    group: &SlotGroup<ShipSlot>,
    ships: &ShipRegistry,
    cache: &LocalizedItemCache,
    locale: &LocaleMap,
    manufacturer_prefixes: &[String],
) -> Vec<String> {
    let opt_count = group.options.len();
    if opt_count == 0 {
        return Vec::new();
    }
    if opt_count == 1 {
        return render_ship_singleton(&group.options[0], ships, cache, locale, manufacturer_prefixes);
    }
    if !has_surface_variance(group) {
        return render_ship_collapsed_range(group, ships, cache, locale, manufacturer_prefixes);
    }
    render_ship_alternatives(group, ships, cache, locale, manufacturer_prefixes)
}

/// Singleton group — one option, no alternatives boundary. Returns a
/// single body line: `"3 ships · <pool> · Skill 80"`.
fn render_ship_singleton(
    opt: &ShipSlot,
    ships: &ShipRegistry,
    cache: &LocalizedItemCache,
    locale: &LocaleMap,
    manufacturer_prefixes: &[String],
) -> Vec<String> {
    let count = opt.concurrent.max(1);
    let ship_list = ship_list_for_slot(opt, ships, cache, locale, manufacturer_prefixes);
    let role = role_hint(&opt.positive)
        .map(|h| format!(" · ({h})"))
        .unwrap_or_default();
    let body = compose_count_and_pool(count, count, &ship_list);
    vec![format!("{body}{role}")]
}

/// Multiple alternatives but only scaling axes (skill) differ — the
/// engine picks one tier and the ship pool is shared. Collapse to a
/// concurrent range: `"1-3 ships · <pool> · Skill 10-30"`.
fn render_ship_collapsed_range(
    group: &SlotGroup<ShipSlot>,
    ships: &ShipRegistry,
    cache: &LocalizedItemCache,
    locale: &LocaleMap,
    manufacturer_prefixes: &[String],
) -> Vec<String> {
    let (lo, hi) = group.concurrent_range;
    let ship_list = union_ship_lists(&group.options, ships, cache, locale, manufacturer_prefixes);
    let role = group
        .options
        .iter()
        .find_map(|o| role_hint(&o.positive))
        .map(|h| format!(" · ({h})"))
        .unwrap_or_default();
    let body = compose_count_and_pool(lo, hi, &ship_list);
    vec![format!("{body}{role}")]
}

/// Surface axis varies (hull / ship class / Distortion / etc.) —
/// render an `"One of:"` header line followed by one indented body
/// line per option. Per-option skill is appended as the engine often
/// pairs surface variance with different skill tiers.
fn render_ship_alternatives(
    group: &SlotGroup<ShipSlot>,
    ships: &ShipRegistry,
    cache: &LocalizedItemCache,
    locale: &LocaleMap,
    manufacturer_prefixes: &[String],
) -> Vec<String> {
    let header_word = if group.weight_uniform {
        "One of"
    } else {
        "One of (weighted)"
    };
    let mut out = vec![format!("{header_word}:")];
    let weight_sum: f32 = group.options.iter().map(|o| o.weight).sum();
    for (idx, opt) in group.options.iter().enumerate() {
        let count = opt.concurrent.max(1);
        let ship_list = ship_list_for_slot(opt, ships, cache, locale, manufacturer_prefixes);
        let body = compose_count_and_pool(count, count, &ship_list);
        let pct = if !group.weight_uniform && weight_sum > 0.0 {
            format!(" ({:.0}%)", opt.weight / weight_sum * 100.0)
        } else {
            String::new()
        };
        let axis_suffix = surface_axis_suffix(group, idx);
        out.push(format!("  {body}{pct}{axis_suffix}"));
    }
    out
}

/// True when at least one player-meaningful axis varies across the
/// group's options. Skill (HumanPilotNN) is intentionally excluded —
/// that's pure scaling noise the player doesn't need surfaced.
fn has_surface_variance(group: &SlotGroup<ShipSlot>) -> bool {
    let a = &group.axes;
    a.hull.varies
        || a.ship_class.varies
        || a.effect.varies
        || a.spawn_flags.varies
        || a.faction.varies
        || a.cargo_size.varies
        || a.value.varies
        || a.combat_class.varies
}

/// Compose " · " suffix listing every surface-axis tag this specific
/// option carries that varies within the group. Skip the skill axis
/// (surfaced separately via [`slot_skill_suffix`]) and the spawn-role
/// axis (Defenders / Target, already implied by the label).
///
/// Output example: ` (Hard, with Distortion)` or ` (CombatShip)`.
fn surface_axis_suffix(group: &SlotGroup<ShipSlot>, opt_idx: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    let axes = &group.axes;

    // Surface only the player-relevant axes that the rest of the
    // display can't communicate:
    //
    // - `Hull` is OMITTED — the resolved ship pool already names the
    //   hull (`135c` vs `Avenger Titan Renegade`), so a tag suffix
    //   like `(135c)` would just repeat raw underscore-tag form.
    // - `CombatClass` is OMITTED — surfaced in the encounter header
    //   as a single tier or range, see `combat_class_range`.
    // - `ShipClass` is INCLUDED but filtered: the
    //   `Missions / VehicleType / Ship / *` subtree mixes broad-class
    //   names (CombatShip, LargeCombatShip, HeavyInterceptor) with
    //   loadout markers (Distortion). Broad-class tags are redundant
    //   with the ship pool and dropped via [`is_broad_ship_class_tag`];
    //   the loadout markers survive.
    //
    // What's left is genuinely orthogonal information the ship-pool
    // and header don't convey: weapon effects, spawn behavior
    // (ArriveViaQT), cargo/value variance between options, and
    // faction overrides.
    for (axis_kind, axis_values) in [
        (AxisKind::Effect, &axes.effect),
        (AxisKind::ShipClass, &axes.ship_class),
        (AxisKind::SpawnFlags, &axes.spawn_flags),
        (AxisKind::CargoSize, &axes.cargo_size),
        (AxisKind::Value, &axes.value),
        (AxisKind::Faction, &axes.faction),
    ] {
        if !axis_values.varies {
            continue;
        }
        let Some(per) = axis_values.per_option.get(opt_idx) else {
            continue;
        };
        for (_, name) in per {
            if axis_kind == AxisKind::ShipClass && is_broad_ship_class_tag(name) {
                continue;
            }
            parts.push(format_axis_value(axis_kind, name));
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}

/// True for `Missions / VehicleType / Ship / *` tags that describe a
/// broad ship class (CombatShip, LargeCombatShip, HeavyInterceptor,
/// etc.) — these are already conveyed by the resolved ship pool and
/// add visual noise when repeated as a suffix. Loadout-style tags
/// under the same path (Distortion, future variants) survive the
/// filter and surface in the suffix.
fn is_broad_ship_class_tag(name: &str) -> bool {
    name == "CombatShip"
        || name == "LargeCombatShip"
        || name.ends_with("Interceptor")
        || name.ends_with("Fighter")
        || name.ends_with("Bomber")
}

/// Per-axis cosmetic formatting for a single tag value. E.g.,
/// `Distortion` reads better as `"with Distortion"` than bare
/// `"Distortion"`. The classifier-driven approach means we don't
/// hardcode tag names — we hardcode per-AXIS phrasing.
fn format_axis_value(axis: AxisKind, name: &str) -> String {
    match axis {
        AxisKind::Effect => format!("with {name}"),
        AxisKind::SpawnFlags => name.to_string(),
        AxisKind::Hull | AxisKind::ShipClass => name.to_string(),
        AxisKind::CombatClass => name.to_string(),
        AxisKind::CargoSize | AxisKind::Value | AxisKind::Faction => name.to_string(),
        _ => name.to_string(),
    }
}

/// Render the body of one slot or alternative as `"3x Cutlass, Sabre"`
/// (singleton) or `"1-3x Scythe"` (collapsed range). The `Nx` form
/// keeps dense waves readable when many alternatives stack — `"N ships"`
/// added noticeable noise in real-world bounty waves. When the ship
/// pool is empty (unresolved tag query) the count stands alone.
fn compose_count_and_pool(lo: i32, hi: i32, ship_list: &[String]) -> String {
    let count_str = if lo == hi {
        format!("{lo}x")
    } else {
        format!("{lo}-{hi}x")
    };
    if ship_list.is_empty() {
        count_str
    } else {
        format!("{count_str} {}", ship_list.join(", "))
    }
}

// ── Entity encounter ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn render_entity_encounter(
    enc: &EntityEncounter,
    tree: &TagTree,
    include_cargo: bool,
    out: &mut Vec<String>,
    summary_tags: &mut Vec<String>,
    enemy_ship_min: &mut i32,
    enemy_ship_max: &mut i32,
) {
    let raw_encounter = clean_encounter_label(&enc.variable_name);
    let friendly = is_friendly_label(&raw_encounter);
    for phase in &enc.phases {
        let raw_phase = clean_phase_label(&phase.name, &raw_encounter);
        let (encounter_label, phase_label) = resolve_labels(&raw_encounter, &raw_phase);
        let label = label_with_phase(&encounter_label, &phase_label);

        if !friendly {
            for group in &phase.groups {
                *enemy_ship_min += group.concurrent_range.0;
                *enemy_ship_max += group.concurrent_range.1;
            }
        }

        let bodies: Vec<Vec<String>> = phase
            .groups
            .iter()
            .map(render_entity_group_body)
            .collect();
        if bodies.iter().any(|b| !b.is_empty()) {
            out.push(format!("{label}:"));
            for body in bodies {
                for line in body {
                    out.push(format!("  {line}"));
                }
            }
        }

        if include_cargo {
            for group in &phase.groups {
                collect_cargo_tags_from_entity_group(group, tree, summary_tags);
            }
        }
    }
}

fn render_entity_group_body(group: &SlotGroup<EntitySlot>) -> Vec<String> {
    let opt_count = group.options.len();
    if opt_count == 0 {
        return Vec::new();
    }
    if opt_count == 1 {
        let opt = &group.options[0];
        let n = opt.amount.max(1);
        return vec![format!("{n} entit{}", if n == 1 { "y" } else { "ies" })];
    }
    // Multiple alternatives — only render as a collapsed range for now.
    // Entity encounters don't have an established renderer for surface
    // variance (no per-entity ship-pool resolution), so even when tags
    // differ we just show the count range.
    let (lo, hi) = group.concurrent_range;
    let body = if lo == hi {
        format!("{lo} entit{}", if lo == 1 { "y" } else { "ies" })
    } else {
        format!("{lo}-{hi} entities")
    };
    vec![body]
}

// ── Label rendering ────────────────────────────────────────────────────────

/// Header label for one slot — the encounter name plus any surviving
/// phase qualifier in brackets. Wrapped in `Color::Underline` so the
/// labels stand out as scan anchors when the player skims a long
/// encounter list.
fn label_with_phase(encounter: &str, phase: &str) -> String {
    let raw = if phase.is_empty() {
        encounter.to_string()
    } else {
        format!("{encounter} [{phase}]")
    };
    apply_color(Color::Underline, raw)
}

// ── Friendly label detection ───────────────────────────────────────────────

/// True when an encounter label clearly names ally / escort / friendly
/// content. Enemy is the default — generator names without these
/// markers count as hostile in the heading totals.
fn is_friendly_label(label: &str) -> bool {
    let lower = label.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    tokens.iter().any(|t| {
        matches!(
            *t,
            "allied" | "allies" | "ally" | "friendly" | "escort" | "attacked"
        )
    })
}

// ── Encounter / phase label cleanup (unchanged from pre-v0.4) ─────────────

/// Pretty-print an encounter `variable_name` and strip generator
/// boilerplate that adds no information for the player.
fn clean_encounter_label(variable_name: &str) -> String {
    strip_generator_chrome(pretty_identifier(variable_name))
}

/// Strip generator boilerplate from a label that has already been
/// passed through `pretty_identifier`. Two passes:
///
/// 1. **Filler suffix strip** — `Spawn Descriptions`, `Ship Spawn
///    Descriptions`, `Ships To Spawn`, `Spawn Description`. Engine
///    plumbing tacked onto variable names.
/// 2. **Wrapper prefix strip** — generator-nesting tokens like
///    `Defend Location Wrapper`, `Escort Ship To/From Landing Area`,
///    `Support Attacked Ship`. The prefix list comes from a full
///    corpus scan; rare prefixes are intentionally not on it.
///
/// Both passes loop so chained / stacked patterns peel cleanly.
fn strip_generator_chrome(label: String) -> String {
    const FILLER_SUFFIXES: &[&str] = &[
        " Ship Spawn Descriptions",
        " Ships To Spawn",
        " Spawn Descriptions",
        " Spawn Description",
    ];
    const WRAPPER_PREFIXES: &[&str] = &[
        "Escort Ship To Landing Area ",
        "Escort Ship From Landing Area ",
        "Defend Location Wrapper ",
        "Support Attacked Ship ",
        "Search And Destroy ",
        "Invisible Timer ",
        "Kill Ship ",
        "First Beat ",
        "Final Beat ",
    ];

    let mut s = label;
    loop {
        let before = s.len();
        for suffix in FILLER_SUFFIXES {
            if let Some(stripped) = s.strip_suffix(suffix) {
                s = stripped.to_string();
                break;
            }
        }
        for prefix in WRAPPER_PREFIXES {
            if let Some(stripped) = s.strip_prefix(prefix) {
                s = stripped.to_string();
                break;
            }
        }
        if s.len() == before {
            break;
        }
    }
    s
}

/// Pretty-print a phase name, but return an empty string when the
/// phase is just an echo of the encounter label.
fn clean_phase_label(phase_name: &str, encounter_label: &str) -> String {
    let pretty = strip_generator_chrome(pretty_identifier(phase_name));
    if pretty.is_empty() {
        return String::new();
    }
    if pretty.eq_ignore_ascii_case(encounter_label) {
        return String::new();
    }
    let pretty_lower = pretty.to_lowercase();
    let enc_lower = encounter_label.to_lowercase();
    if pretty_lower.starts_with(&format!("{enc_lower} "))
        || enc_lower.starts_with(&format!("{pretty_lower} "))
    {
        return String::new();
    }
    if phase_tokens_subset_of_encounter(&pretty_lower, &enc_lower) {
        return String::new();
    }
    pretty
}

fn phase_tokens_subset_of_encounter(phase: &str, encounter: &str) -> bool {
    let phase_tokens: Vec<&str> = phase.split_whitespace().collect();
    if phase_tokens.is_empty() {
        return false;
    }
    let enc_tokens: Vec<&str> = encounter.split_whitespace().collect();
    phase_tokens
        .iter()
        .all(|pt| enc_tokens.iter().any(|et| stem_equivalent(pt, et)))
}

fn stem_equivalent(a: &str, b: &str) -> bool {
    let al = a.to_lowercase();
    let bl = b.to_lowercase();
    if al == bl {
        return true;
    }
    let common = al.chars().zip(bl.chars()).take_while(|(x, y)| x == y).count();
    let max_len = al.chars().count().max(bl.chars().count());
    common >= 4 && max_len - common <= 3
}

/// Decide the final `(encounter, phase)` label pair for display.
fn resolve_labels(encounter: &str, phase: &str) -> (String, String) {
    if phase.is_empty() {
        return (encounter.to_string(), String::new());
    }
    if phase_supersedes_encounter(phase, encounter) {
        return (phase.to_string(), String::new());
    }
    (encounter.to_string(), phase.to_string())
}

fn phase_supersedes_encounter(phase: &str, encounter: &str) -> bool {
    let phase_lower = phase.to_lowercase();
    let enc_lower = encounter.to_lowercase();
    let phase_tokens: Vec<&str> = phase_lower.split_whitespace().collect();
    let enc_tokens: Vec<&str> = enc_lower.split_whitespace().collect();
    if phase_tokens.is_empty() || enc_tokens.is_empty() {
        return false;
    }
    let mut shared = 0usize;
    for pt in &phase_tokens {
        if enc_tokens.iter().any(|et| stem_equivalent(pt, et)) {
            shared += 1;
        }
    }
    shared > 0 && phase_tokens.len() > shared
}

// ── Ship-pool resolution ───────────────────────────────────────────────────

/// Walk a slot's candidates, drop empty display names, dedupe, strip
/// the manufacturer prefix, sort by size+name, then collapse same-hull
/// variants.
fn ship_list_for_slot(
    slot: &ShipSlot,
    ships: &ShipRegistry,
    cache: &LocalizedItemCache,
    locale: &LocaleMap,
    manufacturer_prefixes: &[String],
) -> Vec<String> {
    let mut entries: Vec<(String, i32)> = Vec::new();
    for c in &slot.candidates {
        let Some(display_name) = ships.display_name(&c.entity_guid, cache, locale) else {
            continue;
        };
        if display_name.is_empty() {
            continue;
        }
        let short = strip_manufacturer(manufacturer_prefixes, display_name);
        if !entries.iter().any(|(n, _)| n == &short) {
            entries.push((short, c.size));
        }
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let names: Vec<String> = entries.into_iter().map(|(n, _)| n).collect();
    collapse_variants(&names)
}

/// Union the ship lists across multiple alternatives — used for
/// scaling-only collapsed groups where alternatives should agree but
/// we union defensively in case `HumanPilotNN` tags actually do
/// filter some candidates differently.
fn union_ship_lists(
    options: &[ShipSlot],
    ships: &ShipRegistry,
    cache: &LocalizedItemCache,
    locale: &LocaleMap,
    manufacturer_prefixes: &[String],
) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for opt in options {
        for n in ship_list_for_slot(opt, ships, cache, locale, manufacturer_prefixes) {
            if !seen.contains(&n) {
                seen.push(n);
            }
        }
    }
    seen
}

fn strip_manufacturer(prefixes: &[String], name: &str) -> String {
    for prefix in prefixes {
        if let Some(rest) = name.strip_prefix(prefix.as_str()) {
            return rest.to_string();
        }
    }
    name.to_string()
}

// ── CombatClass range (for the encounter heading) ─────────────────────────

/// Canonical ordering of `AI/Ship/CombatClass` tags from easiest to
/// hardest. Tags outside this list are unrecognised tier extensions
/// and excluded from the range. Order matters — it drives the
/// `VeryEasy-Hard` style range display.
const COMBAT_CLASS_ORDER: &[&str] = &[
    "VeryEasy", "Easy", "Medium", "Hard", "VeryHard", "Super",
];
const COMBAT_CLASS_ORDER_LEN: usize = COMBAT_CLASS_ORDER.len();

/// Compute the mission's CombatClass range across every ship-spawn
/// alternative. Returns:
///
/// - `Some("VeryEasy")` — every group's options share one tier.
/// - `Some("Easy-Hard")` — alternatives span multiple tiers (one
///   option is Easy, another is Hard, etc.).
/// - `None` — no group carries any recognised CombatClass tag.
///
/// Walks both `shared_tags` (tier agreed across all options in a
/// group) AND `axes.combat_class.per_option` (tier differs across
/// alternatives within a group). This covers Settle a Score-style
/// single-tier missions and mixed-tier alternatives like the
/// BountyHunter "engine picks Easy OR Medium" pattern.
///
/// Lives in langpatch rather than sc-contracts because it's a
/// display concern (player-facing tier banner) — promote upstream
/// if a second consumer needs it.
pub fn combat_class_range(encounters: &[Encounter]) -> Option<String> {
    let mut indices: HashSet<usize> = HashSet::new();
    let visit = |name: &str, indices: &mut HashSet<usize>| {
        if let Some(i) = COMBAT_CLASS_ORDER.iter().position(|&o| o == name) {
            indices.insert(i);
        }
    };
    for enc in encounters {
        let Encounter::Ships(s) = enc else { continue };
        for phase in &s.phases {
            for group in &phase.groups {
                for tag in &group.shared_tags {
                    if tag.kind == AxisKind::CombatClass {
                        visit(&tag.name, &mut indices);
                    }
                }
                if group.axes.combat_class.varies {
                    for opt_tags in &group.axes.combat_class.per_option {
                        for (_, name) in opt_tags {
                            visit(name, &mut indices);
                        }
                    }
                }
            }
        }
    }
    if indices.is_empty() {
        return None;
    }
    let lo = *indices.iter().min().unwrap();
    let hi = *indices.iter().max().unwrap();
    if lo == hi {
        Some(COMBAT_CLASS_ORDER[lo].to_string())
    } else {
        Some(format!(
            "{}-{}",
            COMBAT_CLASS_ORDER[lo],
            COMBAT_CLASS_ORDER[hi.min(COMBAT_CLASS_ORDER_LEN - 1)]
        ))
    }
}

// ── Role hints ─────────────────────────────────────────────────────────────

/// Per-slot role hint surfaced from typed `TagBag` predicates.
fn role_hint(bag: &TagBag) -> Option<&'static str> {
    if bag.is_salvage_target() {
        Some("salvage target")
    } else if bag.is_cargo_recovery() {
        Some("cargo recovery")
    } else if bag.is_pre_damaged_wreck() {
        Some("pre-damaged wreck")
    } else {
        None
    }
}

// ── Cargo / value tag collection ───────────────────────────────────────────

/// Pull every cargo / value tag off this slot for the summary line.
/// Returned tags are intended to flow into [`aggregate_tag_summary`].
fn cargo_tags_from_bag(bag: &TagBag, tree: &TagTree) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for c in bag.cargo(tree) {
        let s = c.to_string();
        if !parts.contains(&s) {
            parts.push(s);
        }
    }
    for t in bag.ai_traits(tree) {
        if matches!(t, "HighValue" | "LowValue" | "Mixed") && !parts.iter().any(|x| x == t) {
            parts.push(t.to_string());
        }
    }
    parts
}

fn collect_cargo_tags_from_ship_group(
    group: &SlotGroup<ShipSlot>,
    tree: &TagTree,
    out: &mut Vec<String>,
) {
    for opt in &group.options {
        for t in cargo_tags_from_bag(&opt.positive, tree) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
}

fn collect_cargo_tags_from_entity_group(
    group: &SlotGroup<EntitySlot>,
    tree: &TagTree,
    out: &mut Vec<String>,
) {
    for opt in &group.options {
        for t in cargo_tags_from_bag(&opt.positive, tree) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
}

// ── Tag summary aggregation ────────────────────────────────────────────────

/// Aggregate distinct tags across every group and render them as a
/// single summary line categorised by what the player cares about.
/// Returns `None` when no tags survive filtering.
fn aggregate_tag_summary(all_tags: &[String]) -> Option<String> {
    let mut amounts: Vec<String> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();

    for t in all_tags {
        if is_noise_tag(t) {
            continue;
        }
        let bucket: &mut Vec<String> = if is_cargo_amount(t) {
            &mut amounts
        } else if is_value_tier(t) {
            &mut values
        } else {
            &mut other
        };
        if !bucket.contains(t) {
            bucket.push(t.clone());
        }
    }

    let mut parts: Vec<String> = Vec::new();
    if !amounts.is_empty() {
        parts.push(format!("Cargo: {}", amounts.join(", ")));
    }
    if !values.is_empty() {
        parts.push(format!("Value: {}", values.join(", ")));
    }
    if !other.is_empty() {
        parts.push(format!("Tags: {}", other.join(", ")));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn is_noise_tag(t: &str) -> bool {
    matches!(t, "General")
}

fn is_cargo_amount(t: &str) -> bool {
    t == "Cargo" || t.ends_with(" Cargo") || t.ends_with("Cargo")
}

fn is_value_tier(t: &str) -> bool {
    matches!(t, "HighValue" | "MediumValue" | "LowValue" | "Mixed")
}

// ── NPC counting ───────────────────────────────────────────────────────────

fn count_npcs(encounter: &NpcEncounter) -> i32 {
    let mut total = 0;
    for phase in &encounter.phases {
        match parse_count_from_phase_name(&phase.name) {
            Some(n) => total += n,
            None => total += phase.option_count() as i32,
        }
    }
    total
}

/// Same shape as [`count_npcs`] but only sums slots NOT marked as
/// `mission_allied_marker`. A phase whose every slot is allied is
/// dropped from the count entirely; mixed phases (rare) count as
/// enemy because the worst case for the player is enemies present.
fn count_enemy_npcs(encounter: &NpcEncounter) -> i32 {
    let mut total = 0;
    for phase in &encounter.phases {
        let all_friendly = phase.option_count() > 0
            && phase.all_options().all(|s| s.mission_allied_marker);
        if all_friendly {
            continue;
        }
        match parse_count_from_phase_name(&phase.name) {
            Some(n) => total += n,
            None => total += phase.option_count() as i32,
        }
    }
    total
}

fn parse_count_from_phase_name(name: &str) -> Option<i32> {
    let bytes = name.as_bytes();
    for i in 1..bytes.len() {
        let c = bytes[i];
        if (c == b'x' || c == b'X') && bytes[i - 1].is_ascii_whitespace() {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let start = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > start
                && let Ok(n) = name[start..j].parse::<i32>()
                && n > 0
            {
                return Some(n);
            }
        }
    }
    None
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_count_after_x_with_spaces() {
        assert_eq!(parse_count_from_phase_name("Soldier x 2"), Some(2));
        assert_eq!(parse_count_from_phase_name("Techi x 3"), Some(3));
        assert_eq!(parse_count_from_phase_name("CQC x 4"), Some(4));
    }

    #[test]
    fn parses_count_with_trailing_role_marker() {
        assert_eq!(
            parse_count_from_phase_name("Juggernaut x 1 - Target"),
            Some(1)
        );
    }

    #[test]
    fn parses_count_without_space_after_x() {
        assert_eq!(parse_count_from_phase_name("Sniper x2"), Some(2));
    }

    #[test]
    fn ignores_x_inside_word() {
        assert_eq!(parse_count_from_phase_name("Saxon"), None);
        assert_eq!(parse_count_from_phase_name("Tax2"), None);
    }

    #[test]
    fn returns_none_when_no_count() {
        assert_eq!(parse_count_from_phase_name("Wave1"), None);
        assert_eq!(parse_count_from_phase_name(""), None);
        assert_eq!(parse_count_from_phase_name("Reinforcements"), None);
    }

    #[test]
    fn encounter_label_strips_filler_suffixes() {
        assert_eq!(clean_encounter_label("HostileShipSpawnDescriptions"), "Hostile");
        assert_eq!(clean_encounter_label("AlliedSpawnDescriptions"), "Allied");
        assert_eq!(
            clean_encounter_label("DropoffLocation1ShipsToSpawn"),
            "Dropoff Location 1"
        );
        assert_eq!(clean_encounter_label("MissionTargets"), "Mission Targets");
    }

    #[test]
    fn phase_label_drops_when_matching_encounter() {
        assert_eq!(clean_phase_label("InitialEnemies", "Initial Enemies"), "");
        assert_eq!(clean_phase_label("EscortShip", "Escort Ship"), "");
        assert_eq!(clean_phase_label("Wave1", "Mission Targets"), "Wave 1");
        assert_eq!(
            clean_phase_label("Reinforcements", "Mission Targets"),
            "Reinforcements"
        );
    }

    #[test]
    fn strip_manufacturer_handles_match_and_passthrough() {
        let prefixes = vec!["Aegis ".to_string(), "Drake ".to_string()];
        assert_eq!(strip_manufacturer(&prefixes, "Aegis Avenger"), "Avenger");
        assert_eq!(strip_manufacturer(&prefixes, "Drake Cutlass"), "Cutlass");
        assert_eq!(strip_manufacturer(&prefixes, "300i"), "300i");
    }

    #[test]
    fn encounter_label_strips_wrapper_prefix() {
        assert_eq!(
            clean_encounter_label("DefendLocationWrapperEnemyShips"),
            "Enemy Ships"
        );
        assert_eq!(
            clean_encounter_label("EscortShipToLandingAreaInitialEnemies"),
            "Initial Enemies"
        );
        assert_eq!(
            clean_encounter_label("EscortShipFromLandingAreaEscortReinforcementsWave01"),
            "Escort Reinforcements Wave 01"
        );
        assert_eq!(clean_encounter_label("SupportAttackedShipHostile"), "Hostile");
        assert_eq!(
            clean_encounter_label("SearchAndDestroyReinforcements"),
            "Reinforcements"
        );
        assert_eq!(clean_encounter_label("KillShipMissionTargets"), "Mission Targets");
        assert_eq!(clean_encounter_label("MissionTargets"), "Mission Targets");
    }

    #[test]
    fn phase_drop_handles_extension_pattern() {
        assert_eq!(clean_phase_label("AcePilotShip", "Ace Pilot"), "");
        assert_eq!(
            clean_phase_label("MissionTargets", "Mission Targets Defenders"),
            ""
        );
        assert_eq!(clean_phase_label("Wave1", "Mission Targets"), "Wave 1");
    }

    #[test]
    fn resolve_labels_swaps_when_phase_is_more_specific() {
        assert_eq!(
            resolve_labels("Wave Ships", "Wave 1"),
            ("Wave 1".to_string(), String::new())
        );
        assert_eq!(
            resolve_labels("Wave Ships", "Wave 2"),
            ("Wave 2".to_string(), String::new())
        );
    }

    #[test]
    fn resolve_labels_keeps_both_when_unrelated() {
        assert_eq!(
            resolve_labels("Mission Targets", "Defenders"),
            ("Mission Targets".to_string(), "Defenders".to_string())
        );
        assert_eq!(
            resolve_labels("Hostile", "First Wave"),
            ("Hostile".to_string(), "First Wave".to_string())
        );
    }

    #[test]
    fn resolve_labels_passes_through_empty_phase() {
        assert_eq!(
            resolve_labels("Mission Targets", ""),
            ("Mission Targets".to_string(), String::new())
        );
    }

    #[test]
    fn phase_drop_handles_singular_plural_stem() {
        assert_eq!(clean_phase_label("Target", "Mission Targets"), "");
        assert_eq!(clean_phase_label("Targets", "Mission Target"), "");
        assert_eq!(clean_phase_label("MissionTarget", "Mission Targets"), "");
        assert_eq!(
            clean_phase_label("Defenders", "Mission Targets"),
            "Defenders"
        );
    }

    #[test]
    fn phase_drop_handles_cross_inflection() {
        assert_eq!(clean_phase_label("Allies", "Allied"), "");
        assert_eq!(clean_phase_label("Enemies", "Enemy"), "");
        assert_eq!(clean_phase_label("Defending", "Defender"), "");
    }

    #[test]
    fn stem_equivalent_rejects_unrelated_short_overlap() {
        assert!(!stem_equivalent("Mission", "Mister"));
        assert!(!stem_equivalent("Allied", "Alliance"));
        assert!(!stem_equivalent("Wave", "Hostile"));
    }

    #[test]
    fn stem_equivalent_matches_known_inflections() {
        assert!(stem_equivalent("Target", "Targets"));
        assert!(stem_equivalent("Allied", "Allies"));
        assert!(stem_equivalent("Enemy", "Enemies"));
        assert!(stem_equivalent("Defender", "Defending"));
        assert!(stem_equivalent("Wave", "Waves"));
    }

    #[test]
    fn aggregate_tag_summary_categorises_buckets() {
        let tags = vec![
            "Scraps Cargo".to_string(),
            "LowValue".to_string(),
            "General".to_string(),
            "Bounty".to_string(),
            "Half Cargo".to_string(),
            "Mixed".to_string(),
            "Legal".to_string(),
        ];
        let summary = aggregate_tag_summary(&tags).expect("non-empty");
        assert!(!summary.contains("General"));
        assert!(summary.contains("Cargo: Scraps Cargo, Half Cargo"));
        assert!(summary.contains("Value: LowValue, Mixed"));
        assert!(summary.contains("Tags: Bounty, Legal"));
    }

    #[test]
    fn friendly_label_classifies_typical_cases() {
        assert!(is_friendly_label("Allied"));
        assert!(is_friendly_label("Allied Reinforcements"));
        assert!(is_friendly_label("Escort Ship"));
        assert!(is_friendly_label("Friendly NPCs"));
        assert!(is_friendly_label("Attacked"));
        assert!(!is_friendly_label("Mission Targets"));
        assert!(!is_friendly_label("Initial Enemies"));
        assert!(!is_friendly_label("Hostile"));
        assert!(!is_friendly_label("Enemy Ships"));
        assert!(!is_friendly_label("Defend Location"));
    }

    #[test]
    fn phase_strip_unblocks_supersede_for_wrapper_phase() {
        let raw_encounter = clean_encounter_label("EnemyShips");
        let raw_phase = clean_phase_label("DefendLocationWrapperEnemyShips", &raw_encounter);
        assert_eq!(raw_phase, "");
        let (label_e, label_p) = resolve_labels(&raw_encounter, &raw_phase);
        assert_eq!(label_e, "Enemy Ships");
        assert_eq!(label_p, "");
    }

    #[test]
    fn aggregate_tag_summary_returns_none_when_only_noise() {
        let tags = vec!["General".to_string()];
        assert!(aggregate_tag_summary(&tags).is_none());
    }

    #[test]
    fn compose_count_renders_nx_form() {
        assert_eq!(compose_count_and_pool(1, 1, &[]), "1x");
        assert_eq!(compose_count_and_pool(3, 3, &[]), "3x");
        assert_eq!(compose_count_and_pool(1, 3, &[]), "1-3x");
        assert_eq!(
            compose_count_and_pool(2, 2, &["Cutlass".to_string(), "Sabre".to_string()]),
            "2x Cutlass, Sabre"
        );
        assert_eq!(
            compose_count_and_pool(1, 3, &["Scythe".to_string()]),
            "1-3x Scythe"
        );
    }

    #[test]
    fn broad_ship_class_filter() {
        // Broad class names suppressed (already in the ship pool).
        assert!(is_broad_ship_class_tag("CombatShip"));
        assert!(is_broad_ship_class_tag("LargeCombatShip"));
        assert!(is_broad_ship_class_tag("HeavyInterceptor"));
        assert!(is_broad_ship_class_tag("MediumInterceptor"));
        // Loadout markers under the same DCB subtree survive.
        assert!(!is_broad_ship_class_tag("Distortion"));
        assert!(!is_broad_ship_class_tag("Stealth"));
        // Genuine hull names also survive (we don't classify them as
        // ShipClass in the first place, but be defensive).
        assert!(!is_broad_ship_class_tag("Scythe"));
    }

    #[test]
    fn format_axis_value_phrasing() {
        // Effect → "with X"
        assert_eq!(format_axis_value(AxisKind::Effect, "Distortion"), "with Distortion");
        // Hull / ShipClass / CombatClass → bare
        assert_eq!(format_axis_value(AxisKind::Hull, "Scythe"), "Scythe");
        assert_eq!(format_axis_value(AxisKind::ShipClass, "CombatShip"), "CombatShip");
        assert_eq!(format_axis_value(AxisKind::CombatClass, "Hard"), "Hard");
    }
}
