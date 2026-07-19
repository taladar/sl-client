---
id: viewer-i18n-number-datetime-formats
title: Locale-aware number, currency & date/time formatting
topic: viewer
status: ready
origin: raised during viewer-i18n-fluent-scaffold (2026-07)
blocked_by: [viewer-i18n-fluent-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-i18n-fluent-scaffold]] covers the *text* side of localisation — key
lookup, per-locale **plural** and **gender** selection (from CLDR rules), the
pseudolocale, the LTR/RTL layout direction, and the typographic conventions the
UI inserts (the truncation ellipsis). It does **not** cover locale-aware
**formatting of values**, which is a separate l10n concern:

- **Numbers** — the grouping separator and decimal mark differ by locale
  (`1,234.5` / `1.234,5` / `1 234,5`), as do digit shaping (Arabic-Indic
  digits) and sign placement. The scaffold passes numbers into Fluent *typed*
  (so the plural selector sees a number), but Fluent-rs's `NUMBER()` builtin
  does **not** apply locale grouping — the number is emitted with a bare
  `to_string()`. A viewer full of L$ balances, object counts, coordinates and
  distances needs real grouping.
- **Currency (L$)** — grouping plus the symbol's placement/spacing, which is a
  locale call, not a hardcoded `L$` prefix.
- **Dates & times** — Fluent-rs's `DATETIME()` builtin is effectively a stub, so
  chat timestamps, IM logs, group notices, event times, parcel-sale dates etc.
  need a real locale-aware date/time formatter (and time-zone handling).
- **Units** — distance/area (m vs ft), file sizes; lower priority.

The reason this cannot ride on the scaffold's Fluent path as-is: `fluent-rs`
only implements the *selection* side of `NUMBER()`/`DATETIME()` (enough to drive
plurals), not the *formatting* side. This task should back the formatters with a
real CLDR/ICU implementation — the `icu` crate family (`icu_decimal`,
`icu_datetime`, `fixed_decimal`) is the obvious candidate — and expose them
through the scaffold's `Translator` (e.g. a `NUMBER()`/`DATETIME()` function
registered on each `FluentBundle`, so `.ftl` authors write
`{ NUMBER($balance, useGrouping: "always") }` and get correct output), plus
typed Rust helpers for the common cases.

Reference (Firestorm, read-only): `LLLocale`, `LLResMgr::getMonetaryString` /
`getIntegerString` (its own grouping), and the `xui` `format` attributes.
