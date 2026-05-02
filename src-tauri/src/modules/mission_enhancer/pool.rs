//! Aggregated facts about a set of missions sharing one INI key
//! (title or description). Built once per pool, fed to both the title
//! and description renderers.
//!
//! All "is this consistent across the pool?" questions land here so
//! the renderers stay declarative.

use std::collections::HashSet;

use sc_contracts::{LocalityRegistry, Mission, MissionIndex};
use sc_extract::Guid;
use svarog_datacore::DataCoreDatabase;

use super::crimestat::{self, CrimestatRisk};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriState {
    Unanimous(bool),
    Mixed,
}

impl TriState {
    fn collect<I: Iterator<Item = bool>>(iter: I) -> Self {
        let mut value: Option<bool> = None;
        for v in iter {
            match value {
                None => value = Some(v),
                Some(prev) if prev != v => return TriState::Mixed,
                _ => {}
            }
        }
        match value {
            Some(v) => TriState::Unanimous(v),
            None => TriState::Unanimous(false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlueprintState {
    /// No member has a blueprint reward.
    None,
    /// All members carry a blueprint reward, all pointing at the same pool.
    AllSamePool,
    /// All members carry a blueprint reward, but pool guids differ.
    AllDifferentPools,
    /// Some members carry a blueprint reward and others don't.
    MixedPresence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrimestatState {
    Unanimous(CrimestatRisk),
    Mixed,
}

#[derive(Debug, Clone)]
pub struct PoolFacts<'a> {
    /// Pool members in the order they appeared in the index.
    pub members: Vec<&'a Mission>,
    pub blueprint_state: BlueprintState,
    pub shareable: TriState,
    pub once_only: TriState,
    pub illegal: TriState,
    pub crimestat: CrimestatState,
    /// All-members-agree axes (pulled from MissionIndex divergence
    /// helpers). When `false`, the axis differs across members and
    /// belongs in the variants section.
    pub uec_consistent: bool,
    pub scrip_consistent: bool,
    pub rep_consistent: bool,
    pub cooldowns_consistent: bool,
    pub encounters_consistent: bool,
    pub mission_span_consistent: bool,
    /// Distinct non-empty region labels across pool members.
    pub region_labels: Vec<String>,
}

impl<'a> PoolFacts<'a> {
    /// Build from an explicit member id list. Members that don't
    /// resolve in the index are silently skipped — the pools come
    /// from the same index, so this should not normally trigger.
    pub fn build(
        index: &'a MissionIndex,
        ids: &'a [Guid],
        db: &DataCoreDatabase,
    ) -> Self {
        let members: Vec<&'a Mission> = index.iter_pool(ids).collect();

        let blueprint_state = classify_blueprints(&members);
        let crimestat = classify_crimestat(&members, db);

        let shareable = TriState::collect(members.iter().map(|m| m.shareable));
        let once_only = TriState::collect(members.iter().map(|m| m.availability.once_only));
        let illegal = TriState::collect(members.iter().map(|m| m.illegal_flag));

        let uec_consistent = index.rewards_uec_consistent(ids);
        let scrip_consistent = index.rewards_scrip_consistent(ids);
        let rep_consistent = index.rewards_rep_consistent(ids);
        let cooldowns_consistent = index.cooldowns_consistent(ids);
        let encounters_consistent = index.encounters_shape_consistent(ids);
        let mission_span_consistent = index.mission_span_consistent(ids);

        let region_labels = collect_region_labels(&members, &index.localities);

        PoolFacts {
            members,
            blueprint_state,
            shareable,
            once_only,
            illegal,
            crimestat,
            uec_consistent,
            scrip_consistent,
            rep_consistent,
            cooldowns_consistent,
            encounters_consistent,
            mission_span_consistent,
            region_labels,
        }
    }

    /// True if any pool member differs from the others on any axis the
    /// description renderer breaks out into a variants section.
    pub fn has_variants(&self) -> bool {
        self.members.len() > 1
            && (matches!(
                self.blueprint_state,
                BlueprintState::AllDifferentPools | BlueprintState::MixedPresence
            ) || !self.uec_consistent
                || !self.scrip_consistent
                || !self.rep_consistent
                || !self.cooldowns_consistent
                || !self.encounters_consistent
                || !self.mission_span_consistent
                || matches!(self.shareable, TriState::Mixed)
                || matches!(self.once_only, TriState::Mixed)
                || matches!(self.illegal, TriState::Mixed))
    }

    /// True when at least one mixed axis exists outside blueprints —
    /// drives the title's `[~]` ambiguity marker.
    pub fn has_non_blueprint_mixing(&self) -> bool {
        matches!(self.shareable, TriState::Mixed)
            || matches!(self.once_only, TriState::Mixed)
            || matches!(self.illegal, TriState::Mixed)
    }
}

fn classify_blueprints(members: &[&Mission]) -> BlueprintState {
    if members.is_empty() {
        return BlueprintState::None;
    }
    let mut with: Vec<Guid> = Vec::new();
    let mut without = 0usize;
    for m in members {
        match &m.rewards.blueprint {
            Some(bp) => with.push(bp.pool_guid),
            None => without += 1,
        }
    }
    if with.is_empty() {
        return BlueprintState::None;
    }
    if without > 0 {
        return BlueprintState::MixedPresence;
    }
    let unique: HashSet<&Guid> = with.iter().collect();
    if unique.len() == 1 {
        BlueprintState::AllSamePool
    } else {
        BlueprintState::AllDifferentPools
    }
}

fn classify_crimestat(members: &[&Mission], db: &DataCoreDatabase) -> CrimestatState {
    let mut current: Option<CrimestatRisk> = None;
    for m in members {
        let risk = crimestat::classify(db, m);
        match current {
            None => current = Some(risk),
            Some(prev) if prev != risk => return CrimestatState::Mixed,
            _ => {}
        }
    }
    CrimestatState::Unanimous(current.unwrap_or(CrimestatRisk::None))
}

fn collect_region_labels(members: &[&Mission], localities: &LocalityRegistry) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in members {
        let label = region_label_for(m, localities);
        if !label.is_empty() && !out.contains(&label) {
            out.push(label);
        }
    }
    out
}

/// One mission's combined region label.
///
/// Walks every locality the mission references, parses each
/// `region_label` (`"Pyro: Bloom"`, `"Pyro: Bloom, Rat's Nest"`,
/// `"Pyro (system-wide)"`, `"Stanton + Pyro"`), and merges the parts
/// so bodies in the same system collapse into one entry. See
/// [`merge_region_labels`] for the merge rules — same logic is also
/// reused by the cross-mission group label combiner.
pub fn region_label_for(mission: &Mission, localities: &LocalityRegistry) -> String {
    let mut sources: Vec<&str> = Vec::new();
    for guid in &mission.mission_span {
        let Some(view) = localities.get(guid) else {
            continue;
        };
        if !view.region_label.is_empty() {
            sources.push(view.region_label.as_str());
        }
    }
    // Single-line context (used as variant labels), so join entries
    // with `" / "` rather than newlines.
    merge_region_labels(&sources).join(" / ")
}

/// Merge a slice of region labels into one entry per star system,
/// collapsing duplicate system prefixes.
///
/// Each input is parsed via [`parse_region_label`]:
/// - `Bodies(sys, [bodies])` accumulate per-system, dedup'd, in
///   first-seen order.
/// - `SystemWide(sys)` emit once per system, only when no bodies
///   already cover that system (otherwise redundant).
/// - `CrossSystem(s)` and `Verbatim(s)` pass through untouched.
///
/// Returns one string per system entry — callers pick the joiner.
/// Multi-system blocks like "Available at" join with newlines (one
/// system per row); single-line contexts like variant labels join
/// with `" / "`.
///
/// Used both within one mission (where multiple `LocalityView`s often
/// resolve to the same system with different bodies) and across the
/// missions of a variant group. Pure function over strings — no DCB
/// access.
pub fn merge_region_labels(sources: &[&str]) -> Vec<String> {
    use std::collections::BTreeMap;

    let mut by_system: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut systemwide: Vec<String> = Vec::new();
    let mut passthrough: Vec<String> = Vec::new();

    for raw in sources {
        if raw.is_empty() {
            continue;
        }
        match parse_region_label(raw) {
            ParsedRegion::Bodies(sys, bodies) => {
                let entry = by_system.entry(sys).or_default();
                for body in bodies {
                    if !entry.iter().any(|b| b == &body) {
                        entry.push(body);
                    }
                }
            }
            ParsedRegion::SystemWide(sys) => {
                if !systemwide.contains(&sys) {
                    systemwide.push(sys);
                }
            }
            ParsedRegion::CrossSystem(s) | ParsedRegion::Verbatim(s) => {
                if !passthrough.contains(&s) {
                    passthrough.push(s);
                }
            }
        }
    }

    let mut parts: Vec<String> = Vec::new();
    for (sys, bodies) in &by_system {
        parts.push(format!("{sys}: {}", bodies.join(", ")));
    }
    for sys in &systemwide {
        if !by_system.contains_key(sys) {
            parts.push(format!("{sys} (system-wide)"));
        }
    }
    parts.extend(passthrough);
    parts
}

/// Parsed shape of a single `LocalityView.region_label`.
enum ParsedRegion {
    /// `"System: Body1, Body2"` — bodies after the colon.
    /// `"+N more"` suffixes from the upstream cap are stripped on
    /// parse; the merge step recomputes the visible body list from
    /// scratch anyway.
    Bodies(String, Vec<String>),
    /// `"System (system-wide)"` — locality covers the system but
    /// has no specific body ancestor.
    SystemWide(String),
    /// `"Stanton + Pyro"` — locality crosses multiple systems.
    /// Surfaced as one opaque entry; we don't try to fuse it with
    /// per-system entries.
    CrossSystem(String),
    /// Anything we can't pattern-match. Carried through untouched
    /// so the player still sees something rather than dropping
    /// the locality silently.
    Verbatim(String),
}

fn parse_region_label(s: &str) -> ParsedRegion {
    // System-wide: trailing " (system-wide)".
    if let Some(sys) = s.strip_suffix(" (system-wide)") {
        return ParsedRegion::SystemWide(sys.to_string());
    }
    // Cross-system: "A + B" — system tokens joined by " + ".
    // Distinguished from body lists by the absence of a colon.
    if !s.contains(':') && s.contains(" + ") {
        return ParsedRegion::CrossSystem(s.to_string());
    }
    // "System: bodies" — split on the FIRST ": ".
    if let Some((sys, bodies_raw)) = s.split_once(": ") {
        let bodies: Vec<String> = bodies_raw
            .split(", ")
            .map(|p| p.trim())
            .filter(|p| !p.is_empty() && !p.starts_with('+')) // drop "+N more"
            .map(String::from)
            .collect();
        if !bodies.is_empty() {
            return ParsedRegion::Bodies(sys.to_string(), bodies);
        }
    }
    ParsedRegion::Verbatim(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> ParsedRegion {
        parse_region_label(s)
    }

    #[test]
    fn parse_classifies_all_known_shapes() {
        assert!(matches!(parse("Pyro: Bloom"), ParsedRegion::Bodies(s, b) if s == "Pyro" && b == ["Bloom"]));
        assert!(matches!(
            parse("Pyro: Bloom, Rat's Nest"),
            ParsedRegion::Bodies(s, b) if s == "Pyro" && b == ["Bloom", "Rat's Nest"]
        ));
        assert!(matches!(
            parse("Pyro (system-wide)"),
            ParsedRegion::SystemWide(s) if s == "Pyro"
        ));
        assert!(matches!(
            parse("Stanton + Pyro"),
            ParsedRegion::CrossSystem(s) if s == "Stanton + Pyro"
        ));
    }

    #[test]
    fn parse_strips_plus_n_more_suffix() {
        let r = parse("Pyro: A, B, C, D, E, +3 more");
        match r {
            ParsedRegion::Bodies(_, b) => assert_eq!(b, vec!["A", "B", "C", "D", "E"]),
            _ => panic!("expected Bodies"),
        }
    }

    #[test]
    fn parse_unknown_falls_to_verbatim() {
        assert!(matches!(
            parse("Some weird thing"),
            ParsedRegion::Verbatim(s) if s == "Some weird thing"
        ));
    }

    #[test]
    fn merger_collapses_repeated_system_prefix_across_inputs() {
        // The exact case the user reported: 4 separate mission labels,
        // each a Stanton locality. Merging should fold them into one
        // system entry.
        let inputs = [
            "Stanton: Hurston",
            "Stanton: Hurston, Crusader",
            "Stanton: Hurston, ArcCorp",
            "Stanton: Hurston, microTech",
        ];
        assert_eq!(
            merge_region_labels(&inputs),
            vec!["Stanton: Hurston, Crusader, ArcCorp, microTech"]
        );
    }

    #[test]
    fn merger_keeps_different_systems_separate() {
        let inputs = ["Stanton: Hurston", "Pyro: Bloom"];
        // BTreeMap-sorted system ordering.
        assert_eq!(
            merge_region_labels(&inputs),
            vec!["Pyro: Bloom", "Stanton: Hurston"]
        );
    }

    #[test]
    fn merger_drops_systemwide_when_bodies_present() {
        let inputs = ["Pyro: Bloom", "Pyro (system-wide)"];
        assert_eq!(merge_region_labels(&inputs), vec!["Pyro: Bloom"]);
    }

    #[test]
    fn merger_keeps_systemwide_when_no_bodies() {
        // System-wide entries preserve input order (no auto-sort).
        let inputs = ["Pyro (system-wide)", "Nyx (system-wide)"];
        assert_eq!(
            merge_region_labels(&inputs),
            vec!["Pyro (system-wide)", "Nyx (system-wide)"]
        );
    }

    #[test]
    fn merger_handles_empty_inputs() {
        assert!(merge_region_labels(&[]).is_empty());
        assert!(merge_region_labels(&[""]).is_empty());
    }
}
