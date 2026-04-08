/// Test utilities for building synthetic INI content and verifying output.
#[cfg(test)]
pub mod fixtures {
    use std::collections::HashMap;

    use crate::merge;
    use crate::module::PatchOp;

    /// Build a synthetic global.ini from key-value pairs.
    pub fn make_ini(entries: &[(&str, &str)]) -> String {
        entries
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Assert that a patched INI line has the expected value for a key.
    pub fn assert_ini_value(patched: &str, key: &str, expected_value: &str) {
        let prefix = format!("{key}=");
        let line = patched
            .lines()
            .find(|l| l.starts_with(&prefix))
            .unwrap_or_else(|| panic!("Key '{key}' not found in patched output"));
        let actual = &line[prefix.len()..];
        assert_eq!(
            actual, expected_value,
            "Key '{key}': expected '{expected_value}', got '{actual}'"
        );
    }

    /// Assert that a key was NOT modified (value matches original).
    pub fn assert_ini_unchanged(patched: &str, key: &str, original_value: &str) {
        assert_ini_value(patched, key, original_value);
    }

    /// Count lines in the output (excluding trailing empty line from final \n).
    pub fn line_count(content: &str) -> usize {
        content.lines().count()
    }

    /// Apply a set of patches to synthetic INI content and return the result.
    pub fn apply(ini: &str, patches: &[(&str, PatchOp)]) -> String {
        let map: HashMap<String, PatchOp> = patches
            .iter()
            .map(|(k, op)| (k.to_string(), op.clone()))
            .collect();
        merge::apply_patches(ini, &map)
    }
}
