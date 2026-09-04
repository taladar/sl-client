---
id: viewer-audit-virtual-list-rebinding
title: The recycling virtual list re-binds every pooled row on a one-row scroll
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-core/src/virtual_list.rs:500-505` —
`let index = Some(window.first.saturating_add(slot))` maps pool slot to item by
**offset**, so a one-row scroll changes `VirtualRow::index` on all N pooled rows
and wakes every consumer's `Changed<VirtualRow>` bind.

The module doc (`:8`) promises the opposite: *"a row that scrolls off the top is
re-bound to the item now scrolling in at the bottom"* — which is a modular
mapping (`slot == index % pool_len`). This is performance, not correctness, but
it is the widget's entire reason to exist.

Second defect, `:481-497` — freshly grown pool rows are blank for one frame: the
growth loop `commands.spawn(...)` is deferred, then the bind loop immediately
below does `rows.get_mut(entity)`, which cannot see the un-flushed spawn and
`continue`s. Self-correcting, undocumented.

`scrollbar_geometry` (`:193`) and `max_scroll` (`:366`) are already pure free
functions with no tests, while `row_window` beside them is tested.

Related: `sl-viewer-ui-widgets/src/ui_table.rs:987-999` — `spawn_table_row`
inserts a whole `Node`, stomping the `top` and `display` fields
`layout_virtual_lists` owns (`virtual_list.rs:512-528`), so a parked row briefly
renders at `top: Auto`.

## Done

The mapping is now `slot_index`, a pure function documented as the reason the
widget exists: item `i` lives in slot `i % pool_len`, so one row of scroll moves
one slot. The pool length is settled (grown to cover the window) *before*
anything is bound, because it is the modulus — the growth half and the bind half
of one frame must agree. Growing the pool does rebind all of it, but a pool only
ever grows, and only to what the viewport needs.

Grown rows are spawned already bound and already placed, through a shared
`row_node` the bind path writes the same fields from, so a fresh row and a
recycled one cannot be laid out differently.

The `Node`-stomping was **not** only `ui_table`: `edit_contents`, `inventory`,
`emoji_picker` and `groups` each insert their own row `Node` the same way.
Rather than patch five sites, `virtual_list::amend_row_node` is now the
supported way to dress a pooled row — it amends the component in place, so `top`
and `display` stay the list's — and all five go through it.

Nine new tests. The two that matter are canaries for the two defects: one row of
scroll must wake exactly **one** `Changed<VirtualRow>` (counted through a real
`App` running the real system — it was the whole pool), and a row grown this
frame must come up bound and visible rather than blank. `scrollbar_geometry` is
tested now too, including the minimum-thumb clamp.
