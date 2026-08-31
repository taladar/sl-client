---
id: viewer-edit-selection-interaction-tests
title: Selecting for edit — click, shift-click, rubber band, faces
topic: viewer
status: done
origin: user request (2026-07) — build floater / gizmos named the priority
points: 5
blocked_by: [viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

Done (2026-08-31): six tests in `world_test.rs`, one per bullet below,
each a real gesture through the synthetic pointer and the CPU pick
resolver. A click selects and makes primary; a click on empty world
clears; a held-Shift click accumulates, promotes the newcomer to primary
and toggles it back out; a drag that starts in empty sky sweeps a band
over one of two prims and commits exactly that one; a click on a linked
child selects the linkset **root** in whole-linkset mode and the child
itself under *Edit Linked Parts*, and `promote_selection_to_roots` puts
it back; Select Face picks one face and a Shift-click on the same face
toggles it — and the object with it, that face being its last. Every one
also asserts the **wire**: the `ObjectSelect` (properties request) and
`ObjectDeselect` the simulator would have seen. The gizmo-ordering
assertion is its own test: a press on the +X translate cone — which
hangs out over empty world, where a press *would* otherwise classify as
an empty-world gesture and clear the selection on release — leaves the
set and the wire untouched.

Three pieces of scaffolding landed with them: `seed_prim_numbered`
(the shared seed's local id is `1`, so two prims streamed through
`seed_prim` were one object folded twice), `seed_child_prim` (a
parent-relative two-prim linkset), and `translate_x_cone`, lifted out of
the first gizmo test. `promote_selection_to_roots` is `pub` now: its
child→root jump needs a *populated* `ObjectState`, which only the fixture
world can stand up, and its own unit test could reach no further than the
all-roots no-op.

**Two real bugs, both found by a test that failed before the fix:**

- A plain rubber band selected **nothing**. The commit cleared the
  selection before taking the swept set — and `SelectionSet::clear`
  empties the tentative set along with the committed one, so the loop ran
  over an empty vector. Only a Shift-sweep (which skips the clear) ever
  worked. The sweep is taken before the replace now.
- The editor's own overlays shadowed the world pick. Each overlay reuses
  its face's mesh — the silhouette and drag-hover shells inflated 3.5 %
  (strictly in front of the face), the Select Face grid cursor exactly
  coplanar — and carries no `PrimFaceEntity`, so a ray that struck one
  resolved to the right object with **no face index**. Whole-object
  selection never noticed (the walk up to the `SceneObject` finds the same
  object either way); Select Face did: the second click on a face resolved
  to "no face" and did nothing. The three overlays share an `EditorOverlay`
  marker now, and `SelectPointer::pick_exclusions` excludes them the way it
  already excluded HUD and gizmo geometry.

Left out deliberately: the keyboard-side deselects (`Escape`, `Delete`)
and the event-side folds (`ForceObjectSelect`, an object killed out of
the set) are not pointer gestures; that the highlight overlays actually
*light pixels* is [[viewer-gpu-interaction-readback]].

Drive `edit_selection.rs::handle_select_pointer` headlessly against the
fixture world:

- click selects (`SelectionSet` primary), shift-click accumulates,
  click-empty clears;
- rubber-band drag sweeps `sweep_candidates`;
- `edit_linked` toggles child-vs-root selection (`promote_to_roots`);
- face-select mode toggles `PrimFaceId`s
  (`select_only_face`/`toggle_face`);
- selection changes emit the object-select/deselect `SlCommand`s the
  server expects.

The gizmo systems order before selection exactly as live
(`drive_gizmo_interaction` claims the pointer first) — assert that
ordering holds: a press on a gizmo handle never mutates the selection.
