---
id: viewer-i18n-fluent-scaffold
title: i18n scaffold (Project Fluent via bevy_fluent)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-i18n-localization
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The internationalisation foundation, sequenced deliberately right after the UI
scaffold and **ahead of every UI-bearing panel**, so panels are authored
translatable from day one rather than retrofitted. Integrate **`bevy_fluent`**
(Project Fluent `.ftl` behind Bevy assets, runtime locale switching), load the
string bundles, and expose a string-lookup API usable by every panel.

Critically, the lookup must pass **typed named arguments** (numbers via Fluent
`NUMBER()`, gender, names) into Fluent so `.ftl` selectors resolve
singular/plural/gender correctly — e.g. a field label that reflects a count —
`{ $count -> [one] … *[other] … }`. The API takes named typed args,
**never a pre-formatted string**. Expose the current locale as a resource that
also carries the locale's **LTR/RTL direction**, to drive the layout and skin
([[viewer-ui-skin-tokens]] logical properties + `direction`).

**Do not copy** `LLTrans::getCountString` (a hardcoded if-ladder over three
languages, wrong for Polish — which ships); Fluent's plural rules are per-locale
and correct.

The bundle must also carry the **locale's typographic conventions**, not just
prose — punctuation the UI inserts itself, which differs by language and is a
translator's call, not a hardcoded literal. The first concrete case already
exists: the **truncation ellipsis** the tab widget appends to a clipped label
([[viewer-ui-tab-widget]] `TabSpec::ellipsis`, defaulting to Latin `…`) —
Chinese and Japanese conventionally use a centred six-dot `……` instead. So
expose it as a translatable key (e.g. `ui-ellipsis`) that widgets read from the
bundle rather than a per-call literal, and audit for the same shape as more
chrome lands (quotation marks, list separators, the `:` after a field label).

Locale detection/override is [[viewer-i18n-locale-selection]]; sending the
language to the grid is [[viewer-i18n-agent-language]]; chat MT is
[[viewer-i18n-chat-translation]].

Reference (Firestorm, read-only): `newview/skins/default/xui/<lang>/strings.xml`
(18+ languages), `lltrans`.

## Done

New module `src/i18n.rs` (`ViewerI18nPlugin`), registered right after
`ViewerUiPlugin` (it reads `UiRoot` / `UiScaffoldSystems`). Locale bundles under
`assets/locales/<lang>/main.ftl(.ron)`.

- **`bevy_fluent` 0.15.0 targets `bevy = "0.19"`** — despite the version number
  it is NOT one-behind; it builds and runs clean against our Bevy 0.19. Pulled
  in with `fluent` 0.16, `fluent_content` 0.0.5 and `unic-langid` 0.9 (its
  runtime types; `bevy_fluent` re-exports only `Locale` / `Localization` /
  `LocalizationBuilder` / `FluentPlugin`). Model: a `.ftl.ron` manifest per
  locale (`locale:` + `resources:`) → `BundleAsset`; a `LoadedFolder` of them is
  negotiated by `LocalizationBuilder` into the `Localization` lookup resource,
  which falls back down the chain to `en` for any untranslated key.

- **`Translator` (a `SystemParam`) is the panel-facing API**: `get(key)` and
  `format(key, &TransArgs)`. `TransArgs` is a typed builder (`int` → number for
  plural/`NUMBER()`, `text` → string for names / gender selector keys) that
  builds a `FluentArgs`; `format` passes it *into* Fluent via
  `Request::new(key).args(...)`, so the `.ftl` plural / gender selectors resolve
  against the real values. A missing key returns the key itself (Fluent
  convention: visible, not blank). **A float setter was deliberately dropped** —
  a bare `f64` renders with no locale grouping, which is
  [[viewer-i18n-number-datetime-formats]]'s job (see below); integer counts are
  the only numeric case (and must stay integers, or CLDR sees a visible
  fraction and picks the wrong plural).

- **`UiLocale` resource** carries the active `choice` / `lang`, the layout
  `direction` (derived from the tag by a small RTL script+language table,
  `direction_of`), the resolved `ellipsis` (the `ui-ellipsis` key, refreshed
  from the bundle on every locale change), and a `pseudo` flag.

- **The locale drives `UiDirection`** (`sync_ui_direction`), so an RTL locale
  mirrors the whole layout. The pre-existing `SL_VIEWER_UI_DIRECTION` manual
  knob is preserved as a `DirectionOverride` that still wins when set (new
  `UiDirection::rtl_override_from_env`, which unlike `from_env` distinguishes
  "unset → locale drives" from "forced ltr/rtl").

- **The tab widget reads `ui-ellipsis` from the bundle.** `TabSpec::ellipsis`
  (`DEFAULT_ELLIPSIS = …`) stays as the static fallback; a new
  `TabEllipsisMarker` on each ellipsis node + `apply_locale_ellipsis` rewrite
  every marker to the locale's ellipsis (CJK `……`) on a locale change, and
  freshly-spawned markers via an `Added` filter.

- **Pseudolocalisation folded in as a pseudo-*locale*** (as `ui_pseudoloc`'s doc
  promised): `LocaleChoice::Pseudo` looks up in the `en` bundle and
  post-processes every result through `pseudolocalise`, so one switch turns the
  whole UI pseudo. The gallery / `ui_test` matrix's own use of `pseudolocalise`
  (the `SampleText::Pseudo`/`Script` cells) is untouched and independent — both
  share the one transform function.

- **A reusable `Translated { key }` component + `apply_translations` system**:
  put it on any `Text` node and its text stays resolved from the key — filled in
  once the async bundle loads, re-resolved on a locale switch, and localized the
  frame a fresh label spawns. This is the mechanism for **static**
  (argument-free) labels, including ones a panel does not own the spawn of: the
  tab widget grew a `TabSpec::translate_labels` flag that binds each label node
  to its key, and the floater exposes its `title_text` entity so a caller can
  bind the title.

- **The inventory window is wired as the first real consumer**: its floater
  title, the Everything / Recent / Worn tab labels, and the Expand-all /
  Collapse-all toolbar buttons all resolve from `inventory-*` keys (added to all
  four bundles). It had to use the live-updating `Translated` path rather than a
  spawn-time lookup because the panel is spawned once at startup, *before* the
  async bundles finish loading — a spawn-time `Translator::get` would have
  captured the key, not the translation. Debug overlays (FPS / pipeline stats)
  and chat are intentionally left untranslated — they are not in final form.

- **A live consumer, the `F6` demo panel** (modelled on the `F5` scaffold demo):
  cycles locale at runtime (proving the asset rebuild + direction flip +
  ellipsis refresh) and drives `Translator::format` with a live count and gender
  (proving typed args + per-locale plural / gender). Seeded by
  `SL_VIEWER_UI_LOCALE` (`en`/`ja`/`ar`/`pl`/`pseudo`).

- **Shipped bundles, chosen for what each proves:** `en` (base + fallback),
  `ja` (CJK `……` ellipsis + a plural-less language), `ar` (RTL direction + all
  six CLDR plural categories), `pl` (the `one`/`few`/`many` case the reference
  viewer's if-ladder gets wrong). Unit tests build these bundles from the same
  `.ftl` (via `include_str!`) and assert plural/gender/direction, so the
  "correct for Polish" claim is proven client-side without a grid.

- **Follow-ups filed** (this task surfaced them):
  [[viewer-i18n-number-datetime-formats]] (blocked — `fluent-rs`'s `NUMBER()` /
  `DATETIME()` only do plural *selection*, not locale-aware *formatting*;
  needs an ICU-backed formatter), [[viewer-input-keyboard-layout-bindings]]
  (ready — WASD-position vs. letter-label bindings across AZERTY/QWERTZ/Dvorak),
  and two accessibility/culture ideas: [[viewer-i18n-cultural-color-meanings]]
  and [[viewer-i18n-colorblind-accessibility]].

Verified: 378 viewer lib tests pass, clippy clean under the workspace
restriction lints. Live-run against OpenSim to eyeball the `F6` panel switching
locale (title + plural + direction + `……` ellipsis) — the panel is pure
client-side UI, so it needs no grid content, only a login to bring the window
up.
