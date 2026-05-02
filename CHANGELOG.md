# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Headless preview tooling — `preview_cli` for AI-agent / shell debugging, `preview_tui` (superlighttui-backed) for in-terminal exploration without restarting the game. Shared `PreviewSession` core loads datacore + INI once and exposes pool iteration + rendered output.
- Mission Enhancer rewritten as a pool-first patcher driven by sc-contracts v0.2.0 — title and description pools are walked independently, with aggressive merging across same-key members. The result is `[BP]`/`[BP*]`/`[BP?]` blueprint markers, `[Solo]`/`[Uniq]`/`[Illegal]`/`[~]` flags, `[CS Risk]`/`[CS Risk!]` crimestat tags, and a `Variants (N)` section for description pools whose members diverge.
- Variant labels resolve through a priority ladder (region → mission rank → debug-name location/difficulty hints → numeric fallback), with diagnostic logging for outliers.
- Encounter rendering pipeline — slots aggressively merged within `(encounter, phase, tags, role)`, phase ranges collapsed via numeric-token detection (`Wave 1` + `Wave 2` + `Wave 3` → `Wave 1-3`), identical lines deduped with `(×N)` suffix, skill ranges merged across slots.
- Encounter heading shows enemy spawn totals — `Encounters · 20x Ships · 10x NPC` (escort ships and allied NPCs excluded).
- Cargo / value / faction tags aggregated into a single summary line at the end of the encounters block.
- Region-label merger collapses repeated system prefixes — `Stanton: Hurston / Stanton: ArcCorp` becomes `Stanton: Hurston, ArcCorp`, and `Available at` lists put each star system on its own line.
- Manufacturer prefix discovery walks the DCB so ship-name shortening picks up newly-added manufacturers automatically (no hardcoded list).

### Changed
- `Color` enum in `formatter_helpers` renamed from chat-color names (`White`/`Cyan`/`Green`/`Yellow`/`Red`) to intent-based names (`Plain`/`Faint`/`Soft`/`Underline`/`Highlight`) that match the contracts panel — the only context where these tags reliably render distinctly.
- Section headers and title tags use `Color::Highlight` (renders as blue accent in contracts).
- Encounter labels are wrapped in `Color::Underline` to act as scan anchors down the left edge.
- Skill moved from the trailing meta to the front of each encounter line: `Skill 20 · 4x: ships`.
- NPC encounters collapse into a single `NPCs: N` total instead of per-slot listing — the FPS-mission breakdown was high-volume noise.
- Cooldown formatter now reads `DurationRange.mean_seconds` as minutes (sc-contracts upstream misnomer), renders sub-minute fractions as seconds and ≥1 minute as `Nmin`.
- Bumped sc-extract / sc-contracts / sc-weapons / sc-installs to `sc-holotable/v0.2.0`. Explicit feature flags (`contracts`, `servicebeacon`, `entityclassdefinition`) on `sc-extract` to ensure registry resolution lights up across the git workspace boundary.
- Tauri datacore extraction now uses `AssetConfig::standard()` so the `DisplayNameCache` is populated and downstream resolvers (blueprint item names, ship display names) actually return text.

### Fixed
- Blueprint item names no longer come back empty in patches — the locale was missing from `AssetData` extraction in the patcher path while the preview path had it; both paths now use `standard()`.
- Ship encounter candidates and blueprint pools no longer silently empty — `entityclassdefinition` feature flag wired through explicitly.
- Description-pool variant labels with sibling missions sharing a region no longer repeat the system prefix in the join.
- Encounter labels with generator wrappers (`Defend Location Wrapper`, `Escort Ship To/From Landing Area`, `Support Attacked Ship`, …) strip the wrapper before display. The strip now also runs on phase labels so wrappers don't leak back when the phase supersedes the encounter.
- Phase labels redundant with the encounter (`Initial Enemies [Initial Enemies]`, `Mission Targets [Target]`, `Allied [Allies]`) collapse via a stem-equivalence matcher that handles singular/plural and cross-inflection.
- DCB parse no longer overflows the Windows main-thread stack — `PreviewSession::load` runs the parse on a worker thread with an 8 MB stack.

### Removed
- Old per-slot tag rendering — tags moved into the aggregated cargo-info summary at the end of the encounters block.
- `shorten_ship_name` helper (replaced by inline `strip_manufacturer` since manufacturer-grouped rendering was reverted).

## [0.2.2] - 2026-04-09

### Fixed
- Ship encounter pools now match game data accurately — replaced hardcoded tag lists with dynamically collected tags from ship entities, fixing overly broad matching that included capital ships (Idris, Polaris, 890 Jump) in fighter-class missions

### Changed
- Ship encounter group labels are now highlighted (EM4 markup) for better readability
- Long ship lists wrap to a new line below the label instead of running inline

## [0.2.1] - 2026-04-09

### Added
- Ship encounters in mission descriptions — shows hostile and allied ship types resolved from DCB tag queries
- Crimestat risk tags on mission titles — detects DontHarm flags and allied NPC markers
- Mixed blueprint marker `[BP]*` for contracts where only some variants award blueprints
- Remove patch command — undo patching by removing `global.ini` and cleaning `user.cfg`
- Short prefix format for component grades (e.g. `MIL1C Bracer`)
- Game version detection from `build_manifest.id`
- Debug diff output in debug builds

### Changed
- Improved cooldown display — shows personal and abandon cooldowns when they differ
- `user.cfg` handling now upserts `g_language` instead of only creating when missing
- Switched svarog dependency from VeeLume fork to upstream 19h/svarog
- Cargo.toml formatting cleanup

### Removed
- Legacy TOML modules: `drug_markers`, `blueprint_markers`, `blueprint_rewards`, `component_grades` (superseded by code-derived modules)
- svarog dependency section from README (deps now fetched from upstream automatically)

## [0.2.0] - 2026-04-08

## [0.1.0] - 2026-04-08

### Added
- Initial release
- Auto-detect Star Citizen installations from RSI Launcher log
- Module system with TOML and code-based modules
- Component Grades (Derived) — auto-derive class and grade from game data
- Illegal Goods Markers — mark drugs and contraband from jurisdiction law data
- Mission Enhancer — blueprint rewards, reputation, cooldowns in mission descriptions
- Label Fixes, Key Fixes, Drug Markers, Blueprint Markers, Blueprint Rewards modules
- Auto-update support via GitHub Releases
