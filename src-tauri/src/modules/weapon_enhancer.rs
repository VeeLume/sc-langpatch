use std::collections::HashMap;

use anyhow::Result;
use svarog_datacore::{DataCoreDatabase, Instance, Value};

use crate::module::{Module, ModuleContext, ModuleOption, OptionKind, PatchOp};

// ── Extracted weapon data ──────────────────────────────────────────────────

/// Resolved ammo stats, indexed by GUID string for fast lookup.
struct AmmoResolved {
    speed: f32,
    damage_physical: f32,
    damage_energy: f32,
    damage_distortion: f32,
    damage_thermal: f32,
    damage_biochemical: f32,
    damage_stun: f32,
    penetration: f32,
}

/// All data we extract per weapon (guns only, not missiles).
#[derive(Debug)]
struct WeaponData {
    name_key: String,
    desc_key: String,
    size: i32,
    // Per-shot damage from ammo
    alpha_physical: f32,
    alpha_energy: f32,
    alpha_distortion: f32,
    alpha_thermal: f32,
    alpha_biochemical: f32,
    alpha_stun: f32,
    penetration: f32,
    projectile_speed: f32,
    // Magazine / ammo
    mag_size: i32,
    max_ammo_load: f32,
}

/// All data we extract per missile or torpedo.
#[derive(Debug)]
struct MissileData {
    name_key: String,
    desc_key: String,
    size: i32,
    sub_type: String,
    tracking_signal: String,
    // Damage (from explosion params)
    damage_physical: f32,
    damage_energy: f32,
    damage_distortion: f32,
    damage_thermal: f32,
    damage_biochemical: f32,
    damage_stun: f32,
    // Flight
    speed: f32,
    arm_time: f32,
    // Targeting
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
        let db = match ctx.db {
            Some(db) => db,
            None => return Ok(Vec::new()),
        };

        let opt_size_prefix = ctx.config.get_bool("size_prefix").unwrap_or(true);
        let opt_missile_type = ctx.config.get_bool("missile_type_prefix").unwrap_or(true);
        let opt_weapon_stats = ctx.config.get_bool("weapon_stats").unwrap_or(true);
        let opt_missile_stats = ctx.config.get_bool("missile_stats").unwrap_or(true);

        let (weapons, missiles) = extract_all(db, ctx.ini);
        let mut patches = Vec::new();

        // ── Weapon patches ─────────────────────────────────────────────
        for w in &weapons {
            if w.name_key.is_empty() || !ctx.ini.contains_key(&w.name_key) {
                continue;
            }

            // Name prefix: size
            if opt_size_prefix && w.size > 0 {
                patches.push((
                    w.name_key.clone(),
                    PatchOp::Prefix(format!("S{} ", w.size)),
                ));
            }

            // Description suffix: weapon stats
            if opt_weapon_stats && !w.desc_key.is_empty() && ctx.ini.contains_key(&w.desc_key) {
                let mut lines = Vec::new();

                let alpha = w.alpha_physical
                    + w.alpha_energy
                    + w.alpha_distortion
                    + w.alpha_thermal
                    + w.alpha_biochemical
                    + w.alpha_stun;
                if alpha > 0.0 {
                    let mut parts = Vec::new();
                    if w.alpha_physical > 0.0 {
                        parts.push(format!("{:.0} phys", w.alpha_physical));
                    }
                    if w.alpha_energy > 0.0 {
                        parts.push(format!("{:.0} energy", w.alpha_energy));
                    }
                    if w.alpha_distortion > 0.0 {
                        parts.push(format!("{:.0} dist", w.alpha_distortion));
                    }
                    if w.alpha_thermal > 0.0 {
                        parts.push(format!("{:.0} therm", w.alpha_thermal));
                    }
                    if w.alpha_biochemical > 0.0 {
                        parts.push(format!("{:.0} bio", w.alpha_biochemical));
                    }
                    if w.alpha_stun > 0.0 {
                        parts.push(format!("{:.0} stun", w.alpha_stun));
                    }
                    lines.push(format!("Alpha: {:.0} ({})", alpha, parts.join(", ")));
                }

                if w.penetration > 0.0 {
                    lines.push(format!("Penetration: {:.2}m", w.penetration));
                }

                if w.projectile_speed > 0.0 {
                    lines.push(format!("Projectile Speed: {:.0} m/s", w.projectile_speed));
                }

                // Ballistic weapons have maxAmmoCount (total ammo pool)
                // Energy weapons have maxAmmoLoad (capacitor)
                if w.mag_size > 0 {
                    lines.push(format!("Ammo: {}", w.mag_size));
                }
                if w.max_ammo_load > 0.0 {
                    lines.push(format!("Capacitor: {:.0}", w.max_ammo_load));
                }

                if !lines.is_empty() {
                    let stats_str = lines
                        .iter()
                        .map(|l| format!("\\n{l}"))
                        .collect::<String>();
                    let suffix = format!("\\n\\n<EM4>Weapon Stats</EM4>{stats_str}");
                    patches.push((w.desc_key.clone(), PatchOp::Suffix(suffix)));
                }
            }
        }

        // ── Missile/torpedo patches ────────────────────────────────────
        for m in &missiles {
            if m.name_key.is_empty() || !ctx.ini.contains_key(&m.name_key) {
                continue;
            }

            // Name prefixes: tracking type, then size
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

            // Description suffix: missile stats
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

        Ok(patches)
    }
}

// ── Data extraction ─────────────────────────────────────────────────────────

/// Walk all EntityClassDefinition records and extract weapon + missile data.
fn extract_all(
    db: &DataCoreDatabase,
    ini: &HashMap<String, String>,
) -> (Vec<WeaponData>, Vec<MissileData>) {
    let ammo_index = build_ammo_index(db);
    let mut weapons = Vec::new();
    let mut missiles = Vec::new();
    let mut weapon_count = 0;
    let mut missile_count = 0;

    for record in db.records_by_type_containing("EntityClassDefinition") {
        let components: Vec<Value> = match record.get_array("Components") {
            Some(c) => c.collect(),
            None => continue,
        };

        // Determine what type of item this is from SAttachableComponentParams
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

        if name_key.is_empty() || !ini.contains_key(&name_key) {
            continue;
        }

        match item_type {
            "WeaponGun" => {
                if let Some(w) = extract_weapon(db, &components, &ammo_index, name_key, desc_key, size) {
                    weapons.push(w);
                    weapon_count += 1;
                }
            }
            "Missile" => {
                if let Some(m) = extract_missile(db, &components, name_key, desc_key, size, item_sub_type) {
                    missiles.push(m);
                    missile_count += 1;
                }
            }
            _ => {}
        }
    }

    eprintln!(
        "  [WeaponEnhancer] {weapon_count} weapons, {missile_count} missiles"
    );
    (weapons, missiles)
}

/// Extract weapon data from components.
fn extract_weapon(
    db: &DataCoreDatabase,
    components: &[Value],
    ammo_index: &HashMap<String, AmmoResolved>,
    name_key: String,
    desc_key: String,
    size: i32,
) -> Option<WeaponData> {
    let mut data = WeaponData {
        name_key,
        desc_key,
        size,
        alpha_physical: 0.0,
        alpha_energy: 0.0,
        alpha_distortion: 0.0,
        alpha_thermal: 0.0,
        alpha_biochemical: 0.0,
        alpha_stun: 0.0,
        penetration: 0.0,
        projectile_speed: 0.0,
        mag_size: 0,
        max_ammo_load: 0.0,
    };

    for comp in components {
        let Some(inst) = to_instance(db, comp) else {
            continue;
        };
        let type_name = inst.type_name().unwrap_or("");

        match type_name {
            "SAmmoContainerComponentParams" => {
                data.mag_size = inst.get_i32("maxAmmoCount").unwrap_or(0);

                // Resolve ammo params for damage/speed/penetration
                if let Some(Value::Reference(Some(r))) = inst.get("ammoParamsRecord") {
                    let guid = format!("{}", r.guid);
                    if let Some(ammo) = ammo_index.get(&guid) {
                        data.alpha_physical = ammo.damage_physical;
                        data.alpha_energy = ammo.damage_energy;
                        data.alpha_distortion = ammo.damage_distortion;
                        data.alpha_thermal = ammo.damage_thermal;
                        data.alpha_biochemical = ammo.damage_biochemical;
                        data.alpha_stun = ammo.damage_stun;
                        data.penetration = ammo.penetration;
                        data.projectile_speed = ammo.speed;
                    }
                }
            }
            "SCItemWeaponComponentParams" => {
                // Energy weapons have a capacitor-based ammo system
                if let Some(regen) = inst.get_instance("weaponRegenConsumerParams") {
                    data.max_ammo_load = regen.get_f32("maxAmmoLoad").unwrap_or(0.0);
                }
            }
            _ => {}
        }
    }

    Some(data)
}

/// Extract missile/torpedo data from components.
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

        // Explosion damage
        if let Some(expl) = inst.get_instance("explosionParams") {
            if let Some(dmg) = expl.get_instance("damage") {
                data.damage_physical = dmg.get_f32("DamagePhysical").unwrap_or(0.0);
                data.damage_energy = dmg.get_f32("DamageEnergy").unwrap_or(0.0);
                data.damage_distortion = dmg.get_f32("DamageDistortion").unwrap_or(0.0);
                data.damage_thermal = dmg.get_f32("DamageThermal").unwrap_or(0.0);
                data.damage_biochemical = dmg.get_f32("DamageBiochemical").unwrap_or(0.0);
                data.damage_stun = dmg.get_f32("DamageStun").unwrap_or(0.0);
            }
        }

        // GCS: speed
        if let Some(gcs) = inst.get_instance("GCSParams") {
            data.speed = gcs.get_f32("linearSpeed").unwrap_or(0.0);
        }

        // Targeting: lock time, angle, range
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

// ── Ammo index ──────────────────────────────────────────────────────────────

/// Build a map from AmmoParams GUID → resolved damage/speed/penetration.
fn build_ammo_index(db: &DataCoreDatabase) -> HashMap<String, AmmoResolved> {
    let mut index = HashMap::new();

    for record in db.records_by_type_containing("AmmoParams") {
        let guid = format!("{}", record.id());

        let speed = record.get_f32("speed").unwrap_or(0.0);

        let (dp, de, dd, dt, db_chem, ds) =
            if let Some(proj) = record.get_instance("projectileParams") {
                extract_damage(&proj, "damage")
            } else {
                (0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
            };

        let penetration = record
            .get_instance("projectileParams")
            .and_then(|p| p.get_instance("penetrationParams"))
            .and_then(|pen| pen.get_f32("basePenetrationDistance"))
            .unwrap_or(0.0);

        index.insert(
            guid,
            AmmoResolved {
                speed,
                damage_physical: dp,
                damage_energy: de,
                damage_distortion: dd,
                damage_thermal: dt,
                damage_biochemical: db_chem,
                damage_stun: ds,
                penetration,
            },
        );
    }

    index
}

fn extract_damage(parent: &Instance, field_name: &str) -> (f32, f32, f32, f32, f32, f32) {
    if let Some(dmg) = parent.get_instance(field_name) {
        (
            dmg.get_f32("DamagePhysical").unwrap_or(0.0),
            dmg.get_f32("DamageEnergy").unwrap_or(0.0),
            dmg.get_f32("DamageDistortion").unwrap_or(0.0),
            dmg.get_f32("DamageThermal").unwrap_or(0.0),
            dmg.get_f32("DamageBiochemical").unwrap_or(0.0),
            dmg.get_f32("DamageStun").unwrap_or(0.0),
        )
    } else {
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

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
