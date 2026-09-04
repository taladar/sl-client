---
id: viewer-audit-tab-panel-focus-order
title: Inactive tab panels keep their keyboard tab stops
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-widgets/src/ui_tab.rs:1531` — `reconcile_tab_selection` hides a
non-selected panel with `*visibility = Visibility::Hidden` only. Nothing in
`ui_tab.rs` touches `TabIndex` (the only `TabIndex` writes are at `:663`, on the
strip).

The project's own doc states the rule this breaks
(`sl-viewer-ui-core/src/ui.rs:645`): *"`bevy_input_focus`'s tab navigation walks
the hierarchy and does **not** check visibility or display, so a closed panel's
buttons stay reachable by `Tab`"*. So every tabbed floater — preferences,
profiles, places, search, pickers, inventory — tabs focus into invisible panels.
The fix already exists next door: `ui.rs`'s `ParkedTabIndex` mechanism, which
this widget does not reuse.

Nested variant, one level up: `sl-viewer-ui-core/src/ui.rs:685` —
`apply_panel_visibility` walks
`once(panel).chain(children.iter_descendants(panel))` and, when shown, restores
**every** `ParkedTabIndex` in the subtree. A nested `UiPanelShown(false)` panel
inside it did not change, so nothing re-parks it and its widgets rejoin the tab
cycle while still `Display::None`. The one test (`ui.rs:1756`) uses a single
flat panel and cannot see this.

Both are directly testable: `sl_viewer_testkit::navigate` drives real
`NavAction::Next` and is currently used at exactly two call sites in the whole
workspace (`sl-client-bevy-viewer/src/ui_test.rs:503`, `:512`).

## Outcome (2026-09-04): one place hides a subtree, and it is not a `Visibility`

Both halves came from the same shape — each hiding mechanism wrote its own
field and reasoned about tab stops on its own — so the fix is a single
`PanelVisibility` system parameter in `sl-viewer-ui-core/src/ui.rs` that owns
`Node`, `Visibility`, the `ParkedTabIndex` park/restore and the focus drop
together. `apply_panel_visibility` and `reconcile_tab_selection` now both call
`set_shown(root, shown, HideWith::{Display,Visibility})`; the tab widget no
longer writes `Visibility` itself. `HideWith` is the *whole* remaining
difference between the two — a tab panel must stay laid out so the panel area
holds the largest of them, a scaffold panel must leave the flow.

### Hiding and un-hiding are not mirror images

Parking sweeps the whole subtree unconditionally. Un-parking cannot, and that is
the nested variant: the restore walk **stops at any managed subtree that is
itself hiding**, and **declines entirely** when an ancestor of the root being
shown is still hidden (a tab switching inside a closed floater must not hand the
keyboard back). Both tests read the owner's own live state — `UiPanelShown`, and
`Visibility` beside a `TabStopsFollowVisibility` marker — never a bookkeeping
component, so the two mechanisms need no agreement about who parked what, and
no system ordering between them: the answer is already right in the frame a
panel closes, before the flow field is written.

### A third defect the finding did not name

Neither mechanism ever saw the hide of a panel *born* hidden, because there was
no transition — and that is the normal build order: `spawn_tab_container` stands
its panels up hidden and consumers fill them afterwards. So a tabbed floater's
off-tab widgets were reachable from the moment it opened until the first switch,
which the two fixes above would not have touched.
`park_new_tab_stops_in_hidden_subtrees` (`Added<TabIndex>`, registered beside
`apply_panel_visibility` in the plugin, the testkit and the gallery) closes it.

### Why the opt-in marker

The first cut keyed on bare `Display::None` / `Visibility::Hidden`, which is
wrong in the dangerous direction: this viewer hides things with those fields for
a dozen reasons that are *not* a managed subtree — `virtual_list` parks its
pooled rows with `Display::None` and gives them back by writing `Node` itself, a
scrollbar hides when its content fits, a tooltip hides with no pointer on it —
and none of those owners would ever un-park a stop taken from them. Carrying
`UiPanelShown` is opt-in already; `TabStopsFollowVisibility` is the same
statement for the `Visibility` half, and `spawn_tab_container` puts it on every
panel.

### Pinned by

Six tests, each verified to fail against the old code:
`re_opening_a_panel_leaves_a_nested_closed_one_out_of_the_tab_cycle`,
`opening_a_panel_inside_a_closed_one_keeps_it_parked` and
`a_tab_stop_born_in_a_hidden_subtree_is_parked` (`ui.rs`);
`only_the_selected_panel_keeps_its_tab_stops`,
`tab_never_lands_in_an_unselected_panel` and
`switching_away_drops_focus_that_was_inside_the_panel` (`ui_tab.rs`). The second
of those drives `bevy_input_focus`'s real `TabNavigation`, so what is under test
is the keystroke rather than the presence of a component.

`tab_app()` now needs `InputFocus`: a tab switch drops focus the panel it hides
was holding, which is the scaffold's resource, not this widget's.
