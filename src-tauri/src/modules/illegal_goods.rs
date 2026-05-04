use std::collections::HashMap;

use anyhow::Result;
use svarog_datacore::{DataCoreDatabase, Instance, Value};

use crate::formatter_helpers::{Color, apply_color};
use crate::module::{ChoiceOption, Module, ModuleContext, ModuleOption, OptionKind, PatchOp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IllegalCategory {
    /// Controlled substance (drug) — wrapped in `Color::Underline`.
    Drug,
    /// Prohibited good (contraband) — wrapped in `Color::Highlight`.
    Contraband,
}

#[derive(Debug)]
struct IllegalGood {
    /// INI name key, resolved from ResourceType.displayName
    name_key: String,
    category: IllegalCategory,
    jurisdictions: Vec<String>,
}

pub struct IllegalGoods;

impl Module for IllegalGoods {
    fn id(&self) -> &str {
        "illegal_goods"
    }

    fn name(&self) -> &str {
        "Illegal Goods Markers"
    }

    fn description(&self) -> &str {
        "Mark illegal commodities (drugs, contraband) with [!] prefix"
    }

    fn default_enabled(&self) -> bool {
        true
    }

    fn needs_datacore(&self) -> bool {
        true
    }

    fn options(&self) -> Vec<ModuleOption> {
        vec![ModuleOption {
            id: "display".into(),
            label: "Display Style".into(),
            description: "How to mark illegal goods in the commodity name".into(),
            kind: OptionKind::Choice {
                choices: vec![
                    ChoiceOption {
                        value: "color_coded".into(),
                        label: "Emphasised (distinct style for drugs vs contraband)".into(),
                    },
                    ChoiceOption {
                        value: "simple".into(),
                        label: "Plain [!] prefix".into(),
                    },
                ],
            },
            default: "color_coded".into(),
        }]
    }

    fn generate_patches(&self, ctx: &ModuleContext) -> Result<Vec<(String, PatchOp)>> {
        let db = match ctx.db {
            Some(db) => db,
            None => return Ok(Vec::new()),
        };

        let display = ctx.config.get_str("display").unwrap_or("color_coded");

        // Collect all illegal resources from all jurisdictions
        let mut illegal: HashMap<String, IllegalGood> = HashMap::new();

        for record in db.records_by_type_containing("Jurisdiction") {
            let jurisdiction_name = record.name().unwrap_or("Unknown").to_string();

            // Collect prohibited resources
            if let Some(resources) = record.get_array("prohibitedResources") {
                collect_resource_refs(
                    db,
                    resources,
                    IllegalCategory::Contraband,
                    &jurisdiction_name,
                    &mut illegal,
                );
            }

            // Collect controlled substance classes (inline Class instances)
            if let Some(classes) = record.get_array("controlledSubstanceClasses") {
                for class_val in classes {
                    let class_inst = match &class_val {
                        Value::Class { struct_index, data } => {
                            Instance::from_inline_data(db, *struct_index, data)
                        }
                        Value::StrongPointer(Some(r)) | Value::ClassRef(r) => {
                            db.instance(r.struct_index, r.instance_index)
                        }
                        _ => continue,
                    };

                    if let Some(resources) = class_inst.get_array("resources") {
                        collect_resource_refs(
                            db,
                            resources,
                            IllegalCategory::Drug,
                            &jurisdiction_name,
                            &mut illegal,
                        );
                    }
                }
            }
        }

        // Generate patches
        let mut patches = Vec::new();

        for good in illegal.values() {
            if good.name_key.is_empty() || !ctx.ini.contains_key(&good.name_key) {
                continue;
            }

            // Name prefix
            let prefix = match display {
                "simple" => "[!] ".to_string(),
                _ => format!("{} ", apply_color(category_color(good.category), "[!]")),
            };
            patches.push((good.name_key.clone(), PatchOp::Prefix(prefix)));

            // Description suffix
            let desc_key = format!("{}_desc", good.name_key);
            if let Some(desc_value) = ctx.ini.get(&desc_key) {
                if !desc_value.is_empty() && !desc_value.contains("LOC_EMPTY") {
                    let category_label = match good.category {
                        IllegalCategory::Drug => "Controlled Substance",
                        IllegalCategory::Contraband => "Prohibited Good",
                    };
                    let jurisdictions_text = if good.jurisdictions.is_empty() {
                        "All jurisdictions".to_string()
                    } else {
                        good.jurisdictions.join(", ")
                    };
                    let suffix = format!(
                        "\\n\\n{}\\nIllegal in: {jurisdictions_text}",
                        apply_color(category_color(good.category), category_label)
                    );
                    patches.push((desc_key, PatchOp::Suffix(suffix)));
                }
            }
        }

        Ok(patches)
    }
}

fn category_color(category: IllegalCategory) -> Color {
    match category {
        IllegalCategory::Drug => Color::Underline,
        IllegalCategory::Contraband => Color::Highlight,
    }
}

/// Collect Reference values from an array, resolve each to a ResourceType record,
/// and extract the displayName as the INI key.
fn collect_resource_refs<'a>(
    db: &'a DataCoreDatabase,
    values: impl Iterator<Item = Value<'a>>,
    category: IllegalCategory,
    jurisdiction: &str,
    out: &mut HashMap<String, IllegalGood>,
) {
    for val in values {
        let (record_name, name_key) = match &val {
            Value::Reference(Some(r)) => {
                let rec = match db.record(&r.guid) {
                    Some(rec) => rec,
                    None => continue,
                };
                let record_name = rec.name().unwrap_or("").to_string();

                // ResourceType records have a displayName field with the INI key
                let inst = rec.as_instance();
                let display_name_raw = inst.get_str("displayName").unwrap_or("");
                let name_key = display_name_raw
                    .strip_prefix('@')
                    .unwrap_or(display_name_raw)
                    .to_string();

                (record_name, name_key)
            }
            _ => continue,
        };

        if record_name.is_empty() || name_key.is_empty() {
            continue;
        }

        let entry = out
            .entry(record_name)
            .or_insert_with(|| IllegalGood {
                name_key: name_key.clone(),
                category,
                jurisdictions: Vec::new(),
            });

        // Drug takes precedence over contraband
        if category == IllegalCategory::Drug {
            entry.category = IllegalCategory::Drug;
        }

        if !entry.jurisdictions.contains(&jurisdiction.to_string()) {
            entry.jurisdictions.push(jurisdiction.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drug_uses_underline_emphasis() {
        let prefix = format!("{} {}", apply_color(category_color(IllegalCategory::Drug), "[!]"), "WiDoW");
        assert_eq!(prefix, "<EM3>[!]</EM3> WiDoW");
    }

    #[test]
    fn contraband_uses_highlight_emphasis() {
        let prefix = format!(
            "{} {}",
            apply_color(category_color(IllegalCategory::Contraband), "[!]"),
            "Osoian Hides"
        );
        assert_eq!(prefix, "<EM4>[!]</EM4> Osoian Hides");
    }
}
