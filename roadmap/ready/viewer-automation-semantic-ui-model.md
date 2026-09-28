---
id: viewer-automation-semantic-ui-model
title: The semantic UI model — roles, names and states read from the ECS
topic: viewer
status: ready
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-protocol]
refs: [viewer-a11y-screen-reader, viewer-automation-semantic-custom-widgets,
  viewer-automation-accesskit-bridge]
---

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
