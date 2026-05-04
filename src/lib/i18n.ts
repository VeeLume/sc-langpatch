import * as m from "$lib/paraglide/messages";
import type { AppError, AppWarning } from "$lib/bindings";
import {
  getLocale,
  setLocale,
  locales,
  localStorageKey,
  type Locale,
} from "$lib/paraglide/runtime";

export { m, getLocale, setLocale, locales };
export type { Locale };

/**
 * Set the active locale, or clear the override to follow the OS preference
 * (`preferredLanguage` strategy). Reloads the page so all `m.*` calls re-render
 * — Paraglide messages are resolved at call time, not reactively.
 */
export function selectLocale(locale: Locale | "system"): void {
  if (locale === "system") {
    localStorage.removeItem(localStorageKey);
    window.location.reload();
  } else {
    setLocale(locale);
  }
}

/** Whether the user has an explicit language override stored. */
export function hasExplicitLocale(): boolean {
  return localStorage.getItem(localStorageKey) !== null;
}

/**
 * Look up a translated module / option / choice label by its backend id.
 * Falls back to the English string from the backend if no translation exists.
 */
function lookup(key: string, fallback: string): string {
  const fn = (m as Record<string, unknown>)[key];
  return typeof fn === "function" ? (fn as () => string)() : fallback;
}

export function moduleName(id: string, fallback: string): string {
  return lookup(`module_${id}_name`, fallback);
}

export function moduleDescription(id: string, fallback: string): string {
  return lookup(`module_${id}_description`, fallback);
}

export function optionLabel(
  moduleId: string,
  optionId: string,
  fallback: string
): string {
  return lookup(`option_${moduleId}_${optionId}_label`, fallback);
}

export function optionDescription(
  moduleId: string,
  optionId: string,
  fallback: string
): string {
  return lookup(`option_${moduleId}_${optionId}_description`, fallback);
}

export function choiceLabel(
  moduleId: string,
  optionId: string,
  value: string,
  fallback: string
): string {
  return lookup(`choice_${moduleId}_${optionId}_${value}_label`, fallback);
}

/**
 * Render a structured backend error as a localized string. Each
 * `code` maps to a `error_<code>` Paraglide message; the `data` shape
 * matches the message's expected placeholders.
 *
 * Adding a new variant on the backend without updating this switch
 * is a typecheck error (TypeScript exhaustiveness via `never`).
 */
export function formatError(err: AppError): string {
  switch (err.code) {
    case "discovery_failed":
      return m.error_discovery_failed({ message: err.data.message });
    case "task_join_failed":
      return m.error_task_join_failed({ message: err.data.message });
    case "p4k_open_failed":
      return m.error_p4k_open_failed({
        path: err.data.path,
        message: err.data.message,
      });
    case "global_ini_not_found":
      return m.error_global_ini_not_found();
    case "ini_decode_failed":
      return m.error_ini_decode_failed({ message: err.data.message });
    case "output_write_failed":
      return m.error_output_write_failed({ message: err.data.message });
    case "output_remove_failed":
      return m.error_output_remove_failed({ message: err.data.message });
    case "unexpected":
      return m.error_unexpected({ message: err.data.message });
    default: {
      const _exhaustive: never = err;
      return String(_exhaustive);
    }
  }
}

/**
 * Render a structured backend warning as a localized string. Same
 * exhaustiveness contract as `formatError`. Module-level warnings
 * resolve `module_name` against the i18n catalog so the prefix is
 * also localized — falls back to the english name from the backend.
 */
export function formatWarning(w: AppWarning): string {
  switch (w.code) {
    case "language_pack_load_failed":
      return m.warning_language_pack_load_failed({ message: w.data.message });
    case "language_pack_decode_failed":
      return m.warning_language_pack_decode_failed({ message: w.data.message });
    case "module_skipped_no_datacore":
      return m.warning_module_skipped_no_datacore({
        module_name: moduleName(w.data.module_id, w.data.module_name),
      });
    case "module_skipped_no_locale":
      return m.warning_module_skipped_no_locale({
        module_name: moduleName(w.data.module_id, w.data.module_name),
      });
    case "module_rename_failed":
      return m.warning_module_rename_failed({
        module_name: moduleName(w.data.module_id, w.data.module_name),
        message: w.data.message,
      });
    case "module_patch_failed":
      return m.warning_module_patch_failed({
        module_name: moduleName(w.data.module_id, w.data.module_name),
        message: w.data.message,
      });
    case "undeclared_replace_dropped": {
      const name = moduleName(w.data.module_id, w.data.module_name);
      return w.data.count === 1
        ? m.warning_undeclared_replace_dropped_one({ module_name: name })
        : m.warning_undeclared_replace_dropped_other({
            module_name: name,
            count: w.data.count,
          });
    }
    case "unexpected":
      return m.warning_unexpected({ message: w.data.message });
    default: {
      const _exhaustive: never = w;
      return String(_exhaustive);
    }
  }
}
