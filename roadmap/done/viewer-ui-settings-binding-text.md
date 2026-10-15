---
id: viewer-ui-settings-binding-text
title: Text-field↔settings two-way binding
topic: viewer
status: done
origin: split from viewer-ui-settings-binding — deferred as an extension point
  until the text-input widget exists (2026-07)
blocked_by: [viewer-ui-text-input-widget, viewer-ui-settings-binding]
---

Context: [context/viewer.md](../context/viewer.md).

Extend the two-way binding layer ([[viewer-ui-settings-binding]]) to the
reusable text-input widget ([[viewer-ui-text-input-widget]]) for `String`-typed
settings: one more `ValueChange<String>` (or the widget's edit event) observer
and one more idempotent sync pass, in the shape the checkbox and slider bindings
already use.

Text carries two concerns the scalar widgets do not, and this task owns both:

- **When to write.** A text field emits a stream of edits; the `ValueChange`
  `is_final` flag (or focus-loss / Enter) is the point to commit to the store,
  not every keystroke — otherwise every intermediate string is persisted and the
  field fights the sync pass mid-word. The scalar bindings write eagerly because
  their edits are discrete; text must debounce to the commit.
- **Reconciling an external change into a focused field.** The idempotent sync
  must not yank the caret or clobber an in-progress edit when the store moves
  underneath a field the user is typing in — reconcile only when the field is
  not actively being edited.

Reuse the `SettingBinding { name, scope }` component and the plugin plumbing
from [[viewer-ui-settings-binding]]; this is additive. Cover with headless
tests:
commit-on-final (not per-keystroke), external-change reflection while unfocused,
and no-clobber while editing.

Reference (Firestorm, read-only): `llui` `control_name` on `lllineeditor`,
`llviewercontrol` connections.

## Done (found by the 2026-10-15 audit)

Landed with the preferences General tab (commit `8a5039bd`, 2026-08-04,
[[viewer-preferences-general-tab]]) in
`sl-viewer-ui-widgets/src/settings_binding.rs`: `write_bound_text_edits`
and `sync_bound_text_inputs` bind an `EditableText` to a `String` setting
(and, later, to `F32` / `I32` / `U32` settings, the text parsed in the
setting's type, a half-typed number writing nothing).

One decision went the other way from this body: edits made **while the
field holds focus write live** rather than on commit, to match the
checkbox / slider live-edit model the preferences snapshot (Cancel) relies
on. The no-clobber half is as planned — the sync pass skips the focused or
IME-composing field, and a `BoundTextShadow` tells a real edit from an echo
of the layer's own write. Tests: `text_seeds_from_store_when_unfocused`,
`text_focused_edit_writes_setting`, `text_focused_field_not_clobbered`,
`numeric_text_converts_both_ways`,
`a_numeric_text_field_reads_and_writes_its_setting`.
