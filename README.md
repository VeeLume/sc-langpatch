# SC LangPatch

A Windows desktop app that enriches Star Citizen's `global.ini` localization file with information the game doesn't display properly -- component grades, illegal goods markers, mission blueprint rewards, and more.

Built with [Tauri](https://tauri.app/) + [Svelte](https://svelte.dev/) + Rust.

## What it does

SC LangPatch reads Star Citizen's `Data.p4k` archive, extracts game data from the DataCore binary database, and patches the localization file with enriched labels. All patches are modular and individually toggleable:

| Module | What it does |
|--------|-------------|
| Component Grades | Labels ship components with their class and grade (e.g. Military A) |
| Illegal Goods | Marks drugs and contraband based on jurisdiction law data |
| Mission Enhancer | Adds blueprint rewards, reputation, and cooldowns to mission descriptions |
| Label Fixes | Shortens HUD abbreviations and commodity names |
| Key Fixes | Corrects misspelled INI keys |
| Blueprint Markers | Tags blueprint titles |
| Blueprint Rewards | Lists blueprint rewards in descriptions |
| Drug Markers | Prefixes drug names |

## Prerequisites

- [Node.js](https://nodejs.org/) (LTS)
- [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/) (stable, 2024 edition)
- Star Citizen installed (the app auto-detects installations from the RSI Launcher log)

### svarog dependency

This project depends on the [svarog](https://github.com/valerie/svarog) crates for P4K archive extraction and DataCore parsing. The `Cargo.toml` uses relative path dependencies:

```toml
svarog-p4k     = { path = "../../svarog/crates/svarog-p4k" }
svarog-datacore = { path = "../../svarog/crates/svarog-datacore" }
```

Clone the svarog repo as a sibling directory:

```
parent/
  svarog/          # git clone the svarog repo here
  sc-langpatch/    # this repo
```

Or adjust the paths in [src-tauri/Cargo.toml](src-tauri/Cargo.toml) to match your setup.

## Build & Run

```bash
pnpm install
pnpm tauri dev          # Development with hot reload
pnpm tauri build        # Production build
```

### Tests

```bash
cd src-tauri
cargo test
```

## How it works

```
Data.p4k (user's SC install)
  +-- global.ini (UTF-16 LE) -> decode -> HashMap<key, value>
  +-- Game2.dcb (DataCore binary) -> svarog-datacore -> DataCoreDatabase
         |
  Phase 1: Key Renames (priority 0 modules first)
  Phase 2: Value Patches (all other modules)
         |
  Patched global.ini (UTF-8 with BOM) -> written to SC install
```

Modules implement a `Module` trait and produce key-value patch operations. There are two kinds:

- **Code modules** -- query the DataCore database at runtime to derive patches dynamically
- **TOML modules** -- static patches defined in embedded TOML files, supporting key patterns and template captures

## License

[MIT](LICENSE)
