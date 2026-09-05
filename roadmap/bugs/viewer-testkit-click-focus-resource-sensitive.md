---
id: viewer-testkit-click-focus-resource-sensitive
title: A click stops focusing its field when the harness gains any resource
topic: viewer
status: bugs
origin: viewer-floater-interaction-tests (2026-09-05) — hit while adding a marker
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-testkit`'s `interact::tests::a_click_focuses_the_field_it_lands_on`
fails — the click lands, the field is laid out at a real size, and
`InputFocus` is left empty — as soon as **one more resource is inserted while
`install_text_editing` builds the app**. Reproduced three times out of three
with a marker resource, and again with an unrelated empty `ProbeMarker`, so it
is not about which resource: any `init_resource` in that window flips it.
Removing the line makes the test pass again.

That is a test standing on something it does not name. Whatever orders the
click → focus path is sensitive to `ComponentId` allocation (resources share
that registry with components), and nothing in the harness or the test says so.
Two things are wrong with that, in increasing order of seriousness:

- the harness cannot be extended — the next person to add a marker, a
  registration or a plugin to `install_text_editing` gets a failure with no
  visible cause (this cost an hour);
- if the *live* viewer's focus-on-click is order-sensitive in the same way,
  the same perturbation ships. Nobody has looked yet.

What was ruled out already: the field's box (asserted non-zero before the
click, and it is), `centre_of`'s arithmetic (unchanged, and the by-entity
refactor is identical), and the check that motivated the marker (moved to
`interaction_violations`, so the harness is untouched today).

Find the system or observer that sets `InputFocus` on a press —
`bevy_input_focus`'s dispatch, `ui_focus_system`, or a `bevy_ui_widgets`
observer — and work out what a shifted `ComponentId` changes about when it
runs. Then either pin the ordering in `install_ui_interaction` /
`install_text_editing`, or fix it upstream in the bevy fork. Add the
perturbation itself as a regression: a harness that survives a resource being
added is the property worth holding.
