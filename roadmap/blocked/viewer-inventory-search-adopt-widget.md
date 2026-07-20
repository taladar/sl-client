---
id: viewer-inventory-search-adopt-widget
title: Adopt the reusable search-field widget in the inventory search
topic: viewer
status: blocked
origin: follow-up requested during viewer-ui-text-input-widget (2026-07)
blocked_by: [viewer-ui-search-field]
refs: [viewer-inventory-search-filter, viewer-ui-menu-search]
---

Context: [context/viewer.md](../context/viewer.md).

Migrate the inventory window's search box onto the reusable **search-field**
widget once [[viewer-ui-search-field]] extracts it. Today `crate::inventory`
hand-rolls its own single-line `EditableText` plus `read_search_field` (as
[[viewer-inventory-search-filter]] shipped it), which is the same
field-plus-clear affordance `crate::menu_search` grows independently — exactly
the duplication the widget exists to remove.

Replace the inventory's bespoke field with `spawn_search_field`, keeping the
existing behaviour: the search term still drives `read_search_field`'s filter
(the consumer reacts to `EditableText::value` / `Changed<EditableText>`, so the
model-filtering logic is untouched), the `×` clear button and `Escape`-to-clear
come from the widget rather than being re-implemented, and the placeholder /
search glyph come along for free. Net a deletion: the inventory keeps its
filtering, and the widget owns the input chrome.

This is the second consumer that proves the extraction —
[[viewer-ui-search-field]] migrates the menu bar's search; this one migrates the
inventory's — so the widget is validated against two real, independently-written
call sites rather than one.

Reference (Firestorm, read-only): `llfiltereditor` as used by the inventory
panel's filter box.
