# Launcher text

Every piece of text the launcher shows, one file per language. `en.ts` is the source: every other
language has exactly its keys and the same `{placeholders}`, and no text is empty (`test/ui.test.ts`
checks all three).

## Adding a language

1. Copy `en.ts` to `<code>.ts`, where `<code>` is the two-letter language code (ISO 639-1). Rename the
   object, type it `Record<StringKey, string>` as the other languages do, and translate the text.
   Keys, `{placeholders}` and the brand names stay as they are.
2. Add the code to `REGISTRY` in `index.ts`, where you want it in the settings menu.

The game's own text languages are a separate, fixed list (`GAME_LANGUAGES` in `../types.ts`): the 1.4.4
client has text for those only, and "auto" follows the operating system, not the launcher. A new
launcher language does not change it.

Nothing else changes: `Language` and `LANGUAGES` come from the registry, the settings menu lists the
registry, and the language name comes from the `lang_<code>` key, which each file must have.
