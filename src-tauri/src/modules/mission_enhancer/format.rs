//! String helpers for mission-enhancer output.

use sc_extract::{Datacore, LocaleMap};

/// Pretty-print a CamelCase / snake_case identifier as space-separated words.
///
/// Inserts a space:
/// - between a lowercase letter or digit followed by an uppercase letter
///   (`MissionTargets` → `Mission Targets`)
/// - between a letter and a digit run (`Wave1` → `Wave 1`)
/// - between a digit run and a letter (`1stContact` → `1st Contact`)
///
/// Underscores collapse to a single space. Runs of consecutive uppercase
/// letters are preserved (`EnemyNPCs` → `Enemy NPCs`, `BP` stays `BP`).
/// Trailing `_BP` / `BP_` token noise is stripped before splitting.
pub fn pretty_identifier(s: &str) -> String {
    let trimmed = strip_noise_affixes(s);
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let mut out = String::with_capacity(chars.len() + 4);
    for i in 0..chars.len() {
        let c = chars[i];
        if c == '_' {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            continue;
        }
        if i > 0 {
            let prev = chars[i - 1];
            // For the upper-run end case (`URLPath` → `URL Path`) we need
            // to split before `P` because what follows is a real word —
            // two-or-more lowercase chars. In `NPCs` only a single `s`
            // follows, signalling a plural-of-acronym, so we keep it.
            let two_lower_follow = chars
                .get(i + 1)
                .map(|n| n.is_lowercase())
                .unwrap_or(false)
                && chars
                    .get(i + 2)
                    .map(|n| n.is_lowercase())
                    .unwrap_or(false);
            let boundary = (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_ascii_digit() && c.is_alphabetic())
                || (prev.is_alphabetic() && c.is_ascii_digit())
                || (prev.is_uppercase() && c.is_uppercase() && two_lower_follow);
            if boundary && !out.ends_with(' ') {
                out.push(' ');
            }
        }
        out.push(c);
    }
    // Collapse any double spaces from underscores adjacent to camel boundaries.
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out.trim().to_string()
}

/// Strip mission-variable noise affixes that don't add information:
/// `_BP` / `BP_` (blueprint marker on engine variable names).
fn strip_noise_affixes(s: &str) -> &str {
    let mut t = s.trim();
    if let Some(rest) = t.strip_suffix("_BP") {
        t = rest;
    }
    if let Some(rest) = t.strip_prefix("BP_") {
        t = rest;
    }
    t
}

/// Collect a deduped list of "first-word + space" prefixes suitable
/// for stripping from localized ship display names.
///
/// Sourced from sc-extract's pre-built [`sc_extract::ManufacturerRegistry`]
/// (`datacore.snapshot().manufacturers`), which walks the DCB's
/// `SCItemManufacturer` records once at extract time. Two prefix
/// candidates per manufacturer:
/// 1. The localized first word from `name_key` resolved against
///    `locale` (e.g. `"Aegis Dynamics"` → `"Aegis "`). This is what
///    actually shows up on ship `display_name`s.
/// 2. The short code (`"AEGS"`, `"BEHR"`, …). Doesn't typically prefix
///    ship names, but cheap to include — strip is a single
///    `starts_with` check.
pub fn build_manufacturer_prefixes(datacore: &Datacore, locale: &LocaleMap) -> Vec<String> {
    /// Reject prefixes shorter than this — too short for a confident
    /// "this matches a ship name" decision (a `"A "` prefix would
    /// catch any ship name starting with `A`). Real manufacturer
    /// prefixes are 3+ characters (`"RSI"`, `"AEGS"`, `"Aegis"`).
    const MIN_PREFIX_LEN: usize = 3;

    let mut prefixes: Vec<String> = Vec::new();
    let mut push = |word: &str| {
        if word.len() < MIN_PREFIX_LEN {
            return;
        }
        let prefix = format!("{word} ");
        if !prefixes.contains(&prefix) {
            prefixes.push(prefix);
        }
    };

    let registry = &datacore.snapshot().manufacturers;
    for m in registry.all() {
        // Localized first word — the one that matches ship display names.
        if let Some(name_key) = m.name_key.as_deref() {
            let key = name_key.strip_prefix('@').unwrap_or(name_key);
            if let Some(text) = locale.get(key)
                && let Some(first) = text.split_whitespace().next()
            {
                push(first);
            }
        }

        // Short code — `"AEGS"`, `"BEHR"`, …. Harmless to include even
        // though ship display names don't typically use it.
        push(m.code.as_str());
    }
    prefixes.sort();
    prefixes
}

/// Collapse hull variants into base hull names where multiple variants
/// of the same base are present. `["Avenger Stalker", "Avenger Warlock"]`
/// → `["Avenger"]`. Single-variant entries keep their full name.
pub fn collapse_variants(names: &[String]) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_camel_case() {
        assert_eq!(pretty_identifier("MissionTargets"), "Mission Targets");
        assert_eq!(pretty_identifier("Wave1"), "Wave 1");
        assert_eq!(pretty_identifier("ShipToDefend"), "Ship To Defend");
        assert_eq!(pretty_identifier("EnemyNPCs"), "Enemy NPCs");
        assert_eq!(pretty_identifier("LargeCombatShip"), "Large Combat Ship");
        assert_eq!(pretty_identifier("Reinforcements"), "Reinforcements");
    }

    #[test]
    fn pretty_strips_noise_affixes() {
        assert_eq!(pretty_identifier("ShipToDefend_BP"), "Ship To Defend");
        assert_eq!(pretty_identifier("BP_Hostile"), "Hostile");
    }

    #[test]
    fn pretty_handles_underscores() {
        assert_eq!(pretty_identifier("mission_targets"), "mission targets");
        assert_eq!(pretty_identifier("Salvage_Wave_2"), "Salvage Wave 2");
    }

    #[test]
    fn pretty_handles_empty() {
        assert_eq!(pretty_identifier(""), "");
        assert_eq!(pretty_identifier("_BP"), "");
    }

    #[test]
    fn collapse_variants_single_keeps_full() {
        let names = vec!["Avenger Stalker".to_string()];
        assert_eq!(collapse_variants(&names), vec!["Avenger Stalker"]);
    }

    #[test]
    fn collapse_variants_multiple_drops_to_base() {
        let names = vec![
            "Avenger Stalker".to_string(),
            "Avenger Warlock".to_string(),
        ];
        assert_eq!(collapse_variants(&names), vec!["Avenger"]);
    }
}
