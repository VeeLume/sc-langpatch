//! Variant-label resolution for description-pool variants.
//!
//! Each pool member needs a label the player can recognize in-game.
//! Resolution priority:
//!
//! 1. **Region** — `mission_span` → `region_label` (most visible).
//! 2. **Mission rank** — appended from `PrereqView::Reputation.min_standing`
//!    when (a) there's no region or (b) two members collide on region.
//! 3. **Debug-name hints** — when neither resolved, scan the mission's
//!    internal `debug_name` for tokens that *also* show up as
//!    in-game mission tags: difficulty (`Easy`/`Medium`/`Hard` etc.),
//!    Pyro region letter (`RegionA`-`RegionD`), Stanton planet
//!    number (`Stanton1`-`Stanton4`). All are visible to the player
//!    in the mobiGlas mission listing.
//! 4. **Numeric** (`Variant 1`, …) — only when the above don't disambiguate.
//!
//! Diagnostic logging happens upstream in [`super::description::variants_block`]
//! after the dedup pass — so fallbacks for pools that ultimately
//! collapse to a single rendered group don't show up as noise.

use std::collections::HashMap;

use sc_contracts::{LocalityRegistry, Mission, PrereqView};
use sc_extract::Guid;
use svarog_datacore::DataCoreDatabase;

use super::pool::region_label_for;

/// One resolved variant — a pool member with the disambiguator the
/// description renderer should display next to its block.
pub struct VariantLabel<'a> {
    pub mission: &'a Mission,
    pub label: String,
    /// True if the label fell through to the numeric `Variant N`
    /// fallback (no region, no rank, no debug-name hint).
    pub used_numeric: bool,
}

/// Aggregate stats about how labels resolved across the pool. Lets
/// the description renderer log post-dedup outliers without recomputing.
#[derive(Debug, Default)]
pub struct ResolutionStats {
    /// How many distinct region values had ≥2 members.
    pub region_collisions: usize,
    /// Members whose label is just `Variant N`.
    pub numeric_fallbacks: usize,
    /// Debug names of the missions that fell to numeric.
    pub numeric_debug_names: Vec<String>,
    /// Number of times a rank tag was appended (collision resolved).
    pub rank_appended: usize,
}

/// Resolve labels for every member of the pool. Output preserves
/// input order; the second tuple element captures resolution stats
/// for post-dedup diagnostic logging.
pub fn resolve<'a>(
    members: &[&'a Mission],
    localities: &LocalityRegistry,
    db: &DataCoreDatabase,
) -> (Vec<VariantLabel<'a>>, ResolutionStats) {
    // Pass 1: region label for every member.
    let regions: Vec<String> = members
        .iter()
        .map(|m| region_label_for(m, localities))
        .collect();

    // Pass 2: identify collisions among non-empty regions.
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for r in &regions {
        if !r.is_empty() {
            *counts.entry(r.as_str()).or_default() += 1;
        }
    }

    let mut stats = ResolutionStats::default();
    for (region, count) in &counts {
        if *count > 1 {
            let _ = region;
            stats.region_collisions += 1;
        }
    }

    // Pass 3: assemble labels.
    let mut labels: Vec<VariantLabel<'a>> = Vec::with_capacity(members.len());
    let mut numeric_seq = 0usize;

    for (idx, m) in members.iter().enumerate() {
        let region = &regions[idx];
        let collide = !region.is_empty() && counts.get(region.as_str()).copied().unwrap_or(0) > 1;

        let mut label = String::new();
        let mut used_numeric = false;

        if !region.is_empty() {
            label.push_str(region);
            if collide {
                if let Some(rank) = mission_rank(m, db) {
                    label.push_str(" · ");
                    label.push_str(&rank);
                    stats.rank_appended += 1;
                }
            }
        } else if let Some(rank) = mission_rank(m, db) {
            label = rank;
        } else if let Some(hint) = parse_debug_name_hints(&m.debug_name) {
            label = hint;
        } else {
            numeric_seq += 1;
            label = format!("Variant {numeric_seq}");
            used_numeric = true;
            stats.numeric_fallbacks += 1;
            stats.numeric_debug_names.push(m.debug_name.clone());
        }

        labels.push(VariantLabel {
            mission: m,
            label,
            used_numeric,
        });
    }

    // Per-mission numeric dedup is intentionally NOT applied here.
    // It belongs after the post-resolution grouping pass (see
    // [`super::description::variants_block`]) — when many missions
    // collapse into one group with shared facts, suffixing their
    // already-shared label with `(N)` is noise. The renderer applies
    // numeric disambiguation only across actually-rendered groups
    // that collide on the final combined label.

    (labels, stats)
}

/// Pull the first usable mission-rank label from a mission's
/// reputation prereqs. Returns the resolved standing record name with
/// any common prefix stripped (`SReputationStandingDef.Mercenary` →
/// `Mercenary`). `None` if the mission has no reputation prereq with a
/// `min_standing` reference, or that reference can't be resolved.
fn mission_rank(mission: &Mission, db: &DataCoreDatabase) -> Option<String> {
    for prereq in &mission.prerequisites {
        if let PrereqView::Reputation { min_standing: Some(guid), .. } = prereq
            && let Some(name) = standing_name(db, guid)
        {
            return Some(name);
        }
    }
    None
}

/// Scan a mission's internal `debug_name` for tokens that also show
/// up as in-game mobiGlas tags: location (Stanton planet number,
/// Pyro region letter), and difficulty rank. Returns a joined label
/// (`"Pyro Region A · Hard"`, `"Stanton 4 · Medium"`) or [`None`] if
/// no recognized token is present.
///
/// Conservative on purpose — no fallback-to-substring matching, only
/// explicit token shapes the SC mission generator uses consistently.
/// Adding new token classes is a non-breaking addition.
fn parse_debug_name_hints(debug_name: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    // Tokenize on `_` since CIG's debug names use underscore as the
    // primary separator. Hyphens stay attached to their tokens
    // (e.g. `XSOutposts` keeps as one token).
    let tokens: Vec<&str> = debug_name.split('_').collect();

    // Location — Pyro region letter or Stanton planet number.
    // Detected as exact-match tokens for stability.
    let mut location: Option<String> = None;
    for (i, t) in tokens.iter().enumerate() {
        if let Some(letter) = t.strip_prefix("Region")
            && letter.len() == 1
            && letter.chars().all(|c| c.is_ascii_uppercase())
        {
            // Prefix with system if the previous token names one.
            let system = i.checked_sub(1).and_then(|j| tokens.get(j)).copied();
            location = Some(match system {
                Some("Pyro") => format!("Pyro Region {letter}"),
                Some("Stanton") => format!("Stanton Region {letter}"),
                _ => format!("Region {letter}"),
            });
            break;
        }
        if let Some(rest) = t.strip_prefix("Stanton")
            && !rest.is_empty()
            && rest.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            location = Some(format!("Stanton {rest}"));
            break;
        }
        if *t == "Pyro" && i + 1 < tokens.len() {
            // Pyro followed by something else (not RegionA — handled above).
            // Just `Pyro` alone tells the player which system; useful when
            // the name has no Stanton/Pyro region letter.
            location = Some("Pyro".to_string());
            // don't break — keep looking for a more specific Region* tag
        }
        if *t == "Nyx" {
            location = Some("Nyx".to_string());
        }
    }
    if let Some(l) = location {
        parts.push(l);
    }

    // Difficulty — both long forms (`VeryEasy`, `Hard`) and the
    // single-letter abbreviations (`_VE_`, `_E_`, `_M_`, `_H_`,
    // `_VH_`, `_S_`) used in some pool key suffixes.
    let difficulty = tokens.iter().find_map(|t| match *t {
        "VeryEasy" | "VE" => Some("Very Easy"),
        "Easy" | "E" => Some("Easy"),
        "Medium" | "M" => Some("Medium"),
        "Hard" | "H" => Some("Hard"),
        "VeryHard" | "VH" => Some("Very Hard"),
        "Super" | "S" => Some("Super"),
        _ => None,
    });
    if let Some(d) = difficulty {
        parts.push(d.to_string());
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn standing_name(db: &DataCoreDatabase, guid: &Guid) -> Option<String> {
    let record = db.record(guid)?;
    let name = record.name()?;
    // Strip the common type prefixes seen on reputation standing records.
    for prefix in [
        "SReputationStandingDef.",
        "ReputationStanding.",
        "FactionReputationStanding.",
    ] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return Some(rest.to_string());
        }
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pyro_region_letter() {
        assert_eq!(
            parse_debug_name_hints(
                "CFP_Pyro_RegionA_E_FaunaCave_JacksonsSwap_MissingPerson"
            ),
            Some("Pyro Region A · Easy".to_string())
        );
        assert_eq!(
            parse_debug_name_hints("HH_Pyro_RegionD_M_OccuCave_LastLandings_MissingPerson"),
            Some("Pyro Region D · Medium".to_string())
        );
    }

    #[test]
    fn parses_stanton_planet_number() {
        assert_eq!(
            parse_debug_name_hints("HighpointWildernessSpecialists_KillAnimals_Stanton4_Hard"),
            Some("Stanton 4 · Hard".to_string())
        );
        assert_eq!(
            parse_debug_name_hints("Vaughn_Stanton1_Assassination_VeryEasy"),
            Some("Stanton 1 · Very Easy".to_string())
        );
    }

    #[test]
    fn parses_difficulty_alone() {
        assert_eq!(
            parse_debug_name_hints("FoxwellEnforcement_Patrol_Stanton_Easy"),
            Some("Easy".to_string()),
            "bare `Stanton` (no digit) doesn't qualify as a planet, but `Easy` still parses"
        );
    }

    #[test]
    fn parses_nyx() {
        assert_eq!(
            parse_debug_name_hints("RedWind_Nyx_Medium_RecoverCargo"),
            Some("Nyx · Medium".to_string())
        );
    }

    #[test]
    fn returns_none_when_no_tokens_match() {
        assert_eq!(
            parse_debug_name_hints("RedWind_Pyro_BulkGrade_Solar_CFP_StationToRuin_Carbon_CargoHauling_Multi2ToSingle"),
            Some("Pyro".to_string()),
            "lone `Pyro` token surfaces even without a region letter"
        );
        assert_eq!(parse_debug_name_hints("SomeWeirdNameWithNoTokens"), None);
        assert_eq!(parse_debug_name_hints(""), None);
    }

    #[test]
    fn region_letter_takes_precedence_over_bare_pyro() {
        // `Pyro_RegionA` should beat the lone-Pyro fallback.
        let label = parse_debug_name_hints("HH_Pyro_RegionA_E_Rustville_XSOutposts_EliminateAll")
            .expect("matches");
        assert!(
            label.starts_with("Pyro Region A"),
            "expected Pyro Region A precedence, got {label}"
        );
    }
}
