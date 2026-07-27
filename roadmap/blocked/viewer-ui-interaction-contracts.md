---
id: viewer-ui-interaction-contracts
title: An interaction contract per registered element, swept like the matrix
topic: viewer
status: blocked
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
