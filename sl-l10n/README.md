# sl-l10n

Locale-aware **value formatting** for Second Life / OpenSim clients: numbers
(grouping separator and decimal mark), the Linden dollar (L$) currency, and
dates / times — all backed by **CLDR through ICU4X** (the `icu` crate family),
so `1234567` renders `1,234,567` in English, `1.234.567` in German,
`1 234 567` in Polish and `١٬٢٣٤٬٥٦٧` in Arabic, from the same call.

It is the value-formatting counterpart to the *text* side of localisation (the
viewer's Fluent scaffold, `viewer-i18n-fluent-scaffold`): Fluent decides the
words, the plural branch and the gender; this crate decides how the numbers and
dates *inside* those words are written for the locale. The split exists because
`fluent-rs` implements only the *selection* half of its `NUMBER()` /
`DATETIME()` builtins (enough to drive a plural), not the *formatting* half — a
number interpolated into a Fluent string comes out with a bare `to_string()`,
with no grouping, no locale decimal mark and no digit shaping.

Like the other geometry / asset crates (`sl-prim`, `sl-material`, …) it is
**pure**: no Bevy, no I/O. The CLDR data is compiled into the binary by ICU4X's
`compiled_data` feature, so there is nothing to load at runtime.

## What it formats

- **Numbers** — integers and decimals with the locale's grouping separator,
  decimal mark and (where the locale uses them) shaped digits. This is what a
  viewer full of L$ balances, object counts, coordinates and distances needs.
- **Currency (L$)** — the grouped amount plus the Linden-dollar symbol. The
  *number* is fully locale-formatted; the symbol and its placement are left to
  the caller / the Fluent bundle, because L$ is not a CLDR currency with its own
  per-locale placement data (see `LocaleFormatters::currency_l`).
- **Dates & times** — a civil date / time formatted at a chosen length
  (short / medium / long) for the locale. Time-zone conversion (UTC → local or
  → SLT/Pacific) is the caller's concern; this crate formats the civil
  components it is handed.
- **Parsing** (the inverse) — `parse_number` reads a value a user typed in the
  locale's conventions back into an `f64`, so a localized float input field
  accepts `1.234,5` in German and `١٬٢٣٤٫٥` in Arabic. The locale's digits and
  separators are probed once from the formatter, so parsing stays consistent
  with formatting.

Full CLDR **unit** formatting (distance in metres vs feet, area, file sizes)
needs ICU4X's still-unstable `icu_experimental`; it is deliberately out of scope
here (the task marks it lower priority). A caller that needs a unit today
formats the magnitude with [`LocaleFormatters::decimal`] and appends a
bundle-supplied unit label.

## Why ICU4X, not a hand-rolled table

The grouping rules are not "insert a comma every three digits". Indic locales
group `12,34,567`; some locales group only above four digits; the decimal mark,
the separator and the digits themselves all vary, and the CLDR data that
encodes this is what ICU4X carries. A hand-rolled table would be the same
twenty-year retrofit the reference viewer's `LLResMgr::getMonetaryString` is.

## Locale input

Formatters are built for one [`icu_locale_core::Locale`], parsed from a BCP-47
tag (`"en"`, `"pl"`, `"ja"`, `"ar-EG"`). A caller holding a `unic_langid`
`LanguageIdentifier` (as the viewer does) converts by formatting it to its tag
string and parsing that — the two agree on BCP-47.
