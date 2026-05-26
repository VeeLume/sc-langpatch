use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::module::{KeyRename, PatchOp};

/// Parse global.ini content into a key → value map.
///
/// CIG locale-metadata suffixes (e.g. `,P` on variant names) are
/// stripped from keys via [`sc_extract::strip_locale_metadata`] so the
/// returned map keys match the bare DCB-reference form. Without this,
/// downstream consumers (module contexts, the post-overlay LocaleMap)
/// would silently miss every suffixed entry.
pub fn parse_ini(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let raw_key = &line[..eq_pos];
            let key = sc_extract::strip_locale_metadata(raw_key).to_string();
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
            let raw_key = &line[..eq_pos];
            // Strip the `,P` metadata suffix before matching against
            // rename targets; preserve it on rewrite by re-appending
            // the original marker to the new key so the renamed line
            // keeps its locale-metadata flag.
            let (key, marker) = match raw_key.split_once(',') {
                Some((stem, marker)) => (stem, Some(marker)),
                None => (raw_key, None),
            };
            if let Some(&new_key) = rename_map.get(key) {
                let value = &line[eq_pos + 1..];
                output.push_str(new_key);
                if let Some(marker) = marker {
                    output.push(',');
                    output.push_str(marker);
                }
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

/// Apply a stacked op list to an original value in order.
///
/// - `Replace` overwrites the running value outright (a later Replace
///   wipes any prior Prefix / Suffix as well as any prior Replace).
/// - `Prefix` / `Suffix` compose on top of the running value, so two
///   modules can both annotate the same key without losing each
///   other's work.
fn apply_ops(original: &str, ops: &[PatchOp]) -> String {
    let mut value = original.to_string();
    for op in ops {
        match op {
            PatchOp::Replace(v) => value = v.clone(),
            PatchOp::Prefix(p) => value = format!("{p}{value}"),
            PatchOp::Suffix(s) => value = format!("{value}{s}"),
        }
    }
    value
}

/// Apply patches to the global.ini content.
///
/// Processes line-by-line, substituting values where keys match. Each
/// key can carry a stack of ops (see [`apply_ops`]), collected in
/// module-priority order so multiple modules can compose on the same
/// key without losing each other's patches.
pub fn apply_patches(ini_content: &str, patches: &HashMap<String, Vec<PatchOp>>) -> String {
    let mut applied = 0;
    let mut skipped_placeholder = 0;
    let mut output = String::with_capacity(ini_content.len());

    for line in ini_content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let raw_key = &line[..eq_pos];
            // Strip CIG locale-metadata suffix (`,P` etc.) before
            // matching. DCB references — and the keys our modules
            // emit patches for — use the bare form, so an INI line
            // like `item_Descfoo,P=...` would otherwise never be
            // patched. The original line keeps its suffix on write.
            let key = match raw_key.split_once(',') {
                Some((stem, _marker)) => stem,
                None => raw_key,
            };

            if let Some(ops) = patches.get(key) {
                let original_value = &line[eq_pos + 1..];
                // Skip CIG placeholder/uninitialized sentinels. The
                // game ships `LOC_PLACEHOLDER` and `LOC_UNINITIALIZED`
                // entries used as the fallback target for unresolved
                // localization keys; many missions resolve their
                // title/description to one of these. Appending blueprint
                // / mission-info blocks to a shared placeholder line
                // would stack every mission's annotation onto one entry
                // — see the giant "Potential Blueprints" pile-up that
                // surfaces when a single mission's `<EM4>` chain ends
                // up on `LOC_UNINITIALIZED=<= UNINITIALIZED =>`.
                if is_placeholder_value(original_value) {
                    skipped_placeholder += 1;
                    output.push_str(line);
                } else {
                    let new_value = apply_ops(original_value, ops);
                    // Preserve the original key form (including any
                    // `,P` metadata suffix) so the patched INI stays
                    // shape-compatible with the game's parser.
                    output.push_str(raw_key);
                    output.push('=');
                    output.push_str(&new_value);
                    applied += 1;
                }
            } else {
                output.push_str(line);
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }

    eprintln!("  Applied {applied}/{} patches", patches.len());
    if skipped_placeholder > 0 {
        eprintln!(
            "  Skipped {skipped_placeholder} patch(es) targeting CIG placeholder values (LOC_PLACEHOLDER / LOC_UNINITIALIZED)"
        );
    }

    if applied + skipped_placeholder < patches.len() {
        let missing_count = patches.len() - applied - skipped_placeholder;
        eprintln!("  Warning: {missing_count} patch keys not found in global.ini");
    }

    output
}

/// Detect CIG's unresolved-localization sentinel values.
///
/// The game ships two well-known placeholder lines that get used as
/// the value of any localization key whose real text is missing:
///
/// ```text
/// LOC_PLACEHOLDER=S1 ???0A <= PLACEHOLDER =>
/// LOC_UNINITIALIZED=<= UNINITIALIZED =>
/// ```
///
/// Many missions point their title/description keys at one of these
/// when CIG hasn't shipped a translation. Stacking patches (blueprint
/// list, mission info, etc.) on top of those values produces one giant
/// annotation pile-up — every affected mission's enrichment lands on
/// the same shared line.
///
/// Skip the patch when the running value carries the `<= PLACEHOLDER =>`
/// or `<= UNINITIALIZED =>` marker. Matched case-insensitively so any
/// future capitalisation drift doesn't reopen the bug.
fn is_placeholder_value(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    upper.contains("<= PLACEHOLDER =>") || upper.contains("<= UNINITIALIZED =>")
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

/// Decode an INI-style file from a user-provided path. Auto-detects encoding
/// based on BOM: UTF-8 BOM, UTF-16 LE BOM, UTF-16 BE BOM; otherwise falls
/// back to UTF-8.
///
/// Community language packs are usually UTF-8 (with or without BOM), but the
/// game itself ships UTF-16 LE, so we handle both.
pub fn decode_ini_auto(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(b"\xFF\xFE") {
        let (decoded, _, had_errors) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
        if had_errors {
            anyhow::bail!("UTF-16 LE decoding produced errors");
        }
        return Ok(decoded.into_owned());
    }
    if bytes.starts_with(b"\xFE\xFF") {
        let (decoded, _, had_errors) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
        if had_errors {
            anyhow::bail!("UTF-16 BE decoding produced errors");
        }
        return Ok(decoded.into_owned());
    }
    let body = if bytes.starts_with(b"\xEF\xBB\xBF") {
        &bytes[3..]
    } else {
        bytes
    };
    let (decoded, _, had_errors) = encoding_rs::UTF_8.decode(body);
    if had_errors {
        anyhow::bail!("UTF-8 decoding produced errors");
    }
    Ok(decoded.into_owned())
}

/// Overlay a community language pack onto the base INI.
///
/// For every `key=value` line in the pack, replaces the value of the matching
/// key in the base INI. Keys present in the pack but not in the base are
/// appended to the end of the output.
///
/// Order and formatting of lines from the base INI are preserved. Lines
/// without `=` in the pack are ignored.
pub fn apply_language_pack(ini_content: &str, pack_content: &str) -> String {
    let mut overrides: HashMap<&str, &str> = HashMap::new();
    for line in pack_content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let key = &line[..eq_pos];
            let value = &line[eq_pos + 1..];
            overrides.insert(key, value);
        }
    }

    let mut output = String::with_capacity(ini_content.len());
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut replaced = 0usize;

    for line in ini_content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let raw_key = &line[..eq_pos];
            // Same `,P` metadata-suffix strip as `apply_patches`.
            // Community packs are authored against bare DCB key names;
            // without this, language-pack overrides silently miss
            // every `,P`-suffixed base entry (Novian variant names,
            // etc.).
            let key = match raw_key.split_once(',') {
                Some((stem, _marker)) => stem,
                None => raw_key,
            };
            if let Some(new_value) = overrides.get(key) {
                output.push_str(raw_key);
                output.push('=');
                output.push_str(new_value);
                replaced += 1;
                // `seen` is checked against override-map keys (bare
                // form), so insert the stripped form even though we
                // wrote `raw_key` to the output.
                seen.insert(key);
            } else {
                output.push_str(line);
            }
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }

    let mut added = 0usize;
    for (key, value) in &overrides {
        if !seen.contains(key) {
            output.push_str(key);
            output.push('=');
            output.push_str(value);
            output.push('\n');
            added += 1;
        }
    }

    eprintln!("  Language pack: {replaced} keys replaced, {added} keys added");
    output
}

/// Write debug diff files containing only the patched lines.
///
/// Two files are written to `debug_dir`:
///   - `global_{version}_{options_hash}_{timestamp}.ini`          — original values
///   - `global_{version}_{options_hash}_{timestamp}_modified.ini` — patched values
///
/// Lines appear in INI file order. The filenames are deterministic given
/// the same game version and module options.
pub fn write_diff(
    debug_dir: &Path,
    version: &str,
    options_hash: &str,
    ini_content: &str,
    patches: &HashMap<String, Vec<PatchOp>>,
) -> Result<()> {
    let mut original = String::new();
    let mut modified = String::new();

    for line in ini_content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let key = &line[..eq_pos];
            if let Some(ops) = patches.get(key) {
                let original_value = &line[eq_pos + 1..];
                let new_value = apply_ops(original_value, ops);
                original.push_str(key);
                original.push('=');
                original.push_str(original_value);
                original.push('\n');
                modified.push_str(key);
                modified.push('=');
                modified.push_str(&new_value);
                modified.push('\n');
            }
        }
    }

    std::fs::create_dir_all(debug_dir)
        .with_context(|| format!("Failed to create {}", debug_dir.display()))?;

    let base = format!("global_{version}_{options_hash}_{}", chrono::Utc::now().format("%Y%m%d%H%M%S"));
    let orig_path = debug_dir.join(format!("{base}.ini"));
    let mod_path = debug_dir.join(format!("{base}_modified.ini"));

    std::fs::write(&orig_path, original.as_bytes())
        .with_context(|| format!("Failed to write {}", orig_path.display()))?;
    std::fs::write(&mod_path, modified.as_bytes())
        .with_context(|| format!("Failed to write {}", mod_path.display()))?;

    eprintln!("  Written diff: {}", orig_path.display());
    eprintln!("  Written diff: {}", mod_path.display());
    Ok(())
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

    // Upsert g_language = english in user.cfg next to the data/ directory
    if let Some(install_dir) = output_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
    {
        let cfg_path = install_dir.join("user.cfg");
        let existing = if cfg_path.exists() {
            std::fs::read_to_string(&cfg_path)
                .with_context(|| format!("Failed to read {}", cfg_path.display()))?
        } else {
            String::new()
        };
        let updated = upsert_cfg_key(&existing, "g_language", "english");
        if updated != existing {
            std::fs::write(&cfg_path, &updated)
                .with_context(|| format!("Failed to write {}", cfg_path.display()))?;
            eprintln!("  Updated {}", cfg_path.display());
        }
    }

    eprintln!("  Written {}", ini_path.display());
    Ok(())
}

/// Remove the patched global.ini and clean up user.cfg.
///
/// Returns `true` if the file existed and was removed.
pub fn remove_output(output_dir: &Path) -> Result<bool> {
    let ini_path = output_dir.join("global.ini");
    if !ini_path.exists() {
        return Ok(false);
    }

    std::fs::remove_file(&ini_path)
        .with_context(|| format!("Failed to remove {}", ini_path.display()))?;
    eprintln!("  Removed {}", ini_path.display());

    // Remove g_language = english from user.cfg if present
    if let Some(install_dir) = output_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
    {
        let cfg_path = install_dir.join("user.cfg");
        if cfg_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&cfg_path) {
                let cleaned = remove_cfg_key(&content, "g_language");
                if cleaned.trim().is_empty() {
                    let _ = std::fs::remove_file(&cfg_path);
                    eprintln!("  Removed {}", cfg_path.display());
                } else if cleaned != content {
                    let _ = std::fs::write(&cfg_path, &cleaned);
                    eprintln!("  Cleaned {}", cfg_path.display());
                }
            }
        }
    }

    Ok(true)
}

/// Insert or update a `key = value` line in a user.cfg-style file.
fn upsert_cfg_key(content: &str, key: &str, value: &str) -> String {
    let target = format!("{key} = {value}");
    let mut found = false;
    let mut lines: Vec<String> = content
        .lines()
        .map(|l| {
            let stripped = l.trim();
            if stripped.starts_with(key) && stripped[key.len()..].trim_start().starts_with('=') {
                found = true;
                target.clone()
            } else {
                l.to_string()
            }
        })
        .collect();

    if !found {
        lines.push(target);
    }

    let mut result = lines.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Remove all lines setting a given key from a user.cfg-style file.
fn remove_cfg_key(content: &str, key: &str) -> String {
    let lines: Vec<&str> = content
        .lines()
        .filter(|l| {
            let stripped = l.trim();
            !(stripped.starts_with(key) && stripped[key.len()..].trim_start().starts_with('='))
        })
        .collect();

    if lines.is_empty() {
        return String::new();
    }
    let mut result = lines.join("\n");
    result.push('\n');
    result
}
