//! Encounter rendering — structured slot data, then collapse passes.
//!
//! Input: a mission's `Vec<Encounter>`. Output: a multi-line block
//! (without the `Encounters` header — caller adds that).
//!
//! Pipeline:
//!
//! 1. **Collect** — every `ShipSlot` / `EntitySlot` becomes one
//!    [`SlotLine`] holding the resolved ships, tags, skill, role
//!    hints, and the encounter / phase labels (with cleanup).
//! 2. **Skill merge** — slots that share every other axis but differ
//!    on AI skill collapse to a single line with `Skill 40-60`.
//! 3. **Phase merge** — slots that share every other axis but differ
//!    on phase label collapse via numeric-token range detection
//!    (`Wave 1` + `Wave 2` + `Wave 3` → `Wave 1-3`).
//! 4. **Dedup** — fully-identical lines collapse with a `(×N)` suffix.
//! 5. **Render** — one logical line per remaining slot. When the
//!    ship list spans multiple manufacturers (or many ships), the
//!    body becomes a header line + manufacturer-grouped bullets.
//!
//! NPC encounters bypass this pipeline and collapse into a single
//! `NPCs: N` total — see [`count_npcs`].

use sc_contracts::{Encounter, EntitySlot, NpcEncounter, ShipCandidate, ShipSlot, TagBag};
use sc_extract::TagTree;

use super::format::{collapse_variants, pretty_identifier};
use crate::formatter_helpers::{apply_color, Color, NEWLINE};

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
    pub enemy_ship_total: i32,
    pub enemy_npc_total: i32,
}

/// Render every encounter on a mission as a multi-line block.
pub fn render(
    encounters: &[Encounter],
    tree: &TagTree,
    manufacturer_prefixes: &[String],
    include_cargo: bool,
) -> EncounterRendering {
    let mut slots: Vec<SlotLine> = Vec::new();
    let mut npc_total: i32 = 0;
    let mut enemy_ship_total: i32 = 0;
    let mut enemy_npc_total: i32 = 0;

    for enc in encounters {
        match enc {
            Encounter::Ships(s) => {
                let raw_encounter = clean_encounter_label(&s.variable_name);
                let friendly = is_friendly_label(&raw_encounter);
                for phase in &s.phases {
                    let raw_phase = clean_phase_label(&phase.name, &raw_encounter);
                    let (encounter_label, phase_label) =
                        resolve_labels(&raw_encounter, &raw_phase);
                    for slot in &phase.slots {
                        if let Some(line) = build_ship_line(
                            slot,
                            tree,
                            manufacturer_prefixes,
                            include_cargo,
                            encounter_label.clone(),
                            phase_label.clone(),
                        ) {
                            if !friendly {
                                enemy_ship_total += slot.concurrent.max(1);
                            }
                            slots.push(line);
                        }
                    }
                }
            }
            Encounter::Npcs(s) => {
                npc_total += count_npcs(s);
                enemy_npc_total += count_enemy_npcs(s);
            }
            Encounter::Entities(s) => {
                let raw_encounter = clean_encounter_label(&s.variable_name);
                for phase in &s.phases {
                    let raw_phase = clean_phase_label(&phase.name, &raw_encounter);
                    let (encounter_label, phase_label) =
                        resolve_labels(&raw_encounter, &raw_phase);
                    for slot in &phase.slots {
                        if let Some(line) = build_entity_line(
                            slot,
                            tree,
                            include_cargo,
                            encounter_label.clone(),
                            phase_label.clone(),
                        ) {
                            slots.push(line);
                        }
                    }
                }
            }
            Encounter::Unknown { .. } => {}
        }
    }

    let collapsed = merge_slots(slots);
    let collapsed = merge_phases(collapsed);
    let collapsed = dedup_with_count(collapsed);

    let mut out: Vec<String> = Vec::new();
    for slot in &collapsed {
        for rendered_line in render_slot(slot) {
            out.push(rendered_line);
        }
    }

    if npc_total > 0 {
        out.push(format!("NPCs: {npc_total}"));
    }

    // Aggregate cargo / value / faction tags across every slot in
    // the mission and surface as a single summary line. Per-slot
    // tag rendering is dropped — repeating "General, LowValue,
    // Mixed, Scraps Cargo" on every line was high-volume noise; the
    // summary keeps the loot signal without burying the layout.
    if let Some(summary) = aggregate_tag_summary(&collapsed) {
        out.push(summary);
    }

    EncounterRendering {
        body: out.join(NEWLINE),
        enemy_ship_total,
        enemy_npc_total,
    }
}

/// True when an encounter label clearly names ally / escort / friendly
/// content. Enemy is the default — generator names without these
/// markers count as hostile in the heading totals.
fn is_friendly_label(label: &str) -> bool {
    let lower = label.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    tokens.iter().any(|t| {
        matches!(
            *t,
            "allied"
                | "allies"
                | "ally"
                | "friendly"
                | "escort"
                | "attacked"
        )
    })
}

/// Same shape as [`count_npcs`] but only sums slots NOT marked as
/// `mission_allied_marker`. A phase whose every slot is allied is
/// dropped from the count entirely; mixed phases (rare) count as
/// enemy because the worst case for the player is enemies present.
fn count_enemy_npcs(encounter: &NpcEncounter) -> i32 {
    let mut total = 0;
    for phase in &encounter.phases {
        let all_friendly = !phase.slots.is_empty()
            && phase.slots.iter().all(|s| s.mission_allied_marker);
        if all_friendly {
            continue;
        }
        match parse_count_from_phase_name(&phase.name) {
            Some(n) => total += n,
            None => total += phase.slots.len() as i32,
        }
    }
    total
}

// ── Encounter / phase label cleanup ────────────────────────────────────────

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
/// Used by both [`clean_encounter_label`] and [`clean_phase_label`]
/// because phases also carry the same wrapper-encoded generator
/// names — without stripping at the phase layer, when the phase
/// later supersedes the encounter, the wrapper text leaks back
/// into the rendered label.
fn strip_generator_chrome(label: String) -> String {
    const FILLER_SUFFIXES: &[&str] = &[
        " Ship Spawn Descriptions",
        " Ships To Spawn",
        " Spawn Descriptions",
        " Spawn Description",
    ];
    // Wrapper prefixes — stripped only when followed by a space, so
    // `Final Beat` doesn't eat `Final Beats Mission`.
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
///
/// Drop conditions, in order:
/// - exact match (case-insensitive)
/// - one is the other extended by a trailing token (`Ace Pilot` +
///   `Ace Pilot Ship` → drop, `Mission Targets` + `Mission Targets
///   Defenders` → drop)
/// - every phase token is a singular/plural stem of some encounter
///   token (`Mission Targets` + `Target` → drop, since "target" is
///   the stem of "targets")
///
/// Stops short of richer morphology — `Allied` vs `Allies` is kept
/// because the stems differ (`allie` vs `allied`), and the
/// distinction may genuinely matter.
fn clean_phase_label(phase_name: &str, encounter_label: &str) -> String {
    // Apply the same generator-chrome strip as encounters — when
    // a phase later supersedes the encounter, the unfiltered phase
    // text would otherwise leak the wrappers back into the label.
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

/// True when every whitespace-separated token in `phase` is
/// [`stem_equivalent`] to some token in `encounter`. Both inputs
/// should already be lowercase.
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

/// Two tokens look like inflections of the same root word when they
/// share a long common prefix and differ by only a few trailing
/// characters. Threshold: at least 4 leading chars in common AND no
/// more than 3 trailing chars total of difference.
///
/// Catches the inflection patterns that show up in CIG mission
/// labels — singular/plural (`Target` ↔ `Targets`), past-tense vs
/// plural-of-past (`Allied` ↔ `Allies`), noun/verb-form
/// (`Defender` ↔ `Defending`) — without over-matching short or
/// unrelated words. `Cat` ↔ `Cats` falls below the 4-char floor and
/// is intentionally not matched (sub-4-char tokens don't appear in
/// mission labels in practice).
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
///
/// Most pairs are passed through. The non-trivial case: when the
/// phase is *more specific* than the encounter (shares at least one
/// token AND has additional content), the phase replaces the
/// encounter and the phase slot becomes empty. This collapses
/// `Wave Ships [Wave 1]` to just `Wave 1`, since the encounter's
/// `Wave Ships` is generic boilerplate next to the specific
/// `Wave 1`.
fn resolve_labels(encounter: &str, phase: &str) -> (String, String) {
    if phase.is_empty() {
        return (encounter.to_string(), String::new());
    }
    if phase_supersedes_encounter(phase, encounter) {
        return (phase.to_string(), String::new());
    }
    (encounter.to_string(), phase.to_string())
}

/// True when phase shares at least one stem-equivalent token with
/// encounter AND has additional tokens beyond those matches.
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

// ── Slot collection ───────────────────────────────────────────────────────

/// One slot's resolved data, keyed by every axis the collapse passes
/// might merge or compare.
#[derive(Debug, Clone)]
struct SlotLine {
    encounter_label: String,
    phase_label: String,
    /// Sum of source-slot `concurrent` after merge. For a fresh slot
    /// this is the slot's own concurrent count.
    concurrent: i32,
    body: BodyKind,
    tags: Vec<String>,
    /// Unique skill levels across merged slots. Single-element vec
    /// for a fresh slot; range-merged slots accumulate distinct values.
    skills: Vec<u32>,
    ace: bool,
    role_hint: Option<&'static str>,
    /// Number of source slots merged into this line. 1 = unmerged.
    /// Drives the `One of:` vs `{N}x:` distinction in the renderer:
    /// a merged group of single-ship single-concurrent sources reads
    /// as alternatives ("engine picks one of these"), distinct from
    /// a single source slot whose pool of ships happens to be large
    /// (still reads as "{conc}x from this pool").
    source_slot_count: usize,
    /// True iff every source slot had `ship_count == 1` and
    /// `concurrent == 1`. AND-folded during merge.
    all_singleton_sources: bool,
    /// Multiplicity from the post-merge dedup pass. 1 for a fresh slot.
    count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BodyKind {
    /// Resolved ship pool — short hull names with manufacturer
    /// prefix stripped. `collapse_variants` already folded same-hull
    /// variants together.
    Ships(Vec<String>),
    /// Slot's tag query didn't resolve any candidates — coarse class
    /// label from `mission_tags`.
    RoleOnly(String),
    /// Generic entity slot (no candidate resolution available).
    Entities(i32),
}

fn build_ship_line(
    slot: &ShipSlot,
    tree: &TagTree,
    manufacturer_prefixes: &[String],
    include_cargo: bool,
    encounter_label: String,
    phase_label: String,
) -> Option<SlotLine> {
    let ships = ship_list_for_slot(slot, manufacturer_prefixes);
    let body = if !ships.is_empty() {
        BodyKind::Ships(ships)
    } else if let Some(role) = role_hint_for_empty_slot(&slot.positive, tree) {
        BodyKind::RoleOnly(role.to_string())
    } else {
        return None;
    };

    let tags = if include_cargo {
        cargo_tags(&slot.positive, tree)
    } else {
        Vec::new()
    };

    let skills = match slot.positive.ai_skill() {
        Some(s) => vec![s],
        None => Vec::new(),
    };

    let concurrent = slot.concurrent.max(1);
    let ship_count = match &body {
        BodyKind::Ships(s) => s.len(),
        _ => 0,
    };
    let all_singleton_sources = ship_count == 1 && concurrent == 1;

    Some(SlotLine {
        encounter_label,
        phase_label,
        concurrent,
        body,
        tags,
        skills,
        ace: slot.positive.ace_pilot(),
        role_hint: role_hint(&slot.positive),
        source_slot_count: 1,
        all_singleton_sources,
        count: 1,
    })
}

fn build_entity_line(
    slot: &EntitySlot,
    tree: &TagTree,
    include_cargo: bool,
    encounter_label: String,
    phase_label: String,
) -> Option<SlotLine> {
    let tags = if include_cargo {
        cargo_tags(&slot.positive, tree)
    } else {
        Vec::new()
    };
    let skills = match slot.positive.ai_skill() {
        Some(s) => vec![s],
        None => Vec::new(),
    };
    Some(SlotLine {
        encounter_label,
        phase_label,
        concurrent: slot.amount.max(1),
        body: BodyKind::Entities(slot.amount.max(1)),
        tags,
        skills,
        ace: false,
        role_hint: None,
        source_slot_count: 1,
        all_singleton_sources: false,
        count: 1,
    })
}

/// Walk the slot's candidates, drop empty display names, dedupe,
/// strip the manufacturer prefix, sort by size+name, then collapse
/// same-hull variants.
fn ship_list_for_slot(
    slot: &ShipSlot,
    manufacturer_prefixes: &[String],
) -> Vec<String> {
    let mut entries: Vec<(String, i32)> = Vec::new();
    for c in &slot.candidates {
        let ShipCandidate { display_name, size, .. } = c;
        if display_name.is_empty() {
            continue;
        }
        let short = strip_manufacturer(manufacturer_prefixes, display_name);
        if !entries.iter().any(|(n, _)| n == &short) {
            entries.push((short, *size));
        }
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let names: Vec<String> = entries.into_iter().map(|(n, _)| n).collect();
    collapse_variants(&names)
}

/// Strip a known manufacturer prefix from a ship display name.
/// Returns the input unchanged when no prefix matches.
fn strip_manufacturer(prefixes: &[String], name: &str) -> String {
    for prefix in prefixes {
        if let Some(rest) = name.strip_prefix(prefix.as_str()) {
            return rest.to_string();
        }
    }
    name.to_string()
}

/// Concatenated cargo + value-tier descriptors for a ship/entity slot.
fn cargo_tags(bag: &TagBag, tree: &TagTree) -> Vec<String> {
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

/// Coarse class label for a slot whose tag query didn't resolve any
/// candidate ships.
fn role_hint_for_empty_slot(bag: &TagBag, tree: &TagTree) -> Option<&'static str> {
    for t in bag.mission_tags(tree) {
        match t {
            "DefendShip" => return Some("transport/cargo"),
            "CombatShip" | "LargeCombatShip" => return Some("capital"),
            _ => {}
        }
    }
    None
}

// ── Collapse passes ────────────────────────────────────────────────────────

/// Aggressive merge — fold every slot in the same `(encounter,
/// phase, tags, role_hint)` cell into one line.
///
/// Same-cell slots that differ on ships, skill, or concurrent get
/// combined: ships are unioned (preserving first-seen order),
/// skills accumulate for range rendering, and concurrent counts
/// **sum** (interpreted as "this many distinct spawns in this
/// phase across all configurations"). Body kinds must match
/// (ships+ships, entity+entity); a `RoleOnly` slot doesn't merge
/// with a `Ships` slot even if other axes match.
fn merge_slots(slots: Vec<SlotLine>) -> Vec<SlotLine> {
    let mut out: Vec<SlotLine> = Vec::new();
    for slot in slots {
        let position = out.iter().position(|other| {
            other.encounter_label == slot.encounter_label
                && other.phase_label == slot.phase_label
                && other.tags == slot.tags
                && other.role_hint == slot.role_hint
                && std::mem::discriminant(&other.body) == std::mem::discriminant(&slot.body)
        });
        match position {
            Some(idx) => {
                let target = &mut out[idx];
                match (&mut target.body, slot.body) {
                    (BodyKind::Ships(existing), BodyKind::Ships(incoming)) => {
                        for n in incoming {
                            if !existing.contains(&n) {
                                existing.push(n);
                            }
                        }
                    }
                    (BodyKind::Entities(existing_amount), BodyKind::Entities(new_amount)) => {
                        *existing_amount = (*existing_amount).max(new_amount);
                    }
                    (BodyKind::RoleOnly(_), BodyKind::RoleOnly(_)) => {
                        // Same role label — nothing to merge into the body.
                    }
                    _ => unreachable!(
                        "discriminant guard above should have rejected this combination"
                    ),
                }
                for s in slot.skills {
                    if !target.skills.contains(&s) {
                        target.skills.push(s);
                    }
                }
                target.concurrent += slot.concurrent;
                target.ace = target.ace || slot.ace;
                target.source_slot_count += slot.source_slot_count;
                target.all_singleton_sources =
                    target.all_singleton_sources && slot.all_singleton_sources;
            }
            None => out.push(slot),
        }
    }
    out
}

/// Merge slots that share every axis except phase label. Phase
/// labels collapse via numeric-token range detection: `Wave 1` +
/// `Wave 2` + `Wave 3` → `Wave 1-3`.
fn merge_phases(slots: Vec<SlotLine>) -> Vec<SlotLine> {
    let mut groups: Vec<(SlotLine, Vec<String>)> = Vec::new();
    for slot in slots {
        let phase = slot.phase_label.clone();
        let merge_target = groups.iter_mut().find(|(other, _)| {
            other.encounter_label == slot.encounter_label
                && other.concurrent == slot.concurrent
                && other.body == slot.body
                && other.tags == slot.tags
                && other.skills == slot.skills
                && other.ace == slot.ace
                && other.role_hint == slot.role_hint
                && other.source_slot_count == slot.source_slot_count
                && other.all_singleton_sources == slot.all_singleton_sources
        });
        match merge_target {
            Some((_, phases)) => {
                if !phases.contains(&phase) {
                    phases.push(phase);
                }
            }
            None => groups.push((slot, vec![phase])),
        }
    }
    groups
        .into_iter()
        .map(|(mut slot, phases)| {
            slot.phase_label = merge_phase_labels(&phases);
            slot
        })
        .collect()
}

/// Collapse a list of phase labels into one display string.
///
/// - Single label: pass through.
/// - Multiple labels with same token count differing only on a
///   single numeric token: replace that token with `min-max`.
/// - Anything else: comma-join with first-seen order preserved.
fn merge_phase_labels(labels: &[String]) -> String {
    let nonempty: Vec<&str> = labels.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect();
    if nonempty.is_empty() {
        return String::new();
    }
    if nonempty.len() == 1 {
        return nonempty[0].to_string();
    }

    let token_lists: Vec<Vec<&str>> = nonempty.iter().map(|l| l.split_whitespace().collect()).collect();
    let token_count = token_lists[0].len();
    let same_count = token_lists.iter().all(|t| t.len() == token_count);
    if same_count {
        let mut varying: Vec<usize> = Vec::new();
        for i in 0..token_count {
            let first = token_lists[0][i];
            if !token_lists.iter().all(|t| t[i] == first) {
                varying.push(i);
            }
        }
        if varying.len() == 1 {
            let pos = varying[0];
            let nums: Vec<i32> = token_lists
                .iter()
                .filter_map(|t| t[pos].parse::<i32>().ok())
                .collect();
            if nums.len() == token_lists.len() {
                let lo = *nums.iter().min().unwrap();
                let hi = *nums.iter().max().unwrap();
                let range = if lo == hi {
                    lo.to_string()
                } else {
                    format!("{lo}-{hi}")
                };
                let mut tokens: Vec<String> =
                    token_lists[0].iter().map(|s| (*s).to_string()).collect();
                tokens[pos] = range;
                return tokens.join(" ");
            }
        }
    }
    nonempty.join(", ")
}

/// Collapse fully-identical lines into one with a `(×N)` count
/// suffix. Operates on the post-merge lines so that
/// `Wave 1-3` + `Wave 1-3` still dedups.
fn dedup_with_count(slots: Vec<SlotLine>) -> Vec<SlotLine> {
    let mut out: Vec<SlotLine> = Vec::new();
    for slot in slots {
        let target = out.iter_mut().find(|other| {
            other.encounter_label == slot.encounter_label
                && other.phase_label == slot.phase_label
                && other.concurrent == slot.concurrent
                && other.body == slot.body
                && other.tags == slot.tags
                && other.skills == slot.skills
                && other.ace == slot.ace
                && other.role_hint == slot.role_hint
                && other.source_slot_count == slot.source_slot_count
                && other.all_singleton_sources == slot.all_singleton_sources
        });
        match target {
            Some(t) => t.count += 1,
            None => out.push(slot),
        }
    }
    out
}

// ── Rendering ──────────────────────────────────────────────────────────────

/// Render one collapsed slot to one or more output lines.
fn render_slot(slot: &SlotLine) -> Vec<String> {
    // Skill (and Ace, when not at skill 100) lead the body — moves
    // the most uniform piece of info to a predictable position.
    // Tags are NOT rendered per-slot; they aggregate into a single
    // summary line at the end of the encounters block.
    let skill_lead = render_skill_lead(slot);
    let trailing = render_trailing(slot);

    match &slot.body {
        BodyKind::Ships(names) => render_ship_lines(slot, names, &skill_lead, &trailing),
        BodyKind::RoleOnly(role) => {
            let body = format!("{skill_lead}{role}");
            vec![format_inline_line(&label_with_phase(slot), &body, &trailing)]
        }
        BodyKind::Entities(amount) => {
            let amount_str = if *amount > 1 {
                format!("{amount}x entities")
            } else {
                "entity".to_string()
            };
            let body = format!("{skill_lead}{amount_str}");
            vec![format_inline_line(&label_with_phase(slot), &body, &trailing)]
        }
    }
}

/// Render a ships body across one or two lines.
///
/// - **Inline** when the slot is the simplest case (single ship, no
///   merge, concurrent of 1): `Encounter [Phase]: {skill} ship`.
/// - **Multi-line** otherwise: header line `Encounter [Phase]:`,
///   indented body with `{skill} {N}x: ships` or `{skill} One of: ships`
///   when the merge folded multiple single-ship single-concurrent
///   sources into alternatives.
fn render_ship_lines(
    slot: &SlotLine,
    names: &[String],
    skill_lead: &str,
    trailing: &str,
) -> Vec<String> {
    let label = label_with_phase(slot);

    // Trivial case — drop straight inline.
    if names.len() == 1 && slot.source_slot_count == 1 && slot.concurrent == 1 {
        let body = format!("{skill_lead}{}", names[0]);
        return vec![format_inline_line(&label, &body, trailing)];
    }

    let prefix = if names.len() > 1
        && slot.source_slot_count > 1
        && slot.all_singleton_sources
    {
        // Several source slots, each contributing a single ship at
        // concurrent=1 — engine picks one of the alternatives.
        "One of: ".to_string()
    } else {
        format!("{}x: ", slot.concurrent)
    };

    vec![
        format!("{label}:"),
        format!("  {skill_lead}{prefix}{}{trailing}", names.join(", ")),
    ]
}

/// Header label + optional `[phase]`.
/// Header label for one slot — the encounter name plus any
/// surviving phase qualifier in brackets. Wrapped in
/// `Color::Underline` so the labels stand out as scan anchors when
/// the player skims a long encounter list. The trailing colon and
/// body stay plain.
fn label_with_phase(slot: &SlotLine) -> String {
    let raw = if slot.phase_label.is_empty() {
        slot.encounter_label.clone()
    } else {
        format!("{} [{}]", slot.encounter_label, slot.phase_label)
    };
    apply_color(Color::Underline, raw)
}

/// Leading skill / Ace marker for the body. Returns either an empty
/// string (no skill data, no Ace) or a `"Skill 40 · "` /
/// `"Skill 40-60 · Ace · "` formatted segment ready to drop in
/// front of the count + ships.
///
/// `Skill 100` implies an Ace pilot, so the redundant `· Ace`
/// suffix is suppressed when the max skill in the merge is 100.
fn render_skill_lead(slot: &SlotLine) -> String {
    let max_skill = slot.skills.iter().copied().max();
    let suppress_ace = max_skill == Some(100);
    let ace_to_show = slot.ace && !suppress_ace;
    match format_skill(&slot.skills, ace_to_show) {
        Some(s) => format!("{s} · "),
        None => String::new(),
    }
}

/// Trailing metadata: ` · (role hint)` plus `(×N)` multiplicity.
fn render_trailing(slot: &SlotLine) -> String {
    let mut s = String::new();
    if let Some(hint) = slot.role_hint {
        s.push_str(&format!(" · ({hint})"));
    }
    if slot.count > 1 {
        s.push_str(&format!(" (×{})", slot.count));
    }
    s
}

fn format_skill(skills: &[u32], ace: bool) -> Option<String> {
    if skills.is_empty() {
        return if ace { Some("Ace".to_string()) } else { None };
    }
    let mut sorted: Vec<u32> = skills.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let label = if sorted.len() == 1 {
        format!("Skill {}", sorted[0])
    } else {
        format!("Skill {}-{}", sorted.first().unwrap(), sorted.last().unwrap())
    };
    if ace {
        Some(format!("{label} · Ace"))
    } else {
        Some(label)
    }
}

fn format_inline_line(label: &str, body: &str, meta: &str) -> String {
    format!("{label}: {body}{meta}")
}

// ── Tag summary aggregation ───────────────────────────────────────────────

/// Aggregate distinct tags across every collapsed slot and render
/// them as a single summary line categorised by what the player
/// cares about. Returns `None` when no tags survive filtering.
///
/// Buckets:
/// - **Cargo** — anything ending in `Cargo` (`Scraps Cargo`,
///   `Half Cargo`, `Full Cargo`, …)
/// - **Value** — `HighValue` / `MediumValue` / `LowValue` / `Mixed`
/// - **Tags** — everything else (`Bounty`, `Salvage`, faction
///   markers, etc.)
///
/// `General` is dropped as pure noise — it appears on nearly every
/// slot and tells the player nothing.
fn aggregate_tag_summary(slots: &[SlotLine]) -> Option<String> {
    let mut amounts: Vec<String> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();

    for slot in slots {
        for t in &slot.tags {
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

// ── NPC counting (unchanged) ──────────────────────────────────────────────

fn count_npcs(encounter: &NpcEncounter) -> i32 {
    let mut total = 0;
    for phase in &encounter.phases {
        match parse_count_from_phase_name(&phase.name) {
            Some(n) => total += n,
            None => total += phase.slots.len() as i32,
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
        assert_eq!(
            clean_encounter_label("HostileShipSpawnDescriptions"),
            "Hostile"
        );
        assert_eq!(
            clean_encounter_label("AlliedSpawnDescriptions"),
            "Allied"
        );
        assert_eq!(
            clean_encounter_label("DropoffLocation1ShipsToSpawn"),
            "Dropoff Location 1"
        );
        // No filler suffix — pretty_identifier-only.
        assert_eq!(clean_encounter_label("MissionTargets"), "Mission Targets");
    }

    #[test]
    fn phase_label_drops_when_matching_encounter() {
        assert_eq!(clean_phase_label("InitialEnemies", "Initial Enemies"), "");
        assert_eq!(
            clean_phase_label("EscortShip", "Escort Ship"),
            ""
        );
        // Genuine phase qualifier — kept.
        assert_eq!(clean_phase_label("Wave1", "Mission Targets"), "Wave 1");
        assert_eq!(
            clean_phase_label("Reinforcements", "Mission Targets"),
            "Reinforcements"
        );
    }

    #[test]
    fn merges_phase_labels_with_numeric_range() {
        assert_eq!(
            merge_phase_labels(&[
                "Wave 1".to_string(),
                "Wave 2".to_string(),
                "Wave 3".to_string()
            ]),
            "Wave 1-3"
        );
        assert_eq!(
            merge_phase_labels(&[
                "Drop Off 1 Enemy Ships".to_string(),
                "Drop Off 2 Enemy Ships".to_string(),
                "Drop Off 3 Enemy Ships".to_string()
            ]),
            "Drop Off 1-3 Enemy Ships"
        );
    }

    #[test]
    fn merges_phase_labels_falls_back_to_comma_list() {
        // Different token count → fallback.
        assert_eq!(
            merge_phase_labels(&[
                "Wave 1".to_string(),
                "Initial Wave".to_string()
            ]),
            "Wave 1, Initial Wave"
        );
        // Same shape but non-numeric varying token → fallback.
        assert_eq!(
            merge_phase_labels(&["Wave A".to_string(), "Wave B".to_string()]),
            "Wave A, Wave B"
        );
    }

    #[test]
    fn merge_phase_labels_passes_through_single() {
        assert_eq!(
            merge_phase_labels(&["Wave 1".to_string()]),
            "Wave 1"
        );
        assert_eq!(merge_phase_labels(&[]), "");
    }

    #[test]
    fn format_skill_renders_range() {
        assert_eq!(format_skill(&[40], false).unwrap(), "Skill 40");
        assert_eq!(format_skill(&[40, 60], false).unwrap(), "Skill 40-60");
        assert_eq!(format_skill(&[40, 60, 50], false).unwrap(), "Skill 40-60");
        assert_eq!(format_skill(&[40], true).unwrap(), "Skill 40 · Ace");
        assert_eq!(format_skill(&[], true).unwrap(), "Ace");
        assert!(format_skill(&[], false).is_none());
    }

    #[test]
    fn strip_manufacturer_handles_match_and_passthrough() {
        let prefixes = vec!["Aegis ".to_string(), "Drake ".to_string()];
        assert_eq!(strip_manufacturer(&prefixes, "Aegis Avenger"), "Avenger");
        assert_eq!(strip_manufacturer(&prefixes, "Drake Cutlass"), "Cutlass");
        // No prefix match — input passes through unchanged.
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
        assert_eq!(
            clean_encounter_label("SupportAttackedShipHostile"),
            "Hostile"
        );
        assert_eq!(
            clean_encounter_label("SearchAndDestroyReinforcements"),
            "Reinforcements"
        );
        assert_eq!(
            clean_encounter_label("KillShipMissionTargets"),
            "Mission Targets"
        );
        // No wrapper — unchanged.
        assert_eq!(clean_encounter_label("MissionTargets"), "Mission Targets");
    }

    #[test]
    fn encounter_label_strips_wrapper_and_filler_together() {
        // Both passes must run — wrapper prefix AND filler suffix on
        // the same label.
        assert_eq!(
            clean_encounter_label("DropoffLocation1ShipsToSpawn"),
            "Dropoff Location 1"
        );
    }

    #[test]
    fn phase_drop_handles_extension_pattern() {
        // Phase = encounter + " Ship" → drop.
        assert_eq!(clean_phase_label("AcePilotShip", "Ace Pilot"), "");
        // Encounter = phase + " Defenders" → drop.
        assert_eq!(clean_phase_label("MissionTargets", "Mission Targets Defenders"), "");
        // Independent phase — kept.
        assert_eq!(clean_phase_label("Wave1", "Mission Targets"), "Wave 1");
    }

    #[test]
    fn resolve_labels_swaps_when_phase_is_more_specific() {
        // Wave Ships [Wave 1] — phase has the encounter's "Wave"
        // token plus a number; the encounter is generic boilerplate
        // next to the specific phase. Use phase as label.
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
        // Phase token is the singular form of an encounter token.
        assert_eq!(clean_phase_label("Target", "Mission Targets"), "");
        // Plural phase, singular encounter — symmetric.
        assert_eq!(clean_phase_label("Targets", "Mission Target"), "");
        // Multi-token phase: every token must stem-match.
        assert_eq!(
            clean_phase_label("MissionTarget", "Mission Targets"),
            ""
        );
        // Mixed match — one token doesn't stem to anything in encounter.
        assert_eq!(
            clean_phase_label("Defenders", "Mission Targets"),
            "Defenders"
        );
    }

    #[test]
    fn phase_drop_handles_cross_inflection() {
        // -ed vs -ies — both inflect from the same root, drop phase.
        assert_eq!(clean_phase_label("Allies", "Allied"), "");
        // -y vs -ies — common pattern.
        assert_eq!(clean_phase_label("Enemies", "Enemy"), "");
        // -er vs -ing — same root, different forms.
        assert_eq!(clean_phase_label("Defending", "Defender"), "");
    }

    #[test]
    fn stem_equivalent_rejects_unrelated_short_overlap() {
        // 3-character common prefix isn't enough.
        assert!(!stem_equivalent("Mission", "Mister"));
        // Common prefix exists but trailing diff is too large.
        assert!(!stem_equivalent("Allied", "Alliance"));
        // Unrelated words.
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
    fn merge_slots_unions_ships_and_sums_concurrent() {
        let slot_a = SlotLine {
            encounter_label: "Scouts".into(),
            phase_label: "Wave 1".into(),
            concurrent: 2,
            body: BodyKind::Ships(vec!["Cutter".into(), "Avenger".into()]),
            tags: Vec::new(),
            skills: vec![50],
            ace: false,
            role_hint: None,
            source_slot_count: 1,
            all_singleton_sources: false,
            count: 1,
        };
        let slot_b = SlotLine {
            concurrent: 3,
            skills: vec![40],
            body: BodyKind::Ships(vec!["Cutter".into(), "Sabre".into()]),
            ..slot_a.clone()
        };
        let merged = merge_slots(vec![slot_a, slot_b]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].concurrent, 5);
        assert_eq!(merged[0].skills, vec![50, 40]);
        match &merged[0].body {
            BodyKind::Ships(s) => assert_eq!(s, &["Cutter", "Avenger", "Sabre"]),
            _ => panic!("expected Ships"),
        }
        assert_eq!(merged[0].source_slot_count, 2);
        assert!(!merged[0].all_singleton_sources);
    }

    #[test]
    fn merge_slots_keeps_all_singleton_flag_for_one_of_pattern() {
        // Three slots, each with one ship and concurrent==1 — the
        // shape that should render as "One of: ..."
        let make = |ship: &str| SlotLine {
            encounter_label: "Target".into(),
            phase_label: String::new(),
            concurrent: 1,
            body: BodyKind::Ships(vec![ship.into()]),
            tags: vec!["Bounty".into()],
            skills: vec![60],
            ace: false,
            role_hint: None,
            source_slot_count: 1,
            all_singleton_sources: true,
            count: 1,
        };
        let merged = merge_slots(vec![make("Freelancer MIS"), make("Cutlass Black"), make("RAFT")]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source_slot_count, 3);
        assert!(merged[0].all_singleton_sources);
        assert_eq!(merged[0].concurrent, 3);
    }

    #[test]
    fn skill_lead_suppresses_ace_at_skill_100() {
        let slot = SlotLine {
            encounter_label: "Ace Pilot".into(),
            phase_label: String::new(),
            concurrent: 1,
            body: BodyKind::Ships(vec!["F7C Hornet".into()]),
            tags: vec!["Bounty".into()],
            skills: vec![100],
            ace: true,
            role_hint: None,
            source_slot_count: 1,
            all_singleton_sources: false,
            count: 1,
        };
        let lead = render_skill_lead(&slot);
        assert!(
            !lead.contains("Ace"),
            "expected Ace suppressed at skill 100, got {lead}"
        );
        assert!(lead.contains("Skill 100"));
        // Trailing must not carry the skill — that's what moved to the lead.
        let trailing = render_trailing(&slot);
        assert!(!trailing.contains("Skill"));
        assert!(!trailing.contains("Ace"));
    }

    #[test]
    fn skill_lead_keeps_ace_at_lower_skill() {
        let slot = SlotLine {
            encounter_label: "x".into(),
            phase_label: String::new(),
            concurrent: 1,
            body: BodyKind::Ships(vec!["x".into()]),
            tags: Vec::new(),
            skills: vec![80],
            ace: true,
            role_hint: None,
            source_slot_count: 1,
            all_singleton_sources: false,
            count: 1,
        };
        let lead = render_skill_lead(&slot);
        assert!(lead.contains("Skill 80"));
        assert!(lead.contains("Ace"));
    }

    #[test]
    fn aggregate_tag_summary_categorises_buckets() {
        let make = |tags: Vec<&str>| SlotLine {
            encounter_label: "x".into(),
            phase_label: String::new(),
            concurrent: 1,
            body: BodyKind::Ships(vec!["x".into()]),
            tags: tags.into_iter().map(String::from).collect(),
            skills: Vec::new(),
            ace: false,
            role_hint: None,
            source_slot_count: 1,
            all_singleton_sources: false,
            count: 1,
        };
        let slots = vec![
            make(vec!["Scraps Cargo", "LowValue", "General", "Bounty"]),
            make(vec!["Half Cargo", "Mixed", "Legal"]),
        ];
        let summary = aggregate_tag_summary(&slots).expect("non-empty");
        // General is dropped.
        assert!(!summary.contains("General"));
        // All categories present and labelled.
        assert!(summary.contains("Cargo: Scraps Cargo, Half Cargo"));
        assert!(summary.contains("Value: LowValue, Mixed"));
        assert!(summary.contains("Tags: Bounty, Legal"));
    }

    #[test]
    fn friendly_label_classifies_typical_cases() {
        // Friendly markers in the encounter label.
        assert!(is_friendly_label("Allied"));
        assert!(is_friendly_label("Allied Reinforcements"));
        assert!(is_friendly_label("Escort Ship"));
        assert!(is_friendly_label("Friendly NPCs"));
        assert!(is_friendly_label("Attacked"));
        // Non-friendly defaults.
        assert!(!is_friendly_label("Mission Targets"));
        assert!(!is_friendly_label("Initial Enemies"));
        assert!(!is_friendly_label("Hostile"));
        assert!(!is_friendly_label("Enemy Ships"));
        // "Defend Location" alone shouldn't classify as friendly —
        // it's the wrapper for an enemy fight at a location.
        assert!(!is_friendly_label("Defend Location"));
    }

    #[test]
    fn phase_strip_unblocks_supersede_for_wrapper_phase() {
        // The bug we're fixing — phase carrying the wrapper text used
        // to leak back when it superseded the encounter. Now the
        // wrapper strips on phase too, so the supersede surfaces a
        // clean "Enemy Ships" label.
        let raw_encounter = clean_encounter_label("EnemyShips");
        let raw_phase = clean_phase_label(
            "DefendLocationWrapperEnemyShips",
            &raw_encounter,
        );
        // Phase folds to encounter (now equal) and gets dropped.
        assert_eq!(raw_phase, "");
        let (label_e, label_p) = resolve_labels(&raw_encounter, &raw_phase);
        assert_eq!(label_e, "Enemy Ships");
        assert_eq!(label_p, "");
    }

    #[test]
    fn aggregate_tag_summary_returns_none_when_only_noise() {
        let slots = vec![SlotLine {
            encounter_label: "x".into(),
            phase_label: String::new(),
            concurrent: 1,
            body: BodyKind::Ships(vec!["x".into()]),
            tags: vec!["General".into()],
            skills: Vec::new(),
            ace: false,
            role_hint: None,
            source_slot_count: 1,
            all_singleton_sources: false,
            count: 1,
        }];
        assert!(aggregate_tag_summary(&slots).is_none());
    }
}
