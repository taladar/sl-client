---
id: viewer-flycam-stop-button-overlaps-chat
title: "Stop flycam" button overlaps the "Chat" button in the bottom bar
topic: viewer
status: done
origin: user report during viewer-avatar-tongue-protrudes aditi testing (2026-08-05)
---

Context: [context/viewer.md](../context/viewer.md).

In the viewer bottom bar, the **Stand / Stop-flycam button** (added with the
seated-placement / sit-camera work) overlaps the **Chat** button rather than
laying out beside it. The two controls draw on top of each other.

Likely a layout issue in the bottom-bar row: the conditionally-shown
Stand/Stop-flycam button is not participating in the flex layout the way the
fixed buttons are (absolute placement, or inserted without reserving width), so
it lands over the neighbouring Chat button. Fix the bottom-bar row so the
button takes its own slot and pushes/leaves room for the others.

## Outcome (2026-08-14): DONE

Root cause: **both** state buttons (Stand + Stop-flycam) are spawned into the
reserved state slot and toggled with `Visibility::Hidden` — but in Bevy UI
`Visibility::Hidden` only stops rendering, it does not remove a node from flex
layout. So both buttons always occupied layout side-by-side in the fixed-width
slot, their combined width overflowed it, and the overflow drew over the
neighbouring Chat button. Fixed by toggling `Display` (`None`/`Flex`) instead of
`Visibility` (`stand_stop_button.rs`), so the inactive button is removed from
layout and only the shown one takes space.

Follow-up the user then reported and this change also fixes: the shown button
had a larger gap to its neighbour than the inter-button gaps, because it was
centred in the fixed-width slot (the slack sat between it and the first toolbar
button). Aligned the button to the slot's **trailing** edge (`FlexEnd` in
`spawn_state_slot`, `bottom_toolbar.rs`) so its gap is just the bar's
`column_gap`, like every other button; the slack now falls on the window-edge
side where there is no button to space away from.

Overlap and non-overlap both live-verified on OpenSim; the gap alignment is a
static flex property (visual confirm pending in the next run).
