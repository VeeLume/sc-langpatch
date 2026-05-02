# sc-holotable

Shared Rust workspace at `E:\vscode\rust\sc-holotable` (GitHub: `VeeLume/sc-holotable`) that owns the heavy lifting for SC install discovery, P4K / DCB extraction, and domain models. sc-langpatch consumes it instead of poking svarog directly.

## What lives there

| Crate | What it gives us |
|---|---|
| `sc-installs` | SC install discovery (LIVE / PTU / EPTU…), paths to `Data.p4k` / `global.ini` / override dirs. |
| `sc-extract` | Staged entry points: `AssetSource::from_install` → `AssetData::extract` → `Datacore::parse`. Owns DCB + locale parsing + serialization. Re-exports svarog as escape hatch (`sc_extract::svarog_datacore`). |
| `sc-extract-generated` | Workspace-internal, do not depend on directly. Generated `DataPools`, `Handle<T>`, typed enums, `LocaleKey`, poly enums. Re-exported through `sc-extract`. |
| `sc-weapons` | Canonical weapon model. `iter_ship_weapons(&datacore)` / `iter_fps_weapons(&datacore)` yield owned `ShipWeapon` / `FpsWeapon` with damage, ammo, sustain (`HeatModel` / `EnergyModel`), fire actions, multi-mode + charge modifiers, Tier 1/2/3 comparison stats and `effective_dps(LoadoutContext)`. |
| `sc-contracts` | `ContractIndex::build(&datacore, &locale)` returns a `Clone + Debug` bundle of merged `Contract`s plus `ShipRegistry` / `BlueprintPoolRegistry` / `RewardCurrencyCatalog` / `LocationRegistry` / `LocalityRegistry`. Handles handler/contract/sub-contract expansion, 4-level title/desc inheritance with `~mission(...)` runtime-substitution flagging, typed reward model, prereqs, encounters with intent-based ship-tag resolution, mission span (locality → system/body classification), `find_bp_conflicts` for `[BP]*` mixed-presence detection. |

## Cargo

```toml
[dependencies]
sc-holotable = { git = "https://github.com/VeeLume/sc-holotable.git", tag = "v0.1.0" }
```

(Pin to the same tag across `sc-installs`, `sc-extract`, `sc-weapons`, `sc-contracts` — they live in one workspace.)

For heavy iteration, use a `[patch]` section pointing at `E:\vscode\rust\sc-holotable`.

## Rule of thumb

- **Never `use svarog_*` directly.** Go through `sc_extract::svarog_datacore` etc. Same for the generated crate — only depend on `sc-extract`.
- **Don't reimplement registries we already have.** ship pools, blueprint pools, currency, localities all live in `ContractIndex`. The intent-based ship resolver replaces the old `_PU_AI` name filter (workspace rule §5: no string matching where typed alternatives exist).
- **Prefer the curated model.** Reach for the escape hatch (`datacore.db()` raw svarog, or `datacore.records().pools` typed) only when the model genuinely doesn't cover the case.

## Migration notes for our modules

- **`mission_enhancer`** — replace the hand-rolled contract walker with `ContractIndex`. Title rendering should respect `Contract.has_runtime_substitution`. Mixed-BP `[BP]*` annotation comes from `find_bp_conflicts`. Mission span / region label is precomputed (`LocalityView.region_label`).
- **`weapon_enhancer`** — use `iter_ship_weapons` / `iter_fps_weapons`. Sustain numbers (`time_to_overheat`, `sustained_dps`, `effective_dps`) are model-derived, no need to recompute.

## When we need more data

If a module needs a field / registry / behaviour that sc-holotable doesn't expose yet, **do not** add ad-hoc DCB extraction here. Instead:

1. Open a feature request on the sc-holotable repo (GitHub issue, or a doc under `E:\vscode\rust\sc-holotable\docs\` for larger proposals — see `docs/sc-contracts.md` for the pattern).
2. Describe the consumer use case, the DCB shape if known, and which crate it belongs in.
3. While the request is pending, either block the work or stub it with a `TODO(sc-holotable#NNN)` marker.

This keeps the model layer canonical and avoids bulkhead / sc-langpatch / streamdeck-starcitizen each maintaining their own divergent copy of the same extraction.

## References

- Workspace orientation: `E:\vscode\rust\sc-holotable\CLAUDE.md`
- Current state: `E:\vscode\rust\sc-holotable\status.md`
- Contracts consumer guide: `E:\vscode\rust\sc-holotable\docs\sc-contracts-guide.md`
- Contracts design spec: `E:\vscode\rust\sc-holotable\docs\sc-contracts.md`
- Weapons spec: `E:\vscode\rust\sc-holotable\docs\sc-weapons.md`
- DCB binary format: `D:\Obsidian\Star Citizen\Game Files\Datacore.md` (Obsidian vault)
