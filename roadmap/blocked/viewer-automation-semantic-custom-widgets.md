---
id: viewer-automation-semantic-custom-widgets
title: Semantic roles for the custom widgets, and a completeness guard
topic: viewer
status: blocked
origin: viewer automation design review (2026-09-28)
points: 8
blocked_by: [viewer-automation-semantic-ui-model]
refs: [viewer-ui-interaction-contracts, viewer-floaters-decoupled-from-the-session]
---

Context: [context/automation.md](../context/automation.md).

Most of the viewer's interface is widgets `bevy_ui` does not know: floaters,
tab rows, virtual lists, the inventory tree, menus, combos, the pie menu,
the colour picker and trackball. Without roles for them the model sees a
pile of `Text` nodes.

## Wanted

- A `Semantic` component (role, and a name key when no child text names the
  node) put on each custom widget's root at spawn: floater (window, with its
  title and `Floater::id` / `FloaterKey`), tab list and tab (selected),
  virtual-list and tree rows (selected, expanded, level), menu bar, menu,
  menu item (with its accelerator), combo (expanded, value), pie menu and
  slice, colour picker, trackball, spinner.
- **Extend the existing guard, do not add a parallel one**:
  `ui_contract.rs` already fails a focusable node that lacks a contract
  row; add "and it resolves to a role and a non-empty name" to the same
  sweep over `ELEMENTS` and `FLOATERS`, with an allow-list that needs a
  reason. Test ids share the contract's address space.
- Teeth for selected / expanded on a list row and a combo, and for a pie
  slice's name.

Acceptance: the contract sweep passes with the new condition over every
registered element and floater; removing a `Semantic` from a floater makes
it fail naming that floater.
