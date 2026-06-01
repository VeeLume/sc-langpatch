//! Owned-blueprints integration — reads Hearth's export and answers the
//! ownership questions the title/description renderers need.
//!
//! Hearth (the blueprint tracker) writes `owned-blueprints.json` via its
//! `hearth-export` contract; we read it here. The two apps may track
//! different sc-holotable versions, so the contract crosses as **CIG hex
//! guid strings** — we match `BlueprintItem::blueprint_record_guid.to_string()`
//! against the set rather than sharing a `Guid` type. Missing/corrupt file →
//! empty set (the common no-Hearth case), so every owned feature is a safe
//! no-op without Hearth installed.

use std::collections::HashSet;

use sc_contracts::{Mission, MissionIndex};

/// Marker appended to owned blueprints (description bullets) and to the
/// owned-complete title tag. Single point of definition so swapping the
/// glyph (e.g. to `*` if the in-game font drops `✓`) is one edit.
pub const OWNED_MARK: &str = "✓";

/// How the description's "Potential Blueprints" list treats owned entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedMode {
    /// No ownership awareness — render as if Hearth weren't there.
    Off,
    /// Keep owned entries, flag them with [`OWNED_MARK`] + a header count.
    Mark,
    /// Omit owned entries, keep the header count.
    Hide,
}

impl OwnedMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "mark" => Self::Mark,
            "hide" => Self::Hide,
            _ => Self::Off,
        }
    }

    pub fn is_off(self) -> bool {
        matches!(self, Self::Off)
    }
}

/// Read Hearth's owned-blueprints export from its conventional path. Any
/// failure (no file, unreadable, malformed) yields an empty set — logged
/// once at info level (the patcher is a one-shot run). The returned strings
/// are CIG hex guids matching `BlueprintItem::blueprint_record_guid.to_string()`.
pub fn load_owned_set() -> HashSet<String> {
    let Some(path) = dirs::data_dir().map(|d| d.join(hearth_export::EXPORT_RELATIVE_PATH)) else {
        return HashSet::new();
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "  [MissionEnhancer] no Hearth owned-blueprints export at {} ({e}); owned features inert",
                path.display()
            );
            return HashSet::new();
        }
    };
    match serde_json::from_slice::<hearth_export::OwnedBlueprints>(&bytes) {
        Ok(doc) => {
            eprintln!(
                "  [MissionEnhancer] loaded {} owned blueprints from Hearth export",
                doc.owned.len()
            );
            doc.owned
        }
        Err(e) => {
            eprintln!("  [MissionEnhancer] Hearth owned-blueprints export unreadable: {e}");
            HashSet::new()
        }
    }
}

/// Distinct reward `blueprint_record_guid`s (hex form) across a set of pool
/// members — the union of every blueprint in every pool the members reward.
pub fn pool_reward_guids(members: &[&Mission], index: &MissionIndex) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for m in members {
        for bp in &m.rewards.blueprints {
            let Some(pool) = index.blueprints.get(&bp.pool_guid) else {
                continue;
            };
            for item in &pool.items {
                let g = item.blueprint_record_guid.to_string();
                if seen.insert(g.clone()) {
                    out.push(g);
                }
            }
        }
    }
    out
}

/// True when there is at least one reward blueprint and **every** one is
/// owned — the mission is "exhausted" (no new blueprint left to earn).
pub fn all_owned(guids: &[String], owned: &HashSet<String>) -> bool {
    !guids.is_empty() && guids.iter().all(|g| owned.contains(g))
}
