---
id: viewer-combo-stops-opening
title: A combo can stop dropping down (seen on the contact-sets chooser)
topic: viewer
status: bugs
origin: seen while live-checking [[viewer-contact-set-presence-extras]]
  (2026-08-20)
refs: [viewer-contact-sets, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

Seen once on the local OpenSim: after picking **Pseudonyms** in the Contact
Sets panel's set chooser, the chooser stopped opening — clicking it did
nothing, no dropdown appeared, and it stayed that way across a switch to
another People sub-tab and back. The rest of the panel was unaffected.

**Not reproduced** on the next run, with the same build and the same
sequence — the chooser opened on every click. So this is intermittent, and
what state it depends on is unknown. It is filed rather than fixed because a
symptom that cannot be reproduced cannot be verified fixed.

## What is already ruled out

The second run was instrumented (that instrumentation is now the permanent
`debug!` in `toggle_combo_popover`, see below) and showed the healthy path
throughout: `disabled=false`, `open_popovers=0`, `options=4`, and a
`building a combo popover rows=4` for every single press. So in the working
case none of the obvious candidates is happening.

Also ruled out by reading:

- The chooser is **not** one of the widgets this task greyed —
  [[viewer-contact-set-presence-extras]] adds `InteractionDisabled` only to
  entities carrying `ContactSetsButton`, and nothing else in the viewer
  disables this combo (`grep` for `insert(InteractionDisabled)`).
- A hidden floater is not swallowing the press: `UiPanelShown(false)` sets
  `Display::None`, so a closed floater has no box to hit.
- The combo's value text and arrow are both `Pickable::IGNORE`, so a press
  anywhere on the combo targets the anchor.

## Second sighting (2026-08-21)

Seen again on the contact-sets chooser, on aditi — on the build that fixed
[[viewer-clipped-links-still-pickable]], which made the picking clip walk
*stricter*, so that change was the immediate suspect: a control that overflows
its pane used to stay clickable and would now be correctly clipped out of
reach. The very next run, with `ui_combo=debug` on, the chooser opened on every
one of four presses — `disabled=false`, `open_popovers=0`, `mine_open=false`,
`options=4`, and a `building a combo popover rows=4` each time. The healthy
path again, and the failing occurrence itself was never under logging.

So the clip walk is **not** implicated by any evidence — but it is not cleared
either, and it sharpens the leading hypothesis below: if the press really never
arrives, a chooser row that overflows its pane is now a *sufficient* cause,
where before it was only a suspicious one. The next capture should therefore
also dump the anchor's global rect against each clipping ancestor's clip rect,
not just the `combo press` line.

## How to capture it next time

Run with `RUST_LOG=sl_client_bevy_viewer::ui_combo=debug` and reproduce. The
`combo press` line names the cause directly:

- **no line at all** — the press never reached the combo. A hit-test or
  layout problem: something is over it, or its box is not where it is drawn.
  Suspect the chooser row overflowing (combo + three buttons at 100% width in
  a narrow panel).
- `disabled=true` — something marked it `InteractionDisabled`.
- `mine_open=true` — a popover of its own is still alive, so the press
  *closes* instead of opening; if that popover is invisible or placed
  off-window, every other click would look like nothing happening.
- `options=Some(0)` — an empty option list builds an empty popover, which
  looks like no dropdown.
- The healthy line plus `building a combo popover` — it does open, so the
  popover is being drawn somewhere the user cannot see it (placement /
  z-order).

Reference (Firestorm, read-only): none — this is our own widget
(`src/ui_combo.rs`).
