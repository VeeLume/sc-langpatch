# SC LangPatch

A Tauri + Svelte app that enriches Star Citizen's `global.ini` localization file with information the game doesn't display properly — component grades, illegal goods markers, mission blueprint rewards, and more.

## Architecture

### Data flow

```
Data.p4k (user's SC install)
  ├── global.ini (UTF-16 LE) → decode → HashMap<key, value>
  └── Game2.dcb (DataCore binary) → svarog-datacore → DataCoreDatabase
         ↓
  ┌─────────────────────────────────────────────────┐
  │ Phase 1: Key Renames (priority 0 modules first) │
  │ Phase 2: Value Patches (all other modules)      │
  └─────────────────────────────────────────────────┘
         ↓
  Patched global.ini (UTF-8 with BOM) → written to SC install
```

### Module system

Every module implements the `Module` trait (`src/module.rs`). Modules produce `Vec<(String, PatchOp)>` — a list of INI key → operation pairs.

**Two types of modules:**
- **Code modules** (`src/modules/*.rs`) — query the DCB via `svarog-datacore` to derive patches at runtime
- **TOML modules** (`src/modules/toml/*.toml`) — static patches loaded by `TomlModule`, support key patterns and template captures

**Module execution order:**
1. Modules sorted by `priority()` (lower first, default 100)
2. Phase 1: all modules' `generate_renames()` collected and applied to INI
3. Phase 2: all modules' `generate_patches()` collected, merged (last write wins on key conflict), applied

**Current modules:**
| Module | Type | What it does |
|--------|------|-------------|
| `key_fixes` | TOML (priority 0) | Renames misspelled INI keys |
| `label_fixes` | TOML | HUD abbreviations, commodity shortening |
| `drug_markers` | TOML | Legacy drug prefix markers |
| `component_grades` | TOML | Legacy component grade labels |
| `blueprint_markers` | TOML | Legacy blueprint title tags |
| `blueprint_rewards` | TOML | Legacy blueprint description lists |
| `component_grades_derived` | Code | Auto-derives component class + grade from DCB |
| `illegal_goods` | Code | Marks drugs/contraband from jurisdiction law data |
| `mission_enhancer` | Code | Enriches mission titles/descriptions with blueprints, rep, cooldowns |

### Project structure

```
src-tauri/src/
  lib.rs              # Tauri entry, commands, patching pipeline
  main.rs             # Windows entry point
  module.rs           # Module trait, PatchOp, KeyRename, ModuleConfig types
  merge.rs            # INI parsing, patch application, rename application, output writing
  discovery.rs        # SC install detection from RSI launcher logs
  modules/
    mod.rs            # Module registry (builtin_modules, user_modules)
    toml_module.rs    # Generic TOML module loader (patterns, templates, removes, renames)
    component_grades.rs  # Component grades from DCB
    illegal_goods.rs     # Illegal goods from jurisdiction law data
    mission_enhancer.rs  # Mission enrichment from contract generators
    toml/             # Embedded TOML module files
src/                  # Svelte frontend
  routes/+page.svelte # Main UI
  lib/bindings.ts     # Auto-generated TypeScript types (specta)
```

## Working with the DataCore (DCB)

### The svarog-datacore API

The DCB (`Game2.dcb` inside `Data.p4k`) is Star Citizen's binary database containing all game configuration. The `svarog-datacore` crate (fork at `E:\vscode\rust\svarog`) provides:

```rust
let db = DataCoreDatabase::parse(&dcb_bytes)?;

// Find records by struct type name
for record in db.records_by_type_containing("EntityClassDefinition") { ... }
for record in db.records_by_type_containing("Jurisdiction") { ... }
for record in db.records_by_type_containing("ContractGenerator") { ... }

// Access record data
let name = record.name();           // e.g. "EntityClassDefinition.altruciatoxin"
let inst = record.as_instance();    // Get Instance for property access

// Instance property access
inst.get_str("fieldName")           // Option<&str>
inst.get_i32("fieldName")           // Option<i32>
inst.get_f32("fieldName")           // Option<f32>
inst.get_bool("fieldName")          // Option<bool>
inst.get_instance("fieldName")      // Option<Instance> — nested struct
inst.get_array("fieldName")         // Option<ArrayIterator> — array of values
inst.get("fieldName")               // Option<Value> — raw value enum

// Resolve a GUID reference to another record
if let Some(Value::Reference(Some(r))) = inst.get("someRef") {
    if let Some(rec) = db.record(&r.guid) {
        // rec is the referenced record
    }
}
```

### Value types in the DCB

```rust
Value::Bool(bool)
Value::Int32(i32) / Int64(i64) / UInt32(u32)
Value::Float(f32) / Double(f64)
Value::String(&str)                    // Plain string
Value::Locale(&str)                    // Localization string (e.g. "@item_Name...")
Value::Enum(&str)                      // Enum choice (e.g. "Title")
Value::Reference(Option<RecordRef>)    // GUID reference to another record
Value::StrongPointer(Option<InstanceRef>)  // Pointer to a pool instance
Value::Class(ClassRef)                 // Inline struct instance
Value::Array(ArrayRef)                 // Array (iterate via get_array)
```

**Important:** `get_str()` handles String, Locale, AND Enum transparently. Use it for most text fields.

### Resolving inline vs pool instances from arrays

Array elements can be `StrongPointer` (pool-based) or `Class` (inline). Always handle both:

```rust
fn to_instance<'a>(db: &'a DataCoreDatabase, val: &Value<'a>) -> Option<Instance<'a>> {
    match val {
        Value::Class(cr) => Some(Instance::from_class_ref(db, cr)),
        Value::StrongPointer(Some(r)) => Some(db.instance(r.struct_index, r.instance_index)),
        _ => None,
    }
}
```

### Localization key resolution

DCB records reference INI keys with an `@` prefix. Always strip it:

```rust
let raw = inst.get_str("Name").unwrap_or("");      // "@item_NameCOOL_AEGS_S01_Bracer"
let key = raw.strip_prefix('@').unwrap_or(raw);     // "item_NameCOOL_AEGS_S01_Bracer"
let display_name = ini.get(key);                     // Some("Bracer")
```

## How to find data for new modules

### Step 1: Explore extracted XML files

Extract game files to a local directory for easy browsing:

```
C:\Games\StarCitizen\Extracted\libs\foundry\records\
```

The directory structure mirrors the DCB record hierarchy. Key locations:

| Data | Location |
|------|----------|
| Ship components | `entities/scitem/ships/{cooler,powerplant,shieldgenerator,quantumdrive,radar}/` |
| Weapons | `entities/scitem/ships/weapons/` and `entities/scitem/weapons/fps_weapons/` |
| Commodities | `entities/commodities/{vice,food,metals,...}/` |
| Missions/contracts | `contracts/contractgenerator/` |
| Contract templates | `contracts/contracttemplates/` |
| Law/jurisdiction | `lawsystem/jurisdictions/` |
| Blueprints | `crafting/blueprintrewards/blueprintmissionpools/` and `crafting/blueprints/` |
| Reputation | `factions/factionreputation/` and `reputation/rewards/` |
| Manufacturers | `scitemmanufacturer/` |
| Resource types | `resourcetypedatabase/` |

### Step 2: Identify the record type

The XML root element tells you the struct type and record name:
```xml
<EntityClassDefinition.COOL_AEGS_S01_Bracer_SCItem RecordId="...">
```
→ struct type: `EntityClassDefinition`, record name: `COOL_AEGS_S01_Bracer_SCItem`

Use `db.records_by_type_containing("EntityClassDefinition")` to find these in the DCB.

### Step 3: Trace the data chain

Most useful data requires following reference chains:

**Component grades:**
```
EntityClassDefinition → Components[] → SAttachableComponentParams
  → AttachDef → Grade (int), Size (int), Type (str)
  → AttachDef → Localization → Name ("@item_Name...") → INI lookup
  → AttachDef → Localization → Description ("@item_Desc...") → parse "Class: Military"
```

**Illegal goods:**
```
Jurisdiction → prohibitedResources[] → Reference → ResourceType record
  → displayName ("@items_commodities_...") → INI lookup
Jurisdiction → controlledSubstanceClasses[] → Class → resources[] → same chain
```

**Mission blueprints:**
```
ContractGenerator → generators[] → contracts[]/introContracts[]
  → paramOverrides → stringParamOverrides[] → ContractStringParam{param: "Title", value: "@key"}
  → contractResults → contractResults[] → BlueprintRewards → blueprintPool → Reference
    → BlueprintPoolRecord → blueprintRewards[] → blueprintRecord → Reference
      → CraftingBlueprintRecord → blueprint → processSpecificData → entityClass → Reference
        → EntityClassDefinition → Components[] → SAttachableComponentParams → Localization → Name
```

**Mission shareability:**
```
Contract/CareerContract → template → Reference → ContractTemplate
  → contractClass → additionalParams → canBeShared (bool)
```

### Step 4: Write a debug binary

For exploring unfamiliar DCB structures, create a temporary binary in `src/bin/`:

```rust
// src/bin/debug_something.rs
use svarog_datacore::{DataCoreDatabase, Instance, Value};
// ... open Data.p4k, extract Game2.dcb, parse ...

for record in db.records_by_type_containing("SomeType") {
    let inst = record.as_instance();
    for prop in inst.properties() {
        eprintln!("{}: {:?}", prop.name, std::mem::discriminant(&prop.value));
    }
}
```

Run with `cargo run --bin debug_something`. **Delete the file when done** — leaving multiple binaries causes Tauri to fail with "could not determine which binary to run."

### Step 5: Validate against extracted XML

Always cross-reference DCB data with the extracted XML to verify you're reading the right fields. The XML is human-readable and shows the full structure. Some gotchas:

- **Inline vs nested:** XML shows `<paramOverrides><stringParamOverrides>` as nesting, but in DCB `paramOverrides` is a `Class` (inline struct) that you access with `get_instance("paramOverrides")`
- **Enum fields:** XML shows `<param>Title</param>` but DCB stores it as `Value::Enum("Title")`, not `Value::String`
- **Locale fields:** XML shows `<value>@some_key</value>` but DCB stores it as `Value::Locale("@some_key")`, not `Value::String`. Use `get_str()` which handles both
- **Reference resolution:** XML shows `<ReferencedFile>file://...path.xml</ReferencedFile>` but DCB stores a GUID. Use `db.record(&guid)` to resolve

## Design principles

### Only show what we know

All extracted values should use `Option<T>`. Only display data when we successfully resolved it from the DCB. Never use `unwrap_or` with a guessed default — if we can't resolve it, leave it as `None` and skip it in the output.

### Only patch referenced keys

The code module should only patch INI keys that are directly referenced in the DCB via localization fields (e.g. `@item_NameCOOL_AEGS_S01_Bracer`). Don't guess alternate key formats — that's the `key_fixes` module's job.

### Module independence

Each module should be independently toggleable. Modules should not depend on each other's patches. If two modules need to patch the same key, the last one wins (module order in registry determines priority).

## Build & test

```bash
cd src-tauri
cargo test              # Run all tests (43 currently)
cargo check             # Fast compile check
pnpm tauri dev          # Full app with hot reload (from project root)
```

Tests cover: patch application correctness, no data loss, TOML module parsing, key patterns, template captures, value conditions, renames, module conflicts, and all embedded modules parsing.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `svarog-p4k` | P4K archive extraction (path dep: `E:\vscode\rust\svarog`) |
| `svarog-datacore` | DCB game database parsing (path dep: `E:\vscode\rust\svarog`) |
| `tauri` + `tauri-specta` | App framework + TS type generation |
| `specta` + `specta-typescript` | Type-safe Rust↔JS bridge |
| `encoding_rs` | UTF-16 LE decoding |
| `regex` | Launcher log parsing, key patterns |
| `toml` + `serde` | TOML module parsing |
| `dirs` | %APPDATA% path resolution |

## Reference projects

- **svarog fork:** `E:\vscode\rust\svarog` — all extraction crates
- **sc-damage-calculator:** `E:\vscode\rust\sc-damage-calculator` — reference for DCB extraction patterns (weapons, shields, ships)
- **streamdeck-starcitizen:** `E:\vscode\streamdeck\streamdeck-starcitizen` — reference for SC install discovery and svarog usage
- **ScCompLangPackRemix:** `E:\repros\ScCompLangPackRemix` — Python-based fork that derives component data from Game2.dcb
