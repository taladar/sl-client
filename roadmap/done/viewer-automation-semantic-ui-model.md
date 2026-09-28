---
id: viewer-automation-semantic-ui-model
title: The semantic UI model — roles, names and states read from the ECS
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-protocol]
refs: [viewer-a11y-screen-reader, viewer-automation-semantic-custom-widgets,
  viewer-automation-accesskit-bridge]
---

## Done (2026-09-28)

The new `sl-viewer-automation` crate provides `UiModel`, a `SystemParam`
with `snapshot()` and `snapshot_under(entity)`. There is also a
`snapshot(&mut World)` for exclusive callers, and `node_id` / `entity_of`
to go between a `NodeId` and its entity (the id is `Entity::to_bits`). It
reads the ECS only when asked. `Translated` gained a public `key()`.

How the model decides things:

- **Roles, in precedence order:** `EditableText` is a textbox, then
  `Slider`, `Checkbox`, `RadioButton`, `ui_widgets::Button`. A `Text` is
  text only when it has content. An `ImageNode` is an image only when it
  has a `Name` or an `AccessibleLabel`, so decorative skin images stay out
  of the tree. A named or labelled container is a **group**. A node with
  no role is transparent: its children go to its nearest semantic
  ancestor. Widget roles are leaves; their subtree is their label.
- **Names:** `AccessibleLabel` wins. Otherwise a widget takes the first
  `Translated` text in its subtree (the text plus its key), and failing
  that all its descendant text joined. Groups and images are named only
  by a label, because a panel's text is its content, not its name.
- **States:** disabled counts ancestors. The others are read-only
  (`ReadOnlyField`), checked (`Checked`), selected (the stock `Selected`),
  focused (`InputFocus`), and hovered (the `HoverMap`, counting the
  pointer over a descendant — a button's label sits on top of it).
  `Expanded` has no stock-widget source; it arrives with the custom
  widgets.
- **Visibility,** first reason wins:
  - `Hidden`: not inherited-visible, a `Display::None` on the node or an
    ancestor, or zero size.
  - `Clipped`: the box does not intersect its `CalculatedClip`.
  - `OffScreen`: outside the target's viewport.
  - `Covered`: a hit test at the centre of the node's visible part does
    not reach the node or a descendant. The hit test copies `bevy_ui`'s
    picking backend: `UiStack` front to back, the fork's
    `clip_check_recursive`, and a hit that stops the walk unless its
    `Pickable` lets the pointer pass.

Tests. There are 11 teeth tests in the crate, run in an `InteractionTest`
app. They cover role inference, label and key precedence, inherited
disabled, read-only, checked, focused, hover through the real pointer,
display-none, a scroll-clipped row, off-screen, and an overlapping panel
(covered, then passed through with `Pickable::IGNORE`, then removed). In
the viewer, `automation_model.rs` sweeps `ELEMENTS`: every stock widget
of every element is in the tree exactly once with the expected role. It
also snapshots every `FLOATERS` window.

**Measured cost** (release build, fastest of five runs): the slowest
floater to snapshot is `day-cycle-editor` at **3.0 ms for 359 nodes**. The
sweep test fails anything over 250 ms, which would mean the model had gone
quadratic.

The sweep does not require stock buttons to have a *name*. That is the
completeness guard [[viewer-automation-semantic-custom-widgets]] adds to
`ui_contract.rs`, with its allow-list.

Context: [context/automation.md](../context/automation.md).

Tests address widgets today by `Name` string and read text through
`text_of`. A browser test addresses them by **role and accessible name** and
reads their **state**; that is what makes a test survive a layout change and
read like the user's intent. The same model is what a screen reader needs,
so it is built once and serves both — but it must not *depend* on AccessKit,
which exists only under a winit window.

## Wanted

In a new `sl-viewer-automation` crate, a snapshot of the UI as `UiNode`s
for the **stock widgets**:

- **role** inferred from widget components: `ui_widgets::Button` (not the
  prelude `Button`), checkbox, radio, slider, `EditableText`, plain `Text`,
  `ImageNode`;
- **name**: `AccessibleLabel` override, else the `Translated` key (both the
  key and the resolved text are kept), else descendant `Text`; the entity's
  `Name` is the test id;
- **states**: disabled (`InteractionDisabled` on the node **or an
  ancestor** — the component is advisory, so the model is what makes it
  observable), read-only, checked (`Checked`), focused (`InputFocus`),
  hovered; **value** for text fields and sliders;
- **geometry and visibility**: bounds in logical px, `InheritedVisibility`,
  `Display::None` ancestors, clipped out of a scroll area, outside the
  viewport, and **covered** (the UI hit test at the centre lands elsewhere);
- computed on request, never per frame.

Teeth: one test per state, each flipping when the widget's state does and
staying put otherwise — inherited disabled, a read-only field, a clipped
row, a node covered by an overlapping panel.

New crate: expect the extraction gates (see the new-crate memory).

Acceptance: the stock-widget elements of `ELEMENTS` yield a complete tree;
the teeth tests pass and fail as designed; snapshot cost is measured and
stated for the largest floater.
