use std::collections::HashSet;

use anyhow::Result;
use sc_contracts::{
    find_bp_conflicts, BlueprintItem, BlueprintReward, Contract, ContractIndex, EncounterGroup,
    LocalityRegistry, SpawnContext,
};
use sc_extract::Guid;
use svarog_datacore::{DataCoreDatabase, Instance, Value};

use crate::module::{ChoiceOption, Module, ModuleContext, ModuleOption, OptionKind, PatchOp};

// ── Crimestat risk (sc-contracts doesn't model this yet) ────────────────────
//
// TODO(sc-holotable): file feature request for `Contract.crimestat_risk` —
// folds the DontHarm* properties + missionAlliedMarker spawn detection into
// a typed enum on the model.

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum CrimestatRisk {
    #[default]
    None,
    /// Friendlies present WITH HUD markers (or allied ships in space).
    Moderate,
    /// Friendlies present WITHOUT HUD markers — cannot distinguish friend from foe.
    High,
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

    fn needs_locale(&self) -> bool {
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
            ModuleOption {
                id: "cargo_info".into(),
                label: "Cargo Info".into(),
                description: "Show cargo descriptors (Full/Half/Scraps, HighValue/LowValue) on hostile ships".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
            ModuleOption {
                id: "region_info".into(),
                label: "Region Info".into(),
                description: "Append the region / body where the mission is offered".into(),
                kind: OptionKind::Bool,
                default: "true".into(),
            },
        ]
    }

    fn generate_patches(&self, ctx: &ModuleContext) -> Result<Vec<(String, PatchOp)>> {
        let (Some(datacore), Some(locale), Some(db)) =
            (ctx.datacore, ctx.locale, ctx.db)
        else {
            return Ok(Vec::new());
        };

        let opt_blueprint_tag = ctx.config.get_bool("blueprint_tag").unwrap_or(true);
        let opt_blueprint_list = ctx.config.get_bool("blueprint_list").unwrap_or(true);
        let opt_solo_tag = ctx.config.get_bool("solo_tag").unwrap_or(true);
        let opt_once_tag = ctx.config.get_bool("once_tag").unwrap_or(true);
        let opt_mission_info = ctx.config.get_bool("mission_info").unwrap_or(true);
        let opt_ship_encounters = ctx.config.get_bool("ship_encounters").unwrap_or(true);
        let opt_cargo_info = ctx.config.get_bool("cargo_info").unwrap_or(true);
        let opt_region_info = ctx.config.get_bool("region_info").unwrap_or(true);
        let opt_crimestat = ctx.config.get_str("crimestat_tag").unwrap_or("colored");

        let index = ContractIndex::build(datacore, locale);

        // Contracts whose blueprint pool varies across same-title siblings
        // (the `[BP]*` "results may vary" case).
        let mixed_bp_ids: HashSet<Guid> = find_bp_conflicts(&index.contracts)
            .into_iter()
            .filter(|g| g.has_mixed_presence)
            .flat_map(|g| g.members.into_iter().map(|m| m.contract_id))
            .collect();

        // Fast `Guid → &Contract` lookup for walking title_siblings.
        let by_id: std::collections::HashMap<Guid, &Contract> =
            index.contracts.iter().map(|c| (c.id, c)).collect();

        let mut patches = Vec::new();
        let mut hit = 0usize;
        let mut no_keys = 0usize;
        let mut key_missing_in_ini = 0usize;

        for contract in &index.contracts {
            let Some(title_key_raw) = contract.title_key.as_ref() else {
                no_keys += 1;
                continue;
            };
            let title_key = title_key_raw.stripped().to_string();
            if title_key.is_empty() {
                no_keys += 1;
                continue;
            }
            if !ctx.ini.contains_key(&title_key) {
                key_missing_in_ini += 1;
                continue;
            }
            let desc_key = contract
                .description_key
                .as_ref()
                .map(|k| k.stripped().to_string())
                .unwrap_or_default();
            hit += 1;

            // Crimestat — sc-contracts doesn't expose this; raw walk.
            let crimestat = crimestat_for_contract(db, contract.id);

            // ── Title tags ────────────────────────────────────────────────
            let mut title_tags: Vec<&str> = Vec::new();

            if opt_blueprint_tag {
                if mixed_bp_ids.contains(&contract.id) {
                    title_tags.push("<EM4>[BP]*</EM4>");
                } else if contract.blueprint_reward.is_some() {
                    title_tags.push("<EM4>[BP]</EM4>");
                }
            }
            if opt_solo_tag && !contract.shareable {
                title_tags.push("[Solo]");
            }
            if opt_once_tag && contract.availability.once_only {
                title_tags.push("[Uniq]");
            }
            if opt_crimestat != "off" && crimestat != CrimestatRisk::None {
                match (opt_crimestat, crimestat) {
                    ("colored", CrimestatRisk::High) => title_tags.push("<EM3>[CS Risk!]</EM3>"),
                    ("colored", CrimestatRisk::Moderate) => title_tags.push("<EM4>[CS Risk]</EM4>"),
                    _ => title_tags.push("[CS Risk]"),
                }
            }

            if !title_tags.is_empty() {
                let suffix = format!(" {}", title_tags.join(" "));
                patches.push((title_key.clone(), PatchOp::Suffix(suffix)));
            }

            // ── Description ───────────────────────────────────────────────
            // Fall back to a mechanical title→desc transform when the
            // contract has no description_key — rare, but matches what
            // the legacy walker used to do.
            let desc_key = if !desc_key.is_empty() {
                desc_key
            } else {
                title_key.replace("title", "desc").replace("Title", "Desc")
            };
            if !ctx.ini.contains_key(&desc_key) {
                continue;
            }

            let mut desc_parts: Vec<String> = Vec::new();

            // Blueprint list — per-region breakdown for siblings with
            // divergent pools, flat list otherwise.
            if opt_blueprint_list
                && let Some(bp) = &contract.blueprint_reward
            {
                desc_parts.push(format_blueprints(contract, bp, &by_id, &index.localities));
            }

            // Mission info (cooldown, rep, scrip)
            if opt_mission_info {
                if let Some(info) = format_mission_info(contract) {
                    desc_parts.push(info);
                }
            }

            // Ship encounters
            if opt_ship_encounters && !contract.encounters.is_empty() {
                let encounters = format_encounters(&contract.encounters, opt_cargo_info);
                if !encounters.is_empty() {
                    desc_parts.push(format!("<EM4>Ship Encounters</EM4>{encounters}"));
                }
            }

            // Region
            if opt_region_info
                && let Some(region) = format_region(contract, &index.localities)
            {
                desc_parts.push(region);
            }

            if !desc_parts.is_empty() {
                let suffix = format!("\\n\\n{}", desc_parts.join("\\n\\n"));
                patches.push((desc_key, PatchOp::Suffix(suffix)));
            }
        }

        eprintln!(
            "  [MissionEnhancer] {hit} of {} contracts patched \
             (no_keys={no_keys}, key_missing_in_ini={key_missing_in_ini})",
            index.contracts.len()
        );
        Ok(patches)
    }
}

// ── Description blocks ──────────────────────────────────────────────────────

/// Format the blueprint reward block. When the contract shares its
/// title/description with siblings that carry different pools, produce
/// a per-region breakdown driven by each sibling's `mission_span`;
/// otherwise emit the flat pool list.
fn format_blueprints(
    contract: &Contract,
    bp: &BlueprintReward,
    by_id: &std::collections::HashMap<Guid, &Contract>,
    localities: &LocalityRegistry,
) -> String {
    // Collect `(region_label, items)` for self + siblings, deduped by
    // pool guid so two siblings with the same pool and identical region
    // don't duplicate.
    if !contract.title_siblings.is_empty() {
        let mut entries: Vec<(String, &Vec<BlueprintItem>, f32, Guid)> = Vec::new();
        let self_region = region_label_for(contract, localities);
        if let Some(r) = &contract.blueprint_reward {
            entries.push((self_region, &r.items, r.chance, r.pool_guid));
        }
        for sib_id in &contract.title_siblings {
            let Some(sib) = by_id.get(sib_id) else { continue };
            let Some(sr) = &sib.blueprint_reward else { continue };
            let region = region_label_for(sib, localities);
            entries.push((region, &sr.items, sr.chance, sr.pool_guid));
        }

        // Distinct pools across the group. If only one, fall through to flat.
        let distinct_pools: HashSet<Guid> = entries.iter().map(|(_, _, _, g)| *g).collect();
        if distinct_pools.len() >= 2 {
            return format_blueprints_per_region(&entries);
        }
    }

    format_blueprints_flat(bp)
}

fn format_blueprints_flat(bp: &BlueprintReward) -> String {
    let items_str = bp
        .items
        .iter()
        .map(|i: &BlueprintItem| format!("\\n- {}", i.display_name))
        .collect::<String>();
    let chance_str = if bp.chance < 1.0 {
        format!(" ({}% chance)", (bp.chance * 100.0) as i32)
    } else {
        String::new()
    };
    format!("<EM4>Potential Blueprints</EM4>{chance_str}{items_str}")
}

/// Render one line per distinct region, each listing the items that
/// region's sibling awards. Merges siblings that share the same
/// `region_label`, deduplicating items by `display_name`.
fn format_blueprints_per_region(
    entries: &[(String, &Vec<BlueprintItem>, f32, Guid)],
) -> String {
    // Group by region label, preserving first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut grouped: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut chances: std::collections::HashMap<String, Vec<f32>> =
        std::collections::HashMap::new();

    for (region, items, chance, _) in entries {
        let key = if region.is_empty() { "Unspecified".to_string() } else { region.clone() };
        if !grouped.contains_key(&key) {
            order.push(key.clone());
        }
        let list = grouped.entry(key.clone()).or_default();
        for it in items.iter() {
            if it.display_name.is_empty() {
                continue;
            }
            if !list.iter().any(|n| n == &it.display_name) {
                list.push(it.display_name.clone());
            }
        }
        chances.entry(key).or_default().push(*chance);
    }

    // Common-chance header if every group's chances agree on a single <1.0 value.
    let mut all_chances: Vec<f32> = chances.values().flatten().copied().collect();
    all_chances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    all_chances.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
    let header_chance = if all_chances.len() == 1 && all_chances[0] < 1.0 {
        format!(" ({}% chance)", (all_chances[0] * 100.0) as i32)
    } else {
        String::new()
    };

    let mut lines = String::new();
    for region in &order {
        let items = &grouped[region];
        if items.is_empty() {
            continue;
        }
        let chance_suffix = if header_chance.is_empty() {
            // Per-region chance (when groups disagree).
            let cs = &chances[region];
            if cs.len() == 1 && cs[0] < 1.0 {
                format!(" ({}%)", (cs[0] * 100.0) as i32)
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        lines.push_str(&format!(
            "\\n<EM4>{region}{chance_suffix}:</EM4> {}",
            items.join(", ")
        ));
    }

    format!("<EM4>Potential Blueprints (varies by region)</EM4>{header_chance}{lines}")
}

/// Build the short region label for a single contract from its
/// `mission_span`. Joins multiple localities with `" / "`. Empty when
/// no locality resolves.
fn region_label_for(contract: &Contract, localities: &LocalityRegistry) -> String {
    let mut parts: Vec<String> = Vec::new();
    for guid in &contract.mission_span {
        let Some(view) = localities.get(guid) else { continue };
        if !view.region_label.is_empty() && !parts.iter().any(|p| p == &view.region_label) {
            parts.push(view.region_label.clone());
        }
    }
    parts.join(" / ")
}

/// Standalone region section appended to the description when
/// `mission_span` resolves. Returns `None` for contracts with no
/// locality prereqs.
fn format_region(contract: &Contract, localities: &LocalityRegistry) -> Option<String> {
    let label = region_label_for(contract, localities);
    if label.is_empty() {
        return None;
    }
    Some(format!("<EM4>Region</EM4>\\n{label}"))
}

fn format_mission_info(contract: &Contract) -> Option<String> {
    let mut info_lines: Vec<String> = Vec::new();

    // Cooldown — show personal completion CD; if abandon meaningfully differs, include it.
    if contract.availability.has_personal_cooldown {
        let personal_min = contract
            .availability
            .cooldowns
            .completion
            .as_ref()
            .map(|d| d.mean_seconds / 60.0)
            .unwrap_or(0.0);
        let abandon_min = contract
            .availability
            .cooldowns
            .abandon
            .as_ref()
            .map(|d| d.mean_seconds / 60.0)
            .unwrap_or(0.0);

        if personal_min > 0.0
            && abandon_min > 0.0
            && (abandon_min - personal_min).abs() > 0.5
        {
            info_lines.push(format!(
                "Cooldown: {}min (abandon: {}min)",
                personal_min as i32, abandon_min as i32
            ));
        } else if personal_min > 0.0 {
            info_lines.push(format!("Cooldown: {}min", personal_min as i32));
        }
    }

    // Reputation — sum amounts across rep rewards (legacy code only used the
    // first; sum is a strict superset and matches what players see in-game).
    let rep_total: i32 = contract
        .reward_rep
        .iter()
        .filter_map(|r| r.amount)
        .filter(|a| *a > 0)
        .sum();
    if rep_total > 0 {
        info_lines.push(format!("Rep: {rep_total} XP"));
    }

    // Scrip — sum across all scrip rewards (MG + Council).
    let scrip_total: i32 = contract
        .reward_scrip
        .iter()
        .map(|s| s.amount)
        .filter(|a| *a > 0)
        .sum();
    if scrip_total > 0 {
        info_lines.push(format!("Scrip: {scrip_total}"));
    }

    if info_lines.is_empty() {
        return None;
    }
    let info_str = info_lines
        .iter()
        .map(|l| format!("\\n{l}"))
        .collect::<String>();
    Some(format!("<EM4>Mission Info</EM4>{info_str}"))
}

// ── Ship encounter formatting ───────────────────────────────────────────────

fn format_encounters(groups: &[EncounterGroup], include_cargo: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    let mut min_skill: Option<u32> = None;
    let mut max_skill: Option<u32> = None;

    // Skill range across every slot.
    for g in groups {
        for w in &g.waves {
            for slot in &w.slots {
                if let Some(s) = slot.context.ai_skill {
                    min_skill = Some(min_skill.map_or(s, |m| m.min(s)));
                    max_skill = Some(max_skill.map_or(s, |m| m.max(s)));
                }
            }
        }
    }

    for g in groups {
        let role = classify_role(&g.variable_name);
        let cargo_suffix = if include_cargo && role == "Hostile" {
            format_cargo_suffix(g)
        } else {
            String::new()
        };
        let line = format_group(g, role, &cargo_suffix);
        if !line.is_empty() && seen.insert(line.clone()) {
            lines.push(line);
        }
    }

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

fn format_group(group: &EncounterGroup, default_role: &str, cargo_suffix: &str) -> String {
    // Sum concurrent across every slot of every wave.
    let total_count: i32 = group
        .waves
        .iter()
        .flat_map(|w| w.slots.iter())
        .map(|s| s.concurrent)
        .sum();

    // Collect distinct (display_name, size) across every wave/slot/candidate.
    let mut all_ships: Vec<(String, i32)> = Vec::new();
    let mut has_empty_slot = false;
    let mut tag_only_class: Option<&str> = None;

    for wave in &group.waves {
        for slot in &wave.slots {
            if slot.candidates.is_empty() {
                has_empty_slot = true;
                tag_only_class = tag_only_class.or_else(|| classify_tag_only(&slot.context));
                continue;
            }
            for c in &slot.candidates {
                if c.display_name.is_empty() {
                    continue;
                }
                let short = shorten_ship_name(&c.display_name);
                if !all_ships.iter().any(|(n, _)| n == &short) {
                    all_ships.push((short, c.size));
                }
            }
        }
    }

    all_ships.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let short_names: Vec<String> = all_ships.iter().map(|(n, _)| n.clone()).collect();
    let collapsed = collapse_variants(&short_names);

    // Build the label — clean up internal names (e.g. "ShipToDefend" → "Escort").
    let label = if !group.variable_name.is_empty() {
        let clean = group
            .variable_name
            .replace("ShipToDefend", "Escort")
            .replace('_', " ");
        format!("{default_role} {clean}")
    } else {
        default_role.to_string()
    };

    // Build ship list (or fall back to a tag-only label like "transport/cargo").
    let ships_str = if !collapsed.is_empty() {
        collapsed.join(", ")
    } else if has_empty_slot {
        tag_only_class.unwrap_or("ships").to_string()
    } else {
        return String::new();
    };

    let count_str = if total_count > 1 {
        format!(" ~{total_count}x")
    } else {
        String::new()
    };
    let sep = if collapsed.len() > 5 { "\\n" } else { " " };
    format!("<EM4>{label}:{count_str}</EM4>{sep}{ships_str}{cargo_suffix}")
}

/// Aggregate cargo descriptors (`Full Cargo`, `Scraps Cargo`, …) and
/// value-tier traits (`HighValue` / `LowValue` / `Mixed`) across every
/// slot in an encounter group. Empty string when no slot carries
/// cargo/value tags.
fn format_cargo_suffix(group: &EncounterGroup) -> String {
    let mut cargo: Vec<String> = Vec::new();
    let mut values: Vec<String> = Vec::new();
    for wave in &group.waves {
        for slot in &wave.slots {
            for c in &slot.context.cargo {
                if !cargo.iter().any(|x| x == c) {
                    cargo.push(c.clone());
                }
            }
            for t in &slot.context.ai_traits {
                if matches!(t.as_str(), "HighValue" | "LowValue" | "Mixed")
                    && !values.iter().any(|x| x == t)
                {
                    values.push(t.clone());
                }
            }
        }
    }
    if cargo.is_empty() && values.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    parts.extend(cargo);
    parts.extend(values);
    format!(" \u{00B7} {}", parts.join(", "))
}

/// Hostile vs allied based on the contract's mission-variable name.
///
/// TODO(sc-holotable): an `EncounterRole` enum on `EncounterGroup` would
/// remove this string sniff entirely (workspace rule §5).
fn classify_role(var_name: &str) -> &'static str {
    let lower = var_name.to_lowercase();
    if lower.contains("hostile")
        || lower.contains("missiontargets")
        || lower.contains("waveships")
    {
        "Hostile"
    } else if lower.contains("attacked")
        || lower.contains("allied")
        || lower.contains("escort")
        || lower.contains("defend")
    {
        "Allied"
    } else {
        // Bounty targets, generic spawn vars — treat as hostile.
        "Hostile"
    }
}

/// For slots with no resolved ship candidates, classify into a coarse label
/// using the spawn context's mission tags (`DefendShip`, `LargeCombatShip`, …).
fn classify_tag_only(ctx: &SpawnContext) -> Option<&'static str> {
    for t in &ctx.mission_tags {
        match t.as_str() {
            "DefendShip" => return Some("transport/cargo"),
            "CombatShip" | "LargeCombatShip" => return Some("capital"),
            _ => {}
        }
    }
    Some("ships")
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
/// `["Avenger Stalker", "Avenger Warlock"]` → `["Avenger"]`.
/// Single variant keeps full name.
fn collapse_variants(names: &[String]) -> Vec<String> {
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
    for name in names {
        let base = name.split_whitespace().next().unwrap_or(name);
        if let Some(g) = groups.iter_mut().find(|(b, _)| b == base) {
            g.1.push(name);
        } else {
            groups.push((base.to_string(), vec![name]));
        }
    }
    groups
        .into_iter()
        .map(|(base, variants)| {
            if variants.len() == 1 {
                variants[0].to_string()
            } else {
                base
            }
        })
        .collect()
}

// ── Crimestat helper (raw svarog) ───────────────────────────────────────────
//
// TODO(sc-holotable): expose `Contract.crimestat_risk: Option<CrimestatRisk>` —
// the DontHarm* property + missionAlliedMarker spawn detection is part of
// the contract surface but not yet modelled.

fn crimestat_for_contract(db: &DataCoreDatabase, id: Guid) -> CrimestatRisk {
    let Some(record) = db.record(&id) else {
        return CrimestatRisk::None;
    };
    let inst = record.as_instance();

    // contract.paramOverrides.propertyOverrides
    if let Some(po) = inst.get_instance("paramOverrides") {
        let r = detect_risk_in_overrides(db, &po);
        if r != CrimestatRisk::None {
            return r;
        }
    }

    // template.contractProperties (fallback)
    if let Some(template_val) = inst.get("template") {
        let template = match &template_val {
            Value::Reference(Some(r)) => db.record(&r.guid).map(|r| r.as_instance()),
            Value::StrongPointer(Some(r)) | Value::ClassRef(r) => {
                Some(db.instance(r.struct_index, r.instance_index))
            }
            Value::Class { struct_index, data } => {
                Some(Instance::from_inline_data(db, *struct_index, data))
            }
            _ => None,
        };
        if let Some(t) = template
            && let Some(props) = t.get_array("contractProperties")
        {
            let r = detect_risk_in_properties(db, props);
            if r != CrimestatRisk::None {
                return r;
            }
        }
    }

    CrimestatRisk::None
}

fn detect_risk_in_overrides(db: &DataCoreDatabase, parent: &Instance) -> CrimestatRisk {
    let Some(overrides) = parent.get_array("propertyOverrides") else {
        return CrimestatRisk::None;
    };
    detect_risk_in_properties(db, overrides)
}

fn detect_risk_in_properties<'a>(
    db: &'a DataCoreDatabase,
    props: impl Iterator<Item = Value<'a>>,
) -> CrimestatRisk {
    let mut has_dont_harm = false;
    let mut has_allied_marker = false;

    for pv in props {
        let Some(prop) = to_instance(db, &pv) else {
            continue;
        };
        let var_name = prop.get_str("missionVariableName").unwrap_or("");

        if matches!(
            var_name,
            "DontHarmAllies_BP"
                | "BP_DontHarmAllies"
                | "DontHarmCivs_BP"
                | "BP_DontHarmCivs"
        ) && let Some(val) = prop.get_instance("value")
        {
            if let Some(opts) = val.get_array("options") {
                for ov in opts {
                    if let Some(oi) = to_instance(db, &ov)
                        && oi.get_i32("value") == Some(1)
                    {
                        has_dont_harm = true;
                    }
                }
            }
            if val.get_i32("value") == Some(1) {
                has_dont_harm = true;
            }
        }

        if has_npc_allied_marker(db, &prop) {
            has_allied_marker = true;
        }
    }

    if has_dont_harm && !has_allied_marker {
        CrimestatRisk::High
    } else if has_allied_marker {
        CrimestatRisk::Moderate
    } else {
        CrimestatRisk::None
    }
}

fn has_npc_allied_marker(db: &DataCoreDatabase, prop: &Instance) -> bool {
    let Some(val) = prop.get_instance("value") else {
        return false;
    };
    let val_type = val.type_name().unwrap_or("");
    if !val_type.contains("NPC") {
        return false;
    }
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
            if let Some(opt) = to_instance(db, &ov)
                && let Some(auto) = opt.get_instance("autoSpawnSettings")
                && auto.get_bool("missionAlliedMarker") == Some(true)
            {
                return true;
            }
        }
    }
    false
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
