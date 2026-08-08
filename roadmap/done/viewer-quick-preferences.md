---
id: viewer-quick-preferences
title: Quick-preferences panel
topic: viewer
status: done
origin: user request (2026-07)
blocked_by: [viewer-preferences-floater]
refs: [viewer-hover-height, viewer-volume-panel, viewer-quick-preferences-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The small always-reachable panel of the settings you actually change several
times an hour, so you never open the full preferences floater for them: draw
distance, the environment / windlight preset and time of day, avatar hover
height ([[viewer-hover-height]]), rendering quality, avatar complexity limits
and maximum non-imposters, and whatever else turns out to be reached-for often.
Firestorm's Quick Preferences is the model, including that its **contents are
user-configurable** — the panel is a curated view over the settings store, not a
fixed list.

Placement (user request 2026-08-05): the panel lives in the
**bottom-right corner** of the screen — the bottom toolbar's trailing area
([`BottomArea::upper_trailing`], where the parcel-audio cluster already sits) is
its natural home, not the bottom-centre tray. (The bottom bar's *leading* slot
is taken by the Stand Up / Stop flycam state button,
[[viewer-sit-target-and-stand-button]].)

Vintage note (2026-07-22 skin survey): the Vintage bottom bar surfaces a
quick-prefs button (and the AO toggles) inline —
[[viewer-vintage-bottom-bar]] reserves the slot this panel opens from;
the AO half is [[viewer-animation-overrider]].

That is the design question worth settling here: rather than a hard-coded
floater, make it a *view* over the typed settings store the preferences floater
([[viewer-preferences-floater]]) defines, so a setting can be surfaced in the
quick panel without being reimplemented — and so a user can add or remove
entries. Whether the entries are user-editable in the first version, or just a
good default set with the plumbing ready, is a scope call for the implementing
agent.

Cross-refs: [[viewer-preferences-floater]] (the settings store and the full
floater), [[viewer-hover-height]] and [[viewer-volume-panel]] (two entries that
are also tasks in their own right).

Reference (Firestorm, read-only): `fsfloaterquickprefs` (`quick_preferences`
XUI and its user-editable control list), `llfloaterpreference`.

## Done

New viewer module **`src/quick_preferences.rs`** (`QuickPreferencesPlugin`,
floater id `quick-preferences`). A gear button in the bottom toolbar's trailing
area ([`BottomArea::upper_trailing`], beside the parcel-audio cluster) toggles a
small draggable floater that anchors itself to the **bottom-right corner** on
first open (and respects a persisted position thereafter — geometry rides the
floater id's per-avatar persistence for free).

Scope call (user decision 2026-08-08): the **curated default set + the
user-configurable plumbing**, with the in-viewer runtime editor deferred to a
follow-up ([[viewer-quick-preferences-editor]]).

- **A view over the settings store, not a fixed list.** The setting rows are
  built from a data-driven `QuickPrefEntry` list (control name, label, scope,
  control kind, slider min/max/increment) that is **persisted per-avatar** as
  `quick_preferences.json` in the account directory (a self-describing template
  is written there on first login). A power user can add / remove / retype
  entries by editing that file; each row binds through the shared
  [[viewer-ui-settings-binding]] layer, so any registered setting can be
  surfaced without being reimplemented. An entry naming an unregistered or
  type-mismatched setting is skipped, not bound to nothing.
- **Curated default entries, all load-bearing.** *Draw distance*
  (`RenderFarClip`, new; owned + consumed by `session::apply_draw_distance`,
  which announces it to the sim live on every change and re-announces on each
  region handshake — replacing the old hard-coded 512 m constant) and *max
  particles* (`RenderMaxPartCount`, new; consumed live by
  `particles::drive_particles`, replacing the `MAX_PARTICLES` constant). Both
  are `[render]`-section global settings, editable now from the panel and later
  from any graphics tab. The camera far plane is deliberately *not* tied to draw
  distance (the sky dome / stars render at the fixed far plane).
- **Environment section** (fixed, top): a **preset group** combo (shared / the
  region's own day cycle / Legacy WindLight / Modern EEP) crossed with a **time
  of day** combo (sunrise / midday / sunset / midnight), driving the live
  `EnvironmentState::set_fixed` the World ▸ Environment menu already uses —
  mapping Firestorm's sky / water / day-cycle preset combos onto our
  fixed-environment model. The combos reflect an external environment change
  (the menu) and the time combo is disabled while the shared group is selected.
- Plus: a `quick-preferences` gallery specimen (swept by the harness matrix) and
  Fluent keys in `en/main.ftl`.

The entries that need their own backend first (avatar complexity / jelly-doll,
max non-imposters, LOD factor) are left out rather than shipped as no-op
controls; they slot into the same entry model once those rendering features
land. Hover height ([[viewer-hover-height]]) and volume
([[viewer-volume-panel]]) are their own tasks and are surfaced once implemented.

Verified by 12 headless tests (env-combo pick → `set_fixed`, foreign-combo
ignore, sync reflect + time-gate, value readout, JSON round-trip, unknown-type
skip, `binding_kind` type checks, default-entry sanity, index round-trips, plus
`apply_draw_distance` announce/dedup/handshake in `session.rs` and
`particle_cap` in `particles.rs`).
