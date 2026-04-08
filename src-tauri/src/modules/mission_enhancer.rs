use std::collections::HashMap;

use anyhow::Result;
use svarog_datacore::{DataCoreDatabase, Instance, Value};

use crate::module::{Module, ModuleContext, ModuleOption, OptionKind, PatchOp};

// ── Extracted mission data ──────────────────────────────────────────────────

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
    can_reaccept_after_abandon: Option<bool>,
    abandoned_cooldown_min: Option<f32>,
    can_reaccept_after_fail: Option<bool>,
    personal_cooldown_min: Option<f32>,

    // Rewards
    scrip_amount: Option<i32>,
    rep_amount: Option<i32>,
    time_to_complete_hrs: Option<f32>,
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
                description: "Append cooldown, time limit, rep reward to descriptions".into(),
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

        // Phase 1: Extract all mission tiers from all contract generators
        let tiers = extract_all_tiers(db, ctx.ini);

        // Phase 2: Generate patches from extracted data
        let mut patches = Vec::new();

        for tier in &tiers {
            if tier.title_key.is_empty() || !ctx.ini.contains_key(&tier.title_key) {
                continue;
            }

            // Title suffixes
            let mut title_tags = Vec::new();

            if opt_blueprint_tag && !tier.blueprint_items.is_empty() {
                title_tags.push("<EM4>[BP]</EM4>");
            }
            if opt_solo_tag && tier.can_be_shared == Some(false) {
                title_tags.push("[Solo]");
            }
            if opt_once_tag && tier.once_only == Some(true) {
                title_tags.push("[Uniq]");
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

                if let Some(time) = tier.time_to_complete_hrs {
                    if time > 0.0 {
                        let hours = time as i32;
                        let minutes = ((time - hours as f32) * 60.0) as i32;
                        if hours > 0 && minutes > 0 {
                            info_lines.push(format!("Time Limit: {hours}h {minutes}min"));
                        } else if hours > 0 {
                            info_lines.push(format!("Time Limit: {hours}h"));
                        } else {
                            info_lines.push(format!("Time Limit: {minutes}min"));
                        }
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

                if let Some(cooldown) = tier.personal_cooldown_min {
                    if cooldown > 0.0 {
                        info_lines.push(format!("Cooldown: {}min", cooldown as i32));
                    }
                }

                if tier.can_reaccept_after_abandon == Some(false) {
                    info_lines.push("Cannot re-accept after abandoning".to_string());
                }

                if tier.can_reaccept_after_fail == Some(false) {
                    info_lines.push("Cannot re-accept after failing".to_string());
                }

                if !info_lines.is_empty() {
                    let info_str = info_lines
                        .iter()
                        .map(|l| format!("\\n{l}"))
                        .collect::<String>();
                    desc_parts.push(format!("<EM4>Mission Info</EM4>{info_str}"));
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

// ── Data extraction ─────────────────────────────────────────────────────────

/// Walk all ContractGenerator records and extract mission tier data.
fn extract_all_tiers(db: &DataCoreDatabase, ini: &HashMap<String, String>) -> Vec<MissionTier> {
    // Pre-build blueprint pool -> items map
    let pool_items = build_pool_items_map(db, ini);

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
            let avail = extract_availability(&handler);

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

                    extract_contract_tier(db, &contract, &avail, &pool_items, ini, &mut tiers);

                    // CareerContract has subContracts — walk those too
                    if let Some(subs) = contract.get_array("subContracts") {
                        for sv in subs {
                            let Some(sub) = to_instance(db, &sv) else {
                                continue;
                            };
                            extract_contract_tier(db, &sub, &avail, &pool_items, ini, &mut tiers);
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
    can_reaccept_after_abandon: Option<bool>,
    abandoned_cooldown_min: Option<f32>,
    can_reaccept_after_fail: Option<bool>,
    personal_cooldown_min: Option<f32>,
}

fn extract_availability(handler: &Instance) -> Availability {
    let Some(avail) = handler.get_instance("defaultAvailability") else {
        return Availability::default();
    };
    Availability {
        once_only: avail.get_bool("onceOnly"),
        can_reaccept_after_abandon: avail.get_bool("canReacceptAfterAbandoning"),
        abandoned_cooldown_min: avail.get_f32("abandonedCooldownTime"),
        can_reaccept_after_fail: avail.get_bool("canReacceptAfterFailing"),
        personal_cooldown_min: avail.get_f32("personalCooldownTime"),
    }
}

/// Extract one contract tier's data.
fn extract_contract_tier(
    db: &DataCoreDatabase,
    contract: &Instance,
    avail: &Availability,
    pool_items: &HashMap<String, Vec<String>>,
    ini: &HashMap<String, String>,
    out: &mut Vec<MissionTier>,
) {
    let mut tier = MissionTier {
        once_only: avail.once_only,
        can_reaccept_after_abandon: avail.can_reaccept_after_abandon,
        abandoned_cooldown_min: avail.abandoned_cooldown_min,
        can_reaccept_after_fail: avail.can_reaccept_after_fail,
        personal_cooldown_min: avail.personal_cooldown_min,
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
        tier.time_to_complete_hrs = results.get_f32("timeToComplete");
        extract_rewards(db, &results, &mut tier, ini);
    }

    // Resolve blueprint items from pool name
    if !tier.blueprint_pool_name.is_empty() {
        if let Some(items) = pool_items.get(&tier.blueprint_pool_name) {
            tier.blueprint_items = items.clone();
        }
    }

    out.push(tier);
}

/// Resolve canBeShared from the contract's template reference.
/// Returns None if the template can't be resolved.
///
/// The field is nested: template → contractClass → additionalParams → canBeShared
fn resolve_can_be_shared(db: &DataCoreDatabase, contract: &Instance) -> Option<bool> {
    let template_val = contract.get("template")?;

    let template_inst = match &template_val {
        Value::Reference(Some(r)) => {
            let rec = db.record(&r.guid)?;
            Some(rec.as_instance())
        }
        Value::StrongPointer(Some(r)) => Some(db.instance(r.struct_index, r.instance_index)),
        Value::Class(cr) => Some(Instance::from_class_ref(db, cr)),
        _ => None,
    }?;

    template_inst
        .get_instance("contractClass")?
        .get_instance("additionalParams")?
        .get_bool("canBeShared")
}

/// Extract Title and Description from string param overrides.
///
/// Checks both `paramOverrides.stringParamOverrides` (CareerContract)
/// and direct `stringParamOverrides` on the contract itself.
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
    // Try paramOverrides.stringParamOverrides (standard path)
    if let Some(po) = contract.get_instance("paramOverrides") {
        extract_string_params(db, &po, tier);
    }

    // Also try direct stringParamOverrides on the contract
    if tier.title_key.is_empty() {
        extract_string_params(db, contract, tier);
    }

    // Try contractParams.stringParamOverrides (handler-level params)
    if tier.title_key.is_empty() {
        if let Some(cp) = contract.get_instance("contractParams") {
            extract_string_params(db, &cp, tier);
        }
    }
}

/// Extract rewards from contract results array.
fn extract_rewards(
    db: &DataCoreDatabase,
    results: &Instance,
    tier: &mut MissionTier,
    ini: &HashMap<String, String>,
) {
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
                // Scrip reward
                tier.scrip_amount = result_inst.get_i32("amount");
            }
            "ContractResult_LegacyReputation" => {
                if let Some(rep) = result_inst.get_instance("contractResultReputationAmounts") {
                    // Resolve reward reference to get the actual XP amount
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

        // Try SAttachableComponentParams -> AttachDef -> Localization -> Name
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

        // Fallback: SCItemPurchasableParams -> displayName
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

// ── Helpers ─────────────────────────────────────────────────────────────────

fn to_instance<'a>(db: &'a DataCoreDatabase, val: &Value<'a>) -> Option<Instance<'a>> {
    match val {
        Value::Class(cr) => Some(Instance::from_class_ref(db, cr)),
        Value::StrongPointer(Some(r)) => Some(db.instance(r.struct_index, r.instance_index)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
