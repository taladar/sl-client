---
id: viewer-quick-preferences-editor
title: Quick-preferences in-viewer editor
topic: viewer
status: ready
origin: split from viewer-quick-preferences (2026-08-08 scope decision)
blocked_by: [viewer-quick-preferences]
refs: [viewer-ui-settings-binding-combo]
---

Context: [context/viewer.md](../context/viewer.md).

The in-viewer **edit mode** for the Quick Preferences panel
([[viewer-quick-preferences]]) — the wrench toggle Firestorm's
`fsfloaterquickprefs` opens: add / remove / reorder entries and edit each one's
label, control type and (for a slider) integer flag / min / max / increment,
all from a picker over the registered settings, with the result written back to
the per-avatar `quick_preferences.json`.

The plumbing is already in place from [[viewer-quick-preferences]]: the entry
list is a data-driven `QuickPrefEntry` model, persisted per-avatar, and every
row binds by name through the settings-binding layer. What is missing is the
editing UI (and a settings picker driven by `SettingsStore::names()` +
`declaration()` for the type / default). The scope decision on the parent task
(2026-08-08) was to ship the curated default set + plumbing first and split the
editor out here.

Watch the "floaters: build once, update in place" rule — a structural edit
(user adds / removes an entry) legitimately spawns / despawns rows, but must not
turn into a per-frame rebuild; only structural changes touch the row set.

Reference (Firestorm, read-only): `quickprefs` (`FloaterQuickPrefs::addControl`
/ `removeControl` / `swapControls` / `onEditModeChanged`, the
`edit_overlay_panel` and `panel_quickprefs_item` XUI).
