---
id: viewer-object-pie-multi-select-take
title: Object pie multi-selection take slices
topic: viewer
status: blocked
origin: follow-up from viewer-object-context-menu (2026-07-21)
blocked_by: [viewer-object-selection-core]
refs: [viewer-object-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's take sub-pie switches its slices on single vs multi selection:
single shows `Take Copy` / `Take` (wired by [[viewer-object-context-menu]]),
multi shows `Copies: Separately`, `Take: Combined`, `Copy: Combined` and
`Take: Separately`. There is no multi-object selection yet, so the four
multi-selection slices sit greyed (`UNIMPLEMENTED`) at their reference
positions.

Once [[viewer-object-selection-core]] provides the maintained selection set,
wire them:

- **Combined** take / copy: one `DeRezObject` batch over every selected root
  (they land as a single coalesced inventory object, the server's combining
  behaviour).
- **Separately**: one derez per selected root, each with its own transaction
  id.
- Visibility: the reference hides the single-selection pair on a
  multi-selection and vice versa; our model keeps every slice in place and
  should instead enable the set that applies (single-selection conditions vs
  a new multi-selection condition), preserving the pie's angular stability.
- The open-time condition set must then read the selection, not just the
  picked prim — the enable flags become "all selected roots owned / copyable"
  as the reference's `Tools.EnableTakeCopy` / `Object.EnableTakeMultiple` do.
