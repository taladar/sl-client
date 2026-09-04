---
id: viewer-ui-interaction-contracts
title: An interaction contract per registered element, swept like the matrix
topic: viewer
status: done
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 8
refs: [viewer-ui-test-harness]
blocked_by: [viewer-ui-interaction-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The pie menu pins committed compass-address tables
(`every_action_keeps_its_declared_address`); generalise that shape to the
whole `ELEMENTS` registry. Each `UiElement` declares a contract table —
named node × input kind → expected reaction (`Emits(UiAction)` / state
probe / inert).

The sweep runs every element × every named interactive node × the full
input alphabet — left/middle/right click, scroll up/down, drag-across,
Tab/Shift-Tab, Enter/Space/Escape/arrows while focused — with the default
expectation **inert-and-harmless**: no panic, no *undeclared* `UiAction`,
and `layout_violations` still empty **after** the interaction. That
default is what makes the sweep scale to the whole registry without
hand-writing per-element cases, and it turns every existing layout check
into a post-interaction regression check for free; declared contracts
tighten it where the element does something.

A registry guard (like `the_matrix_covers_the_whole_registry`) asserts no
focusable node lacks a contract row. This is the tier that ends manual
re-testing for the UI: a new element inherits the sweep by being
registered.

## Landed (2026-09-04)

`sl-client-bevy-viewer/src/ui_contract.rs` (the sweep) and
`ui_contract/contracts.rs` (the pinned table, 54 elements). Four sweeps split by
gesture family so cargo's test threads carry them — the whole tier is ~80 s —
plus five guards and the teeth. The address space is
`interactive_nodes` (a named node carrying `ObservedBy`, `Button`, `TabIndex` or
`EditableText`) for the pointer alphabet and `focusable_nodes` for the keyboard
one; "every named node" was the first draft and it spends a whole app per
gesture re-proving that captions ignore the mouse.

The first run found more in the harness than in the elements, which is the
finding worth keeping: **a `spawn` function attaches observers whose system
parameters only a plugin registers**, so an element hosted without its plugin
takes the app down on the first gesture rather than doing nothing. Three
instances, each fixed where it belonged — `UiPointerClaim` into
`install_ui_interaction` (it is pointer-stack vocabulary, not the combo's
private state), and `OpenPieMenu` / `MediaSurfaces` into the sweep's
`install_element_hosting` beside the gallery's own widget-plugin set. This is a
new face of [[viewer-audit-plugin-resource-registration]]: that entry is about
*systems* reading unregistered resources, and an observer attached at spawn is
worse, because nothing about the element's registration says it needs a plugin.

`layout_violations` gained the first exemption it has ever needed beyond
`TextMayClip`: an open `Popover` and its ancestor chain
(`popover_ancestors`). A drop-down is *defined* by sitting outside the anchor it
is a child of, and taffy folds its box into every ancestor's `content_size`, so
without this an open menu reports five violations that are all the widget
working. Keyed on the `Popover` component the widgets already carry, so the next
popover inherits it. `viewport_violations` is deliberately **not** exempted —
escaping a parent is allowed, leaving the window is not — and that division is
what kept [[viewer-chat-volume-dropdown-opens-off-screen]] visible.

Two real defects found and filed rather than fixed here, both pinned in the
table so the correction has to pass through it:
[[viewer-widget-any-mouse-button-activates]] (122 rows; the fix is in the bevy
fork) and [[viewer-chat-volume-dropdown-opens-off-screen]] (a
`LayoutClaim::KnownBroken` canary that fails when the bug is fixed).
