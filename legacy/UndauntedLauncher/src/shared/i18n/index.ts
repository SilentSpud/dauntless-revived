// Every piece of UI text, one file per language (MIGRATION_PLAN.md L13). Keys are shared; tests check that
// every language has exactly the same keys and the same {placeholders}. To add a language, see README.md.

import de from "./de";
import en, { type StringKey } from "./en";
import es from "./es";
import fi from "./fi";
import fr from "./fr";
import it from "./it";
import ja from "./ja";
import pt from "./pt";
import ru from "./ru";

export type { StringKey };

// The registry: the launcher's languages, in the order the settings menu lists them. Language and
// LANGUAGES (shared/types.ts) come from here.
const REGISTRY = { en, fi, de, es, fr, it, ja, pt, ru };

export type Language = keyof typeof REGISTRY;
export const LANGUAGES = Object.keys(REGISTRY) as readonly Language[];

export const STRINGS: Record<Language, Record<StringKey, string>> = REGISTRY;

export function translate(lang: Language, key: StringKey, vars?: Record<string, string | number>): string {
  const table = STRINGS[lang] ?? STRINGS.en;
  let text = table[key] ?? STRINGS.en[key] ?? key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) text = text.split(`{${k}}`).join(String(v));
  }
  return text;
}

export function isStringKey(k: string): k is StringKey {
  return Object.prototype.hasOwnProperty.call(en, k);
}

// The language a first start picks from the operating system's locale ("es-ES", "fi", "en-US"...).
export function systemLanguage(locale: string): Language {
  const code = locale.toLowerCase().slice(0, 2);
  return LANGUAGES.find((l) => l === code) ?? "en";
}
