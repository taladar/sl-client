---
id: viewer-ui-floater-persist-geometry
title: Floaters remember their position and size per user
topic: viewer
status: ready
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
