# Translating SC LangPatch

The SC LangPatch UI lives in two JSON catalogs at `messages/`:

```
messages/
  en.json    ← base catalog (source of truth)
  de.json    ← German translation
```

Adding a new language is purely additive — drop in a new file, update one config line, open a PR. No frontend or Rust changes required.

## Adding a new locale (step-by-step)

### 1. Pick a locale code

Use a [BCP 47 language tag](https://www.iana.org/assignments/language-subtag-registry/language-subtag-registry) — usually a two-letter [ISO 639-1](https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes) code like `fr` (French), `es` (Spanish), `pl` (Polish), `pt-BR` (Brazilian Portuguese). Stick to the lowercase form that appears in the language tag registry.

### 2. Copy the base catalog

```bash
cp messages/en.json messages/<your-locale>.json
```

So for French: `messages/fr.json`.

### 3. Translate the values

Open the new file and translate every value (the part on the right of each `"key": "value"`). **Do not change the keys** — they're consumed by the app code and renaming one will break the build.

A few rules:

- **Placeholders like `{message}`, `{module_name}`, `{count}`, `{path}`** must be preserved exactly — they're substituted at runtime. You can move them around in the sentence (e.g. German word order), but the braces and the name inside them must stay.
- **`<code>` tags inside `_html` keys** are rendered as inline code styling. Keep them where they make sense in the translated sentence.
- **`<br>` tags** inside `_html` keys are rendered as line breaks. Use them as your reference catalog does.
- **`<strong>` tags** mark inline emphasis. Wrap the equivalent words in your translation.
- **Plural forms** — keys ending in `_one` and `_other` are pluralization variants. `_one` is used when the count is exactly 1; `_other` otherwise. If your language has more plural categories (Polish, Russian, Arabic, …) and the existing two-form scheme produces awkward output, open an issue — we'll switch the message to ICU MessageFormat.

Don't translate:

- Tag/badge codes that mirror the in-game patch markup: `[Solo]`, `[Uniq]`, `[BP]`, `[BP*]`, `[BP?]`, `[Illegal]`, `[CS Risk]`, `MIL1C`, `S3`, `[EM]`, `[IR]`, `[CS]`, `[!]`. These appear inside example strings — keep them verbatim. (The actual text written into the game `global.ini` is **not** translated by the app — that comes from the community language pack the user loads.)
- File names: `global.ini`, `Data.p4k`, `user.cfg`, `Game2.dcb`.
- Channel names: `LIVE`, `PTU`, `EPTU`, `TECH-PREVIEW`.
- The schema URL at `"$schema"` — leave it as-is.

### 4. Register the locale

Edit [`project.inlang/settings.json`](project.inlang/settings.json) and add your code to the `locales` array:

```json
{
  "$schema": "https://inlang.com/schema/project-settings",
  "baseLocale": "en",
  "locales": ["en", "de", "fr"],
  ...
}
```

### 5. Wire the drift test

Edit [`src-tauri/src/tests.rs`](src-tauri/src/tests.rs) and add your code to the `TRANSLATED_LOCALES` slice in the `i18n_catalog` test module:

```rust
const TRANSLATED_LOCALES: &[&str] = &["de", "fr"];
```

This test asserts your catalog has exactly the same keys as `en.json` — no missing translations, no stale keys. Run it locally with:

```bash
cd src-tauri
cargo test i18n_catalog
```

If it fails, the error message tells you which keys are missing or extra.

### 6. Verify in the app

```bash
pnpm tauri dev
```

Click the gear icon top-right; your new locale should appear in the dropdown automatically (rendered as its [autonym](https://en.wikipedia.org/wiki/Autonym) — e.g. "Français" for `fr`, "Polski" for `pl`). Pick it and walk through every screen to confirm wording fits the layout (German tends to run ~30% longer than English; languages like Hungarian or Finnish even more — watch button widths).

### 7. Open a pull request

Title it something like `i18n: add French translation`. The CI will run the drift test plus all other tests; if those pass the PR is good to merge.

## Updating an existing translation

Same process, minus steps 1–2 and 4–5. Edit your locale's JSON, run the drift test, open a PR.

When new features land in the app, new keys appear in `en.json` and the drift test starts failing for translated locales until they catch up. If you want to be pinged for new strings, watch the repo and look for PRs that touch `messages/en.json`.

## What gets translated, and what doesn't

**Translated by these catalogs (UI chrome):**
- All buttons, headings, labels, hints, tooltips
- Module names, descriptions, option labels, choice labels
- Error messages and warnings surfaced to the user
- Badge text and the legend explaining them

**NOT translated by these catalogs (in-game patch output):**
- The text the patcher writes into `global.ini` — mission descriptions, weapon stats, blueprint lists, encounter blocks, all the title tags `[Solo]` / `[BP]` / etc.
- Component grade prefixes (`MIL1C`, `S3`).
- Illegal-goods markers (`[!]`).

That output is generated against the **community language pack** the user has loaded (or English if none). If you want the in-game text in your language, that's a separate project — load a language pack like [rjcncpt's German INI](https://github.com/rjcncpt/StarCitizen-Deutsch-INI) into the **Community language pack** field. The app's enrichments (`[Solo]`, `Blueprints:`, `Encounters:`, etc.) currently render in English regardless of UI locale; localising the patch output is a possible future feature, not in scope for these JSON catalogs.

## Questions, problems, suggestions?

Open an issue or PR — translator contributions are welcome and credited in the changelog.
