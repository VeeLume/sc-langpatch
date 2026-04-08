use anyhow::Result;
use svarog_datacore::Value;

use crate::module::{ChoiceOption, Module, ModuleContext, ModuleOption, OptionKind, PatchOp};

/// Component types we want to patch with grade/class info.
const COMPONENT_TYPES: &[&str] = &["Cooler", "PowerPlant", "Radar", "Shield", "QuantumDrive"];

/// Map numeric grade (from DCB) to letter.
fn grade_letter(grade: i32) -> &'static str {
    match grade {
        1 => "A",
        2 => "B",
        3 => "C",
        _ => "D",
    }
}

/// Parse class from the item description string (e.g. "Class: Military").
fn parse_class(description: &str) -> Option<&str> {
    for segment in description.split("\\n") {
        let trimmed = segment.trim();
        if let Some(class) = trimmed.strip_prefix("Class: ") {
            return Some(class.trim());
        }
    }
    None
}

pub struct ComponentGrades;

impl Module for ComponentGrades {
    fn id(&self) -> &str {
        "component_grades_derived"
    }

    fn name(&self) -> &str {
        "Component Grades (Derived)"
    }

    fn description(&self) -> &str {
        "Auto-derive class and grade info from game data (e.g. 'Bracer Military C')"
    }

    fn default_enabled(&self) -> bool {
        true
    }

    fn needs_datacore(&self) -> bool {
        true
    }

    fn options(&self) -> Vec<ModuleOption> {
        vec![ModuleOption {
            id: "format".into(),
            label: "Name Format".into(),
            description: "How to format the component name".into(),
            kind: OptionKind::Choice {
                choices: vec![
                    ChoiceOption {
                        value: "name_class_grade".into(),
                        label: "Name Class Grade (e.g. Bracer Military C)".into(),
                    },
                    ChoiceOption {
                        value: "compact_prefix".into(),
                        label: "Compact Prefix (e.g. M1C Bracer)".into(),
                    },
                ],
            },
            default: "name_class_grade".into(),
        }]
    }

    fn generate_patches(&self, ctx: &ModuleContext) -> Result<Vec<(String, PatchOp)>> {
        let db = match ctx.db {
            Some(db) => db,
            None => return Ok(Vec::new()),
        };

        let format = ctx.config.get_str("format").unwrap_or("name_class_grade");
        let mut patches = Vec::new();

        for record in db.records_by_type_containing("EntityClassDefinition") {
            let components = match record.get_array("Components") {
                Some(c) => c,
                None => continue,
            };

            for component in components {
                let inst = match &component {
                    Value::StrongPointer(Some(r)) => db.instance(r.struct_index, r.instance_index),
                    _ => continue,
                };

                if inst.type_name() != Some("SAttachableComponentParams") {
                    continue;
                }

                let attach_def = match inst.get_instance("AttachDef") {
                    Some(a) => a,
                    None => continue,
                };

                let item_type = attach_def.get_str("Type").unwrap_or("");
                if !COMPONENT_TYPES.contains(&item_type) {
                    continue;
                }

                let grade = attach_def.get_i32("Grade").unwrap_or(0);

                let loc = match attach_def.get_instance("Localization") {
                    Some(l) => l,
                    None => continue,
                };

                let name_key = loc
                    .get_str("Name")
                    .unwrap_or("")
                    .strip_prefix('@')
                    .unwrap_or("");
                let desc_key = loc
                    .get_str("Description")
                    .unwrap_or("")
                    .strip_prefix('@')
                    .unwrap_or("");

                if name_key.is_empty() {
                    continue;
                }

                // Only patch keys that exist in the INI
                let display_name = match ctx.ini.get(name_key) {
                    Some(n) => n.clone(),
                    None => continue,
                };

                let class = ctx
                    .ini
                    .get(desc_key)
                    .and_then(|desc| parse_class(desc))
                    .unwrap_or("Unknown");

                let grade_str = grade_letter(grade);

                let new_value = match format {
                    "compact_prefix" => {
                        let class_code = match class {
                            "Military" => "M",
                            "Civilian" => "C",
                            "Industrial" => "I",
                            "Stealth" => "S",
                            "Competition" => "X",
                            _ => "?",
                        };
                        let size = attach_def.get_i32("Size").unwrap_or(0);
                        format!("{class_code}{size}{grade_str} {display_name}")
                    }
                    _ => {
                        format!("{display_name} {class} {grade_str}")
                    }
                };

                patches.push((name_key.to_string(), PatchOp::Replace(new_value)));
            }
        }

        Ok(patches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_mapping() {
        assert_eq!(grade_letter(1), "A");
        assert_eq!(grade_letter(2), "B");
        assert_eq!(grade_letter(3), "C");
        assert_eq!(grade_letter(4), "D");
        assert_eq!(grade_letter(0), "D");
        assert_eq!(grade_letter(99), "D");
    }

    #[test]
    fn parse_class_from_description() {
        let desc = r"Item Type: Cooler\nManufacturer: Aegis Dynamics \nSize: 1\nGrade: C\nClass: Military\n\nSome description text.";
        assert_eq!(parse_class(desc), Some("Military"));

        let desc2 = r"Item Type: Cooler\nClass: Civilian\nGrade: B";
        assert_eq!(parse_class(desc2), Some("Civilian"));
    }

    #[test]
    fn parse_class_missing() {
        let desc = r"Item Type: Cooler\nGrade: B";
        assert_eq!(parse_class(desc), None);
    }

    #[test]
    fn parse_class_stealth() {
        let desc = r"Class: Stealth\nGrade: A";
        assert_eq!(parse_class(desc), Some("Stealth"));
    }
}
