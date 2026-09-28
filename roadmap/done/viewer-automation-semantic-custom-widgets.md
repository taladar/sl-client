---
id: viewer-automation-semantic-custom-widgets
title: Semantic roles for the custom widgets, and a completeness guard
topic: viewer
status: done
origin: viewer automation design review (2026-09-28)
points: 8
blocked_by: [viewer-automation-semantic-ui-model]
refs: [viewer-ui-interaction-contracts, viewer-floaters-decoupled-from-the-session]
---

## Done (2026-09-28)

`sl_viewer_ui_core::semantic` holds what a widget says about itself:

- `Semantic` is put on a widget's root at spawn. It gives a role, and says
  where the name comes from: the content, `labelled_by` a caption node, or
  a Fluent `name_key`. It can also carry a tree `level`, an `accelerator`,
  and `value_from`, the node whose text is the value. It overrides the role
  the model would infer.
- `Expanded` is a marker the owning widget keeps in step through
  `sync_expanded`.
- `LabelledBy(Entity)` names a control, or every control in a row, by a
  caption node. This is the `<label>` of a form row, and
  `spawn_labeled_row` puts it on every row it builds.
- A row the skin draws as a list row (`sk-list-row`, `sk-table-row`,
  `sk-combo-option`) is a list item without saying so. `sk-selected`
  selects it.

The protocol gained the roles `radiogroup`, `colorwell`, `trackball`,
`tablist`, `menubar`, `menu`, `list`, `tree` and `treeitem`. `UiNode`
gained `level` and `accelerator`.

The widgets:

- **Floater**: a `window`, named by its title.
- **Tab strip and tab**: a `tablist` and `tab`s. The checked tab reads as
  *selected*. A tab is named by its caption, without the ellipsis.
- **Menus**: the bar is a `menubar` and a popup is a `menu`. Bar buttons,
  command lines, dynamic lines and submenu lines are `menuitem`s. An entry
  is named by its label key and carries its accelerator. A bar button or
  submenu line is expanded while its menu is open.
- **Combo**: a `combobox`. Its value is the shown entry, it is expanded
  while its list is open, and the list is a `list` of options.
- **Pie menu**: a `menu`. A slice is a `menuitem` named by its key; a
  disabled slice also carries `InteractionDisabled`.
- **Colour swatch and trackball**: a `colorwell` and a `trackball`. The
  trackball is named by body; two keys are new.
- **Virtual list and inventory**: every virtual list is a `list` by
  default. The inventory is a `tree` of `treeitem`s, whose level and
  expanded state follow the row each is bound to.
- **Spawner defaults**: every button from `ui_spawn::spawn_button` is a
  `button`, and the model now also counts `bevy_ui`'s own `Button`. A
  `RadioGroup` is a `radiogroup`.

How the model uses all this:

- **A leaf keeps its owned popup.** A combo's list and a submenu are
  spawned under their anchor, which is a leaf. The model still collects
  any descendant carrying a container `Semantic` as the leaf's child (as
  `aria-owns` does), and keeps it out of the leaf's name.
- **The guard is `ui_contract::every_focus_stop_has_a_contract_row`,**
  extended rather than duplicated. It now also sweeps every `FLOATERS`
  window. It fails a focus stop that has no role: none, or only the `group`
  a bare `Name` gives. It fails one that has no name, unless its role is a
  composite whose items carry the names (`radiogroup`, `tablist`, `list`,
  `tree`, `menu`, `menubar`). And it fails a floater that is not a named
  `window`. The allow-list `UNNAMED_STOPS` holds one entry, `browser-view`:
  a web page's own tree is not exposed by the offscreen browser.

The first run of the guard found 437 stops with no role or no name. Fixing
them took `ButtonSpec::name_key`, `TextInputSpec::name_key` and
`RichTextSpec::name_key`, and naming search fields by their placeholder. It
also took `LabelledBy` across about 40 panels and 57 new name keys, in an
"Accessible names" section at the end of the English bundle.

**Scope note:** there is no spinner widget in this viewer. The reference's
spinners are plain numeric text fields here, and are already `textbox`es.
So no spin-button role was added; it is deferred to
[[viewer-build-numeric-field-spinners]], the task that builds the widget.

Tests:

- **Model teeth** (`sl-viewer-automation`): a list row's class-driven
  selection; a tree row's level and expanded state; a leaf owning a popup
  without taking its text; naming by another node and by a key.
- **Viewer teeth** (`automation_model.rs`), driven through the real
  pointer: a combo expanded while open, with the picked value; the
  selected tab following a click; a pie slice named by its key and
  resolved to English; a menu button expanded while open, whose entries
  carry keys and accelerators and stay out of its name.
- **Guard teeth** (`the_semantic_guard_names_what_lost_its_role`): taking
  the `Semantic` off a registered floater fails naming that floater, and a
  bare focus stop fails naming the stop.

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
