use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::module::{KeyRename, PatchOp};

/// Parse global.ini content into a key → value map.
pub fn parse_ini(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].to_string();
            let value = line[eq_pos + 1..].to_string();
            map.insert(key, value);
        }
    }
    map
}

/// Apply key renames to INI content. Returns the modified content.
///
/// For each rename, finds the line with `from` key and changes the key to `to`,
/// keeping the value. If `from` doesn't exist, the rename is skipped.
pub fn apply_renames(ini_content: &str, renames: &[KeyRename]) -> String {
    let rename_map: HashMap<&str, &str> = renames
        .iter()
        .map(|r| (r.from.as_str(), r.to.as_str()))
        .collect();

    let mut output = String::with_capacity(ini_content.len());
    let mut applied = 0;

    for line in ini_content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let key = &line[..eq_pos];
            if let Some(&new_key) = rename_map.get(key) {
                let value = &line[eq_pos + 1..];
                output.push_str(new_key);
                output.push('=');
                output.push_str(value);
                applied += 1;
            } else {
                output.push_str(line);
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }

    if applied > 0 {
        eprintln!("  Renamed {applied} keys");
    }

    output
}

/// Apply patches to the global.ini content.
///
/// Processes line-by-line, substituting values where keys match.
pub fn apply_patches(ini_content: &str, patches: &HashMap<String, PatchOp>) -> String {
    let mut applied = 0;
    let mut output = String::with_capacity(ini_content.len());

    for line in ini_content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let key = &line[..eq_pos];

            if let Some(op) = patches.get(key) {
                let original_value = &line[eq_pos + 1..];
                let new_value = match op {
                    PatchOp::Replace(v) => v.clone(),
                    PatchOp::Prefix(p) => format!("{p}{original_value}"),
                    PatchOp::Suffix(s) => format!("{original_value}{s}"),
                };
                output.push_str(key);
                output.push('=');
                output.push_str(&new_value);
                applied += 1;
            } else {
                output.push_str(line);
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }

    eprintln!("  Applied {applied}/{} patches", patches.len());

    if applied < patches.len() {
        let missing_count = patches.len() - applied;
        eprintln!("  Warning: {missing_count} patch keys not found in global.ini");
    }

    output
}

/// Decode global.ini bytes from the p4k (UTF-16 LE) to a String.
pub fn decode_ini(bytes: &[u8]) -> Result<String> {
    let (decoded, _, had_errors) = encoding_rs::UTF_16LE.decode(bytes);
    if had_errors {
        anyhow::bail!("UTF-16 LE decoding produced errors");
    }
    let s = decoded.into_owned();
    Ok(s.strip_prefix('\u{FEFF}').unwrap_or(&s).to_owned())
}

/// Write the patched global.ini and user.cfg to the output directory.
pub fn write_output(output_dir: &Path, content: &str) -> Result<()> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create {}", output_dir.display()))?;

    let ini_path = output_dir.join("global.ini");
    let mut bom_content = Vec::with_capacity(3 + content.len());
    bom_content.extend_from_slice(b"\xEF\xBB\xBF");
    bom_content.extend_from_slice(content.as_bytes());
    std::fs::write(&ini_path, &bom_content)
        .with_context(|| format!("Failed to write {}", ini_path.display()))?;

    // Write user.cfg next to the data/ directory
    if let Some(install_dir) = output_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
    {
        let cfg_path = install_dir.join("user.cfg");
        if !cfg_path.exists() {
            std::fs::write(&cfg_path, "g_language = english\n")
                .with_context(|| format!("Failed to write {}", cfg_path.display()))?;
            eprintln!("  Created {}", cfg_path.display());
        }
    }

    eprintln!("  Written {}", ini_path.display());
    Ok(())
}
