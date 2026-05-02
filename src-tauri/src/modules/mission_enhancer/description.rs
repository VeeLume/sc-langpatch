//! Description-block rendering — splits a pool's facts into a top
//! "unanimous" section and an optional "Variants" section listing
//! per-member differences.
//!
//! Region placement rule (per design):
//! - No variants section, single region across pool → render `Region` block at top.
//! - No variants section, multiple regions → render `Available at` block at top.
//! - Variants section, all variants share one region → keep `Region` at top, omit from labels.
//! - Variants section, regions differ → drop `Region` block; region appears as variant label.

use sc_contracts::{BlueprintReward, Cooldowns, Mission, MissionIndex, RewardAmount};
use sc_extract::TagTree;
use svarog_datacore::DataCoreDatabase;

use super::encounters;
use super::pool::{BlueprintState, PoolFacts};
use super::variants::{self, ResolutionStats, VariantLabel};
use crate::formatter_helpers::{bullet, header, NEWLINE, PARAGRAPH_BREAK};

#[derive(Debug, Clone, Copy)]
pub struct DescOptions {
    pub blueprint_list: bool,
    pub mission_info: bool,
    pub ship_encounters: bool,
    pub cargo_info: bool,
    pub region_info: bool,
    /// Whether to emit per-pool fallback diagnostics to stderr.
    ///
    /// True for the one-shot patcher run (where the lines are useful
    /// outlier signals), false for the immediate-mode preview TUI
    /// (which re-renders ~30 fps and would flood stderr otherwise)
    /// and for the CLI bin (which prefers clean stdout for piping).
    pub diagnostics: bool,
}

/// Render the suffix appended to the description's INI value. Always
/// includes the leading `\n\n` separator from the original text.
/// Empty string when nothing renders (e.g. all options disabled).
pub fn render(
    facts: &PoolFacts<'_>,
    index: &MissionIndex,
    db: &DataCoreDatabase,
    manufacturer_prefixes: &[String],
    desc_key: &str,
    opts: DescOptions,
) -> String {
    let head = match facts.members.first() {
        Some(m) => *m,
        None => return String::new(),
    };

    // Decide between singleton and variants rendering.
    //
    // We first check whether [`PoolFacts`] thinks any axis diverges
    // (data-level mixing). When it doesn't, the fast singleton path
    // takes over. When it does, we compute the post-dedup group count
    // — if every member's *rendered* diff lines collapse to one group,
    // the data-level mixing didn't survive rendering (e.g. cooldowns
    // differ by milliseconds but round to the same minutes), and we
    // fall back to singleton rendering. Avoids the "Variants (1)"
    // wart the user reported.
    let mut blocks: Vec<String> = Vec::new();

    if !facts.has_variants() {
        push_singleton_blocks(&mut blocks, head, facts, index, manufacturer_prefixes, opts);
    } else {
        let (labels, stats) = variants::resolve(&facts.members, &index.localities, db);
        let groups = group_by_diff_lines(facts, &labels, &index.tag_tree, manufacturer_prefixes, opts);
        if groups.len() <= 1 {
            // Functionally one variant — the data-level divergence
            // didn't produce different rendered output. Render as a
            // singleton using the head member's full info.
            push_singleton_blocks(&mut blocks, head, facts, index, manufacturer_prefixes, opts);
        } else {
            push_variants_blocks(
                &mut blocks,
                facts,
                &groups,
                &labels,
                &stats,
                index,
                manufacturer_prefixes,
                desc_key,
                opts,
            );
        }
    }

    if blocks.is_empty() {
        return String::new();
    }
    format!("{PARAGRAPH_BREAK}{}", blocks.join(PARAGRAPH_BREAK))
}

/// Singleton rendering: every section pulls from `head` directly. Used
/// for one-member pools and pools whose members render identically.
fn push_singleton_blocks(
    blocks: &mut Vec<String>,
    head: &Mission,
    facts: &PoolFacts<'_>,
    index: &MissionIndex,
    manufacturer_prefixes: &[String],
    opts: DescOptions,
) {
    if opts.blueprint_list
        && let Some(bp) = &head.rewards.blueprint
    {
        blocks.push(blueprint_block(bp));
    }
    if opts.mission_info
        && let Some(info) = mission_info_block(head)
    {
        blocks.push(info);
    }
    if opts.ship_encounters
        && let Some(enc) = encounter_block(
            head,
            &index.tag_tree,
            manufacturer_prefixes,
            opts.cargo_info,
        )
    {
        blocks.push(enc);
    }
    if opts.region_info
        && let Some(region) = region_block(facts)
    {
        blocks.push(region);
    }
}

/// Multi-variant rendering: unanimous top section + Variants list.
#[allow(clippy::too_many_arguments)]
fn push_variants_blocks(
    blocks: &mut Vec<String>,
    facts: &PoolFacts<'_>,
    groups: &[DiffGroup<'_>],
    labels: &[VariantLabel<'_>],
    stats: &variants::ResolutionStats,
    index: &MissionIndex,
    manufacturer_prefixes: &[String],
    desc_key: &str,
    opts: DescOptions,
) {
    // Top section — only the axes that are unanimous across all members.
    if opts.blueprint_list
        && matches!(facts.blueprint_state, BlueprintState::AllSamePool)
        && let Some(bp) = facts.members.first().and_then(|m| m.rewards.blueprint.as_ref())
    {
        blocks.push(blueprint_block(bp));
    }
    if opts.mission_info
        && let Some(info) = unanimous_mission_info_block(facts)
    {
        blocks.push(info);
    }
    if opts.ship_encounters
        && facts.encounters_consistent
        && let Some(head) = facts.members.first()
        && let Some(enc) = encounter_block(
            head,
            &index.tag_tree,
            manufacturer_prefixes,
            opts.cargo_info,
        )
    {
        blocks.push(enc);
    }
    if opts.region_info
        && facts.region_labels.len() == 1
        && let Some(region) = region_block(facts)
    {
        blocks.push(region);
    }

    if let Some(block) = render_variants_section(groups, labels, stats, desc_key, opts) {
        let _ = labels;
        blocks.push(block);
    }
}

// ── Top-section blocks (singleton path) ────────────────────────────────────

fn blueprint_block(bp: &BlueprintReward) -> String {
    let mut s = header("Potential Blueprints");
    if bp.chance < 1.0 {
        s.push_str(&format!(" ({}% chance)", (bp.chance * 100.0) as i32));
    }
    for item in &bp.items {
        if item.display_name.is_empty() {
            continue;
        }
        s.push_str(NEWLINE);
        s.push_str(&bullet(&item.display_name));
    }
    s
}

fn mission_info_block(mission: &Mission) -> Option<String> {
    let lines = mission_info_lines(mission);
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "{}{NEWLINE}{}",
        header("Mission Info"),
        lines.join(NEWLINE)
    ))
}

fn mission_info_lines(mission: &Mission) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    if let Some(line) = cooldown_line(&mission.availability.cooldowns, mission.availability.has_personal_cooldown) {
        lines.push(line);
    }
    if let Some(line) = rep_line(&mission.rewards.reputation) {
        lines.push(line);
    }
    if let Some(line) = scrip_line(&mission.rewards.scrip) {
        lines.push(line);
    }
    if let Some(line) = uec_line(&mission.rewards.uec) {
        lines.push(line);
    }
    lines
}

fn cooldown_line(cd: &Cooldowns, has_personal: bool) -> Option<String> {
    // TODO(sc-holotable): cooldowns are usually 0 here because
    // sc-contracts' `apply_int_overrides` only handles
    // `MaxPlayersPerInstance` and drops every other ContractIntParam
    // (including `PersonalCooldownTime`/`AbandonedCooldownTime`).
    // The handler-level fallback `a_personal_mean` is the only source
    // consulted, and most generators set the cooldown via contract-
    // or sub-contract int param overrides — so we read 0 here for
    // anything not configured at the handler level. Until upstream
    // wires through the int-param path (or we add a raw-DCB
    // workaround similar to `crimestat::classify`), we suppress the
    // "0min" line entirely instead of showing misleading data.
    //
    // ALSO TODO(sc-holotable): `DurationRange.mean_seconds` is named
    // for seconds but actually holds minutes. We work around it
    // locally by treating the value as minutes; rename and remove
    // the workaround once upstream is fixed.
    if !has_personal {
        return None;
    }
    let personal_min = cd.completion.as_ref().map(|d| d.mean_seconds).unwrap_or(0.0);
    let abandon_min = cd.abandon.as_ref().map(|d| d.mean_seconds).unwrap_or(0.0);
    if personal_min <= 0.0 {
        return None;
    }

    let personal_str = format_minutes(personal_min);
    // Half a minute apart is the threshold for "meaningfully different".
    if abandon_min > 0.0 && (abandon_min - personal_min).abs() > 0.5 {
        Some(format!(
            "Cooldown: {personal_str} (abandon: {})",
            format_minutes(abandon_min)
        ))
    } else {
        Some(format!("Cooldown: {personal_str}"))
    }
}

/// Format a duration *in minutes* as the smallest sensible unit:
/// `"30min"` for whole minutes, `"45s"` for sub-minute fractions
/// (`0 < min < 1`), `"<1s"` for vanishingly small but non-zero
/// values. Never emits `"0min"`.
fn format_minutes(minutes: f32) -> String {
    if minutes <= 0.0 {
        // Caller filters this out — defensive.
        return "0min".to_string();
    }
    if minutes < 1.0 / 60.0 {
        return "<1s".to_string();
    }
    if minutes < 1.0 {
        let secs = (minutes * 60.0).round() as i32;
        return format!("{secs}s");
    }
    let rounded = minutes.round() as i32;
    if rounded == 0 {
        // Shouldn't hit; sub-1-minute branch covers it.
        let secs = (minutes * 60.0).round() as i32;
        return format!("{secs}s");
    }
    format!("{rounded}min")
}

fn rep_line(reps: &[sc_contracts::RepReward]) -> Option<String> {
    let total: i32 = reps.iter().filter_map(|r| r.amount).filter(|a| *a > 0).sum();
    if total > 0 {
        Some(format!("Rep: {total} XP"))
    } else {
        None
    }
}

fn scrip_line(scrip: &[sc_contracts::ScripReward]) -> Option<String> {
    if scrip.is_empty() {
        return None;
    }
    // Render one entry per distinct currency (MG / Council); each
    // amount is summed within its currency.
    use std::collections::BTreeMap;
    let mut by_name: BTreeMap<&str, i32> = BTreeMap::new();
    for s in scrip {
        if s.amount <= 0 {
            continue;
        }
        *by_name.entry(s.display_name.as_str()).or_insert(0) += s.amount;
    }
    if by_name.is_empty() {
        return None;
    }
    let parts: Vec<String> = by_name
        .into_iter()
        .map(|(name, amt)| {
            if name.is_empty() {
                format!("{amt} scrip")
            } else {
                format!("{amt} {name}")
            }
        })
        .collect();
    Some(format!("Scrip: {}", parts.join(", ")))
}

fn uec_line(uec: &RewardAmount) -> Option<String> {
    match uec {
        RewardAmount::Fixed(n) if *n > 0 => Some(format!("UEC: {n}")),
        _ => None,
    }
}

fn encounter_block(
    mission: &Mission,
    tree: &TagTree,
    manufacturer_prefixes: &[String],
    include_cargo: bool,
) -> Option<String> {
    if mission.encounters.is_empty() {
        return None;
    }
    let rendering = encounters::render(
        &mission.encounters,
        tree,
        manufacturer_prefixes,
        include_cargo,
    );
    if rendering.body.is_empty() {
        return None;
    }
    let heading = format_encounter_heading(
        rendering.enemy_ship_total,
        rendering.enemy_npc_total,
    );
    Some(format!("{}{NEWLINE}{}", header(heading), rendering.body))
}

/// Build the `Encounters` section header, optionally augmented with
/// enemy-side spawn totals: `Encounters · 20x Ships · 10x NPC`.
/// Friendly slots (escort ships, allied NPCs) are excluded from
/// these counts upstream — see `encounters::render`.
fn format_encounter_heading(ship_total: i32, npc_total: i32) -> String {
    let mut parts: Vec<String> = vec!["Encounters".to_string()];
    if ship_total > 0 {
        parts.push(format!("{ship_total}x Ships"));
    }
    if npc_total > 0 {
        parts.push(format!("{npc_total}x NPC"));
    }
    parts.join(" · ")
}

fn region_block(facts: &PoolFacts<'_>) -> Option<String> {
    use crate::modules::mission_enhancer::pool::merge_region_labels;

    if facts.region_labels.is_empty() {
        return None;
    }
    // Each `region_labels` entry is one member's already-merged
    // label, possibly composite ("Stanton: Hurston / Nyx (system-wide)"
    // when the mission spans both). Splitting on `" / "` and
    // re-merging across the pool collapses repeated `"Nyx (system-wide)"`
    // and same-system bodies that show up in multiple members.
    let pieces: Vec<&str> = facts
        .region_labels
        .iter()
        .flat_map(|l| l.split(" / "))
        .collect();
    let entries = merge_region_labels(&pieces);
    if entries.is_empty() {
        return None;
    }
    // One star system per row — the block lives on its own paragraph,
    // so vertical stacking is more legible than `" / "` for any pool
    // touching multiple systems.
    //
    // Header is always "Available at" (the player-facing label
    // doesn't change between singleton and variants — that
    // distinction is internal-only).
    Some(format!(
        "{}{NEWLINE}{}",
        header("Available at"),
        entries.join(NEWLINE)
    ))
}

// ── Variants section (multi-member, mixed axes) ────────────────────────────

fn unanimous_mission_info_block(facts: &PoolFacts<'_>) -> Option<String> {
    let head = facts.members.first()?;
    let mut lines: Vec<String> = Vec::new();
    if facts.cooldowns_consistent
        && let Some(line) = cooldown_line(
            &head.availability.cooldowns,
            head.availability.has_personal_cooldown,
        )
    {
        lines.push(line);
    }
    if facts.rep_consistent
        && let Some(line) = rep_line(&head.rewards.reputation)
    {
        lines.push(line);
    }
    if facts.scrip_consistent
        && let Some(line) = scrip_line(&head.rewards.scrip)
    {
        lines.push(line);
    }
    if facts.uec_consistent
        && let Some(line) = uec_line(&head.rewards.uec)
    {
        lines.push(line);
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "{}{NEWLINE}{}",
        header("Mission Info"),
        lines.join(NEWLINE)
    ))
}

/// One variant after dedup-by-rendered-content. Members that produce
/// identical diff lines collapse into one entry.
pub(super) struct DiffGroup<'a> {
    pub labels: Vec<&'a VariantLabel<'a>>,
    pub diff_lines: Vec<String>,
}

/// Group pool members by their rendered `variant_diff_lines`. Members
/// whose diff lines match exactly collapse into one [`DiffGroup`].
/// Many pools have N missions that share the same generator output
/// (e.g. Foxwell_SecurityPatrol × 4 — all identical to the player);
/// grouping prevents rendering N copies of the same block.
fn group_by_diff_lines<'a>(
    facts: &PoolFacts<'_>,
    labels: &'a [VariantLabel<'a>],
    tree: &TagTree,
    manufacturer_prefixes: &[String],
    opts: DescOptions,
) -> Vec<DiffGroup<'a>> {
    let mut groups: Vec<DiffGroup<'a>> = Vec::new();
    for v in labels {
        let diff = variant_diff_lines(facts, v.mission, manufacturer_prefixes, opts, tree);
        match groups.iter_mut().find(|g| g.diff_lines == diff) {
            Some(existing) => {
                if !existing.labels.iter().any(|l| l.label == v.label) {
                    existing.labels.push(v);
                }
            }
            None => {
                groups.push(DiffGroup {
                    labels: vec![v],
                    diff_lines: diff,
                });
            }
        }
    }
    groups
}

/// Render the `Variants (N)` section for groups. Returns `None` if
/// `groups` is empty. Caller is responsible for choosing between this
/// and singleton rendering — see [`render`].
fn render_variants_section(
    groups: &[DiffGroup<'_>],
    labels: &[VariantLabel<'_>],
    stats: &ResolutionStats,
    desc_key: &str,
    opts: DescOptions,
) -> Option<String> {
    if groups.is_empty() {
        return None;
    }
    let count = groups.len();
    let mut s = header(format!("Variants ({count})"));

    // Build the per-group display label, then dedup colliding group
    // labels with a numeric suffix. Two groups can end up with the
    // same combined label when they share a region but differ on
    // some other axis (rewards, encounters); the numeric suffix gives
    // the player a way to refer to them.
    let mut group_labels: Vec<String> = groups
        .iter()
        .map(|g| combine_group_labels(&g.labels))
        .collect();
    let mut label_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for l in &group_labels {
        *label_counts.entry(l.clone()).or_default() += 1;
    }
    if label_counts.values().any(|c| *c > 1) {
        let mut seen: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for l in group_labels.iter_mut() {
            if label_counts.get(l).copied().unwrap_or(0) > 1 {
                let n = seen.entry(l.clone()).or_insert(0);
                *n += 1;
                *l = format!("{l} ({n})");
            }
        }
    }

    for (g, label) in groups.iter().zip(group_labels.iter()) {
        s.push_str(PARAGRAPH_BREAK);
        s.push_str(&header(format!("· {label}")));

        for line in &g.diff_lines {
            s.push_str(NEWLINE);
            s.push_str("  ");
            s.push_str(line);
        }
    }

    // Outlier diagnostic — fires on pools where at least one rendered
    // group fell to numeric (no region, no rank, no debug-name hint).
    // Suppressed in immediate-mode contexts (TUI / CLI) via
    // `opts.diagnostics`, where it would otherwise fire every frame.
    if opts.diagnostics
        && count > 1
        && groups
            .iter()
            .any(|g| g.labels.iter().any(|l| l.used_numeric))
    {
        let names: Vec<&str> = groups
            .iter()
            .flat_map(|g| g.labels.iter())
            .filter(|l| l.used_numeric)
            .map(|l| l.mission.debug_name.as_str())
            .collect();
        eprintln!(
            "  [mission_enhancer] variant fallback: key='{desc_key}' n={} groups={count} — {} member(s) with no region/rank (debug names: {})",
            labels.len(),
            stats.numeric_fallbacks,
            names.join(", "),
        );
    }
    let _ = stats; // (other stats fields reserved for future diagnostics)

    Some(s)
}

/// Combine the labels of members that grouped together into a single
/// display string.
///
/// Members sharing identical labels collapse to one entry. Numeric
/// `Variant N` labels are ignored when any real label is present in
/// the group — the numbering loses meaning once groups merge.
///
/// When labels carry a rank suffix (`"Stanton: Hurston · Mercenary"`),
/// the place portion is split off, members with the same rank are
/// merged together via [`super::pool::merge_region_labels`] (so
/// `"Stanton: Hurston"` + `"Stanton: ArcCorp"` → `"Stanton: Hurston, ArcCorp"`),
/// then re-joined with their rank suffix. Members with different
/// rank suffixes stay in separate entries so the player can still
/// distinguish them.
fn combine_group_labels(labels: &[&VariantLabel<'_>]) -> String {
    use crate::modules::mission_enhancer::pool::merge_region_labels;

    let mut unique_real: Vec<&str> = Vec::new();
    for l in labels {
        if l.used_numeric {
            continue;
        }
        let s = l.label.as_str();
        if !unique_real.iter().any(|existing| *existing == s) {
            unique_real.push(s);
        }
    }
    if unique_real.is_empty() {
        // All members fell to numeric — pick the first.
        return labels
            .first()
            .map(|l| l.label.as_str())
            .unwrap_or("")
            .to_string();
    }

    // Group by trailing rank suffix (text after " · "), merge the
    // place portion within each group via the region-label merger,
    // re-attach the rank, then join across rank groups.
    let mut by_rank: Vec<(String, Vec<&str>)> = Vec::new();
    for label in &unique_real {
        let (place, rank) = match label.split_once(" · ") {
            Some((p, r)) => (p, r.to_string()),
            None => (*label, String::new()),
        };
        match by_rank.iter_mut().find(|(r, _)| *r == rank) {
            Some((_, places)) => {
                if !places.contains(&place) {
                    places.push(place);
                }
            }
            None => {
                by_rank.push((rank, vec![place]));
            }
        }
    }

    let rendered: Vec<String> = by_rank
        .into_iter()
        .map(|(rank, places)| {
            // Single-line label context — join system entries with
            // `" / "` rather than newlines.
            let merged = merge_region_labels(&places).join(" / ");
            if rank.is_empty() {
                merged
            } else {
                format!("{merged} · {rank}")
            }
        })
        .collect();
    rendered.join(" / ")
}

fn variant_diff_lines(
    facts: &PoolFacts<'_>,
    mission: &Mission,
    manufacturer_prefixes: &[String],
    opts: DescOptions,
    tree: &TagTree,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    // Cooldown (when mixed)
    if opts.mission_info && !facts.cooldowns_consistent {
        if let Some(line) = cooldown_line(
            &mission.availability.cooldowns,
            mission.availability.has_personal_cooldown,
        ) {
            lines.push(line);
        } else {
            lines.push("No cooldown".to_string());
        }
    }
    if opts.mission_info && !facts.rep_consistent {
        match rep_line(&mission.rewards.reputation) {
            Some(line) => lines.push(line),
            None => lines.push("No rep reward".to_string()),
        }
    }
    if opts.mission_info && !facts.scrip_consistent {
        match scrip_line(&mission.rewards.scrip) {
            Some(line) => lines.push(line),
            None => lines.push("No scrip".to_string()),
        }
    }
    if opts.mission_info && !facts.uec_consistent {
        match uec_line(&mission.rewards.uec) {
            Some(line) => lines.push(line),
            None => lines.push("UEC: calculated".to_string()),
        }
    }

    // Blueprint when mixed (different pools or mixed presence).
    // Render as a header line + one bullet per item, matching the
    // singleton-pool layout — long item lists were unreadable as a
    // single comma-joined line.
    let blueprint_mixed = matches!(
        facts.blueprint_state,
        BlueprintState::AllDifferentPools | BlueprintState::MixedPresence
    );
    if opts.blueprint_list && blueprint_mixed {
        match &mission.rewards.blueprint {
            Some(bp) if !bp.items.is_empty() => {
                let names: Vec<&str> = bp
                    .items
                    .iter()
                    .map(|i| i.display_name.as_str())
                    .filter(|s| !s.is_empty())
                    .collect();
                if names.is_empty() {
                    lines.push("Blueprints: (pool empty)".to_string());
                } else {
                    let chance = if bp.chance < 1.0 {
                        format!(" ({}% chance)", (bp.chance * 100.0) as i32)
                    } else {
                        String::new()
                    };
                    lines.push(format!("Blueprints{chance}:"));
                    for name in names {
                        lines.push(bullet(name));
                    }
                }
            }
            _ => lines.push("No blueprint".to_string()),
        }
    }

    // Per-flag deltas — only when that flag is mixed AND this member is the one carrying the value.
    use super::pool::TriState;
    if matches!(facts.shareable, TriState::Mixed) && !mission.shareable {
        lines.push("Solo only".to_string());
    }
    if matches!(facts.shareable, TriState::Mixed) && mission.shareable {
        lines.push("Shareable".to_string());
    }
    if matches!(facts.once_only, TriState::Mixed) && mission.availability.once_only {
        lines.push("One-time only".to_string());
    }
    if matches!(facts.illegal, TriState::Mixed) && mission.illegal_flag {
        lines.push("Illegal".to_string());
    }

    // Encounters (per-variant) when shape differs
    if opts.ship_encounters && !facts.encounters_consistent && !mission.encounters.is_empty() {
        let rendering = encounters::render(
            &mission.encounters,
            tree,
            manufacturer_prefixes,
            opts.cargo_info,
        );
        if !rendering.body.is_empty() {
            // Render as `Encounters:` header + indented body lines.
            // The body already uses NEWLINE between lines — expand them
            // into our two-space indent.
            let heading = format_encounter_heading(
                rendering.enemy_ship_total,
                rendering.enemy_npc_total,
            );
            let indent = format!("{NEWLINE}  ");
            let body = rendering.body.replace(NEWLINE, &indent);
            lines.push(format!("{heading}:{NEWLINE}  {body}"));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_format_whole_values() {
        assert_eq!(format_minutes(1.0), "1min");
        assert_eq!(format_minutes(30.0), "30min");
        assert_eq!(format_minutes(60.0), "60min");
    }

    #[test]
    fn minutes_round_fractional_to_nearest() {
        assert_eq!(format_minutes(1.5), "2min");
        assert_eq!(format_minutes(29.6), "30min");
    }

    #[test]
    fn minutes_under_one_render_as_seconds() {
        // 0.5 min = 30s
        assert_eq!(format_minutes(0.5), "30s");
        // 0.75 min = 45s
        assert_eq!(format_minutes(0.75), "45s");
    }

    #[test]
    fn minutes_vanishingly_small_marker() {
        // Below 1 second.
        assert_eq!(format_minutes(0.001), "<1s");
    }
}
