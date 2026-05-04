use std::path::Path;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Deserialize;

use crate::module::{KeyRename, Module, ModuleContext, PatchOp};

// ── TOML schema ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TomlFile {
    module: TomlModuleMeta,
    #[serde(default)]
    patch: Vec<TomlPatchEntry>,
    #[serde(default)]
    rename: Vec<TomlRenameEntry>,
    #[serde(default)]
    remove: Vec<TomlRemoveEntry>,
}

#[derive(Debug, Deserialize)]
struct TomlModuleMeta {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_true")]
    default_enabled: bool,
    #[serde(default = "default_priority")]
    priority: u32,
}

fn default_priority() -> u32 {
    100
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct TomlPatchEntry {
    /// Single exact key.
    key: Option<String>,
    /// Multiple exact keys.
    keys: Option<Vec<String>>,
    /// Glob-style pattern with optional `{name}` captures.
    key_pattern: Option<String>,

    /// Only apply if the current value contains this string.
    value_contains: Option<String>,

    /// Patch operations (exactly one must be set).
    replace: Option<String>,
    prefix: Option<String>,
    suffix: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TomlRenameEntry {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct TomlRemoveEntry {
    key: Option<String>,
    keys: Option<Vec<String>>,
}

// ── Compiled patch rule ─────────────────────────────────────────────────────

#[derive(Debug)]
enum KeyMatcher {
    /// Match specific keys exactly.
    Exact(Vec<String>),
    /// Match keys against a regex with optional named captures.
    Pattern {
        regex: Regex,
        /// Names of captures (e.g. ["size"] from `{size}` in the pattern).
        captures: Vec<String>,
    },
}

#[derive(Debug)]
struct CompiledRule {
    matcher: KeyMatcher,
    value_contains: Option<String>,
    /// Template string that may contain `{capture_name}` placeholders.
    template: String,
    /// Which operation: replace, prefix, or suffix.
    op_kind: OpKind,
}

#[derive(Debug, Clone, Copy)]
enum OpKind {
    Replace,
    Prefix,
    Suffix,
}

// ── TomlModule ──────────────────────────────────────────────────────────────

pub struct TomlModule {
    id: String,
    meta: TomlModuleMeta,
    rules: Vec<CompiledRule>,
    renames: Vec<KeyRename>,
    remove_keys: Vec<String>,
}

impl TomlModule {
    /// Load from an embedded TOML string (compiled into the binary).
    pub fn from_embedded(id: &str, toml_str: &str) -> Self {
        // Embedded modules are trusted — panic on parse failure.
        let file: TomlFile =
            toml::from_str(toml_str).unwrap_or_else(|e| panic!("Bad embedded module {id}: {e}"));
        Self::from_parsed(id.to_string(), file)
    }

    /// Load from a file path (user-defined module).
    pub fn from_file(path: &Path) -> Result<Self> {
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        let file: TomlFile =
            toml::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))?;

        Ok(Self::from_parsed(id, file))
    }

    fn from_parsed(id: String, file: TomlFile) -> Self {
        let mut rules = Vec::new();
        for entry in &file.patch {
            if let Ok(rule) = compile_rule(entry) {
                rules.push(rule);
            }
        }

        let renames: Vec<KeyRename> = file
            .rename
            .iter()
            .map(|r| KeyRename {
                from: r.from.clone(),
                to: r.to.clone(),
            })
            .collect();

        let mut remove_keys = Vec::new();
        for entry in &file.remove {
            if let Some(k) = &entry.key {
                remove_keys.push(k.clone());
            }
            if let Some(ks) = &entry.keys {
                remove_keys.extend(ks.iter().cloned());
            }
        }

        Self {
            id,
            meta: file.module,
            rules,
            renames,
            remove_keys,
        }
    }
}

impl Module for TomlModule {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.meta.name
    }

    fn description(&self) -> &str {
        &self.meta.description
    }

    fn default_enabled(&self) -> bool {
        self.meta.default_enabled
    }

    fn priority(&self) -> u32 {
        self.meta.priority
    }

    fn uses_replace_ops(&self) -> bool {
        self.rules.iter().any(|r| matches!(r.op_kind, OpKind::Replace))
    }

    fn generate_renames(&self, _ctx: &ModuleContext) -> Result<Vec<KeyRename>> {
        Ok(self.renames.clone())
    }

    fn generate_patches(&self, ctx: &ModuleContext) -> Result<Vec<(String, PatchOp)>> {
        let mut patches = Vec::new();

        for rule in &self.rules {
            match &rule.matcher {
                KeyMatcher::Exact(keys) => {
                    for key in keys {
                        if let Some(value) = ctx.ini.get(key) {
                            if rule.matches_value(value) {
                                patches.push((
                                    key.clone(),
                                    rule.make_op(&rule.template),
                                ));
                            }
                        }
                    }
                }
                KeyMatcher::Pattern { regex, captures } => {
                    for (key, value) in ctx.ini {
                        if let Some(caps) = regex.captures(key) {
                            if rule.matches_value(value) {
                                let resolved = resolve_template(&rule.template, captures, &caps);
                                patches.push((key.clone(), rule.make_op(&resolved)));
                            }
                        }
                    }
                }
            }
        }

        // Apply removals: mark keys to suppress from other modules.
        // We represent removal as a special empty replace that the merge
        // system will skip. For now, we don't emit these — the registry
        // handles removals by filtering the merged patch set.
        // The remove_keys are exposed via a method for the registry.

        Ok(patches)
    }
}

impl TomlModule {
    /// Keys that this module wants to suppress from other modules.
    pub fn remove_keys(&self) -> &[String] {
        &self.remove_keys
    }
}

impl CompiledRule {
    fn matches_value(&self, value: &str) -> bool {
        match &self.value_contains {
            Some(needle) => value.contains(needle.as_str()),
            None => true,
        }
    }

    fn make_op(&self, resolved_template: &str) -> PatchOp {
        match self.op_kind {
            OpKind::Replace => PatchOp::Replace(resolved_template.to_string()),
            OpKind::Prefix => PatchOp::Prefix(resolved_template.to_string()),
            OpKind::Suffix => PatchOp::Suffix(resolved_template.to_string()),
        }
    }
}

// ── Compilation helpers ─────────────────────────────────────────────────────

fn compile_rule(entry: &TomlPatchEntry) -> Result<CompiledRule> {
    let (template, op_kind) = resolve_op(entry)?;
    let matcher = resolve_matcher(entry)?;

    Ok(CompiledRule {
        matcher,
        value_contains: entry.value_contains.clone(),
        template,
        op_kind,
    })
}

fn resolve_op(entry: &TomlPatchEntry) -> Result<(String, OpKind)> {
    let ops: Vec<_> = [
        entry.replace.as_ref().map(|v| (v.clone(), OpKind::Replace)),
        entry.prefix.as_ref().map(|v| (v.clone(), OpKind::Prefix)),
        entry.suffix.as_ref().map(|v| (v.clone(), OpKind::Suffix)),
    ]
    .into_iter()
    .flatten()
    .collect();

    match ops.len() {
        0 => bail!("patch entry must have one of replace, prefix, or suffix"),
        1 => Ok(ops.into_iter().next().unwrap()),
        _ => bail!("patch entry must have only one of replace, prefix, or suffix"),
    }
}

fn resolve_matcher(entry: &TomlPatchEntry) -> Result<KeyMatcher> {
    let has_key = entry.key.is_some();
    let has_keys = entry.keys.is_some();
    let has_pattern = entry.key_pattern.is_some();

    let set_count = [has_key, has_keys, has_pattern]
        .iter()
        .filter(|&&b| b)
        .count();

    if set_count == 0 {
        bail!("patch entry must have key, keys, or key_pattern");
    }
    if set_count > 1 {
        bail!("patch entry must have only one of key, keys, or key_pattern");
    }

    if let Some(k) = &entry.key {
        return Ok(KeyMatcher::Exact(vec![k.clone()]));
    }
    if let Some(ks) = &entry.keys {
        return Ok(KeyMatcher::Exact(ks.clone()));
    }
    if let Some(pattern) = &entry.key_pattern {
        let (regex, captures) = compile_pattern(pattern)?;
        return Ok(KeyMatcher::Pattern { regex, captures });
    }

    unreachable!()
}

/// Convert a user-friendly pattern like `item_Name*_S{size}_*` into a regex.
///
/// - `*` becomes `.*` (match anything)
/// - `{name}` becomes a named capture group `(?P<name>[^=]*)`
/// - Everything else is escaped for literal matching
fn compile_pattern(pattern: &str) -> Result<(Regex, Vec<String>)> {
    let mut regex_str = String::from("^");
    let mut captures = Vec::new();
    let mut chars = pattern.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '*' => regex_str.push_str(".*"),
            '{' => {
                // Collect capture name until '}'
                let mut name = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    name.push(c);
                }
                if name.is_empty() {
                    bail!("Empty capture name in pattern: {pattern}");
                }
                regex_str.push_str(&format!("(?P<{name}>[^_]*)"));
                captures.push(name);
            }
            _ => {
                // Escape regex metacharacters
                if regex::escape(&ch.to_string()) != ch.to_string() {
                    regex_str.push('\\');
                }
                regex_str.push(ch);
            }
        }
    }
    regex_str.push('$');

    let regex = Regex::new(&regex_str)
        .with_context(|| format!("Invalid pattern regex: {regex_str} (from pattern: {pattern})"))?;

    Ok((regex, captures))
}

/// Substitute `{name}` placeholders in a template with captured values.
fn resolve_template(template: &str, capture_names: &[String], caps: &regex::Captures) -> String {
    let mut result = template.to_string();
    for name in capture_names {
        if let Some(m) = caps.name(name) {
            result = result.replace(&format!("{{{name}}}"), m.as_str());
        }
    }
    result
}
