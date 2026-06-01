//! Title-tag rendering — emits the trailing `[BP] [Solo] [Uniq] [~]`
//! suffix appended to a title pool's INI value.
//!
//! Only unanimous facts produce explicit tags. When a non-blueprint
//! axis is mixed across the pool, a single `[~]` marker is appended
//! to flag "behavior varies — see description."

use super::crimestat::CrimestatRisk;
use super::owned::OWNED_MARK;
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
    /// Append a `[✓]` tag when every reward blueprint is already owned
    /// (per Hearth's export). The completeness itself is computed by the
    /// caller and passed to [`render`]; this only gates emission.
    pub owned: bool,
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
///
/// `owned_complete` is true when every reward blueprint of this title pool
/// is owned (computed by the caller, which has the owned set + pool index).
/// Gated by [`TitleOptions::owned`].
pub fn render(facts: &PoolFacts<'_>, opts: TitleOptions, owned_complete: bool) -> String {
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

    // Owned-complete (exhausted) marker — sits next to the BP tag so the
    // "grants BP / already have them all" pair reads together. Underlined
    // so it stands out from plain title text in the contracts panel.
    if opts.owned && owned_complete {
        tags.push(apply_color(Color::Underline, bracket(OWNED_MARK)));
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
