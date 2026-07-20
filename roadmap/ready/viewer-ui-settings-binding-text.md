---
id: viewer-ui-settings-binding-text
title: Text-field↔settings two-way binding
topic: viewer
status: ready
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
