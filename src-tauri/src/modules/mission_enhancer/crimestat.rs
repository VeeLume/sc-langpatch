//! Crimestat-risk detection — temporary raw-svarog walk.
//!
//! sc-contracts v0.2.0 surfaces `NpcSlot.mission_allied_marker` as a
//! typed bool, but the matching `DontHarm*` bool-param overrides on the
//! contract's `paramOverrides` aren't yet modelled. Until they are
//! ([upstream feature request pending]), we walk the raw DCB to catch
//! the contract-level "don't harm allies" flag.
//!
//! Risk classification:
//! - `High` — `DontHarm*` set without any allied-marker NPC spawn:
//!   friendlies present but indistinguishable from foes.
//! - `Moderate` — `DontHarm*` set AND at least one NPC slot carries
//!   `mission_allied_marker = true`: friendlies have HUD markers.
//! - `None` — neither signal present.

use sc_contracts::{Encounter, Mission};
use sc_extract::Guid;
use svarog_datacore::{DataCoreDatabase, Instance, Value};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CrimestatRisk {
    #[default]
    None,
    /// Friendlies present WITH HUD markers (or allied ships in space).
    Moderate,
    /// Friendlies present WITHOUT HUD markers — cannot distinguish friend from foe.
    High,
}

/// Classify the crimestat risk for one mission.
///
/// Walks the contract's `paramOverrides.propertyOverrides` (and the
/// template's `contractProperties` as a fallback) for `DontHarm*` bool
/// flags. Any NPC encounter with `mission_allied_marker = true`
/// downgrades the risk from `High` to `Moderate`.
pub fn classify(db: &DataCoreDatabase, mission: &Mission) -> CrimestatRisk {
    let has_dont_harm = mission_has_dont_harm_flag(db, mission.id);
    if !has_dont_harm {
        return CrimestatRisk::None;
    }

    // Cheap pass through the typed encounter model — anything carrying
    // `mission_allied_marker = true` proves the friendly NPCs have HUD
    // markers, downgrading to Moderate.
    let has_allied_marker = mission.encounters.iter().any(|e| match e {
        Encounter::Npcs(npc) => npc
            .phases
            .iter()
            .flat_map(|p| p.slots.iter())
            .any(|slot| slot.mission_allied_marker),
        _ => false,
    });

    if has_allied_marker {
        CrimestatRisk::Moderate
    } else {
        CrimestatRisk::High
    }
}

fn mission_has_dont_harm_flag(db: &DataCoreDatabase, id: Guid) -> bool {
    let Some(record) = db.record(&id) else {
        return false;
    };
    let inst = record.as_instance();

    if let Some(po) = inst.get_instance("paramOverrides")
        && property_overrides_have_dont_harm(db, &po)
    {
        return true;
    }

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
            && properties_have_dont_harm(db, props)
        {
            return true;
        }
    }

    false
}

fn property_overrides_have_dont_harm(db: &DataCoreDatabase, parent: &Instance) -> bool {
    let Some(overrides) = parent.get_array("propertyOverrides") else {
        return false;
    };
    properties_have_dont_harm(db, overrides)
}

fn properties_have_dont_harm<'a>(
    db: &'a DataCoreDatabase,
    props: impl Iterator<Item = Value<'a>>,
) -> bool {
    for pv in props {
        let Some(prop) = to_instance(db, &pv) else {
            continue;
        };
        let var_name = prop.get_str("missionVariableName").unwrap_or("");
        if !is_dont_harm_var(var_name) {
            continue;
        }
        let Some(val) = prop.get_instance("value") else {
            continue;
        };
        // Two shapes: `value.options[].value == 1` or `value.value == 1`.
        if val.get_i32("value") == Some(1) {
            return true;
        }
        if let Some(opts) = val.get_array("options") {
            for ov in opts {
                if let Some(oi) = to_instance(db, &ov)
                    && oi.get_i32("value") == Some(1)
                {
                    return true;
                }
            }
        }
    }
    false
}

fn is_dont_harm_var(name: &str) -> bool {
    matches!(
        name,
        "DontHarmAllies_BP"
            | "BP_DontHarmAllies"
            | "DontHarmCivs_BP"
            | "BP_DontHarmCivs"
    )
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
