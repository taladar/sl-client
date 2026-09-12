---
id: viewer-self-closing-widget-leaks-its-press
title: A widget that closes on a press hands that press to the world, which deselects
topic: viewer
status: done
origin: live check of [[viewer-region-estate-group-picker]] on the local grid
  (2026-09-12) — "the build tools commit somehow seems to destroy the selection"
points: 2
refs: [viewer-region-estate-group-picker, viewer-audit-system-ordering-claims,
  viewer-combo-stops-opening]
---

Done (2026-09-12). The claim is made generically, at press time, by the UI
scaffold rather than by each widget that remembers to.

Context: [context/viewer.md](../context/viewer.md).

Choosing a group for the selected object and pressing **OK** left the object
**deselected**. Nothing in the group-set path touches the selection — the
symptom is one step removed from its cause, which is why it read as spooky.

## What happens

The build tool's world-pick gesture (`edit_selection::handle_select_pointer`)
does not use `bevy_picking` events. It reads raw `ButtonInput<MouseButton>` and
decides **on the press frame** whether the pointer is over UI, by looking every
hovered entity up in the hover map (`pointer_over_blocking_ui`). A hover entry
only counts as UI if it still has a `ComputedNode` with positive area.

The group picker is a **keyed** floater, so OK despawns the whole window —
including the OK button, which is the one entity in that hover map — during the
very press that asked for it. The lookup then finds no `ComputedNode`, reads
the entry as "not a UI surface", and the guard **fails open**: the press is
recorded as a press on empty world, and the release next frame runs
`selection.clear()`. A picker opens *beside* the floater that asked rather than
on top of it, so what is behind it is the world.

The hazard was already known — `UiPointerClaim` exists for exactly it, and the
combo widget's own comment spells out "leaking the press to the world as an
empty-space click that deselects". But `claim()` had **two** call sites in the
whole workspace, both in `ui_combo`, and every other self-closing surface was
one press from the same bug: all five pickers, every floater's ✕, every asset
editor, and every pooled list row despawned by the rebuild its own click
triggered.

## The fix

`sl_viewer_ui_core::ui::install_ui_pointer_claim` installs the resource, its
per-frame reset, and a **global `Pointer<Press>` observer** that claims any
press landing on a blocking UI node. It runs as a picking observer — in the
press, while the node is still alive — so nothing depends on a widget
remembering, and nothing depends on `Update` ordering between the despawn and
the gesture (which is unordered, and is the shape
[[viewer-audit-system-ordering-claims]] catalogues elsewhere).

The predicate is `pointer_over_blocking_ui`'s, so the claim only ever *adds* a
reason to skip the world pick: a node that blocks here is one the hover-map test
would also have called blocking, had it survived to be asked. A press on an
object in the world has no `ComputedNode` and is never claimed.

`ui_combo`'s two hand-written claims stay — they are still correct, and the
second one now claims twice, which is what an idempotent flag is for.

The installer is idempotent and called from every fold that consults the claim:
the viewer's `ViewerUiPlugin`, `ComboWidgetPlugin` (so the widget crate stands
alone in its own tests), `sl_viewer_testkit::interact`, and the world-test
build-tools fold — which used to `init_resource` the flag by hand and therefore
had a claim nothing could ever set.

## Verified

`confirming_a_group_keeps_the_selection` in `build_floater_test` drives the
whole gesture through the real pointer — press Set…, press a row, press OK —
and asserts the object is still selected and the group set went out. Without
the observer it fails with "confirming a group deselected the object it was
chosen for", which is the user's report verbatim.

The sibling test that writes `GroupPicked` directly passes either way: writing
the message skips the press, which is exactly the step the bug lives in. That
is the lesson worth keeping — a message-level test of a button proves the
handler and says nothing about the gesture.

`cargo clippy --workspace --all-targets` clean; the workspace suite green.
