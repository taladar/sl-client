---
id: viewer-preferences-floater
title: Preferences floater shell + settings store binding
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-ui-settings-binding]
---

Context: [context/viewer.md](../context/viewer.md).

The settings floater **shell**: the tabbed preferences window plus the binding
that wires each control to the persistent, **typed settings store**
([[viewer-ui-settings-binding]]), with sensible **defaults** and **per-account
overrides**. This is the root of the preferences cluster — the individual tabs
plug into the shell and read / write through the same store.

The per-tab content (graphics, audio, chat / privacy, camera / move-and-view,
and the raw debug-settings editor) lives in the sibling tasks that depend on
this one. Note: the input system's key-rebinding tab lives with the input
cluster, not here.

Reference (Firestorm, read-only): `llfloaterpreference*`, `llviewercontrol`
(settings backend), `fspanelprefs`.

Builds on: [[viewer-ui-settings-binding]].

## Done

New viewer module **`src/preferences.rs`** (`PreferencesPlugin`, floater id
`preferences`), opened from Avatar ▸ Preferences… (`Ctrl+P`):

- **Shell**: a resizable floater (760×520 default) with a search box on top,
  a **leading** tab strip (`TabPlacement::InlineStart`, mirrors under RTL,
  draggable divider, width persisted), one scrolling panel per tab, and an
  OK / Cancel footer. Content is built once on first open (deferred), and
  geometry persists per avatar for free via the floater id.
- **Tab registry**: a static `PREF_TABS` list (`PreferencesTabDef { id,
  label_key, build }`) in the `TOP_MENU_BAR` / `ELEMENTS` idiom — a sibling
  tab task appends one entry and provides a build `fn`; the shell gives its
  rows snapshot / revert, search and the account guard for free via the
  `spawn_pref_checkbox` / `spawn_pref_slider` / `spawn_pref_section` helpers.
- **Commit semantics** (reference-faithful): controls edit the store live
  through the existing two-way binding; on open the shell snapshots every
  bound setting's per-scope override (`SettingsStore::get_override`, a new
  one-liner in `sl-settings`); Cancel, the window X and `Ctrl+W` all revert
  through the shared close edge (set-back vs reset-to-default); OK
  re-snapshots, saves both scopes to disk, fires the `PreferencesApplied`
  message (the per-tab apply hook) and closes. If the account scope loads
  while the window is open, the snapshot's account entries refresh so a
  later Cancel keeps the just-loaded overrides.
- **Search / filter**: the term (matched against the *resolved translated*
  label text, so any locale works; re-filters on locale switch) collapses
  non-matching rows, highlights hits in the shared menu-search accent, jumps
  to the first tab with a hit, and **dims** (not hides — indices must stay
  stable) tabs left empty. Clearing restores everything; the remembered tab
  concept was dropped by user decision (always opens on the first tab).
- **Account guard**: controls bound at `Scope::Account` carry
  `InteractionDisabled` until the account scope loads at login.
- **First tab "UI & world display"**: bound checkbox / slider rows over
  already-registered, live-consumed global settings no sibling tab claims —
  property lines, status-bar coordinates, six mini-map toggles + zoom and
  opacity sliders, five world-map marker toggles.
- Plus: a `preferences` gallery specimen (swept by the harness matrix),
  Fluent keys in `en/main.ftl`, `SearchFieldHandle` now exposing its
  placeholder entity so it can be `Translated`.

Verified by 6 headless shell tests (snapshot revert set-back vs reset, OK
re-snapshot + apply-once, cancel-close routing, account guard lift, filter
hide / highlight / tab-jump, clear restore) + a `get_override` unit test in
`sl-settings`, and live on the local grid (layout screenshot, live slider
values, geometry persistence round-trip). Combo / text-input rows arrive
with [[viewer-ui-settings-binding-combo]] /
[[viewer-ui-settings-binding-text]]; the per-tab content with the sibling
`viewer-preferences-*-tab` tasks this unblocks.
