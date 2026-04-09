# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
