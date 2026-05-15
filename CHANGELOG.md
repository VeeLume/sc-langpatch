# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed
- Update svarog & sc-holotable dependencies for datacore v8 support which was introduced with 4.8.

## [0.4.0] - 2026-05-04

### Added
- App UI is now translated. English and Deutsch ship in the box — auto-detected from the OS locale, switchable via a new gear-icon popover top-right. Module names, descriptions, option labels, choice values, error messages, and warnings are all translated. A drift-detection test in `cargo test` blocks PRs that add a new module/option/error variant without a matching catalog entry.
- Translation guide (`TRANSLATING.md`) for adding new locales — drop a new JSON file into `messages/`, register it in `project.inlang/settings.json`, open a PR. The language picker auto-includes new locales, rendered as autonyms (e.g. "Polski", "Français") via `Intl.DisplayNames`. No frontend code changes required.
- First-run notice ("App now available in English and Deutsch — switch via the gear icon") so users who suddenly see translated text understand where it came from. Dismissible, persists across reloads.
- German README (`README.de.md`) for non-tech German community members, with a step-by-step walkthrough for combining SC LangPatch with the [rjcncpt German language pack](https://github.com/rjcncpt/StarCitizen-Deutsch-INI). Linked from the English README.
- Warning badges on modules in the GUI:
  - `DCB` marks modules that need the DataCore database extracted from `Game2.dcb`.
  - `REPLACE` marks modules that overwrite entire INI values rather than appending — useful for spotting which modules may overwrite text from a loaded community language pack.
  - Hover tooltips per badge, plus a `?` legend next to the Modules heading explaining both.
- Runtime guard against undeclared `Replace` ops. The `Module` trait gained `uses_replace_ops()` (default `false`). If a module emits a `Replace` op without declaring it, the op is dropped — protecting text the user's language pack supplied — and a warning surfaces in the patch results. `TomlModule` derives the declaration from its rules automatically; code modules declare explicitly.
- Structured backend errors. `AppError` and `AppWarning` enums replace ad-hoc strings at the command boundary. The frontend dispatches on the variant code and renders via a localized message — predictable user-actionable cases (P4K not found, INI write failed, language-pack load failed, module skipped, …) get fully translated; uncaught errors fall through as English inside a localized "Unexpected error: {message}" frame for bug-report content.
- `?` info toggle next to "Community language pack" — the helper paragraph is collapsed by default to match the visual rhythm of the other sections; click to expand.
- "optional" badge next to the **Community language pack** heading so the optionality is visible at a glance.

### Changed
- **Community Language Pack** section reworked into a single combined input. Accepts a URL or a local path; folder-icon Browse opens the file picker; `×` clears. Saves on Enter or blur. Replaces the previous URL row + "or" + Pick file UI.
- Settings gear popover now sits inside the Installations heading row (right-aligned), so it doesn't compete with the page heading or take up its own header band.
- Illegal Goods option labels rewritten — the old "Color coded (red for drugs, yellow for contraband)" claim doesn't match how `<EM3>` / `<EM4>` actually render in the commodity name field. Now reads "Emphasised (distinct style for drugs vs contraband)" / "Plain [!] prefix". Output bytes unchanged. Internally refactored to use the centralised `Color` vocabulary from `formatter_helpers` instead of inline emphasis tags.

### Fixed
- Release builds no longer emit a dead-code warning for `options_hash` (debug-build-only helper, now properly `#[cfg(debug_assertions)]`-gated).

## [0.3.2] - 2026-05-04

### Fixed
- Weapon names no longer accumulate stacked size prefixes when multiple in-game variants share one localization key. `BEHR_LaserCannon_S7` (twelve colliding variants) used to render as `S7 S9 S7 S7 S7 S7 S7 S7 S7 S12 S8 S9 M9A Cannon`; it now renders as `S7 M9A Cannon`. Same fix applies to weapon descriptions, which had been accumulating one `<EM4>Weapon Stats</EM4>` block per colliding variant.
- Missile names with the same problem (`MISL_S02_CS_FSKI_Tempest` showing `S2 [CS] S2 [CS] Tempest II Missile`) collapse to one prefix as well.
- A handful of Vanduul plasma cannons (`VNCL_PlasmaCannon_S2` / `_S3`) no longer get a paragraph break + stats block embedded inside the *name* field. Those entities have no dedicated `item_Desc…` entry, so their description fallback lands on the name key — the patcher now defensively skips name keys when applying description-style suffixes.
- `superlighttui` is properly feature-gated behind `preview_tui` instead of being a hard dep — the Tauri release build no longer pulls it. Build the TUI with `cargo run --bin preview_tui --features preview_tui`.

### Changed
- Weapon enhancer now picks the variant whose size matches the loc-key suffix (`item_Name…_S7` → prefer the size-7 variants) before rendering. CIG re-uses entities for capital-ship hardpoints, point-defence turrets, and Idris/Javelin variants under one display name; the size-match filter restores the value the player actually sees in the in-game market listing.
- When the size-matched variants still disagree on a stat, the patcher renders a range (`Alpha: 2076-6750`, `Penetration: 10.50-15.00m`, `S7-S12`) instead of silently picking one. Damage breakdowns are dropped when alpha diverges to avoid combining different damage profiles into one misleading line.
- Missile tracking-tag prefix (`[IR]` / `[EM]` / `[CS]`) is omitted when colliding variants disagree on tracking signal, rather than rendering the wrong tag.
- Bumped sc-extract / sc-contracts / sc-weapons / sc-installs to `sc-holotable/v0.3.0`.
- Patcher now uses `AssetConfig::minimal()` for the parse-time asset bundle — sc-holotable's keys-only localization restructure means we no longer need the parse-time `LocaleMap` build, since cross-record name resolution (blueprint pool → entity name, ship-spawn lists, currency labels) happens at the call site against the post-overlay `LocaleMap` instead. Cuts a few hundred ms off cold-start parse time and ensures community language packs translate cross-record text consistently.

## [0.3.1] - 2026-05-02

### Fixed
- Removed an accidental `[patch]` override in `src-tauri/Cargo.toml` that pointed svarog crates at a local working tree (`E:/repros/Svarog`), breaking builds for anyone else.

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
