---
id: viewer-build-tools-default-to-create
title: The build window opens on a manipulator with nothing to manipulate
topic: viewer
status: done
origin: reported while merging the ui-features and fake-grid-features branches
  (2026-09-13)
points: 3
refs: [viewer-keyed-floater-audit]
---

Done (2026-09-13). The open edge chooses Create when there is nothing to
manipulate, and the two entry points that know their own tool say so.

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

`mirror_floater_into_state` (`sl-viewer-edit/src/edit_tool.rs`) now acts on the
**open** edge as well as the close one: an open that arrives with an empty
selection sets `EditTool::Create`, a non-empty one leaves the resting tool
alone. That is the reference's `LLToolMgr::enterBuildMode`, which selects
`LLToolCompCreate` on every plain entry (`Ctrl+B`, the Build menu item, the
toolbar button) — all three of which are still plain visibility flips and need
no change.

The radio group, the second source of truth, was settled from both sides:

- `build_build_tools_content` spawns it on `state.tool.radio_index()` rather
  than `EditTool::default()`, so the lazily-built first open shows the dot the
  open just chose.
- `sync_build_tool_from_radio` skips a selection it sees for the first time
  (`Ref::is_added`): Bevy's `Changed` fires on a component being *added*, and a
  newborn group is not a user pick. Without this the group pushed its own
  initial index back over the open edge's choice.
- `sync_radio_from_build_tool` reconciles a newly added group too, not only a
  changed resource — the content can be built a frame after the open edge wrote
  the tool, by which time the resource's change tick has gone stale.

**Two departures from the plan above**, both toward the reference:

- The plan left the resting tool alone on a non-empty selection and expected
  that to keep pie ▸ **Edit** on a manipulator. It does not: after a build
  session the resting tool *is* Create, so Edit would have opened on the tool
  that rezzes with the object to edit selected underneath it — failing this
  file's own verification step 3. The reference does not infer the tool there
  either; `handle_object_edit` picks it outright with
  `setEditTool(LLToolCompTranslate)`. So `edit_picked_object` now sets
  `EditTool::Move` itself, and the selection branch of the open edge is what
  keeps that choice rather than what makes it.
- No ordering constraint was added against the pie ▸ Edit path, because none is
  needed: `edit_picked_object` shows the window and fills the selection in a
  single system, so no other system can observe one write without the other,
  whichever frame it looks in.

The object and land pies' **Create** slices are wired (both were
`UNIMPLEMENTED` placeholders): each opens the build window on `EditTool::Create`
through the new shared `edit_tool::open_build_tools_with`, the reference's
`LLObjectBuild` / `LLLandBuild`. Both read **enabled unconditionally** — the
reference gates them on `EnableEdit` (`enable_object_edit`), which asks whether
the current *selection* is editable and so says nothing about a slice whose
purpose is to build with nothing selected; what actually bounds a rez is the
parcel's build rights, which the simulator enforces on the `ObjectAdd`.

`world_test::open_build_floater` — the fixture 20 build-floater tests open the
window with — now picks the move manipulator explicitly after the open. Those
tests select a prim and edit it, and under Create a click in the world rezzes as
well as selects; the fixture says which tool it wants instead of inheriting one.

Tests: `an_open_on_nothing_lands_on_the_create_tool` and
`an_open_with_a_selection_keeps_the_manipulator` (`build_floater_test.rs`, both
through the real `Ctrl+B` path, asserting the dot as well as the state),
`edit_picks_the_manipulator_and_create_picks_the_create_tool` and
`land_create_opens_the_build_tools_on_the_create_tool` (`world_test.rs`, the pie
dispatch), plus the two pie enabled-set tables.

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
