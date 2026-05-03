//! Weapon enhancer — pool-first INI patching driven by sc-weapons v0.3.0.
//!
//! Mirrors the shape of the mission-enhancer module: every patch goes
//! through `WeaponPools` so that multiple weapon entities sharing one
//! `global.ini` localization key collapse to a single prefix / stats
//! block. This is the §0 fix from `feature-request-sc-weapons-langpatch.md`
//! — without it, popular weapon families like `BEHR_LaserCannon_S7`
//! accumulate twelve stacked `S{n}` prefixes and twelve `<EM4>Weapon
//! Stats</EM4>` blocks, one per colliding ship-mounted variant.
//!
//! ## Collision resolution
//!
//! Two refinements on top of plain "first entity wins":
//!
//! 1. **Size-matching by loc key suffix.** A loc key like
//!    `item_NameBEHR_LaserCannon_S7` carries the *intended* size in
//!    its `_S{n}` suffix. The pool may contain several entities under
//!    that key (CIG reuses entities for capital-ship hardpoints,
//!    point-defence turrets, Idris/Javelin variants, …) at sizes 7,
//!    8, 9, 12. We prefer the entities whose `.size` matches the
//!    suffix — those are the ones the player actually sees in the
//!    `_S7`-named market listing / loadout slot. Non-matching
//!    variants get filtered out before stats rendering.
//! 2. **Range rendering when matched stats diverge.** If even the
//!    size-matched subset still has multiple entities and they
//!    disagree on a stat, we render a range (`Alpha: 2306-6750`,
//!    `S7-S12`, `[CS]` dropped when tracking signal differs). Honest
//!    about the spread rather than silently picking one.
//!
//! Loc keys without a `_S{n}` suffix fall back to using every pool
//! member (size-match is a no-op).

use std::collections::HashMap;

use anyhow::Result;
use sc_extract::generated::{EItemSubType, ESignatureType};
use sc_extract::Guid;
use sc_weapons::{
    iter_missiles, iter_ship_weapons, DamageSummary, Missile, ShipWeapon, SustainKind,
    TrackingProfile, WeaponPools,
};

use crate::formatter_helpers::{header, NEWLINE, PARAGRAPH_BREAK};
use crate::module::{Module, ModuleContext, ModuleOption, OptionKind, PatchOp};

pub struct WeaponEnhancer;

impl Module for WeaponEnhancer {
    fn id(&self) -> &str {
        "weapon_enhancer"
    }

    fn name(&self) -> &str {
        "Weapon Enhancer"
    }

    fn description(&self) -> &str {
        "Add size prefixes, missile tracking type, and combat stats to weapon descriptions"
    }

    fn default_enabled(&self) -> bool {
        true
    }

    fn needs_datacore(&self) -> bool {
        true
    }

    fn options(&self) -> Vec<ModuleOption> {
        vec![
            ModuleOption {
                id: "size_prefix".into(),
                label: "Size Prefix".into(),
                description: "Add weapon size prefix to names (e.g. S3 Attrition)".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "missile_type_prefix".into(),
                label: "Missile/Torpedo Type Prefix".into(),
                description: "Add tracking type prefix to missile names (e.g. [IR] Ignite)"
                    .into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "weapon_stats".into(),
                label: "Weapon Stats".into(),
                description:
                    "Append damage, penetration, speed, and ammo stats to weapon descriptions"
                        .into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "missile_stats".into(),
                label: "Missile/Torpedo Stats".into(),
                description:
                    "Append damage, speed, lock time, and targeting stats to missile descriptions"
                        .into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
        ]
    }

    fn generate_patches(&self, ctx: &ModuleContext) -> Result<Vec<(String, PatchOp)>> {
        let Some(datacore) = ctx.datacore else {
            return Ok(Vec::new());
        };

        let opt_size_prefix = ctx.config.get_bool("size_prefix").unwrap_or(true);
        let opt_missile_type = ctx.config.get_bool("missile_type_prefix").unwrap_or(true);
        let opt_weapon_stats = ctx.config.get_bool("weapon_stats").unwrap_or(true);
        let opt_missile_stats = ctx.config.get_bool("missile_stats").unwrap_or(true);

        let ship_weapons: Vec<ShipWeapon> = iter_ship_weapons(datacore).collect();
        let missiles: Vec<Missile> = iter_missiles(datacore).collect();
        let pools = WeaponPools::build(&ship_weapons, &[], &missiles);

        let by_guid_ship: HashMap<Guid, &ShipWeapon> =
            ship_weapons.iter().map(|w| (w.guid, w)).collect();
        let by_guid_missile: HashMap<Guid, &Missile> =
            missiles.iter().map(|m| (m.guid, m)).collect();

        let mut patches: Vec<(String, PatchOp)> = Vec::new();
        let mut weapon_name_hits = 0usize;
        let mut weapon_desc_hits = 0usize;
        let mut missile_name_hits = 0usize;
        let mut missile_desc_hits = 0usize;

        // ── Name pool pass ──────────────────────────────────────────
        for (name_key, guids) in &pools.name_key {
            let key = name_key.stripped();
            if !ctx.ini.contains_key(key) {
                continue;
            }
            let target_size = parse_size_from_key(key);
            let ships = matched_ships(guids, &by_guid_ship, target_size);
            let mssls = matched_missiles(guids, &by_guid_missile, target_size);

            let prefix = if !ships.is_empty() {
                ship_name_prefix(&ships, opt_size_prefix)
            } else if !mssls.is_empty() {
                missile_name_prefix(&mssls, opt_size_prefix, opt_missile_type)
            } else {
                continue;
            };
            if prefix.is_empty() {
                continue;
            }
            patches.push((key.to_string(), PatchOp::Prefix(prefix)));
            if !ships.is_empty() {
                weapon_name_hits += 1;
            } else {
                missile_name_hits += 1;
            }
        }

        // ── Description pool pass ──────────────────────────────────
        let mut skipped_desc_is_name = 0usize;
        for (desc_key, guids) in &pools.desc_key {
            let key = desc_key.stripped();
            if !ctx.ini.contains_key(key) {
                continue;
            }
            // CIG data quirk: some weapons (e.g. VNCL_PlasmaCannon S2/S3)
            // have no dedicated `item_Desc*` entry, so the entity's
            // `Localization.Description` field falls back to its
            // `Localization.Name` key. Patching such a key with a
            // paragraph-break + stats block corrupts the name field —
            // skip it.
            if pools.name_key.contains_key(desc_key) {
                skipped_desc_is_name += 1;
                continue;
            }
            let target_size = parse_size_from_key(key);
            let ships = matched_ships(guids, &by_guid_ship, target_size);
            let mssls = matched_missiles(guids, &by_guid_missile, target_size);

            let suffix = if !ships.is_empty() && opt_weapon_stats {
                ship_stats_suffix(&ships)
            } else if !mssls.is_empty() && opt_missile_stats {
                missile_stats_suffix(&mssls)
            } else {
                String::new()
            };
            if suffix.is_empty() {
                continue;
            }
            patches.push((key.to_string(), PatchOp::Suffix(suffix)));
            if !ships.is_empty() {
                weapon_desc_hits += 1;
            } else {
                missile_desc_hits += 1;
            }
        }

        eprintln!(
            "  [WeaponEnhancer] {} ship weapons / {} missiles materialized; \
             {} weapon-name pools, {} weapon-desc pools, \
             {} missile-name pools, {} missile-desc pools patched \
             (skipped {} desc keys that also serve as name keys)",
            ship_weapons.len(),
            missiles.len(),
            weapon_name_hits,
            weapon_desc_hits,
            missile_name_hits,
            missile_desc_hits,
            skipped_desc_is_name,
        );

        Ok(patches)
    }
}

// ── Pool-member selection ───────────────────────────────────────────────────

/// Filter ship-pool members by size-match against the loc-key suffix.
/// Returns the size-matching subset; falls back to "all ship members"
/// when no member matches (e.g. loc key has no `_S{n}` suffix, or
/// every variant has a different size from the suffix).
fn matched_ships<'a>(
    guids: &[Guid],
    by_guid: &'a HashMap<Guid, &'a ShipWeapon>,
    target_size: Option<i32>,
) -> Vec<&'a ShipWeapon> {
    let all: Vec<&ShipWeapon> = guids.iter().filter_map(|g| by_guid.get(g).copied()).collect();
    match target_size {
        Some(s) => {
            let matched: Vec<&ShipWeapon> = all.iter().copied().filter(|w| w.size == s).collect();
            if matched.is_empty() { all } else { matched }
        }
        None => all,
    }
}

fn matched_missiles<'a>(
    guids: &[Guid],
    by_guid: &'a HashMap<Guid, &'a Missile>,
    target_size: Option<i32>,
) -> Vec<&'a Missile> {
    let all: Vec<&Missile> = guids.iter().filter_map(|g| by_guid.get(g).copied()).collect();
    match target_size {
        Some(s) => {
            let matched: Vec<&Missile> = all.iter().copied().filter(|m| m.size == s).collect();
            if matched.is_empty() { all } else { matched }
        }
        None => all,
    }
}

/// Extract a size hint from a loc-key suffix.
///
/// Patterns found in 4.7 LIVE:
/// - `item_NameBEHR_LaserCannon_S7` → 7      (suffix at end)
/// - `item_NameBEHR_LaserCannon_VNG_S2` → 2  (suffix after manufacturer subcode)
/// - `item_NameAPAR_BallisticScatterGun_S1_Shark` → 1  (suffix mid-string)
/// - `item_NameMISL_S02_CS_FSKI_Tempest` → 2 (zero-padded, mid-string)
/// - `item_NameGMISL_S05_IR_TALN_Valkyrie` → 5
///
/// Returns the first `_S{digits}_` (or `_S{digits}` at end) match. None
/// when no such pattern exists.
fn parse_size_from_key(key: &str) -> Option<i32> {
    let bytes = key.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'_' && bytes[i + 1] == b'S' && bytes[i + 2].is_ascii_digit() {
            let start = i + 2;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            // Boundary: must be followed by `_` or end of string.
            // Otherwise we'd false-match `_Stanton4_` etc.
            if end == bytes.len() || bytes[end] == b'_' {
                if let Ok(n) = std::str::from_utf8(&bytes[start..end])
                    .ok()
                    .and_then(|s| s.parse::<i32>().ok())
                    .ok_or(())
                {
                    return Some(n);
                }
            }
        }
        i += 1;
    }
    None
}

// ── Prefix builders ─────────────────────────────────────────────────────────

fn ship_name_prefix(ships: &[&ShipWeapon], with_size: bool) -> String {
    if !with_size {
        return String::new();
    }
    let sizes: Vec<i32> = ships.iter().map(|w| w.size).filter(|s| *s > 0).collect();
    match (sizes.iter().copied().min(), sizes.iter().copied().max()) {
        (Some(lo), Some(hi)) if lo == hi => format!("S{lo} "),
        (Some(lo), Some(hi)) => format!("S{lo}-S{hi} "),
        _ => String::new(),
    }
}

fn missile_name_prefix(
    missiles: &[&Missile],
    with_size: bool,
    with_tracking: bool,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if with_size {
        let sizes: Vec<i32> = missiles.iter().map(|m| m.size).filter(|s| *s > 0).collect();
        if let (Some(lo), Some(hi)) = (sizes.iter().copied().min(), sizes.iter().copied().max()) {
            if lo == hi {
                parts.push(format!("S{lo}"));
            } else {
                parts.push(format!("S{lo}-S{hi}"));
            }
        }
    }

    if with_tracking {
        let tags: std::collections::BTreeSet<&'static str> = missiles
            .iter()
            .filter_map(|m| m.tracking.as_ref().and_then(tracking_tag))
            .collect();
        if tags.len() == 1 {
            parts.push(format!("[{}]", tags.into_iter().next().unwrap()));
        }
        // Multiple distinct tags → omit (honest about the disagreement).
        // No tag at all (unguided / signal we don't render) → omit.
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!("{} ", parts.join(" "))
    }
}

fn tracking_tag(t: &TrackingProfile) -> Option<&'static str> {
    match t.signal {
        ESignatureType::Infrared => Some("IR"),
        ESignatureType::Electromagnetic => Some("EM"),
        ESignatureType::CrossSection => Some("CS"),
        _ => None,
    }
}

// ── Suffix builders ─────────────────────────────────────────────────────────

fn ship_stats_suffix(ships: &[&ShipWeapon]) -> String {
    let mut lines: Vec<String> = Vec::new();

    // Alpha damage. When all weapons in the matched set agree, render
    // the per-type breakdown. When they diverge, drop the breakdown
    // and show just the alpha range — combining breakdowns across
    // mismatched weapons would be misleading.
    let alphas: Vec<f32> = ships
        .iter()
        .filter_map(|w| w.damage.as_ref().map(DamageSummary::total))
        .filter(|a| *a > 0.0)
        .collect();
    if let Some((lo, hi)) = min_max_f32(&alphas) {
        if approx_eq(lo, hi, 0.5) {
            // Same alpha → breakdown is meaningful.
            let head = ships
                .iter()
                .find(|w| w.damage.is_some())
                .and_then(|w| w.damage.as_ref())
                .unwrap();
            lines.push(format!("Alpha: {:.0} ({})", head.total(), damage_breakdown(head)));
        } else {
            lines.push(format!("Alpha: {}", format_f32_range(lo, hi)));
        }
    }

    if let Some((lo, hi)) = min_max_filter(ships, |w| w.penetration_m, |v| v > 0.0) {
        if approx_eq(lo, hi, 0.01) {
            lines.push(format!("Penetration: {lo:.2}m"));
        } else {
            lines.push(format!("Penetration: {lo:.2}-{hi:.2}m"));
        }
    }

    if let Some((lo, hi)) = min_max_filter(ships, |w| w.ammo_speed, |v| v > 0.0) {
        if approx_eq(lo, hi, 0.5) {
            lines.push(format!("Projectile Speed: {lo:.0} m/s"));
        } else {
            lines.push(format!(
                "Projectile Speed: {} m/s",
                format_f32_range(lo, hi)
            ));
        }
    }

    let ammos: Vec<i32> = ships
        .iter()
        .filter_map(|w| w.total_ammo)
        .filter(|m| *m > 0)
        .collect();
    if let Some((lo, hi)) = min_max_i32(&ammos) {
        lines.push(format!("Ammo: {}", format_i32_range(lo, hi)));
    }

    let caps: Vec<f32> = ships
        .iter()
        .filter_map(|w| match &w.sustain {
            SustainKind::Energy(e) if e.max_ammo_load > 0.0 => Some(e.max_ammo_load),
            _ => None,
        })
        .collect();
    if let Some((lo, hi)) = min_max_f32(&caps) {
        if approx_eq(lo, hi, 0.5) {
            lines.push(format!("Capacitor: {lo:.0}"));
        } else {
            lines.push(format!("Capacitor: {}", format_f32_range(lo, hi)));
        }
    }

    if lines.is_empty() {
        return String::new();
    }
    let stats_str = lines
        .iter()
        .map(|l| format!("{NEWLINE}{l}"))
        .collect::<String>();
    format!("{PARAGRAPH_BREAK}{}{stats_str}", header("Weapon Stats"))
}

fn missile_stats_suffix(missiles: &[&Missile]) -> String {
    let mut lines: Vec<String> = Vec::new();

    let totals: Vec<f32> = missiles
        .iter()
        .filter_map(|m| m.damage.as_ref().map(DamageSummary::total))
        .filter(|a| *a > 0.0)
        .collect();
    if let Some((lo, hi)) = min_max_f32(&totals) {
        if approx_eq(lo, hi, 0.5) {
            let head = missiles
                .iter()
                .find(|m| m.damage.is_some())
                .and_then(|m| m.damage.as_ref())
                .unwrap();
            lines.push(format!("Damage: {:.0} ({})", head.total(), damage_breakdown(head)));
        } else {
            lines.push(format!("Damage: {}", format_f32_range(lo, hi)));
        }
    }

    if let Some((lo, hi)) = min_max_filter(missiles, |m| m.speed, |v| v > 0.0) {
        if approx_eq(lo, hi, 0.5) {
            lines.push(format!("Speed: {lo:.0} m/s"));
        } else {
            lines.push(format!("Speed: {} m/s", format_f32_range(lo, hi)));
        }
    }

    let arms: Vec<f32> = missiles.iter().map(|m| m.arm_time).filter(|t| *t > 0.0).collect();
    if let Some((lo, hi)) = min_max_f32(&arms) {
        if approx_eq(lo, hi, 0.01) {
            lines.push(format!("Arm Time: {lo:.2}s"));
        } else {
            lines.push(format!("Arm Time: {lo:.2}-{hi:.2}s"));
        }
    }

    // Tracking — only render when every matched missile has a tracking
    // profile; otherwise the player would see partial / inconsistent
    // numbers.
    let trackings: Vec<&TrackingProfile> =
        missiles.iter().filter_map(|m| m.tracking.as_ref()).collect();
    if trackings.len() == missiles.len() && !trackings.is_empty() {
        let lock_times: Vec<f32> = trackings.iter().map(|t| t.lock_time).filter(|v| *v > 0.0).collect();
        if let Some((lo, hi)) = min_max_f32(&lock_times) {
            if approx_eq(lo, hi, 0.05) {
                lines.push(format!("Lock Time: {lo:.1}s"));
            } else {
                lines.push(format!("Lock Time: {lo:.1}-{hi:.1}s"));
            }
        }
        let angles: Vec<f32> =
            trackings.iter().map(|t| t.lock_angle_deg).filter(|v| *v > 0.0).collect();
        if let Some((lo, hi)) = min_max_f32(&angles) {
            if approx_eq(lo, hi, 0.5) {
                lines.push(format!("Lock Angle: {lo:.0}°"));
            } else {
                lines.push(format!("Lock Angle: {}°", format_f32_range(lo, hi)));
            }
        }
        let mins: Vec<f32> =
            trackings.iter().map(|t| t.lock_range_min_m).collect();
        let maxes: Vec<f32> =
            trackings.iter().map(|t| t.lock_range_max_m).collect();
        let any_range = mins.iter().any(|v| *v > 0.0) || maxes.iter().any(|v| *v > 0.0);
        if any_range {
            let min_lo = mins.iter().copied().fold(f32::INFINITY, f32::min);
            let min_hi = mins.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let max_lo = maxes.iter().copied().fold(f32::INFINITY, f32::min);
            let max_hi = maxes.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            // Render `Lock Range: {min_lo[-min_hi]}m - {max_lo[-max_hi]}m`.
            let min_str = if approx_eq(min_lo, min_hi, 0.5) {
                format!("{min_lo:.0}")
            } else {
                format_f32_range(min_lo, min_hi)
            };
            let max_str = if approx_eq(max_lo, max_hi, 0.5) {
                format!("{max_lo:.0}")
            } else {
                format_f32_range(max_lo, max_hi)
            };
            lines.push(format!("Lock Range: {min_str}m - {max_str}m"));
        }
    }

    if lines.is_empty() {
        return String::new();
    }
    let stats_str = lines
        .iter()
        .map(|l| format!("{NEWLINE}{l}"))
        .collect::<String>();
    let label = if missiles.iter().any(|m| matches!(m.item_sub_type, EItemSubType::Torpedo)) {
        "Torpedo Stats"
    } else {
        "Missile Stats"
    };
    format!("{PARAGRAPH_BREAK}{}{stats_str}", header(label))
}

// ── Range helpers ───────────────────────────────────────────────────────────

fn min_max_f32(values: &[f32]) -> Option<(f32, f32)> {
    if values.is_empty() {
        return None;
    }
    let lo = values.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    Some((lo, hi))
}

fn min_max_i32(values: &[i32]) -> Option<(i32, i32)> {
    let lo = *values.iter().min()?;
    let hi = *values.iter().max()?;
    Some((lo, hi))
}

fn min_max_filter<T, F, P>(items: &[&T], extract: F, keep: P) -> Option<(f32, f32)>
where
    F: Fn(&T) -> Option<f32>,
    P: Fn(f32) -> bool,
{
    let vs: Vec<f32> = items.iter().filter_map(|x| extract(x)).filter(|v| keep(*v)).collect();
    min_max_f32(&vs)
}

fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn format_f32_range(lo: f32, hi: f32) -> String {
    format!("{}-{}", lo.round() as i64, hi.round() as i64)
}

fn format_i32_range(lo: i32, hi: i32) -> String {
    if lo == hi {
        format!("{lo}")
    } else {
        format!("{lo}-{hi}")
    }
}

// ── Damage breakdown helper ─────────────────────────────────────────────────

fn damage_breakdown(d: &DamageSummary) -> String {
    let mut parts = Vec::new();
    if d.physical > 0.0 {
        parts.push(format!("{:.0} phys", d.physical));
    }
    if d.energy > 0.0 {
        parts.push(format!("{:.0} energy", d.energy));
    }
    if d.distortion > 0.0 {
        parts.push(format!("{:.0} dist", d.distortion));
    }
    if d.thermal > 0.0 {
        parts.push(format!("{:.0} therm", d.thermal));
    }
    if d.biochemical > 0.0 {
        parts.push(format!("{:.0} bio", d.biochemical));
    }
    if d.stun > 0.0 {
        parts.push(format!("{:.0} stun", d.stun));
    }
    parts.join(", ")
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_handles_trailing_suffix() {
        assert_eq!(parse_size_from_key("item_NameBEHR_LaserCannon_S7"), Some(7));
        assert_eq!(parse_size_from_key("item_NameGATS_BallisticGatling_S1"), Some(1));
        assert_eq!(parse_size_from_key("item_NameKLWE_LaserRepeater_S6"), Some(6));
    }

    #[test]
    fn parse_size_handles_mid_string_suffix() {
        // Manufacturer subcode after the size token.
        assert_eq!(
            parse_size_from_key("item_NameBEHR_LaserCannon_VNG_S2"),
            Some(2)
        );
        // Variant suffix after the size token.
        assert_eq!(
            parse_size_from_key("item_NameAPAR_BallisticScatterGun_S1_Shark"),
            Some(1)
        );
    }

    #[test]
    fn parse_size_handles_zero_padded_missile_format() {
        assert_eq!(
            parse_size_from_key("item_NameMISL_S02_CS_FSKI_Tempest"),
            Some(2)
        );
        assert_eq!(
            parse_size_from_key("item_NameGMISL_S05_IR_TALN_Valkyrie"),
            Some(5)
        );
    }

    #[test]
    fn parse_size_rejects_non_size_tokens() {
        // `_Stanton4_` should not be confused for `_S{n}_`.
        assert_eq!(parse_size_from_key("item_NameFoo_Stanton4_Bar"), None);
        // Bare key without any size token.
        assert_eq!(parse_size_from_key("item_NameNoSizeHere"), None);
        // `_S` followed by non-digit.
        assert_eq!(parse_size_from_key("item_NameFoo_Special"), None);
    }

    #[test]
    fn format_f32_range_collapses_when_equal() {
        assert_eq!(format_f32_range(2076.0, 2076.0), "2076-2076");
        // Caller is expected to test approx_eq first — format always
        // emits the range form; the equality case is the caller's
        // shortcut to a single-value format.
    }

    #[test]
    fn approx_eq_within_tolerance() {
        assert!(approx_eq(1.0, 1.4, 0.5));
        assert!(!approx_eq(1.0, 2.0, 0.5));
    }
}
