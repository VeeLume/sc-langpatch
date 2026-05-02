# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.0] - 2026-05-02

### Added
- Community language pack support — overlay a user-provided `global.ini` (file path or URL) onto the base English INI before patches run. Auto-rewrites GitHub blob/raw URLs to `raw.githubusercontent.com`.
- Persisted user settings — module toggles, option values, selected install channels, and the language pack source survive restarts (saved to `%APPDATA%\sc-langpatch\config.toml`).
- Mission `Variants` section — when a description key is shared across multiple missions whose facts diverge, the patch surfaces a per-variant breakdown with a region / mission-rank / location-hint label per group.
- Mission `Available at` section — lists the star systems and bodies the mission is offered at, with each star system on its own line.
- Encounter section heading shows enemy spawn totals: `Encounters · 20x Ships · 10x NPC` (allied NPCs and escort ships excluded).
- Cargo / value / faction tag summary line at the end of the encounters block (replaces the per-slot tag clutter).
- Phase ranges (`Wave 1` + `Wave 2` + `Wave 3` → `Wave 1-3`), skill ranges (`Skill 40-60`), and identical-line dedup (`(×3)` suffix) within the encounters block.
- `One of:` rendering when an encounter merges several single-ship single-concurrent slot alternatives.
- New title tags: `[BP*]` (blueprint pool varies between sibling missions), `[BP?]` (mixed presence — some siblings have BP, others don't), `[~]` (other behavior axes vary). Existing `[Solo]`, `[Uniq]`, `[Illegal]`, `[CS Risk]`, `[CS Risk!]` markers are now consistent across the rendered output.
- Per-module patch statistics in the UI — counts and override details surface in their own panel instead of being mixed into warnings.
- Headless preview tooling for debugging — `preview_cli` (text output, scriptable) and `preview_tui` (interactive superlighttui-based browser). Loads datacore + INI once, lets you iterate on rendering without restarting the game.

### Changed
- Patches now stack per key — `Prefix` and `Suffix` from multiple modules compose in module-priority order instead of silently overwriting each other. Only `Replace` over `Replace` is a true conflict.
- Encounter labels are underlined in the contracts panel for easier scanning down the left edge.
- Encounter section uses a multi-line layout: encounter / phase label on its own line, body (skill, count, ships) indented underneath. Skill moved to the leading position so it's always at a predictable spot.
- NPC encounters collapse into a single `NPCs: N` total instead of one line per spawn slot — the FPS-mission per-slot breakdown was high-volume noise.
- Region labels merge across mission span entries — `Stanton: Hurston / Stanton: ArcCorp` becomes `Stanton: Hurston, ArcCorp` (system prefix appears once).
- Wrapper-prefix encounter names are cleaned for display: `Defend Location Wrapper Enemy Ships` → `Enemy Ships`, `Escort Ship To Landing Area Initial Enemies` → `Initial Enemies`, etc.
- Phase labels that echo the encounter (`Initial Enemies [Initial Enemies]`, `Mission Targets [Target]`, `Allied [Allies]`) collapse to just the encounter — handles singular/plural and cross-inflection (`-ed` ↔ `-ies`, `-er` ↔ `-ing`).
- Mission cooldowns display in minutes (was incorrectly reading the upstream value as seconds; the field is misnamed in sc-contracts).
- Bumped sc-extract / sc-contracts / sc-weapons / sc-installs to `sc-holotable/v0.2.0`.

### Fixed
- Blueprint item names no longer come back empty — the patcher's datacore extraction now passes a populated locale into the `DisplayNameCache` build.
- Ship encounter candidates and blueprint pools resolve correctly — required `sc-extract` feature flags (`contracts`, `servicebeacon`, `entityclassdefinition`) are now opted in explicitly so they propagate across the git workspace boundary.

### Removed
- Per-slot inline tag rendering on ship encounters — tags now aggregate into the single summary line at the end of the encounters block.

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
