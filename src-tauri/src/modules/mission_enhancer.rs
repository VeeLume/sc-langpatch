use std::collections::{HashMap, HashSet};

use anyhow::Result;
use svarog_datacore::{DataCoreDatabase, Instance, Value};

use crate::module::{ChoiceOption, Module, ModuleContext, ModuleOption, OptionKind, PatchOp};

// ── Extracted mission data ──────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum CrimestatRisk {
    #[default]
    None,
    /// Friendlies present WITH HUD markers (or allied ships in space).
    Moderate,
    /// Friendlies present WITHOUT HUD markers — cannot distinguish friend from foe.
    High,
}

/// All data we extract per mission tier (one title = one tier).
#[derive(Debug, Default)]
struct MissionTier {
    title_key: String,
    desc_key: String,

    // Blueprint data
    blueprint_pool_name: String,
    blueprint_items: Vec<String>,
    blueprint_chance: f32,

    // Availability (inherited from handler level)
    can_be_shared: Option<bool>,
    once_only: Option<bool>,
    has_personal_cooldown: Option<bool>,
    personal_cooldown_min: Option<f32>,
    abandoned_cooldown_min: Option<f32>,

    // Rewards
    scrip_amount: Option<i32>,
    rep_amount: Option<i32>,

    // Ship spawn data
    hostile_spawns: Vec<SpawnGroup>,
    allied_spawns: Vec<SpawnGroup>,

    // Crimestat risk
    crimestat_risk: CrimestatRisk,
}

/// A named wave of ship spawns (e.g. "Starter Wave", "First Wave").
#[derive(Debug, Clone)]
struct SpawnGroup {
    name: String,
    slots: Vec<SpawnSlot>,
}

/// One slot in a spawn group — all options are alternatives (one is picked).
#[derive(Debug, Clone)]
struct SpawnSlot {
    count: i32,
    /// Tag-based description of this slot.
    _difficulty: Option<String>,
    ship_class: Option<String>,
    ai_skill: Option<String>,
    /// Resolved ship (display_name, size) pairs, sorted by size.
    ships: Vec<(String, i32)>,
}

// ── Module ──────────────────────────────────────────────────────────────────

pub struct MissionEnhancer;

impl Module for MissionEnhancer {
    fn id(&self) -> &str {
        "mission_enhancer"
    }

    fn name(&self) -> &str {
        "Mission Enhancer"
    }

    fn description(&self) -> &str {
        "Enrich mission titles and descriptions with blueprint rewards, cooldowns, and more"
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
                id: "blueprint_tag".into(),
                label: "Blueprint Tag".into(),
                description: "Add [BP] to titles of missions that reward blueprints".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "blueprint_list".into(),
                label: "Blueprint List".into(),
                description: "Append blueprint item list to mission descriptions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "solo_tag".into(),
                label: "Solo Tag".into(),
                description: "Add [Solo] to titles of solo-only missions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "once_tag".into(),
                label: "One-Time Tag".into(),
                description: "Add [Uniq] to titles of one-time-only missions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "mission_info".into(),
                label: "Mission Info".into(),
                description: "Append cooldown, rep reward, and scrip to descriptions".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "crimestat_tag".into(),
                label: "Crimestat Risk Tag".into(),
                description: "Mark missions where killing friendly NPCs gives crimestat".into(),
                kind: OptionKind::Choice {
                    choices: vec![
                        ChoiceOption { value: "off".into(), label: "Off".into() },
                        ChoiceOption { value: "simple".into(), label: "Simple [CS Risk]".into() },
                        ChoiceOption { value: "colored".into(), label: "Colored (yellow/red)".into() },
                    ],
                },
                default: "colored".into(),
            },
            ModuleOption {
                id: "ship_encounters".into(),
                label: "Ship Encounters".into(),
                description: "Show hostile and allied ship types in mission descriptions".into(),
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

        let opt_blueprint_tag = ctx.config.get_bool("blueprint_tag").unwrap_or(true);
        let opt_blueprint_list = ctx.config.get_bool("blueprint_list").unwrap_or(true);
        let opt_solo_tag = ctx.config.get_bool("solo_tag").unwrap_or(true);
        let opt_once_tag = ctx.config.get_bool("once_tag").unwrap_or(true);
        let opt_mission_info = ctx.config.get_bool("mission_info").unwrap_or(true);
        let opt_ship_encounters = ctx.config.get_bool("ship_encounters").unwrap_or(true);
        let opt_crimestat = ctx.config.get_str("crimestat_tag").unwrap_or("colored");

        // Phase 1: Extract all mission tiers from all contract generators
        let tiers = extract_all_tiers(db, ctx.ini);

        // Phase 2: Identify title keys with mixed blueprint status
        // Some contracts share a title key but only some variants award blueprints.
        // Those get [BP]* instead of [BP].
        let mixed_bp_keys: HashSet<String> = {
            let mut bp_status: HashMap<&str, (bool, bool)> = HashMap::new();
            for tier in &tiers {
                if tier.title_key.is_empty() { continue; }
                let entry = bp_status.entry(&tier.title_key).or_insert((false, false));
                if tier.blueprint_items.is_empty() {
                    entry.1 = true; // has a no-BP variant
                } else {
                    entry.0 = true; // has a BP variant
                }
            }
            bp_status.iter()
                .filter(|(_, (has_bp, no_bp))| *has_bp && *no_bp)
                .map(|(key, _)| key.to_string())
                .collect()
        };

        // Phase 3: Generate patches from extracted data
        let mut patches = Vec::new();

        for tier in &tiers {
            if tier.title_key.is_empty() || !ctx.ini.contains_key(&tier.title_key) {
                continue;
            }

            // Title suffixes
            let mut title_tags = Vec::new();

            if opt_blueprint_tag {
                if mixed_bp_keys.contains(&tier.title_key) {
                    // Some variants of this contract award BPs, some don't
                    title_tags.push("<EM4>[BP]*</EM4>");
                } else if !tier.blueprint_items.is_empty() {
                    title_tags.push("<EM4>[BP]</EM4>");
                }
            }
            if opt_solo_tag && tier.can_be_shared == Some(false) {
                title_tags.push("[Solo]");
            }
            if opt_once_tag && tier.once_only == Some(true) {
                title_tags.push("[Uniq]");
            }
            if opt_crimestat != "off" && tier.crimestat_risk != CrimestatRisk::None {
                match (opt_crimestat, tier.crimestat_risk) {
                    ("colored", CrimestatRisk::High) => title_tags.push("<EM3>[CS Risk!]</EM3>"),
                    ("colored", CrimestatRisk::Moderate) => title_tags.push("<EM4>[CS Risk]</EM4>"),
                    (_, _) => title_tags.push("[CS Risk]"),
                }
            }

            if !title_tags.is_empty() {
                let suffix = format!(" {}", title_tags.join(" "));
                patches.push((tier.title_key.clone(), PatchOp::Suffix(suffix)));
            }

            // Description suffix
            let desc_key = if !tier.desc_key.is_empty() {
                tier.desc_key.clone()
            } else {
                // Convention: title key with "title" replaced by "desc"
                tier.title_key
                    .replace("title", "desc")
                    .replace("Title", "Desc")
            };

            if !ctx.ini.contains_key(&desc_key) {
                continue;
            }

            let mut desc_parts: Vec<String> = Vec::new();

            // Blueprint list
            if opt_blueprint_list && !tier.blueprint_items.is_empty() {
                let items_str = tier
                    .blueprint_items
                    .iter()
                    .map(|i| format!("\\n- {i}"))
                    .collect::<String>();
                let chance_str = if tier.blueprint_chance < 1.0 {
                    format!(" ({}% chance)", (tier.blueprint_chance * 100.0) as i32)
                } else {
                    String::new()
                };
                desc_parts.push(format!(
                    "<EM4>Potential Blueprints</EM4>{chance_str}{items_str}"
                ));
            }

            // Mission info block
            if opt_mission_info {
                let mut info_lines = Vec::new();

                if tier.has_personal_cooldown == Some(true) {
                    let personal = tier.personal_cooldown_min.unwrap_or(0.0);
                    let abandon = tier.abandoned_cooldown_min.unwrap_or(0.0);

                    if personal > 0.0 && abandon > 0.0 && (abandon - personal).abs() > 0.5 {
                        // Show both when they meaningfully differ
                        info_lines.push(format!(
                            "Cooldown: {}min (abandon: {}min)",
                            personal as i32, abandon as i32
                        ));
                    } else if personal > 0.0 {
                        info_lines.push(format!("Cooldown: {}min", personal as i32));
                    }
                }

                if let Some(rep) = tier.rep_amount {
                    if rep > 0 {
                        info_lines.push(format!("Rep: {rep} XP"));
                    }
                }

                if let Some(scrip) = tier.scrip_amount {
                    if scrip > 0 {
                        info_lines.push(format!("Scrip: {scrip}"));
                    }
                }

                if !info_lines.is_empty() {
                    let info_str = info_lines
                        .iter()
                        .map(|l| format!("\\n{l}"))
                        .collect::<String>();
                    desc_parts.push(format!("<EM4>Mission Info</EM4>{info_str}"));
                }
            }

            // Ship encounter block
            if opt_ship_encounters
                && (!tier.hostile_spawns.is_empty() || !tier.allied_spawns.is_empty())
            {
                let encounter_str =
                    format_encounters(&tier.hostile_spawns, &tier.allied_spawns);
                if !encounter_str.is_empty() {
                    desc_parts.push(format!("<EM4>Ship Encounters</EM4>{encounter_str}"));
                }
            }

            if !desc_parts.is_empty() {
                let suffix = format!("\\n\\n{}", desc_parts.join("\\n\\n"));
                patches.push((desc_key, PatchOp::Suffix(suffix)));
            }
        }

        Ok(patches)
    }
}

/// Format all ship encounters (hostile + allied) into newline-separated output.
/// Each spawn group gets its own line. Skill range shown once at the end.
fn format_encounters(hostile: &[SpawnGroup], allied: &[SpawnGroup]) -> String {
    let mut lines = Vec::new();
    let mut min_skill: Option<u32> = None;
    let mut max_skill: Option<u32> = None;

    // Collect skill range across all groups
    for groups in [hostile, allied] {
        for g in groups {
            for slot in &g.slots {
                if let Some(skill) = &slot.ai_skill {
                    if let Some(level) = parse_skill_level(skill) {
                        min_skill = Some(min_skill.map_or(level, |m: u32| m.min(level)));
                        max_skill = Some(max_skill.map_or(level, |m: u32| m.max(level)));
                    }
                }
            }
        }
    }

    // Format each group as a line
    let mut seen_lines = HashSet::new();
    for g in hostile {
        let line = format_group(g, "Hostile");
        if !line.is_empty() && seen_lines.insert(line.clone()) {
            lines.push(line);
        }
    }
    for g in allied {
        let line = format_group(g, "Allied");
        if !line.is_empty() && seen_lines.insert(line.clone()) {
            lines.push(line);
        }
    }

    // Append skill range
    if let (Some(lo), Some(hi)) = (min_skill, max_skill) {
        if lo == hi {
            lines.push(format!("Skill {lo}"));
        } else {
            lines.push(format!("Skill {lo}-{hi}"));
        }
    }

    if lines.is_empty() {
        return String::new();
    }

    lines.iter().map(|l| format!("\\n{l}")).collect()
}

/// Format a single spawn group as one line.
/// Example: "Hostile Wave 1: ~4x Gladius, Vanguard, Arrow, Cutlass"
fn format_group(group: &SpawnGroup, default_role: &str) -> String {
    if group.slots.is_empty() {
        return String::new();
    }

    // Sum ship count across all slots
    let total_count: i32 = group.slots.iter().map(|s| s.count).sum();

    // Collect all ships across slots, sorted by size
    let mut all_ships: Vec<(String, i32)> = Vec::new();
    let mut has_tag_only_slots = false;

    for slot in &group.slots {
        if !slot.ships.is_empty() {
            for (name, size) in &slot.ships {
                let short = shorten_ship_name(name);
                if !all_ships.iter().any(|(n, _)| n == &short) {
                    all_ships.push((short, *size));
                }
            }
        } else {
            has_tag_only_slots = true;
        }
    }

    // Sort by size, then collapse hull variants
    all_ships.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let short_names: Vec<String> = all_ships.iter().map(|(n, _)| n.clone()).collect();
    let collapsed = collapse_variants(&short_names);

    // Build the label — clean up internal names
    let label = if !group.name.is_empty() {
        let clean_name = group.name
            .replace("ShipToDefend", "Escort")
            .replace("_", " ");
        format!("{default_role} {clean_name}")
    } else {
        default_role.to_string()
    };

    // Build ship list
    let ships_str = if !collapsed.is_empty() {
        collapsed.join(", ")
    } else if has_tag_only_slots {
        // Fallback for tag-only slots (e.g. DefendShip)
        let tag_label = group.slots.iter()
            .find_map(|s| s.ship_class.as_ref())
            .map(|c| match c.as_str() {
                "DefendShip" => "transport/cargo",
                "CombatShip" | "LargeCombatShip" => "capital",
                _ => "ships",
            })
            .unwrap_or("ships");
        tag_label.to_string()
    } else {
        return String::new();
    };

    let count_str = if total_count > 1 {
        format!(" ~{total_count}x")
    } else {
        String::new()
    };
    // Short lists stay on the same line; long lists wrap to a new line
    let sep = if collapsed.len() > 5 { "\\n" } else { " " };
    format!("<EM4>{label}:{count_str}</EM4>{sep}{ships_str}")
}

/// Strip manufacturer prefix from ship display name.
fn shorten_ship_name(name: &str) -> String {
    let prefixes = [
        "Aegis ", "Anvil ", "Argo ", "Aopoa ", "Banu ", "C.O. ", "Crusader ",
        "Drake ", "Esperia ", "MISC ", "Mirai ", "Origin ", "RSI ", "CHCO ",
    ];
    for prefix in prefixes {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    name.to_string()
}

/// Collapse hull variants into base hull names.
/// ["Avenger Stalker", "Avenger Warlock", "Avenger Titan Renegade"] → ["Avenger"]
/// ["Cutlass Black"] → ["Cutlass Black"] (single variant keeps full name)
/// ["Gladius", "Gladius Valiant"] → ["Gladius"]
fn collapse_variants(names: &[String]) -> Vec<String> {
    // Group by first word
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();

    for name in names {
        let base = name.split_whitespace().next().unwrap_or(name);
        if let Some(group) = groups.iter_mut().find(|(b, _)| b == base) {
            group.1.push(name);
        } else {
            groups.push((base.to_string(), vec![name]));
        }
    }

    groups
        .into_iter()
        .map(|(base, variants)| {
            if variants.len() == 1 {
                // Single variant: keep full name
                variants[0].to_string()
            } else {
                // Multiple variants: collapse to base hull
                base
            }
        })
        .collect()
}

/// Parse skill level from tag like "HumanPilot70" → 70.
fn parse_skill_level(tag: &str) -> Option<u32> {
    tag.strip_prefix("HumanPilot").and_then(|s| s.parse().ok())
}

// ── Data extraction ─────────────────────────────────────────────────────────

/// Walk all ContractGenerator records and extract mission tier data.
fn extract_all_tiers(db: &DataCoreDatabase, ini: &HashMap<String, String>) -> Vec<MissionTier> {
    // Pre-build indexes
    let pool_items = build_pool_items_map(db, ini);
    let tag_names = build_tag_name_map(db);
    let (ship_index, ship_tags) = build_ship_tag_index(db, ini, &tag_names);

    let mut tiers = Vec::new();
    let mut contract_count = 0;
    let mut generator_count = 0;

    for record in db.records_by_type_containing("ContractGenerator") {
        generator_count += 1;
        let inst = record.as_instance();

        let Some(handlers) = inst.get_array("generators") else {
            continue;
        };

        for handler_val in handlers {
            let Some(handler) = to_instance(db, &handler_val) else {
                continue;
            };

            // Extract handler-level availability (shared across all tiers)
            let mut avail = extract_availability(&handler);

            // Detect handler-level crimestat risk from contractParams
            if let Some(cp) = handler.get_instance("contractParams") {
                avail.crimestat_risk = detect_crimestat_risk_in_overrides(db, &cp);
            }

            // Extract handler-level ship spawns from contractParams
            let handler_spawns = extract_spawns_from_params(db, &handler, &tag_names, &ship_index, &ship_tags);

            // Walk both regular contracts and intro contracts
            for array_name in ["contracts", "introContracts"] {
                let Some(contracts) = handler.get_array(array_name) else {
                    continue;
                };

                for cv in contracts {
                    contract_count += 1;
                    let Some(contract) = to_instance(db, &cv) else {
                        continue;
                    };

                    extract_contract_tier(
                        db, &contract, &avail, &handler_spawns, &pool_items,
                        &tag_names, &ship_index, &ship_tags, &mut tiers,
                    );

                    // CareerContract has subContracts — walk those too
                    if let Some(subs) = contract.get_array("subContracts") {
                        for sv in subs {
                            let Some(sub) = to_instance(db, &sv) else {
                                continue;
                            };
                            extract_contract_tier(
                                db, &sub, &avail, &handler_spawns, &pool_items,
                                &tag_names, &ship_index, &ship_tags, &mut tiers,
                            );
                        }
                    }
                }
            }
        }
    }

    eprintln!(
        "  [MissionEnhancer] {generator_count} generators, {contract_count} contracts -> {} tiers with titles",
        tiers.len()
    );
    tiers
}

/// Availability data inherited from the handler level.
#[derive(Debug, Default, Clone)]
struct Availability {
    once_only: Option<bool>,
    has_personal_cooldown: Option<bool>,
    personal_cooldown_min: Option<f32>,
    abandoned_cooldown_min: Option<f32>,
    /// Handler-level crimestat risk (from contractParams).
    crimestat_risk: CrimestatRisk,
}

fn extract_availability(handler: &Instance) -> Availability {
    let Some(avail) = handler.get_instance("defaultAvailability") else {
        return Availability::default();
    };
    Availability {
        once_only: avail.get_bool("onceOnly"),
        has_personal_cooldown: avail.get_bool("hasPersonalCooldown"),
        personal_cooldown_min: avail.get_f32("personalCooldownTime"),
        abandoned_cooldown_min: avail.get_f32("abandonedCooldownTime"),
        crimestat_risk: CrimestatRisk::None, // Set later from handler contractParams
    }
}

/// Handler-level spawns that may be inherited by contracts.
#[derive(Debug, Default, Clone)]
struct HandlerSpawns {
    hostile: Vec<SpawnGroup>,
    allied: Vec<SpawnGroup>,
}

/// Extract one contract tier's data.
fn extract_contract_tier(
    db: &DataCoreDatabase,
    contract: &Instance,
    avail: &Availability,
    handler_spawns: &HandlerSpawns,
    pool_items: &HashMap<String, Vec<String>>,
    tag_names: &HashMap<String, String>,
    ship_index: &[ShipEntity],
    ship_tags: &HashSet<String>,
    out: &mut Vec<MissionTier>,
) {
    let mut tier = MissionTier {
        once_only: avail.once_only,
        has_personal_cooldown: avail.has_personal_cooldown,
        personal_cooldown_min: avail.personal_cooldown_min,
        abandoned_cooldown_min: avail.abandoned_cooldown_min,
        ..Default::default()
    };

    // Extract title and description
    extract_title_from_contract(db, contract, &mut tier);

    if tier.title_key.is_empty() {
        return;
    }

    // Resolve canBeShared from the contract template
    tier.can_be_shared = resolve_can_be_shared(db, contract);

    // Extract rewards from contractResults
    if let Some(results) = contract.get_instance("contractResults") {
        extract_rewards(db, &results, &mut tier);
    }

    // Resolve blueprint items from pool name
    if !tier.blueprint_pool_name.is_empty() {
        if let Some(items) = pool_items.get(&tier.blueprint_pool_name) {
            tier.blueprint_items = items.clone();
        }
    }

    // Extract ship spawns — contract-level overrides take precedence over handler
    let contract_spawns = extract_spawns_from_contract(db, contract, tag_names, ship_index, ship_tags);
    if !contract_spawns.hostile.is_empty() || !contract_spawns.allied.is_empty() {
        tier.hostile_spawns = contract_spawns.hostile;
        tier.allied_spawns = contract_spawns.allied;
    } else {
        tier.hostile_spawns = handler_spawns.hostile.clone();
        tier.allied_spawns = handler_spawns.allied.clone();
    }

    // Detect crimestat risk — contract-level overrides handler-level
    let contract_risk = detect_crimestat_risk(db, contract);
    tier.crimestat_risk = if contract_risk != CrimestatRisk::None {
        contract_risk
    } else if avail.crimestat_risk != CrimestatRisk::None {
        avail.crimestat_risk
    } else if !tier.allied_spawns.is_empty() {
        // Allied ships in space = moderate risk (collision/stray fire)
        CrimestatRisk::Moderate
    } else {
        CrimestatRisk::None
    };

    out.push(tier);
}

/// Resolve canBeShared from the contract's template reference.
fn resolve_can_be_shared(db: &DataCoreDatabase, contract: &Instance) -> Option<bool> {
    let template_val = contract.get("template")?;

    let template_inst = match &template_val {
        Value::Reference(Some(r)) => {
            let rec = db.record(&r.guid)?;
            Some(rec.as_instance())
        }
        Value::StrongPointer(Some(r)) | Value::ClassRef(r) => {
            Some(db.instance(r.struct_index, r.instance_index))
        }
        Value::Class { struct_index, data } => {
            Some(Instance::from_inline_data(db, *struct_index, data))
        }
        _ => None,
    }?;

    template_inst
        .get_instance("contractClass")?
        .get_instance("additionalParams")?
        .get_bool("canBeShared")
}

// ── Crimestat risk detection ───────────────────────────────────────────────

/// Detect crimestat risk for a contract by checking multiple signals.
fn detect_crimestat_risk(db: &DataCoreDatabase, contract: &Instance) -> CrimestatRisk {
    // Check contract-level paramOverrides
    if let Some(po) = contract.get_instance("paramOverrides") {
        let risk = detect_crimestat_risk_in_overrides(db, &po);
        if risk != CrimestatRisk::None {
            return risk;
        }
    }

    // Check template contractProperties as fallback
    if let Some(template_val) = contract.get("template") {
        let template_inst = match &template_val {
            Value::Reference(Some(r)) => db.record(&r.guid).map(|rec| rec.as_instance()),
            Value::StrongPointer(Some(r)) | Value::ClassRef(r) => {
                Some(db.instance(r.struct_index, r.instance_index))
            }
            Value::Class { struct_index, data } => {
                Some(Instance::from_inline_data(db, *struct_index, data))
            }
            _ => None,
        };
        if let Some(ti) = template_inst {
            if let Some(props) = ti.get_array("contractProperties") {
                let risk = detect_crimestat_risk_in_properties(db, props);
                if risk != CrimestatRisk::None {
                    return risk;
                }
            }
        }
    }

    CrimestatRisk::None
}

/// Detect crimestat risk from a propertyOverrides array.
fn detect_crimestat_risk_in_overrides(db: &DataCoreDatabase, parent: &Instance) -> CrimestatRisk {
    let Some(overrides) = parent.get_array("propertyOverrides") else {
        return CrimestatRisk::None;
    };
    detect_crimestat_risk_in_properties(db, overrides)
}

/// Detect crimestat risk from a MissionProperty iterator.
/// Checks for DontHarm* flags and missionAlliedMarker on NPC spawns.
fn detect_crimestat_risk_in_properties<'a>(
    db: &'a DataCoreDatabase,
    props: impl Iterator<Item = Value<'a>>,
) -> CrimestatRisk {
    let mut has_dont_harm = false;
    let mut has_allied_markers = false;

    for pv in props {
        let Some(prop) = to_instance(db, &pv) else {
            continue;
        };
        let var_name = prop.get_str("missionVariableName").unwrap_or("");

        // Signal 1: DontHarmAllies / DontHarmCivs properties
        if var_name == "DontHarmAllies_BP"
            || var_name == "BP_DontHarmAllies"
            || var_name == "DontHarmCivs_BP"
            || var_name == "BP_DontHarmCivs"
        {
            if let Some(val) = prop.get_instance("value") {
                // Check options array (MissionPropertyValueOption_Integer)
                if let Some(opts) = val.get_array("options") {
                    for ov in opts {
                        if let Some(oi) = to_instance(db, &ov) {
                            if oi.get_i32("value") == Some(1) {
                                has_dont_harm = true;
                            }
                        }
                    }
                }
                // Direct integer value
                if val.get_i32("value") == Some(1) {
                    has_dont_harm = true;
                }
            }
        }

        // Signal 2: NPC spawn descriptions with missionAlliedMarker
        if has_npc_allied_marker(db, &prop) {
            has_allied_markers = true;
        }
    }

    if has_dont_harm && !has_allied_markers {
        CrimestatRisk::High
    } else if has_allied_markers {
        CrimestatRisk::Moderate
    } else {
        CrimestatRisk::None
    }
}

/// Check if a MissionProperty contains NPC spawns with missionAlliedMarker = true.
fn has_npc_allied_marker(db: &DataCoreDatabase, prop: &Instance) -> bool {
    let Some(val) = prop.get_instance("value") else {
        return false;
    };

    // NPC spawn descriptions use MissionPropertyValue_NPCSpawnDescriptions
    let val_type = val.type_name().unwrap_or("");
    if !val_type.contains("NPC") {
        return false;
    }

    // Walk: value.spawnDescriptions[] → options[] → autoSpawnSettings.missionAlliedMarker
    let Some(descs) = val.get_array("spawnDescriptions") else {
        return false;
    };

    for dv in descs {
        let Some(desc) = to_instance(db, &dv) else {
            continue;
        };
        let Some(options) = desc.get_array("options") else {
            continue;
        };
        for ov in options {
            let Some(opt) = to_instance(db, &ov) else {
                continue;
            };
            if let Some(auto) = opt.get_instance("autoSpawnSettings") {
                if auto.get_bool("missionAlliedMarker") == Some(true) {
                    return true;
                }
            }
        }
    }

    false
}

/// Extract Title and Description from string param overrides.
fn extract_string_params(db: &DataCoreDatabase, po: &Instance, tier: &mut MissionTier) {
    let Some(overrides) = po.get_array("stringParamOverrides") else {
        return;
    };
    for val in overrides {
        let Some(param_inst) = to_instance(db, &val) else {
            continue;
        };
        let param_name = match param_inst.get("param") {
            Some(Value::Enum(e)) => e.to_string(),
            Some(Value::String(s)) => s.to_string(),
            _ => continue,
        };
        let param_value = param_inst.get_str("value").unwrap_or("");
        let key = param_value.strip_prefix('@').unwrap_or(param_value);

        match param_name.as_str() {
            "Title" => tier.title_key = key.to_string(),
            "Description" => tier.desc_key = key.to_string(),
            _ => {}
        }
    }
}

/// Try to extract string params from multiple locations on the contract.
fn extract_title_from_contract(db: &DataCoreDatabase, contract: &Instance, tier: &mut MissionTier) {
    if let Some(po) = contract.get_instance("paramOverrides") {
        extract_string_params(db, &po, tier);
    }
    if tier.title_key.is_empty() {
        extract_string_params(db, contract, tier);
    }
    if tier.title_key.is_empty() {
        if let Some(cp) = contract.get_instance("contractParams") {
            extract_string_params(db, &cp, tier);
        }
    }
}

/// Extract rewards from contract results array.
fn extract_rewards(db: &DataCoreDatabase, results: &Instance, tier: &mut MissionTier) {
    let Some(result_arr) = results.get_array("contractResults") else {
        return;
    };

    for val in result_arr {
        let Some(result_inst) = to_instance(db, &val) else {
            continue;
        };
        let tname = result_inst.type_name().unwrap_or("");

        match tname {
            "BlueprintRewards" => {
                tier.blueprint_chance = result_inst.get_f32("chance").unwrap_or(0.0);
                if let Some(Value::Reference(Some(r))) = result_inst.get("blueprintPool") {
                    if let Some(rec) = db.record(&r.guid) {
                        tier.blueprint_pool_name = rec.name().unwrap_or("").to_string();
                    }
                }
            }
            "ContractResult_Item" => {
                tier.scrip_amount = result_inst.get_i32("amount");
            }
            "ContractResult_LegacyReputation" => {
                if let Some(rep) = result_inst.get_instance("contractResultReputationAmounts") {
                    if let Some(Value::Reference(Some(r))) = rep.get("reward") {
                        if let Some(rec) = db.record(&r.guid) {
                            let reward_inst = rec.as_instance();
                            tier.rep_amount = reward_inst.get_i32("reputationAmount");
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ── Ship spawn resolution ──────────────────────────────────────────────────

/// A ship entity with its tag set and display name, used for spawn query matching.
struct ShipEntity {
    display_name: String,
    tags: HashSet<String>,
    /// Vehicle size from AttachDef (1=small, 2=medium, 3=large, etc.)
    size: i32,
}

/// Build a map from Tag GUID to tag name.
fn build_tag_name_map(db: &DataCoreDatabase) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for record in db.records_by_type_containing("Tag") {
        if record.type_name() != Some("Tag") {
            continue;
        }
        let guid = format!("{}", record.id());
        let inst = record.as_instance();
        let name = inst.get_str("tagName").unwrap_or("").to_string();
        if !name.is_empty() {
            map.insert(guid, name);
        }
    }
    map
}

/// Build an index of AI ship entities with their tags and display names.
/// Also returns the union of all tags found across ship entities, used to
/// identify which spawn-option tags are ship-selection-relevant.
fn build_ship_tag_index(
    db: &DataCoreDatabase,
    ini: &HashMap<String, String>,
    tag_names: &HashMap<String, String>,
) -> (Vec<ShipEntity>, HashSet<String>) {
    let mut ships = Vec::new();
    let mut all_ship_tags = HashSet::new();

    for record in db.records_by_type_containing("EntityClassDefinition") {
        let rec_name = record.name().unwrap_or("");
        if !rec_name.contains("_PU_AI") && !rec_name.contains("_pu_ai") {
            continue;
        }

        let inst = record.as_instance();
        let entity_tags: HashSet<String> = if let Some(tags) = inst.get_array("tags") {
            tags.filter_map(|t| {
                if let Value::Reference(Some(r)) = t {
                    let guid = format!("{}", r.guid);
                    tag_names.get(&guid).cloned()
                } else {
                    None
                }
            })
            .collect()
        } else {
            HashSet::new()
        };

        let display_name = get_entity_display_name(db, &inst, ini);
        if display_name.is_empty() {
            continue;
        }

        // Extract vehicle size from SAttachableComponentParams
        let size = get_entity_size(db, &inst);

        all_ship_tags.extend(entity_tags.iter().cloned());
        ships.push(ShipEntity {
            display_name,
            tags: entity_tags,
            size,
        });
    }

    (ships, all_ship_tags)
}

/// Resolve a spawn tag query against the ship index.
/// Returns deduplicated (display_name, size) pairs, sorted by size ascending.
fn resolve_spawn_query(
    positive_tags: &HashSet<String>,
    negative_tags: &HashSet<String>,
    ship_index: &[ShipEntity],
) -> Vec<(String, i32)> {
    let mut matches: Vec<(String, i32)> = ship_index
        .iter()
        .filter(|ship| {
            positive_tags.iter().all(|t| ship.tags.contains(t))
                && negative_tags.iter().all(|t| !ship.tags.contains(t))
        })
        .map(|ship| (ship.display_name.clone(), ship.size))
        .collect();
    matches.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    matches.dedup_by(|a, b| a.0 == b.0);
    matches
}

/// Collect tag GUIDs from a tag array, resolving to names.
fn collect_tag_names(
    _db: &DataCoreDatabase,
    inst: &Instance,
    field: &str,
    tag_names: &HashMap<String, String>,
) -> HashSet<String> {
    let mut result = HashSet::new();
    let Some(tags_inst) = inst.get_instance(field) else {
        return result;
    };
    let Some(tags_arr) = tags_inst.get_array("tags") else {
        return result;
    };
    for tv in tags_arr {
        if let Value::Reference(Some(r)) = tv {
            let guid = format!("{}", r.guid);
            if let Some(name) = tag_names.get(&guid) {
                result.insert(name.clone());
            }
        }
    }
    result
}

/// Tags that select ship difficulty tier.
const DIFFICULTY_TAGS: &[&str] = &[
    "VeryEasy", "Easy", "Medium", "Hard", "VeryHard", "Super",
];

/// Tags that select ship class/role (used for display categorization only).
const CLASS_TAGS: &[&str] = &[
    "Light_Fighter", "Medium_Fighter", "Heavy_Fighter", "Gun_Boat",
    "LightInterceptor", "Medium_Interceptor", "HeavyFighter",
    "Heavy_Interceptor",
    "Dropship", "Industrial", "DefendShip",
    "CombatShip", "LargeCombatShip",
];

/// Check if a tag is an AI skill tag (HumanPilot10..100, AcePilot).
fn is_ai_skill_tag(tag: &str) -> bool {
    tag.starts_with("HumanPilot") || tag == "AcePilot"
}

/// Extract spawn groups from a MissionPropertyValue_ShipSpawnDescriptions instance.
fn extract_spawn_groups_from_value(
    db: &DataCoreDatabase,
    value: &Instance,
    tag_names: &HashMap<String, String>,
    ship_index: &[ShipEntity],
    ship_tags: &HashSet<String>,
) -> Vec<SpawnGroup> {
    let mut groups = Vec::new();
    let Some(descs) = value.get_array("spawnDescriptions") else {
        return groups;
    };

    for dv in descs {
        let Some(desc) = to_instance(db, &dv) else {
            continue;
        };
        let name = desc.get_str("Name").unwrap_or("").to_string();
        let mut slots = Vec::new();

        let Some(ships_arr) = desc.get_array("ships") else {
            continue;
        };

        for sv in ships_arr {
            let Some(ship_options) = to_instance(db, &sv) else {
                continue;
            };

            let Some(options) = ship_options.get_array("options") else {
                continue;
            };

            // Collect tag info and ship matches across all options in this slot
            let mut all_ships: Vec<(String, i32)> = Vec::new();
            let mut max_count = 1i32;
            let mut slot_difficulty: Option<String> = None;
            let mut slot_class: Option<String> = None;
            let mut slot_skill: Option<String> = None;

            for ov in options {
                let Some(opt) = to_instance(db, &ov) else {
                    continue;
                };

                let count = opt.get_i32("concurrentAmount").unwrap_or(1);
                if count > max_count {
                    max_count = count;
                }

                let positive = collect_tag_names(db, &opt, "tags", tag_names);
                let negative = collect_tag_names(db, &opt, "negativeTags", tag_names);

                // Extract tag categories for display
                for tag in &positive {
                    if DIFFICULTY_TAGS.contains(&tag.as_str()) && slot_difficulty.is_none() {
                        slot_difficulty = Some(tag.clone());
                    }
                    if CLASS_TAGS.contains(&tag.as_str()) && slot_class.is_none() {
                        slot_class = Some(tag.clone());
                    }
                    if is_ai_skill_tag(tag) && slot_skill.is_none() {
                        slot_skill = Some(tag.clone());
                    }
                }

                // Resolve ships — keep only tags that exist on ship entities
                // (this naturally excludes AI skill tags and other non-ship tags)
                let match_positive: HashSet<String> = positive
                    .iter()
                    .filter(|t| ship_tags.contains(t.as_str()))
                    .cloned()
                    .collect();
                if !match_positive.is_empty() {
                    let matches = resolve_spawn_query(&match_positive, &negative, ship_index);
                    for entry in matches {
                        if !all_ships.iter().any(|(n, _)| n == &entry.0) {
                            all_ships.push(entry);
                        }
                    }
                }
            }

            if !all_ships.is_empty() || slot_difficulty.is_some() || slot_class.is_some() {
                // Re-sort by size after merging options
                all_ships.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
                slots.push(SpawnSlot {
                    count: max_count,
                    _difficulty: slot_difficulty,
                    ship_class: slot_class,
                    ai_skill: slot_skill,
                    ships: all_ships,
                });
            }
        }

        if !slots.is_empty() {
            groups.push(SpawnGroup { name, slots });
        }
    }

    groups
}

/// Variable name patterns that indicate hostile vs allied ship spawns.
fn classify_spawn_variable(var_name: &str) -> Option<SpawnRole> {
    let lower = var_name.to_lowercase();
    if lower.contains("hostile") || lower.contains("missiontargets") || lower.contains("waveships") {
        Some(SpawnRole::Hostile)
    } else if lower.contains("attacked") || lower.contains("allied") || lower.contains("escort")
        || lower.contains("defend")
    {
        Some(SpawnRole::Allied)
    } else {
        None
    }
}

enum SpawnRole {
    Hostile,
    Allied,
}

/// Extract ship spawn data from a contract's paramOverrides.propertyOverrides.
fn extract_spawns_from_contract(
    db: &DataCoreDatabase,
    contract: &Instance,
    tag_names: &HashMap<String, String>,
    ship_index: &[ShipEntity],
    ship_tags: &HashSet<String>,
) -> HandlerSpawns {
    let mut result = HandlerSpawns::default();

    // Check paramOverrides.propertyOverrides
    if let Some(po) = contract.get_instance("paramOverrides") {
        extract_spawns_from_property_overrides(db, &po, tag_names, ship_index, ship_tags, &mut result);
    }

    // Also check template's contractProperties as fallback
    if result.hostile.is_empty() && result.allied.is_empty() {
        if let Some(template_val) = contract.get("template") {
            let template_inst = match &template_val {
                Value::Reference(Some(r)) => db.record(&r.guid).map(|rec| rec.as_instance()),
                Value::StrongPointer(Some(r)) | Value::ClassRef(r) => {
                    Some(db.instance(r.struct_index, r.instance_index))
                }
                Value::Class { struct_index, data } => {
                    Some(Instance::from_inline_data(db, *struct_index, data))
                }
                _ => None,
            };
            if let Some(ti) = template_inst {
                if let Some(cp) = ti.get_array("contractProperties") {
                    extract_spawns_from_mission_properties(
                        db, cp, tag_names, ship_index, ship_tags, &mut result,
                    );
                }
            }
        }
    }

    result
}

/// Extract ship spawn data from a handler's contractParams.
fn extract_spawns_from_params(
    db: &DataCoreDatabase,
    handler: &Instance,
    tag_names: &HashMap<String, String>,
    ship_index: &[ShipEntity],
    ship_tags: &HashSet<String>,
) -> HandlerSpawns {
    let mut result = HandlerSpawns::default();

    if let Some(cp) = handler.get_instance("contractParams") {
        extract_spawns_from_property_overrides(db, &cp, tag_names, ship_index, ship_tags, &mut result);
    }

    result
}

/// Extract spawns from a propertyOverrides array.
fn extract_spawns_from_property_overrides(
    db: &DataCoreDatabase,
    parent: &Instance,
    tag_names: &HashMap<String, String>,
    ship_index: &[ShipEntity],
    ship_tags: &HashSet<String>,
    result: &mut HandlerSpawns,
) {
    let Some(overrides) = parent.get_array("propertyOverrides") else {
        return;
    };
    for pv in overrides {
        let Some(prop) = to_instance(db, &pv) else {
            continue;
        };
        let var_name = prop.get_str("missionVariableName").unwrap_or("");
        if !var_name.contains("ShipSpawnDescriptions") && !var_name.contains("MissionTargets")
            && !var_name.contains("WaveShips")
        {
            continue;
        }

        let Some(role) = classify_spawn_variable(var_name) else {
            // If can't classify, treat as hostile (bounty targets, etc.)
            let Some(val) = prop.get_instance("value") else { continue };
            let groups = extract_spawn_groups_from_value(db, &val, tag_names, ship_index, ship_tags);
            if !groups.is_empty() {
                result.hostile.extend(groups);
            }
            continue;
        };

        let Some(val) = prop.get_instance("value") else {
            continue;
        };
        let groups = extract_spawn_groups_from_value(db, &val, tag_names, ship_index, ship_tags);
        match role {
            SpawnRole::Hostile => result.hostile.extend(groups),
            SpawnRole::Allied => result.allied.extend(groups),
        }
    }
}

/// Extract spawns from template contractProperties (MissionProperty array).
fn extract_spawns_from_mission_properties<'a>(
    db: &'a DataCoreDatabase,
    props: impl Iterator<Item = Value<'a>>,
    tag_names: &HashMap<String, String>,
    ship_index: &[ShipEntity],
    ship_tags: &HashSet<String>,
    result: &mut HandlerSpawns,
) {
    for cpv in props {
        let Some(prop) = to_instance(db, &cpv) else {
            continue;
        };
        let var_name = prop.get_str("missionVariableName").unwrap_or("");
        if !var_name.contains("ShipSpawnDescriptions") && !var_name.contains("MissionTargets")
            && !var_name.contains("WaveShips")
        {
            continue;
        }

        let Some(val) = prop.get_instance("value") else {
            continue;
        };
        let groups = extract_spawn_groups_from_value(db, &val, tag_names, ship_index, ship_tags);
        if groups.is_empty() {
            continue;
        }

        match classify_spawn_variable(var_name) {
            Some(SpawnRole::Hostile) => result.hostile.extend(groups),
            Some(SpawnRole::Allied) => result.allied.extend(groups),
            None => result.hostile.extend(groups),
        }
    }
}

// ── Blueprint pool resolution ───────────────────────────────────────────────

/// Build a map from BlueprintPoolRecord name to resolved item display names.
fn build_pool_items_map(
    db: &DataCoreDatabase,
    ini: &HashMap<String, String>,
) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();

    for record in db.records_by_type_containing("BlueprintPoolRecord") {
        let pool_name = record.name().unwrap_or("").to_string();
        if pool_name.is_empty() {
            continue;
        }

        let mut items = Vec::new();
        let inst = record.as_instance();

        let Some(rewards) = inst.get_array("blueprintRewards") else {
            continue;
        };

        for val in rewards {
            let Some(reward_inst) = to_instance(db, &val) else {
                continue;
            };

            let Some(Value::Reference(Some(bp_ref))) = reward_inst.get("blueprintRecord") else {
                continue;
            };
            let Some(bp_rec) = db.record(&bp_ref.guid) else {
                continue;
            };

            let bp_inst = bp_rec.as_instance();
            let display = resolve_blueprint_display_name(db, &bp_inst, ini);
            if !display.is_empty() {
                items.push(display);
            }
        }

        map.insert(pool_name, items);
    }

    map
}

/// Resolve a CraftingBlueprintRecord to its item's display name.
fn resolve_blueprint_display_name(
    db: &DataCoreDatabase,
    bp_inst: &Instance,
    ini: &HashMap<String, String>,
) -> String {
    let Some(blueprint) = bp_inst.get_instance("blueprint") else {
        return String::new();
    };
    let Some(psd) = blueprint.get_instance("processSpecificData") else {
        return String::new();
    };
    let Some(Value::Reference(Some(ec_ref))) = psd.get("entityClass") else {
        return String::new();
    };
    let Some(ec_rec) = db.record(&ec_ref.guid) else {
        return String::new();
    };

    get_entity_display_name(db, &ec_rec.as_instance(), ini)
}

/// Get the display name of an entity from its components.
fn get_entity_display_name(
    db: &DataCoreDatabase,
    inst: &Instance,
    ini: &HashMap<String, String>,
) -> String {
    let Some(comps) = inst.get_array("Components") else {
        return String::new();
    };

    for comp in comps {
        let Some(ci) = to_instance(db, &comp) else {
            continue;
        };

        if ci.type_name() == Some("SAttachableComponentParams") {
            if let Some(ad) = ci.get_instance("AttachDef") {
                if let Some(loc) = ad.get_instance("Localization") {
                    let key = loc
                        .get_str("Name")
                        .unwrap_or("")
                        .strip_prefix('@')
                        .unwrap_or("");
                    if let Some(display) = ini.get(key) {
                        return display.clone();
                    }
                }
            }
        }

        if ci.type_name() == Some("SCItemPurchasableParams") {
            let key = ci
                .get_str("displayName")
                .unwrap_or("")
                .strip_prefix('@')
                .unwrap_or("");
            if let Some(display) = ini.get(key) {
                return display.clone();
            }
        }
    }

    String::new()
}

/// Get the vehicle size from SAttachableComponentParams.AttachDef.Size.
fn get_entity_size(db: &DataCoreDatabase, inst: &Instance) -> i32 {
    let Some(comps) = inst.get_array("Components") else {
        return 0;
    };
    for comp in comps {
        let Some(ci) = to_instance(db, &comp) else {
            continue;
        };
        if ci.type_name() == Some("SAttachableComponentParams") {
            if let Some(ad) = ci.get_instance("AttachDef") {
                return ad.get_i32("Size").unwrap_or(0);
            }
        }
    }
    0
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
