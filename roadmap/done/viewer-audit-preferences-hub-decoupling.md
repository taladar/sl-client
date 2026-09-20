---
id: viewer-audit-preferences-hub-decoupling
title: sl-viewer-preferences is a 12-crate hub whose own decoupling mechanism is under-applied
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-settings/src/keys.rs` now holds the keys of twelve more modules, and
`sl-viewer-preferences` no longer names `sl-viewer-world-objects` or
`sl-viewer-world-avatar` from its library at all:

- `hover_text` (1), `name_tag_billboard` (5) and `render_priority`
  (`SETTING_LOD_FACTOR` plus the `LOD_FACTOR_MIN` / `_MAX` bounds three
  different sliders clamp to) leave `sl-viewer-world-objects`;
- `derender::SETTING_FRIENDS_ONLY`, `name_tag_content` (10) and
  `avatar_complexity` (3 keys, 4 slider bounds, and the `ComplexityMode`
  numbering the combo writes and the feature reads) leave
  `sl-viewer-world-avatar`;
- `glow` (4), `exposure` (2), `tonemap` (3 keys + 3 curve values), `probes`
  (4), `particles` (1) and `parcel_borders::SETTING_SHOW_PROPERTY_LINES` leave
  `sl-viewer-world-scene`.

Each owner re-exports its keys from `keys`, so `glow::SETTING_ENABLED` still
resolves inside `sl-viewer-world-scene` and every other caller is untouched;
the defaults, the sections, the descriptions and the behaviour stay with the
owner, as that module's doc requires. The 50 `pub(crate) use` re-aliases in
`sl-viewer-preferences/src/lib.rs` are 44, over 10 crates instead of 12.

## The two dropped edges cost nothing, and the note could have known

`cargo tree -p sl-viewer-preferences` holds **912 packages before and 912
after**. `sl-viewer-world-view` is still a dependency — `CameraTuning`,
`MovementTuning` and `session::apply_draw_distance` are behaviour, not names —
and it depends on both crates the library just stopped naming, so the closure,
the compile order and the rebuild blast radius are exactly what they were.
Measuring which packages *only* `world-view` carries says the same thing from
the other side: 10 before (`sl-viewer-spacenav`, `evdev`, `nix`, …), 12 after,
the two extra being `sl-viewer-world-avatar` and `sl-viewer-ui-sounds` handed
over from the dropped edges.

So this is the measure-the-closure-not-the-edge lesson again, and the same
shape as [[viewer-audit-ui-core-sound-coupling]]: the audit counted
direct edges. What it bought is the rule applied uniformly — where a name lives
is a question about the name, which is why `sl-viewer-world-scene`'s six
key-only modules moved too although that crate remains a dependency — and a
`sl-viewer-preferences` that could drop `world-view` and *then* be free of the
world tier, rather than one that still had three other reasons not to be.

Two tests in this crate do want the owners: `phototools`' `REGISTRARS` and
`preferences_graphics`' `graphics_app` register the real declarations so that
"every row binds a setting the viewer actually declares" is a claim about the
viewer and not about a test fixture. Those two crates are therefore
**dev-dependencies** now, named through their own paths rather than a
`crate::` alias (which resolves to the key module). A key is shared; a default
is not.

## `quick_preferences` is not a second row stack, and should not become one

The note's other finding — that `quick_preferences.rs` re-renders six settings
"using the lower-level `bound_checkbox` / `bound_slider` / `spawn_combo`
instead of the `spawn_pref_*` layer" — does not survive reading the two
builders side by side. The overlap is the *settings*, deliberately (the entry
table says so: "same setting key, so all three views agree"), not the code:

- a `spawn_pref_*` row takes a `&'static str` Fluent key; a quick-prefs entry's
  label may be a runtime `String` read out of the per-avatar JSON file;
- a `spawn_pref_*` row is a `PrefSearchRow` with a `PrefRowLabel`, because the
  preferences floater filters its rows by a search box the popover has not got;
- a quick-prefs slider row carries a trailing value read-out in a min-width
  slot, and its scope is per entry (`Scope::Account` for friends-only), where a
  tab row's scope is the tab's.

Rebuilding the popover on `spawn_pref_*` would mean parameterising all three
away and subscribing the popover's rows to the floater's filter. What *is*
duplicated is one layer down and worth having: the checkbox **box** —
`bound_checkbox(binding)` plus a square `Node`, a border, `BackgroundColor`,
`TabIndex` and a crate-local marker — is spelled out four times in this crate
alone (three of them with their own copy of the same three colours) and in
`sl-viewer-notices`, `-people` and `-search` besides, each with a near-identical
`drive_*_checkboxes` system behind it. That is
[[viewer-audit-checkbox-box-widget]].

## Not a finding, for the record

The preferences panels themselves are the best-factored area in the viewer.
`preferences.rs:356-600` defines seven shared row builders and all eight tabs
use them consistently (alerts 15 calls, audio 17, camera_move 19, chat 41,
colors_skins 10, general 31, graphics 38, network_cache 17), and save/revert is
one shared snapshot lifecycle at `:790-905`.
