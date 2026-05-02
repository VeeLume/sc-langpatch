//! Title-tag rendering — emits the trailing `[BP] [Solo] [Uniq] [~]`
//! suffix appended to a title pool's INI value.
//!
//! Only unanimous facts produce explicit tags. When a non-blueprint
//! axis is mixed across the pool, a single `[~]` marker is appended
//! to flag "behavior varies — see description."

use super::crimestat::CrimestatRisk;
use super::pool::{BlueprintState, CrimestatState, PoolFacts, TriState};
use crate::formatter_helpers::{apply_color, bracket, Color};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleOptions {
    pub blueprint: bool,
    pub solo: bool,
    pub once: bool,
    pub illegal: bool,
    /// Crimestat tag mode — "off" / "simple" / "colored".
    pub crimestat: CrimestatTagMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrimestatTagMode {
    Off,
    Simple,
    Colored,
}

impl CrimestatTagMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "off" => CrimestatTagMode::Off,
            "simple" => CrimestatTagMode::Simple,
            _ => CrimestatTagMode::Colored,
        }
    }
}

/// Render the trailing tag string (without leading space). Empty
/// string when no tags apply.
pub fn render(facts: &PoolFacts<'_>, opts: TitleOptions) -> String {
    let mut tags: Vec<String> = Vec::new();

    if opts.blueprint {
        match facts.blueprint_state {
            BlueprintState::AllSamePool => tags.push(apply_color(Color::Highlight, bracket("BP"))),
            BlueprintState::AllDifferentPools => {
                tags.push(apply_color(Color::Highlight, bracket("BP*")))
            }
            BlueprintState::MixedPresence => {
                tags.push(apply_color(Color::Highlight, bracket("BP?")))
            }
            BlueprintState::None => {}
        }
    }

    if opts.solo
        && let TriState::Unanimous(false) = facts.shareable
    {
        tags.push(bracket("Solo"));
    }

    if opts.once
        && let TriState::Unanimous(true) = facts.once_only
    {
        tags.push(bracket("Uniq"));
    }

    if opts.illegal
        && let TriState::Unanimous(true) = facts.illegal
    {
        tags.push(bracket("Illegal"));
    }

    if !matches!(opts.crimestat, CrimestatTagMode::Off)
        && let CrimestatState::Unanimous(risk) = facts.crimestat
        && risk != CrimestatRisk::None
    {
        tags.push(crimestat_tag(risk, opts.crimestat));
    }

    if facts.has_non_blueprint_mixing() {
        tags.push(bracket("~"));
    }

    tags.join(" ")
}

fn crimestat_tag(risk: CrimestatRisk, mode: CrimestatTagMode) -> String {
    match (mode, risk) {
        (CrimestatTagMode::Colored, CrimestatRisk::High) => {
            apply_color(Color::Highlight, bracket("CS Risk"))
        }
        (CrimestatTagMode::Colored, CrimestatRisk::Moderate) => {
            apply_color(Color::Underline, bracket("CS Risk"))
        }
        _ => bracket("CS Risk"),
    }
}
