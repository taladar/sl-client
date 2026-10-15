---
id: viewer-ui-settings-binding-combo
title: Combo↔settings two-way binding
topic: viewer
status: done
origin: split from viewer-ui-settings-binding — deferred as an extension point
  until a combo widget exists (2026-07)
blocked_by: [viewer-ui-combo-widget, viewer-ui-settings-binding]
---

Context: [context/viewer.md](../context/viewer.md).

Extend the two-way binding layer ([[viewer-ui-settings-binding]]) to the combo /
dropdown widget ([[viewer-ui-combo-widget]]): one more `ValueChange` observer
and one more idempotent sync pass, in the exact shape the checkbox and slider
bindings already use (read on build, write on change, react to external change).

The wrinkle a combo adds over a checkbox/slider is that its value is a **named
option**, not a scalar. So this task owns the mapping between a combo's option
key and the stored [`SettingValue`]: an enum-like setting stored as a small
`I32` index or a `String` key, plus the option table that turns the store's
value into the selected row and back. Decide there whether the binding stores
the index or the key (the reference stores the control's string value); a
string key survives an option list being reordered, an index does not.

Reuse the `SettingBinding { name, scope }` component and the plugin's
already-established plumbing from [[viewer-ui-settings-binding]] — this is
additive, not a rewrite. Cover it with headless tests mirroring that task's
(read-on-build, write-on-change, external-change reflection, two-combos-one-
setting) plus the option-key round-trip.

Reference (Firestorm, read-only): `llui` `control_name` on `llcombobox`,
`llviewercontrol` connections.

## Done (found by the 2026-10-15 audit)

Landed with the preferences General tab (commit `8a5039bd`, 2026-08-04,
[[viewer-preferences-general-tab]]) in
`sl-viewer-ui-widgets/src/settings_binding.rs`. A combo whose anchor
carries a `ComboBindingValues` beside its `SettingBinding` binds through
`write_bound_combo_changes` (a user pick, a `ComboChanged`, writes the
option's value) and `sync_bound_combos` (the idempotent pass moves
`ComboSelection` to the option matching the store). The mapping question
was settled as an option-index → `SettingValue` table, so the stored value
is the option's own value (a string key or a number as the setting
declares), not its index; a stored value no option covers leaves the combo
untouched. Tests in the same file: `combo_reads_setting_on_build`,
`combo_user_pick_writes_setting`, `combo_follows_external_change`,
`combo_ignores_unknown_stored_value`. `spawn_pref_combo` makes searchable
preference rows of it.
