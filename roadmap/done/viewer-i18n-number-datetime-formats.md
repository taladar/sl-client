---
id: viewer-i18n-number-datetime-formats
title: Locale-aware number, currency & date/time formatting
topic: viewer
status: done
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

## Done

New pure crate **`sl-l10n`** (ICU4X 2.2) plus the viewer wiring. The value side
of localisation, the counterpart to the scaffold's text side.

**Scope deviation from the plan (the one thing done differently).** The task
suggested registering a `NUMBER()` / `DATETIME()` Fluent *function* on each
`FluentBundle`, so `.ftl` authors write `{ NUMBER($balance, useGrouping:
"always") }`. That is **infeasible with `bevy_fluent` 0.15**: it builds each
`FluentBundle` inside its async asset loader and stores it in an immutable
`Arc<FluentBundle>` (`BundleAsset`), with no hook to add a function — and
`LocalizationBuilder` only clones those finished bundles into the `Localization`
lookup. Registering a function would mean forking `bevy_fluent`. Instead the
formatting is exposed through the scaffold's **`Translator`** as typed Rust
helpers (`integer` / `decimal` / `currency_l` / `datetime` / `parse_number`),
which is the task's "plus typed Rust helpers for the common cases" made the
primary surface. A panel that wants a value *inside* a Fluent string formats it
with a helper and passes the result as a string argument (`{ $balance }`) — the
value is locale-formatted by ICU, the words around it by Fluent.

- **`sl-l10n` (pure, no Bevy, no I/O).** `LocaleFormatters::from_tag("pl")` →
  `integer` / `decimal(value, fraction_digits)` / `currency_l` /
  `datetime(CivilDateTime, DateTimeStyle, DateTimeLength)` / `parse_number`.
  Backed by `icu_decimal` (grouping + decimal mark), `icu_datetime` +
  `icu_calendar` + `icu_time` (date/time), `fixed_decimal` (the `Decimal` value,
  `ryu` feature for the `f64` entry point). CLDR data is compiled in
  (`compiled_data`, a default feature), so the crate stays I/O-free.
- **`icu_provider`'s `sync` feature is load-bearing.** ICU4X formatters use
  `Rc` internally by default (`CartableOptionPointer<Rc<…>>`), so
  `DecimalFormatter` is **not `Send + Sync`** and cannot sit in a Bevy
  resource. `sync` switches the internal pointer to `Arc`; the whole `icu`
  graph shares this one
  `icu_provider`, so enabling it once makes every formatter thread-safe. A
  `formatters_are_send_and_sync` guard test pins the invariant.
- **Input parsing was added** (raised mid-task): `parse_number` reads a value a
  user typed in the locale's conventions (`1.234,5` in German, Arabic-Indic
  digits in `ar`) back to an `f64`, for a localized float field. The digits and
  separators are **probed once** from the decimal formatter's own output, so
  parsing can never disagree with formatting across ICU versions.
- **Currency (L$)** formats the amount locale-correctly and prefixes `L$`; L$ is
  not a CLDR currency with per-locale placement data, so the symbol/placement is
  a documented default (a bundle can override placement via `{ $amount }` word
  order). **Units** are out of scope by design: SL is **metric everywhere**
  (metres even in the US), so the only unit worth formatting is the metre value
  itself — which goes through `decimal` — and full CLDR unit *system* selection
  (m vs ft, area, file size) would need the still-unstable `icu_experimental`.
  (Confirmed with the maintainer: SL deals only in metres, time and derived
  units like speed / acceleration, so per-locale unit systems never arise.)
- **Viewer wiring.** `LocaleFormatting` resource (rebuilt on a locale change via
  `maintain_value_formatters`, bridging `unic_langid` → ICU on the BCP-47 tag);
  the `Translator` `SystemParam` gains the helper methods (value formatting is
  **not** pseudolocalised — a number's digits are not the length axis the
  pseudolocale tests). The F6 demo grew four value lines + a parse round-trip
  line, so number grouping, the decimal mark, currency, date/time order and
  input parsing all switch live per locale (en / ja / ar / pl / pseudo).
- **No user-facing numeric panels exist yet** to convert (coordinates / counts
  today live only in monospace debug overlays, where grouping would break column
  alignment); the helpers are landed *ahead* of the panels, the same "infra
  before the hundredth panel retrofit" rationale as the scaffold. New numeric
  readouts (a location bar, an L$ balance, an object count) adopt a one-line
  `translator.integer/decimal/currency_l` call as they land.
