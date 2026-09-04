---
id: viewer-audit-table-interaction-disabled
title: The table widget ignores InteractionDisabled
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`grep InteractionDisabled sl-viewer-ui-widgets/src/ui_table.rs` returns zero
hits, yet `sort_header_on_press` (`:1121`), `resize_border_on_drag` (`:1147`)
and `select_table_row_on_press` (`:1455`) all act on a press with no disabled
check.

`ui_combo.rs:288`, `ui_radio.rs:328`, `ui_text_input.rs:872` and
`ui_color_picker.rs:116` all **do** honour it, so this is an inconsistency
rather than a policy. `ui_tab.rs`, `ui_search.rs` and `floater.rs` are likewise
zero-hit and should be checked in the same pass.

The project's rule is that `InteractionDisabled` is advisory in Bevy and each
widget must honour it in its own press/change observers.

## Outcome (2026-09-04): five widgets, and the marker means two things

All four named modules — plus `sl-viewer-media/src/browser_widget.rs`, which the
same audit had left as "niche" — now honour the marker in their own observers.
The rule the pass settled on, and wrote into each module's docs:

- **A gesture that changes the widget is refused.** Sorting, resizing and
  selecting a table; switching a tab and dragging its divider; clearing a search
  field; dragging, resizing, minimizing, docking or closing a floater; every
  pointer and key event forwarded into a browser view.
- **Reading is not.** Every scroll observer was left alone, and so was the
  table viewport's focus-on-click (which is what routes the wheel to it): a
  table, list or page the agent may not *change* is still one they must be able
  to *read* to the bottom. A tab strip keeps its active highlight for the same
  reason — hiding which panel is open would cost more than the greyed label
  buys.

Both ends of each gesture are tested, not just the widget root, so the marker
addresses two different scopes with no new API: on the root it freezes the whole
widget, on one header cell, row, tab button or chrome button it takes away that
one affordance. That is the reference's per-part enable
(`LLTabContainer::enableTabButton`, `LLFloater::setCanClose`) expressed with
Bevy's own marker rather than a second mechanism.

### The visible half

`InteractionDisabled` is advisory — Bevy's only built-in effect is the a11y tree
— so a widget that merely refuses the click looks broken. Three greying systems
were added beside the refusals: `reflect_table_disabled` (header labels and sort
arrows), `reflect_tab_disabled` (tab labels and their ellipsis markers) and
`reflect_search_clear_disabled` (the `×` glyph). Each restores the spec's colour
when the marker goes away, and each write is change-guarded.

Two places deliberately have **no** greying, and say why in the code:

- **Table body cells.** Their colour is the consumer's, written on every bind by
  `set_table_cell`, so a widget system that greyed them would have no colour to
  put back — and the next bind would undo the greying anyway.
- **The search clear button's circle** and a floater's chrome, which carry skin
  classes: their background belongs to the `bevy_flair` cascade, not to a system
  overwriting it every frame.

### What upstream already did, and why the check stayed anyway

`bevy_ui_widgets`' `RadioButton` (which every tab button is) already refuses a
click or key on a *disabled button*, but nothing upstream checks the **group**,
which is the tab strip. The strip check was therefore the load-bearing half. The
per-button check was kept regardless: it also refuses a `ValueChange` written
straight into the world, which is how a consumer drives a strip programmatically
and how the test drives it here.

### Where it did not fit

`persist_on_drag_end` refuses too — not because a refused drag is dangerous, but
because marking the table dirty would write the column widths to the settings
file on a gesture the widget had just declined.

A floater's chrome press still emits `BringToFront` before the disabled check.
Raising a window is not a change to it, and the floater root's own press
observer raises on any press anywhere, so skipping it in the chrome would have
made the buttons behave differently from the window body under them.

### Verification

Eight new unit tests across four modules, each pairing the refusal with the
enabled behaviour in the same test — a test that only asserts "nothing
happened" would keep passing on an observer that had stopped running at all.
`ui_search`'s drives a **real click** through the interaction harness rather
than a poked observer, because the clear button is `Display::None` until the
field holds a term: aiming at it by entity would not have noticed it becoming
unreachable. The rest trigger the `Pointer` events by hand, which is what the
table, tab and floater harnesses have (no picking backend).

Not covered: the browser view's forwarding into a live CEF surface, which needs
an engine. Its test asserts the load-bearing half that is reachable headlessly —
a disabled view refuses the *focus* a click would grant it, which is what would
otherwise route every subsequent keystroke into the page.
