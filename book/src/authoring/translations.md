# Writing Translations

The viewer's user-facing strings are translated with
[**Project Fluent**](https://projectfluent.org/), loaded through
[`bevy_fluent`](https://github.com/kgv/bevy_fluent). Every UI panel looks its
strings up by **key** rather than embedding an English literal, so a new locale
ships as a set of `.ftl` files without any panel changing.

This chapter is the translator's reference. Fluent's own documentation covers
the syntax; here we concentrate on **how this project uses Fluent** and the few
places it deliberately differs.

> **Start with the Fluent docs.** If you have not written Fluent before, read
> the [Fluent Syntax Guide][fluent-syntax] first — it is short, and everything
> below assumes you know messages, terms, selectors and variables. The
> [Fluent Playground][fluent-playground] lets you experiment live.

## Where translations live

```text
sl-client-bevy-viewer/assets/locales/
├── en/
│   ├── main.ftl        # the English strings (the base + fallback)
│   └── main.ftl.ron    # the bundle manifest (locale tag + resource list)
├── ja/  (main.ftl, main.ftl.ron)
├── ar/  (main.ftl, main.ftl.ron)   # right-to-left
└── pl/  (main.ftl, main.ftl.ron)   # a four-plural-category language
```

- **`main.ftl`** is the message bundle: `key = value` pairs.
- **`main.ftl.ron`** is a small [`bevy_fluent`] manifest that names the locale
  and lists its `.ftl` resources (paths relative to the manifest):

  ```ron
  (
      locale: "en",
      resources: [ "main.ftl" ],
  )
  ```

**English (`en`) is the base and the fallback.** Any key missing from another
locale falls back to English, so a partial translation is safe to ship —
untranslated strings simply appear in English.

## Translating an existing locale

For a language the viewer already lists (`en`, `ja`, `ar`, `pl`):

1. Open that locale's `main.ftl`.
2. Translate each value.
   **Keep the keys and the `{ $variable }` names exactly as in English** — they
   are the contract with the code. Only the text changes.
3. Adjust plural/gender **branches** to your language's categories (see below).
4. Save. With `--watch-skins`-style asset watching the running viewer reloads; a
   normal run picks it up on the next launch.

Adding a **brand-new language** (a tag not yet listed) currently also needs a
small Rust change — the locale set is an enum in `src/i18n.rs` (`LocaleChoice`)
plus its switcher — so open an issue or a PR for the new tag; the `.ftl` files
alone are not yet auto-discovered.

## Fluent as this project uses it

### Variables are typed, and their type matters

The code passes each argument as a **number** or a **string**:

- A **number** (a count, an amount) is what a **plural selector** branches on,
  and what value formatting sees. Reference it bare: `{ $count }`.
- A **string** (a name, a place) is inserted **verbatim** — never translated.

```ftl
# A verbatim string argument. Fluent wraps the inserted run in Unicode bidi
# isolation marks automatically, so a right-to-left name stays intact inside a
# left-to-right sentence (and vice-versa) — you do not add any marks yourself.
greeting = Hello, { $name }!
```

### Plurals — use CLDR categories, not an if-ladder

Fluent picks the plural branch from **your locale's CLDR rules**, so you author
only the categories your language actually has:

```ftl
# English: two categories (one / other).
items-selected =
    { $count ->
        [one] { $count } item selected
       *[other] { $count } items selected
    }
```

```ftl
# Japanese: a single category — no branching needed.
items-selected = { $count } 個を選択中
```

Polish and Arabic have more categories (`few`, `many`, …); add the branches your
language needs. The `*` marks the **default** branch and is required. See
[Fluent selectors][fluent-selectors] and the [CLDR plural rules][cldr-plurals]
for which categories your language uses. This replaces the reference viewer's
hardcoded three-language `if`-ladder — do **not** hardcode plural logic in the
string.

### Gender and other selectors

Selectors are not only for plurals — a typed string argument can drive a gender
choice, for example:

```ftl
friend-status =
    { $gender ->
        [male] He is online
        [female] She is online
       *[other] They are online
    }
```

### Numbers, currency and dates are formatted by the code — not in the `.ftl`

This is the **main departure from stock Fluent.** Fluent ships `NUMBER()` and
`DATETIME()` built-ins, but they are limited; the viewer formats values with a
full **CLDR/ICU** stack (the `sl-l10n` crate) in Rust instead. In practice:

- **Do not** wrap display values in `NUMBER()` / `DATETIME()` in your `.ftl`.
- A grouped number, a currency amount (L$ 12,345) or a date is **formatted by
  the code** for your locale and handed in as an already-formatted **string** —
  place it with a bare `{ $balance }` / `{ $when }`.
- A **bare number argument is still fine for a plural selector** — that is what
  the selector needs. The rule is only about *display formatting*: leave
  grouping, currency symbols and date patterns to the code.

So: `{ $count }` in a plural branch, yes; `NUMBER($count, …)` to format a big
balance, no — the code already did that.

### Typographic punctuation is translatable

Punctuation the UI inserts *itself* is a translator's call, exposed as a key —
not a hardcoded literal. The clearest example is the truncation ellipsis:

```ftl
# Latin: a single horizontal ellipsis.
ui-ellipsis = …
```

```ftl
# Japanese overrides it with the centred six-dot CJK form.
ui-ellipsis = ……
```

If your language has its own convention for such punctuation, override the key.

### `language-name` — the endonym

Every locale defines `language-name` as its **own** name in its **own** script
(`English`, `日本語`, `العربية`, `Polski`). The locale switcher shows this, so
each language names itself. Always translate it to the endonym, never leave it
in English.

## Direction (left-to-right / right-to-left)

A locale's writing direction is derived from its language automatically (Arabic
and Hebrew are right-to-left; Latin, CJK and Cyrillic are left-to-right). You do
**not** set direction in the `.ftl`. The **layout** mirrors for free (the UI
uses logical, direction-aware layout — see [Writing Skins & Themes](skins.md)),
and Fluent isolates inserted arguments so mixed-direction text stays correct.
Your job is just the translated text.

## The pseudolocale

There is a built-in **pseudolocale** (select it with
`SL_VIEWER_UI_LOCALE=pseudo`, or cycle to it in the i18n demo). It is
**not a language** — it runs the English strings through an accenting/expanding
transform to surface hardcoded strings (they stay un-accented) and layouts that
break when text grows. You never write a `.ftl` for it; it exists to test that
the UI is *translatable*. Note that value formatting (numbers, dates) is
deliberately **not** pseudolocalised — the axis it tests is string length.

## Checklist

- Keys and `{ $variable }` names identical to English.
- Plural/gender branches match your language's CLDR categories, with a `*`
  default.
- `language-name` is the endonym.
- No `NUMBER()` / `DATETIME()` for display formatting — leave it to the code.
- Punctuation keys (`ui-ellipsis`, …) follow your language's convention.
- Untranslated keys are fine — they fall back to English.

## Reference

- [Project Fluent][fluent] — the overview.
- [Fluent Syntax Guide][fluent-syntax] — messages, terms, selectors, variables.
- [Fluent Playground][fluent-playground] — experiment live.
- [Fluent selectors][fluent-selectors] and [CLDR plural rules][cldr-plurals] —
  which plural categories your language needs.
- [`bevy_fluent`][bevy-fluent] — how the bundles are loaded.

[fluent]: https://projectfluent.org/
[fluent-syntax]: https://projectfluent.org/fluent/guide/
[fluent-playground]: https://projectfluent.org/play/
[fluent-selectors]: https://projectfluent.org/fluent/guide/selectors.html
[cldr-plurals]: https://www.unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html
[bevy-fluent]: https://github.com/kgv/bevy_fluent
