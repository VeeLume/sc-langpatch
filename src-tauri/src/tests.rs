#[cfg(test)]
mod patch_application {
    use std::collections::HashMap;

    use crate::merge;
    use crate::module::PatchOp;
    use crate::test_helpers::fixtures::*;

    #[test]
    fn replace_changes_value() {
        let ini = make_ini(&[("key_a", "Original"), ("key_b", "Untouched")]);
        let patched = apply(&ini, &[("key_a", PatchOp::Replace("New Value".into()))]);

        assert_ini_value(&patched, "key_a", "New Value");
        assert_ini_unchanged(&patched, "key_b", "Untouched");
    }

    #[test]
    fn prefix_prepends_to_value() {
        let ini = make_ini(&[("drug", "Altruciatoxin")]);
        let patched = apply(&ini, &[("drug", PatchOp::Prefix("[!] ".into()))]);

        assert_ini_value(&patched, "drug", "[!] Altruciatoxin");
    }

    #[test]
    fn suffix_appends_to_value() {
        let ini = make_ini(&[("title", "Mining Contract")]);
        let patched = apply(&ini, &[("title", PatchOp::Suffix(" [BP]".into()))]);

        assert_ini_value(&patched, "title", "Mining Contract [BP]");
    }

    #[test]
    fn no_data_loss_all_lines_preserved() {
        let ini = make_ini(&[
            ("key_a", "Value A"),
            ("key_b", "Value B"),
            ("key_c", "Value C"),
            ("key_d", "Value D"),
        ]);
        let patched = apply(&ini, &[("key_b", PatchOp::Replace("Changed".into()))]);

        // All 4 lines must be present
        assert_eq!(line_count(&patched), 4);
        assert_ini_unchanged(&patched, "key_a", "Value A");
        assert_ini_value(&patched, "key_b", "Changed");
        assert_ini_unchanged(&patched, "key_c", "Value C");
        assert_ini_unchanged(&patched, "key_d", "Value D");
    }

    #[test]
    fn unmatched_patch_does_not_corrupt() {
        let ini = make_ini(&[("exists", "Value")]);
        let patched = apply(
            &ini,
            &[("nonexistent", PatchOp::Replace("Ghost".into()))],
        );

        assert_ini_unchanged(&patched, "exists", "Value");
        // The nonexistent key should NOT appear in output
        assert!(
            !patched.contains("nonexistent="),
            "Unmatched patch key must not be inserted"
        );
    }

    #[test]
    fn values_with_equals_signs_preserved() {
        // INI values can contain '=' (e.g. base64 or formulas)
        let ini = make_ini(&[("formula", "a=b+c=d")]);
        let patched = apply(&ini, &[]);

        assert_ini_value(&patched, "formula", "a=b+c=d");
    }

    #[test]
    fn suffix_on_value_with_markup() {
        let ini = make_ini(&[(
            "desc",
            "Contract details\\nLocation: Pyro",
        )]);
        let patched = apply(
            &ini,
            &[("desc", PatchOp::Suffix("\\n\\nBlueprints:\\n- Item A".into()))],
        );

        assert_ini_value(
            &patched,
            "desc",
            "Contract details\\nLocation: Pyro\\n\\nBlueprints:\\n- Item A",
        );
    }

    #[test]
    fn empty_value_replace() {
        let ini = make_ini(&[("empty", "")]);
        let patched = apply(&ini, &[("empty", PatchOp::Replace("Now has value".into()))]);

        assert_ini_value(&patched, "empty", "Now has value");
    }

    #[test]
    fn empty_value_prefix_suffix() {
        let ini = make_ini(&[("empty", "")]);
        let patched = apply(&ini, &[("empty", PatchOp::Prefix("pre".into()))]);
        assert_ini_value(&patched, "empty", "pre");

        let patched = apply(&ini, &[("empty", PatchOp::Suffix("suf".into()))]);
        assert_ini_value(&patched, "empty", "suf");
    }

    #[test]
    fn last_patch_wins_on_key_conflict() {
        // When building the HashMap, last insert wins
        let mut patches: HashMap<String, PatchOp> = HashMap::new();
        patches.insert("key".into(), PatchOp::Replace("First".into()));
        patches.insert("key".into(), PatchOp::Replace("Second".into()));

        let ini = make_ini(&[("key", "Original")]);
        let patched = merge::apply_patches(&ini, &patches);

        assert_ini_value(&patched, "key", "Second");
    }

    #[test]
    fn line_order_preserved() {
        let ini = "zebra=Z\nalpha=A\nmiddle=M\n";
        let patched = apply(ini, &[("middle", PatchOp::Replace("Changed".into()))]);

        let keys: Vec<&str> = patched
            .lines()
            .filter_map(|l| l.split('=').next())
            .collect();
        assert_eq!(keys, vec!["zebra", "alpha", "middle"]);
    }

    #[test]
    fn non_kv_lines_passthrough() {
        let ini = "# comment\nkey=value\n\n; another comment\n";
        let patched = apply(ini, &[]);

        assert!(patched.contains("# comment"));
        assert!(patched.contains("; another comment"));
    }
}

#[cfg(test)]
mod toml_module {
    use crate::merge;
    use crate::module::{Module, ModuleContext, ModuleConfig};
    use crate::modules::toml_module::TomlModule;
    use crate::test_helpers::fixtures::*;

    #[test]
    fn exact_key_replace() {
        let toml = r#"
            [module]
            name = "Test"
            [[patch]]
            key = "item_Name"
            replace = "New Name"
        "#;
        let module = TomlModule::from_embedded("test", toml);
        let ini_map = merge::parse_ini("item_Name=Old Name\nother=Untouched\n");
        let ctx = ModuleContext { db: None, ini: &ini_map, config: &ModuleConfig::default() };

        let patches = module.generate_patches(&ctx).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].0, "item_Name");
    }

    #[test]
    fn exact_keys_batch() {
        let toml = r#"
            [module]
            name = "Test"
            [[patch]]
            keys = ["a", "b", "c"]
            prefix = "[!] "
        "#;
        let module = TomlModule::from_embedded("test", toml);
        let ini_map = merge::parse_ini("a=Alpha\nb=Beta\nc=Charlie\nd=Delta\n");
        let ctx = ModuleContext { db: None, ini: &ini_map, config: &ModuleConfig::default() };

        let patches = module.generate_patches(&ctx).unwrap();
        assert_eq!(patches.len(), 3);
        // "d" should NOT be patched
        assert!(!patches.iter().any(|(k, _)| k == "d"));
    }

    #[test]
    fn key_pattern_glob() {
        let toml = r#"
            [module]
            name = "Test"
            [[patch]]
            key_pattern = "item_Name*_SCItem"
            prefix = "[W] "
        "#;
        let module = TomlModule::from_embedded("test", toml);
        let ini_map = merge::parse_ini(
            "item_NameWEAP_Laser_SCItem=Laser\n\
             item_NameCOOL_Fan_SCItem=Fan\n\
             item_NameWEAP_Cannon=Cannon\n\
             unrelated=Foo\n",
        );
        let ctx = ModuleContext { db: None, ini: &ini_map, config: &ModuleConfig::default() };

        let patches = module.generate_patches(&ctx).unwrap();
        // Should match Laser and Fan (both end with _SCItem), NOT Cannon or unrelated
        assert_eq!(patches.len(), 2);
        let keys: Vec<&str> = patches.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"item_NameWEAP_Laser_SCItem"));
        assert!(keys.contains(&"item_NameCOOL_Fan_SCItem"));
    }

    #[test]
    fn key_pattern_with_template_capture() {
        let toml = r#"
            [module]
            name = "Test"
            [[patch]]
            key_pattern = "item_Name*_S{size}_*"
            suffix = " [S{size}]"
        "#;
        let module = TomlModule::from_embedded("test", toml);
        let ini_map = merge::parse_ini(
            "item_NameCOOL_AEGS_S01_Bracer=Bracer\n\
             item_NamePOWR_AMRS_S03_Turbo=Turbo\n\
             no_match=Foo\n",
        );
        let ctx = ModuleContext { db: None, ini: &ini_map, config: &ModuleConfig::default() };

        let patches = module.generate_patches(&ctx).unwrap();
        assert_eq!(patches.len(), 2);

        // Apply and verify the captured size is substituted
        let ini = make_ini(&[
            ("item_NameCOOL_AEGS_S01_Bracer", "Bracer"),
            ("item_NamePOWR_AMRS_S03_Turbo", "Turbo"),
            ("no_match", "Foo"),
        ]);
        let patched = apply_module_patches(&ini, &patches);
        assert_ini_value(&patched, "item_NameCOOL_AEGS_S01_Bracer", "Bracer [S01]");
        assert_ini_value(&patched, "item_NamePOWR_AMRS_S03_Turbo", "Turbo [S03]");
        assert_ini_unchanged(&patched, "no_match", "Foo");
    }

    #[test]
    fn value_contains_filter() {
        let toml = r#"
            [module]
            name = "Test"
            [[patch]]
            key_pattern = "item_Desc*"
            value_contains = "Grade: A"
            prefix = "[A] "
        "#;
        let module = TomlModule::from_embedded("test", toml);
        let ini_map = merge::parse_ini(
            "item_DescWeapon1=Type: Weapon\\nGrade: A\\nSize: 2\n\
             item_DescWeapon2=Type: Weapon\\nGrade: C\\nSize: 1\n\
             item_DescShield=Type: Shield\\nGrade: A\\nSize: 3\n",
        );
        let ctx = ModuleContext { db: None, ini: &ini_map, config: &ModuleConfig::default() };

        let patches = module.generate_patches(&ctx).unwrap();
        // Only Weapon1 and Shield have Grade: A
        assert_eq!(patches.len(), 2);
        let keys: Vec<&str> = patches.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"item_DescWeapon1"));
        assert!(keys.contains(&"item_DescShield"));
        assert!(!keys.contains(&"item_DescWeapon2"));
    }

    #[test]
    fn missing_key_produces_no_patch() {
        let toml = r#"
            [module]
            name = "Test"
            [[patch]]
            key = "nonexistent"
            replace = "Nope"
        "#;
        let module = TomlModule::from_embedded("test", toml);
        let ini_map = merge::parse_ini("other_key=Value\n");
        let ctx = ModuleContext { db: None, ini: &ini_map, config: &ModuleConfig::default() };

        let patches = module.generate_patches(&ctx).unwrap();
        assert!(patches.is_empty());
    }

    #[test]
    fn rename_entries_parsed() {
        let toml = r#"
            [module]
            name = "Key Fixes"
            priority = 0
            [[rename]]
            from = "old_key"
            to = "new_key"
            [[rename]]
            from = "typo_key"
            to = "correct_key"
        "#;
        let module = TomlModule::from_embedded("test", toml);
        let ini_map = merge::parse_ini("old_key=Val\n");
        let config = ModuleConfig::default();
        let ctx = ModuleContext { db: None, ini: &ini_map, config: &config };

        let renames = module.generate_renames(&ctx).unwrap();
        assert_eq!(renames.len(), 2);
        assert_eq!(renames[0].from, "old_key");
        assert_eq!(renames[0].to, "new_key");
        assert_eq!(module.priority(), 0);
    }

    #[test]
    fn remove_keys_collected() {
        let toml = r#"
            [module]
            name = "Test"
            [[remove]]
            key = "unwanted"
            [[remove]]
            keys = ["also_bad", "remove_me"]
        "#;
        let module = TomlModule::from_embedded("test", toml);
        let remove = module.remove_keys();
        assert_eq!(remove.len(), 3);
        assert!(remove.contains(&"unwanted".to_string()));
        assert!(remove.contains(&"also_bad".to_string()));
        assert!(remove.contains(&"remove_me".to_string()));
    }

    #[test]
    fn default_enabled_true() {
        let toml = r#"
            [module]
            name = "Test"
        "#;
        let module = TomlModule::from_embedded("test", toml);
        assert!(module.default_enabled());
    }

    #[test]
    fn default_enabled_override() {
        let toml = r#"
            [module]
            name = "Test"
            default_enabled = false
        "#;
        let module = TomlModule::from_embedded("test", toml);
        assert!(!module.default_enabled());
    }

    /// Helper: apply module-generated patches to INI content.
    fn apply_module_patches(
        ini: &str,
        patches: &[(String, crate::module::PatchOp)],
    ) -> String {
        use std::collections::HashMap;
        let map: HashMap<String, crate::module::PatchOp> = patches
            .iter()
            .map(|(k, op)| (k.clone(), op.clone()))
            .collect();
        crate::merge::apply_patches(ini, &map)
    }
}

#[cfg(test)]
mod module_integration {
    use std::collections::HashMap;

    use crate::merge;
    use crate::module::{Module, ModuleContext, ModuleConfig, PatchOp};
    use crate::modules::toml_module::TomlModule;
    use crate::test_helpers::fixtures::*;

    #[test]
    fn two_modules_different_keys_no_conflict() {
        let mod_a = TomlModule::from_embedded(
            "mod_a",
            r#"
            [module]
            name = "Module A"
            [[patch]]
            key = "key_a"
            replace = "From A"
        "#,
        );
        let mod_b = TomlModule::from_embedded(
            "mod_b",
            r#"
            [module]
            name = "Module B"
            [[patch]]
            key = "key_b"
            suffix = " (B)"
        "#,
        );

        let ini = make_ini(&[("key_a", "Original A"), ("key_b", "Original B"), ("key_c", "Untouched")]);
        let ini_map = merge::parse_ini(&ini);
        let config = ModuleConfig::default();
        let ctx = ModuleContext {
            db: None,
            ini: &ini_map,
            config: &config,
        };

        let patches_a = mod_a.generate_patches(&ctx).unwrap();
        let patches_b = mod_b.generate_patches(&ctx).unwrap();

        // Merge: both should apply independently
        let mut merged: HashMap<String, PatchOp> = HashMap::new();
        for (k, op) in patches_a {
            merged.insert(k, op);
        }
        for (k, op) in patches_b {
            merged.insert(k, op);
        }

        let patched = merge::apply_patches(&ini, &merged);
        assert_ini_value(&patched, "key_a", "From A");
        assert_ini_value(&patched, "key_b", "Original B (B)");
        assert_ini_unchanged(&patched, "key_c", "Untouched");
    }

    #[test]
    fn two_modules_same_key_last_wins() {
        let mod_a = TomlModule::from_embedded(
            "mod_a",
            r#"
            [module]
            name = "Module A"
            [[patch]]
            key = "shared_key"
            replace = "From A"
        "#,
        );
        let mod_b = TomlModule::from_embedded(
            "mod_b",
            r#"
            [module]
            name = "Module B"
            [[patch]]
            key = "shared_key"
            replace = "From B"
        "#,
        );

        let ini = make_ini(&[("shared_key", "Original")]);
        let ini_map = merge::parse_ini(&ini);
        let config = ModuleConfig::default();
        let ctx = ModuleContext {
            db: None,
            ini: &ini_map,
            config: &config,
        };

        let patches_a = mod_a.generate_patches(&ctx).unwrap();
        let patches_b = mod_b.generate_patches(&ctx).unwrap();

        // Module B is inserted last → wins
        let mut merged: HashMap<String, PatchOp> = HashMap::new();
        for (k, op) in patches_a {
            merged.insert(k, op);
        }
        for (k, op) in patches_b {
            merged.insert(k, op);
        }

        let patched = merge::apply_patches(&ini, &merged);
        assert_ini_value(&patched, "shared_key", "From B");
    }

    #[test]
    fn disabled_module_produces_no_patches() {
        let module = TomlModule::from_embedded(
            "test",
            r#"
            [module]
            name = "Test"
            default_enabled = false
            [[patch]]
            key = "key_a"
            replace = "Changed"
        "#,
        );

        // The module itself still generates patches when called
        let ini_map = merge::parse_ini("key_a=Original\n");
        let config = ModuleConfig::default();
        let ctx = ModuleContext {
            db: None,
            ini: &ini_map,
            config: &config,
        };
        let patches = module.generate_patches(&ctx).unwrap();
        assert_eq!(patches.len(), 1);

        // But default_enabled is false — the caller (registry/lib.rs) should skip it
        assert!(!module.default_enabled());
    }

    #[test]
    fn embedded_modules_parse_without_panic() {
        // Verify all shipped TOML modules parse correctly
        let modules = crate::modules::builtin_modules();
        assert!(modules.len() >= 5, "Expected at least 5 built-in modules");

        for m in &modules {
            // Just verify they have valid metadata
            assert!(!m.id().is_empty());
            assert!(!m.name().is_empty());
        }
    }

    #[test]
    fn full_pipeline_no_data_loss() {
        // Simulate the full pipeline: multiple modules patching a larger INI
        let ini_lines: Vec<String> = (0..100)
            .map(|i| format!("key_{i:03}=value_{i}"))
            .collect();
        let ini = ini_lines.join("\n");
        let ini_map = merge::parse_ini(&ini);

        let mod_replace = TomlModule::from_embedded(
            "replacer",
            r#"
            [module]
            name = "Replacer"
            [[patch]]
            keys = ["key_010", "key_020", "key_030"]
            replace = "replaced"
        "#,
        );
        let mod_prefix = TomlModule::from_embedded(
            "prefixer",
            r#"
            [module]
            name = "Prefixer"
            [[patch]]
            keys = ["key_040", "key_050"]
            prefix = "[!] "
        "#,
        );
        let mod_suffix = TomlModule::from_embedded(
            "suffixer",
            r#"
            [module]
            name = "Suffixer"
            [[patch]]
            keys = ["key_060", "key_070"]
            suffix = " [BP]"
        "#,
        );

        let config = ModuleConfig::default();
        let ctx = ModuleContext {
            db: None,
            ini: &ini_map,
            config: &config,
        };

        let mut merged: HashMap<String, PatchOp> = HashMap::new();
        for module in [&mod_replace as &dyn Module, &mod_prefix, &mod_suffix] {
            for (k, op) in module.generate_patches(&ctx).unwrap() {
                merged.insert(k, op);
            }
        }

        let patched = merge::apply_patches(&ini, &merged);

        // All 100 lines must be present
        assert_eq!(line_count(&patched), 100);

        // Verify patched keys
        assert_ini_value(&patched, "key_010", "replaced");
        assert_ini_value(&patched, "key_020", "replaced");
        assert_ini_value(&patched, "key_040", "[!] value_40");
        assert_ini_value(&patched, "key_060", "value_60 [BP]");

        // Verify unpatched keys are untouched
        assert_ini_unchanged(&patched, "key_000", "value_0");
        assert_ini_unchanged(&patched, "key_099", "value_99");
        assert_ini_unchanged(&patched, "key_015", "value_15");
    }
}

#[cfg(test)]
mod merge_unit {
    use crate::merge;

    #[test]
    fn parse_ini_basic() {
        let map = merge::parse_ini("alpha=one\nbeta=two\n");
        assert_eq!(map.get("alpha").unwrap(), "one");
        assert_eq!(map.get("beta").unwrap(), "two");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn parse_ini_value_with_equals() {
        let map = merge::parse_ini("formula=a=b+c\n");
        assert_eq!(map.get("formula").unwrap(), "a=b+c");
    }

    #[test]
    fn parse_ini_empty_value() {
        let map = merge::parse_ini("empty=\n");
        assert_eq!(map.get("empty").unwrap(), "");
    }

    #[test]
    fn parse_ini_skips_non_kv_lines() {
        let map = merge::parse_ini("# comment\nkey=value\n\n");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("key").unwrap(), "value");
    }

    #[test]
    fn rename_changes_key_keeps_value() {
        use crate::module::KeyRename;

        let ini = "old_key=MyValue\nother=Untouched\n";
        let renames = vec![KeyRename {
            from: "old_key".into(),
            to: "new_key".into(),
        }];

        let result = merge::apply_renames(ini, &renames);
        assert!(result.contains("new_key=MyValue"));
        assert!(!result.contains("old_key="));
        assert!(result.contains("other=Untouched"));
    }

    #[test]
    fn rename_missing_key_is_noop() {
        use crate::module::KeyRename;

        let ini = "existing=Value\n";
        let renames = vec![KeyRename {
            from: "nonexistent".into(),
            to: "new_key".into(),
        }];

        let result = merge::apply_renames(ini, &renames);
        assert!(result.contains("existing=Value"));
        assert!(!result.contains("new_key="));
    }

    #[test]
    fn rename_preserves_line_count() {
        use crate::module::KeyRename;

        let ini = "a=1\nb=2\nc=3\n";
        let renames = vec![KeyRename {
            from: "b".into(),
            to: "b_new".into(),
        }];

        let result = merge::apply_renames(ini, &renames);
        assert_eq!(result.lines().count(), 3);
    }

    #[test]
    fn rename_then_patch_pipeline() {
        use crate::module::KeyRename;
        use crate::module::PatchOp;
        use std::collections::HashMap;

        // Simulate: typo key "item_NameSHLD_S01_CMP_YORM_Targa" exists with value "Targa"
        // Rename it to the correct key, then patch it with grade info
        let ini = "item_NameSHLD_S01_CMP_YORM_Targa=Targa\nother=Foo\n";

        let renames = vec![KeyRename {
            from: "item_NameSHLD_S01_CMP_YORM_Targa".into(),
            to: "item_NameSHLD_YORM_S01_Targa".into(),
        }];

        let renamed = merge::apply_renames(ini, &renames);

        // Now the code module would find item_NameSHLD_YORM_S01_Targa and patch it
        let mut patches = HashMap::new();
        patches.insert(
            "item_NameSHLD_YORM_S01_Targa".to_string(),
            PatchOp::Replace("Targa Competition B".into()),
        );

        let patched = merge::apply_patches(&renamed, &patches);
        assert!(patched.contains("item_NameSHLD_YORM_S01_Targa=Targa Competition B"));
        assert!(!patched.contains("item_NameSHLD_S01_CMP_YORM_Targa"));
    }

    #[test]
    fn decode_ini_strips_bom() {
        // UTF-16 LE BOM is FF FE, then U+FEFF BOM appears in decoded string
        let mut bytes = vec![0xFF, 0xFE]; // UTF-16 LE BOM
        bytes.extend_from_slice(&[0xFF, 0xFE]); // U+FEFF in UTF-16 LE
        // 'A' in UTF-16 LE
        bytes.extend_from_slice(&[0x41, 0x00]);

        let decoded = merge::decode_ini(&bytes).unwrap();
        assert_eq!(decoded, "A", "BOM should be stripped");
    }
}

#[cfg(test)]
mod language_pack {
    use crate::merge;
    use crate::test_helpers::fixtures::*;

    #[test]
    fn replaces_matching_keys() {
        let base = make_ini(&[
            ("item_NameABC", "Bracer"),
            ("item_DescABC", "A cooler."),
            ("other", "untouched"),
        ]);
        let pack = make_ini(&[
            ("item_NameABC", "Armreif"),
            ("item_DescABC", "Ein Kühler."),
        ]);

        let result = merge::apply_language_pack(&base, &pack);

        assert_ini_value(&result, "item_NameABC", "Armreif");
        assert_ini_value(&result, "item_DescABC", "Ein Kühler.");
        assert_ini_unchanged(&result, "other", "untouched");
    }

    #[test]
    fn appends_new_keys_not_in_base() {
        let base = make_ini(&[("existing", "value")]);
        let pack = make_ini(&[
            ("existing", "translated"),
            ("brand_new", "neuer eintrag"),
        ]);

        let result = merge::apply_language_pack(&base, &pack);

        assert_ini_value(&result, "existing", "translated");
        assert_ini_value(&result, "brand_new", "neuer eintrag");
    }

    #[test]
    fn preserves_base_line_order() {
        let base = "zebra=Z\nalpha=A\nmiddle=M\n";
        let pack = "middle=übersetzt\n";

        let result = merge::apply_language_pack(base, pack);

        let keys: Vec<&str> = result
            .lines()
            .filter_map(|l| l.split('=').next())
            .collect();
        assert_eq!(keys, vec!["zebra", "alpha", "middle"]);
    }

    #[test]
    fn ignores_lines_without_equals() {
        let base = make_ini(&[("key", "original")]);
        let pack = "; comment line\n\nkey=translated\nrandom garbage line\n";

        let result = merge::apply_language_pack(&base, pack);

        assert_ini_value(&result, "key", "translated");
    }

    #[test]
    fn preserves_values_with_embedded_equals() {
        let base = make_ini(&[("formula", "a=b+c=d")]);
        let pack = "formula=x=y+z=w\n";

        let result = merge::apply_language_pack(&base, pack);

        assert_ini_value(&result, "formula", "x=y+z=w");
    }

    #[test]
    fn decode_auto_utf8_with_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("key=Wert\n".as_bytes());
        let decoded = merge::decode_ini_auto(&bytes).unwrap();
        assert_eq!(decoded, "key=Wert\n");
    }

    #[test]
    fn decode_auto_utf8_no_bom() {
        let decoded = merge::decode_ini_auto("key=Wert\n".as_bytes()).unwrap();
        assert_eq!(decoded, "key=Wert\n");
    }

    #[test]
    fn decode_auto_utf16_le_with_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for ch in "key=Wert".encode_utf16() {
            bytes.extend_from_slice(&ch.to_le_bytes());
        }
        let decoded = merge::decode_ini_auto(&bytes).unwrap();
        assert_eq!(decoded, "key=Wert");
    }
}

#[cfg(test)]
mod language_pack_url {
    use crate::normalize_language_pack_url;

    #[test]
    fn github_blob_url_rewrites_to_raw() {
        let input =
            "https://github.com/rjcncpt/StarCitizen-Deutsch-INI/blob/main/live/global.ini";
        let expected =
            "https://raw.githubusercontent.com/rjcncpt/StarCitizen-Deutsch-INI/main/live/global.ini";
        assert_eq!(normalize_language_pack_url(input), expected);
    }

    #[test]
    fn github_raw_web_url_rewrites_to_raw() {
        let input =
            "https://github.com/rjcncpt/StarCitizen-Deutsch-INI/raw/main/live/global.ini";
        let expected =
            "https://raw.githubusercontent.com/rjcncpt/StarCitizen-Deutsch-INI/main/live/global.ini";
        assert_eq!(normalize_language_pack_url(input), expected);
    }

    #[test]
    fn already_raw_url_passes_through() {
        let input =
            "https://raw.githubusercontent.com/rjcncpt/StarCitizen-Deutsch-INI/main/live/global.ini";
        assert_eq!(normalize_language_pack_url(input), input);
    }

    #[test]
    fn non_github_url_passes_through() {
        let input = "https://example.com/packs/de.ini";
        assert_eq!(normalize_language_pack_url(input), input);
    }

    #[test]
    fn github_repo_root_url_passes_through() {
        // Not a blob/raw URL — do not touch it (we have no path to rewrite)
        let input = "https://github.com/rjcncpt/StarCitizen-Deutsch-INI";
        assert_eq!(normalize_language_pack_url(input), input);
    }
}
