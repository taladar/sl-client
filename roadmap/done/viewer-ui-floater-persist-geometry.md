---
id: viewer-ui-floater-persist-geometry
title: Floaters remember their position and size per user
topic: viewer
status: done
origin: noticed live-testing the floater manager (2026-07)
blocked_by: [viewer-ui-floater-basic, viewer-ui-settings-store]
refs: [viewer-settings-toml-format]
---

Context: [context/viewer.md](../context/viewer.md).

A floater ([[viewer-ui-floater-basic]] / [[viewer-ui-floater-resize-dock]])
should **remember where it was and how big it was** across sessions, per user —
open the inventory window, drag and resize it, quit, log back in, and it comes
back where you left it. Today its position and content size live only in the
`Floater` component and reset to the `FloaterSpec` defaults every launch.

Persist through the typed settings store ([[viewer-ui-settings-store]], which
already does per-account overrides layered over global defaults) — the
reference's model, where every floater's rect is a `gSavedSettings` control
keyed by the floater's name (`LLFloater::storeRectControl` / `applyRectControl`,
the `RectControl`/`PositioningControl` params). Key the stored geometry by
[`Floater::id`]; write it back on move / resize / dock (debounced), and seed a
floater's `position` / `content_size` from the store at spawn when a saved value
exists, falling back to the `FloaterSpec` default otherwise.

Scope notes:

- **Position, size, and probably dock state** — restore a floater that was left
  docked back into its host, and a torn-off one free. Minimized state is a
  maybe (the reference persists it via `Floater.minimized` control); decide when
  building.
- Keep the on-screen clamp on restore: a saved rect from a larger monitor must
  still land at least `FLOATER_MIN_VISIBLE` pixels on screen, so a smaller
  display never restores a window fully out of sight.
- This is floater-manager infrastructure, not per-panel: the inventory window
  gets it for free, and so does every future floater, by having an `id`.

Reference (Firestorm, read-only): `indra/llui/llfloater.cpp`
(`storeRectControl` / `applyRectControl` / `storeVisibilityControl`,
`mRectControl`), and the per-floater `save_rect` / `save_visibility` XUI params.

## Done

New module **`src/floater_persist.rs`** (`FloaterPersistPlugin`):
floater-manager infrastructure, not per-panel — every floater with a
[`Floater::id`](../../sl-client-bevy-viewer/src/floater.rs) gets remembered
geometry for free (the inventory window is the first beneficiary). Persisted
through the typed settings store into the **`Account`** scope — the per-(grid,
avatar-name) file from [[viewer-settings-account-scope-persist]] — so it is per
user in the sense we just built, **not** the reference's per-install
`gSavedSettings` (a deliberate deviation, since two characters on one machine
should keep their own layouts). Four settings per floater under `[floater]`:
`<id>_rect` (a `[left, top, right, bottom]` logical-pixel rect whose extent
carries the content size — a zero-extent rect means a content-sized window),
`<id>_visible`, `<id>_minimized`, `<id>_docked`.

Four-stage lifecycle: **register** each floater's settings the frame it spawns
(before login, so the account file coerces to the declared types); **seed** once
the account scope is loaded post-login, applying each stored value only when it
is actually present (new `SettingsStore::is_overridden`, so a floater with
nothing saved keeps its `FloaterSpec` default) — docking replays through the
manager's command path, and the existing `clamp_floaters_on_screen` recovers a
rect saved on a larger monitor onto a smaller display; **persist** on any move /
resize / minimize / dock / open / close; **flush** to disk at most every 30 s
while dirty (plus the clean-logout save), so a crash in a long session loses at
most that window rather than everything since login.

Also: position/size are stored in **logical pixels** (readable in the file,
reference-faithful; the on-screen clamp is the low-resolution recovery
mechanism, not a normalized rect). `SettingsStore::is_overridden` added to
`sl-settings` (with a unit test) to tell a saved value from the bare default.

The inventory's duplicate open flag was removed: `UiPanelShown` is now the
single source of truth (so restore-open and `Ctrl+I` can't drift), which also
retired the now-orphaned `FloaterClosed` event — a consumer observes
`Changed<UiPanelShown>` (a new `refresh_inventory_on_show` does the folder
refresh for both `Ctrl+I` and restore-open).

Unit tests: the rect codec (sized / content-driven / negative-offset round-trip,
wild-value clamp) in `floater_persist`, and `is_overridden` in `sl-settings`.
The seed/persist/flush wiring is integration-only (needs a live login) and is
best verified by opening the inventory, moving/resizing it, quitting, and
logging back in.
