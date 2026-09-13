---
id: viewer-build-tools-default-to-create
title: The build window opens on a manipulator with nothing to manipulate
topic: viewer
status: bugs
origin: reported while merging the ui-features and fake-grid-features branches
  (2026-09-13)
points: 3
refs: [viewer-keyed-floater-audit]
---

Context: [context/viewer.md](../context/viewer.md).

Opening the Build Tools window with **nothing selected** puts you on the
**Move** manipulator, which has no target. The reference opens on **Create** in
that case — a build window you opened with nothing selected is a build window
you opened in order to make something.

## What the viewer does today

Every open site is a plain visibility flip and none of them touches the mode:

- Build menu ▸ Build Tools, and its `Ctrl+B` accelerator —
  `menu_bar.rs:566` (definition), `menu_bar.rs:1278` (dispatch).
- The bottom toolbar's **Build** button — `bottom_toolbar.rs:299`,
  dispatched at `bottom_toolbar.rs:660`.
- The object and attachment pie menus' **Edit** slice —
  `object_menu.rs:898` (`edit_picked_object`, shared by both).

The one system that reacts to the window opening is
`mirror_floater_into_state` (`sl-viewer-edit/src/edit_tool.rs:865`). It copies
visibility into `EditToolState::active`, and on the **close** edge clears the
selection. It never writes `tool`.

So the mode is simply whatever `EditToolState::tool` was last left at:
`EditTool::Move` from the `Default` impl
(`sl-viewer-world-api/src/lib.rs:432`) on a fresh session, and the last tool
you clicked thereafter — the resource is never reset on close. Nothing anywhere
consults `SelectionSet` (`sl-viewer-world-api/src/lib.rs:112`) to decide a
mode.

`the_tool_radio_and_the_tool_state_follow_each_other`
(`build_floater_test.rs:745`) pins the current behaviour, asserting `Move` with
the message "the floater opens on the move tool". That assertion is what
changes.

## The fix

On the **open edge** — where `mirror_floater_into_state` already sees
`active` go false → true — choose the mode from the selection: empty selection
gives `EditTool::Create`, a non-empty one leaves the resting tool alone. The
close edge already clears the selection, so the next plain open is an empty one
and lands on Create without any extra state.

Two things make this more than a one-line change:

- **The radio is a second source of truth, and it wins.**
  `build_build_tools_content` spawns the group with
  `active: EditTool::default().radio_index()` (`edit_tool.rs:368`), and
  `sync_build_tool_from_radio` (`edit_tool.rs:890`) reacts to
  `Changed<RadioSelection>` — which in Bevy fires when the component is
  *added*, not only when it is clicked. The window's content is built lazily on
  first open
  (`DeferredFloaterContent`, consumed at
  `sl-viewer-ui-widgets/src/floater.rs:1032`), so on that first open the
  freshly-spawned radio pushes `Move` straight back over anything the open
  edge just chose. The initial index has to follow the chosen tool rather than
  `EditTool::default()`, or the fix works on the second open and not the first.

- **Ordering against the pie ▸ Edit path.** `edit_picked_object` shows the
  window *before* it fills the selection (`object_menu.rs:911` then `:918`).
  Both writes are direct, so any later system sees the two together — but the
  open-edge rule must be ordered **after** it, or a pie ▸ Edit open is read as
  an empty selection and flips to Create, which is precisely the case that
  should not. Note also that `edit_picked_object` only fills the selection if
  the entity resolves (`object_menu.rs:919`); an unresolvable pick genuinely
  does open on nothing, and Create is the right answer there.

While here: the object and land pie menus each carry a **Create** slice
(`object_menu.rs:698`, `land_menu.rs:80`) that is still `UNIMPLEMENTED` with no
dispatch arm. Those are the entry points that would want the same "open on
Create" behaviour explicitly rather than by falling out of an empty selection,
and are worth wiring in the same pass.

## How to verify

1. With nothing selected, open the build window from the toolbar **Build**
   button — it must open on **Create**. Repeat with `Ctrl+B` and the Build menu
   item.
2. Do it as the **first** open of a fresh session, which is the case the
   lazily-built radio breaks: the radio dot must be on Create too, not just
   `EditToolState::tool`.
3. Right-click an object ▸ **Edit** — the window must open on a manipulator
   with the object selected, *not* on Create.
4. Pick a manipulator, close the window, reopen it with nothing selected — back
   to Create, because the close cleared the selection.
