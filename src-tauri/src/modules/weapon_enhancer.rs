use std::collections::HashMap;

use anyhow::Result;
use sc_weapons::{DamageSummary, SustainKind};
use svarog_datacore::{DataCoreDatabase, Instance, Value};

use crate::module::{Module, ModuleContext, ModuleOption, OptionKind, PatchOp};

// ── Missile data (legacy raw-svarog extraction) ─────────────────────────────
//
// TODO(sc-holotable): sc-weapons v1 only covers ship guns and FPS weapons.
// Missiles / torpedoes need a feature request upstream before we can lift
// this extraction into the shared crate.

/// All data we extract per missile or torpedo.
#[derive(Debug)]
struct MissileData {
    name_key: String,
    desc_key: String,
    size: i32,
    sub_type: String,
    tracking_signal: String,
    damage_physical: f32,
    damage_energy: f32,
    damage_distortion: f32,
    damage_thermal: f32,
    damage_biochemical: f32,
    damage_stun: f32,
    speed: f32,
    arm_time: f32,
    lock_time: f32,
    lock_angle: f32,
    lock_range_min: f32,
    lock_range_max: f32,
}

// ── Module ──────────────────────────────────────────────────────────────────

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
        let (Some(datacore), Some(db)) = (ctx.datacore, ctx.db) else {
            return Ok(Vec::new());
        };

        let opt_size_prefix = ctx.config.get_bool("size_prefix").unwrap_or(true);
        let opt_missile_type = ctx.config.get_bool("missile_type_prefix").unwrap_or(true);
        let opt_weapon_stats = ctx.config.get_bool("weapon_stats").unwrap_or(true);
        let opt_missile_stats = ctx.config.get_bool("missile_stats").unwrap_or(true);

        let mut patches = Vec::new();
        let mut weapon_count = 0;

        // ── Ship weapons via sc-weapons ──────────────────────────────────
        for w in sc_weapons::iter_ship_weapons(datacore) {
            let Some(loc) = loc_keys_for(db, w.guid) else {
                continue;
            };
            if !ctx.ini.contains_key(&loc.name_key) {
                continue;
            }
            weapon_count += 1;

            // Name prefix: size
            if opt_size_prefix && w.size > 0 {
                patches.push((
                    loc.name_key.clone(),
                    PatchOp::Prefix(format!("S{} ", w.size)),
                ));
            }

            // Description suffix: weapon stats
            if opt_weapon_stats
                && !loc.desc_key.is_empty()
                && ctx.ini.contains_key(&loc.desc_key)
            {
                let mut lines = Vec::new();

                if let Some(dmg) = w.damage {
                    let alpha = dmg.total();
                    if alpha > 0.0 {
                        lines.push(format!(
                            "Alpha: {:.0} ({})",
                            alpha,
                            damage_breakdown(&dmg)
                        ));
                    }
                }

                // Penetration — not in sc-weapons v1. TODO(sc-holotable):
                // request ShipWeapon.penetration_m exposed on the model.
                if let Some(pen) = legacy_penetration(db, w.guid)
                    && pen > 0.0
                {
                    lines.push(format!("Penetration: {pen:.2}m"));
                }

                if let Some(speed) = w.ammo_speed
                    && speed > 0.0
                {
                    lines.push(format!("Projectile Speed: {speed:.0} m/s"));
                }

                // Ballistic weapons: physical round count.
                if let Some(mag) = w.total_ammo
                    && mag > 0
                {
                    lines.push(format!("Ammo: {mag}"));
                }
                // Energy weapons: capacitor size from the energy sustain model.
                if let SustainKind::Energy(ref e) = w.sustain
                    && e.max_ammo_load > 0.0
                {
                    lines.push(format!("Capacitor: {:.0}", e.max_ammo_load));
                }

                if !lines.is_empty() {
                    let stats_str = lines
                        .iter()
                        .map(|l| format!("\\n{l}"))
                        .collect::<String>();
                    let suffix = format!("\\n\\n<EM4>Weapon Stats</EM4>{stats_str}");
                    patches.push((loc.desc_key.clone(), PatchOp::Suffix(suffix)));
                }
            }
        }

        // ── Missile/torpedo patches (raw-svarog, sc-weapons doesn't cover) ─
        let missiles = extract_missiles(db, ctx.ini);
        for m in &missiles {
            if m.name_key.is_empty() || !ctx.ini.contains_key(&m.name_key) {
                continue;
            }

            let mut prefix_parts = Vec::new();
            if opt_size_prefix && m.size > 0 {
                prefix_parts.push(format!("S{}", m.size));
            }
            if opt_missile_type && !m.tracking_signal.is_empty() {
                let tag = match m.tracking_signal.as_str() {
                    "Infrared" => "IR",
                    "Electromagnetic" => "EM",
                    "CrossSection" => "CS",
                    other => other,
                };
                prefix_parts.push(format!("[{tag}]"));
            }
            if !prefix_parts.is_empty() {
                let prefix = format!("{} ", prefix_parts.join(" "));
                patches.push((m.name_key.clone(), PatchOp::Prefix(prefix)));
            }

            if opt_missile_stats && !m.desc_key.is_empty() && ctx.ini.contains_key(&m.desc_key) {
                let mut lines = Vec::new();

                let total_dmg = m.damage_physical
                    + m.damage_energy
                    + m.damage_distortion
                    + m.damage_thermal
                    + m.damage_biochemical
                    + m.damage_stun;
                if total_dmg > 0.0 {
                    let mut parts = Vec::new();
                    if m.damage_physical > 0.0 {
                        parts.push(format!("{:.0} phys", m.damage_physical));
                    }
                    if m.damage_energy > 0.0 {
                        parts.push(format!("{:.0} energy", m.damage_energy));
                    }
                    if m.damage_distortion > 0.0 {
                        parts.push(format!("{:.0} dist", m.damage_distortion));
                    }
                    if m.damage_thermal > 0.0 {
                        parts.push(format!("{:.0} therm", m.damage_thermal));
                    }
                    if m.damage_biochemical > 0.0 {
                        parts.push(format!("{:.0} bio", m.damage_biochemical));
                    }
                    if m.damage_stun > 0.0 {
                        parts.push(format!("{:.0} stun", m.damage_stun));
                    }
                    lines.push(format!("Damage: {:.0} ({})", total_dmg, parts.join(", ")));
                }

                if m.speed > 0.0 {
                    lines.push(format!("Speed: {:.0} m/s", m.speed));
                }
                if m.arm_time > 0.0 {
                    lines.push(format!("Arm Time: {:.2}s", m.arm_time));
                }
                if m.lock_time > 0.0 {
                    lines.push(format!("Lock Time: {:.1}s", m.lock_time));
                }
                if m.lock_angle > 0.0 {
                    lines.push(format!("Lock Angle: {:.0}°", m.lock_angle));
                }
                if m.lock_range_min > 0.0 || m.lock_range_max > 0.0 {
                    lines.push(format!(
                        "Lock Range: {:.0}m - {:.0}m",
                        m.lock_range_min, m.lock_range_max
                    ));
                }

                if !lines.is_empty() {
                    let stats_str = lines
                        .iter()
                        .map(|l| format!("\\n{l}"))
                        .collect::<String>();
                    let label = if m.sub_type == "Torpedo" {
                        "Torpedo Stats"
                    } else {
                        "Missile Stats"
                    };
                    let suffix = format!("\\n\\n<EM4>{label}</EM4>{stats_str}");
                    patches.push((m.desc_key.clone(), PatchOp::Suffix(suffix)));
                }
            }
        }

        eprintln!(
            "  [WeaponEnhancer] {weapon_count} weapons, {} missiles",
            missiles.len()
        );
        Ok(patches)
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

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

// ── Localization-key + penetration helpers (raw svarog) ─────────────────────
//
// TODO(sc-holotable): these walk the raw DCB because sc-weapons doesn't
// expose (a) the INI localization keys we need for patching and (b) the
// ammo penetration distance. File feature requests when landing this.

struct LocKeys {
    name_key: String,
    desc_key: String,
}

fn loc_keys_for(db: &DataCoreDatabase, guid: sc_extract::Guid) -> Option<LocKeys> {
    let record = db.record(&guid)?;
    let inst = record.as_instance();
    let components = inst.get_array("Components")?;
    for comp in components {
        let Some(comp_inst) = to_instance(db, &comp) else {
            continue;
        };
        if comp_inst.type_name() != Some("SAttachableComponentParams") {
            continue;
        }
        let attach_def = comp_inst.get_instance("AttachDef")?;
        let loc = attach_def.get_instance("Localization")?;
        let name_key = loc
            .get_str("Name")
            .unwrap_or("")
            .strip_prefix('@')
            .unwrap_or("")
            .to_string();
        let desc_key = loc
            .get_str("Description")
            .unwrap_or("")
            .strip_prefix('@')
            .unwrap_or("")
            .to_string();
        if name_key.is_empty() {
            return None;
        }
        return Some(LocKeys { name_key, desc_key });
    }
    None
}

fn legacy_penetration(db: &DataCoreDatabase, guid: sc_extract::Guid) -> Option<f32> {
    let record = db.record(&guid)?;
    let inst = record.as_instance();
    let components = inst.get_array("Components")?;
    for comp in components {
        let Some(comp_inst) = to_instance(db, &comp) else {
            continue;
        };
        if comp_inst.type_name() != Some("SAmmoContainerComponentParams") {
            continue;
        }
        let Some(Value::Reference(Some(r))) = comp_inst.get("ammoParamsRecord") else {
            continue;
        };
        let ammo_record = db.record(&r.guid)?;
        let ammo_inst = ammo_record.as_instance();
        return ammo_inst
            .get_instance("projectileParams")
            .and_then(|p| p.get_instance("penetrationParams"))
            .and_then(|pen| pen.get_f32("basePenetrationDistance"));
    }
    None
}

// ── Missile extraction (raw svarog) ─────────────────────────────────────────

fn extract_missiles(db: &DataCoreDatabase, ini: &HashMap<String, String>) -> Vec<MissileData> {
    let mut missiles = Vec::new();

    for record in db.records_by_type_containing("EntityClassDefinition") {
        let components: Vec<Value> = match record.get_array("Components") {
            Some(c) => c.collect(),
            None => continue,
        };

        let mut item_type = "";
        let mut item_sub_type = "";
        let mut size = 0i32;
        let mut name_key = String::new();
        let mut desc_key = String::new();

        for comp in &components {
            let Some(inst) = to_instance(db, comp) else {
                continue;
            };
            if inst.type_name() != Some("SAttachableComponentParams") {
                continue;
            }
            let Some(attach_def) = inst.get_instance("AttachDef") else {
                continue;
            };
            item_type = attach_def.get_str("Type").unwrap_or("");
            item_sub_type = attach_def.get_str("SubType").unwrap_or("");
            size = attach_def.get_i32("Size").unwrap_or(0);

            if let Some(loc) = attach_def.get_instance("Localization") {
                name_key = loc
                    .get_str("Name")
                    .unwrap_or("")
                    .strip_prefix('@')
                    .unwrap_or("")
                    .to_string();
                desc_key = loc
                    .get_str("Description")
                    .unwrap_or("")
                    .strip_prefix('@')
                    .unwrap_or("")
                    .to_string();
            }
            break;
        }

        if item_type != "Missile"
            || name_key.is_empty()
            || !ini.contains_key(&name_key)
        {
            continue;
        }

        if let Some(m) = extract_missile(db, &components, name_key, desc_key, size, item_sub_type) {
            missiles.push(m);
        }
    }

    missiles
}

fn extract_missile(
    db: &DataCoreDatabase,
    components: &[Value],
    name_key: String,
    desc_key: String,
    size: i32,
    sub_type: &str,
) -> Option<MissileData> {
    let mut data = MissileData {
        name_key,
        desc_key,
        size,
        sub_type: sub_type.to_string(),
        tracking_signal: String::new(),
        damage_physical: 0.0,
        damage_energy: 0.0,
        damage_distortion: 0.0,
        damage_thermal: 0.0,
        damage_biochemical: 0.0,
        damage_stun: 0.0,
        speed: 0.0,
        arm_time: 0.0,
        lock_time: 0.0,
        lock_angle: 0.0,
        lock_range_min: 0.0,
        lock_range_max: 0.0,
    };

    for comp in components {
        let Some(inst) = to_instance(db, comp) else {
            continue;
        };
        if inst.type_name() != Some("SCItemMissileParams") {
            continue;
        }

        data.arm_time = inst.get_f32("armTime").unwrap_or(0.0);

        if let Some(expl) = inst.get_instance("explosionParams")
            && let Some(dmg) = expl.get_instance("damage")
        {
            data.damage_physical = dmg.get_f32("DamagePhysical").unwrap_or(0.0);
            data.damage_energy = dmg.get_f32("DamageEnergy").unwrap_or(0.0);
            data.damage_distortion = dmg.get_f32("DamageDistortion").unwrap_or(0.0);
            data.damage_thermal = dmg.get_f32("DamageThermal").unwrap_or(0.0);
            data.damage_biochemical = dmg.get_f32("DamageBiochemical").unwrap_or(0.0);
            data.damage_stun = dmg.get_f32("DamageStun").unwrap_or(0.0);
        }

        if let Some(gcs) = inst.get_instance("GCSParams") {
            data.speed = gcs.get_f32("linearSpeed").unwrap_or(0.0);
        }

        if let Some(tgt) = inst.get_instance("targetingParams") {
            data.tracking_signal = tgt.get_str("trackingSignalType").unwrap_or("").to_string();
            data.lock_time = tgt.get_f32("lockTime").unwrap_or(0.0);
            data.lock_angle = tgt.get_f32("lockingAngle").unwrap_or(0.0);
            data.lock_range_min = tgt.get_f32("lockRangeMin").unwrap_or(0.0);
            data.lock_range_max = tgt.get_f32("lockRangeMax").unwrap_or(0.0);
        }

        break;
    }

    Some(data)
}

fn to_instance<'a>(db: &'a DataCoreDatabase, val: &Value<'a>) -> Option<Instance<'a>> {
    match val {
        Value::Class { struct_index, data } => {
            Some(Instance::from_inline_data(db, *struct_index, data))
        }
        Value::StrongPointer(Some(r)) | Value::ClassRef(r) => {
            Some(db.instance(r.struct_index, r.instance_index))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {}
