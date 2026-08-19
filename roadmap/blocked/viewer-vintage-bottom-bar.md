---
id: viewer-vintage-bottom-bar
title: Classic (Vintage) bottom-bar arrangement
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-animation-overrider, viewer-toolbar-customization]
refs: [viewer-toolbar-customization, viewer-animation-overrider, viewer-quick-preferences, viewer-movement-controls-floater]
---

Context: [context/viewer.md](../context/viewer.md).

Assemble the Vintage skin's classic bottom bar — the one real **layout**
difference Vintage carries over the default skin (its
`panel_toolbar_view.xml` override): left-to-right, the inline
**nearby-chat entry** (our chat bar already lives there), the command
buttons, then the right-hand **utility cluster** — parcel audio / media
play-pause, the master **volume slider + mute** with the volume popup
([[viewer-volume-panel]]), and the **AO** and **quick-prefs** buttons
([[viewer-animation-overrider]], [[viewer-quick-preferences]]).

Deliberately **not** included (user decision, 2026-07-22): Vintage's
stand / stop-flying strip docked above the bar — it wastes a row between
the conversations floater and the bar; those buttons are placed by
[[viewer-movement-controls-floater]] instead.

This task is the arrangement + the utility cluster wiring, kept
compatible with toolbar customization
([[viewer-toolbar-customization]]) — the classic layout is the default
arrangement, not a hard-coded special case.

Reference (Firestorm, read-only):
`skins/vintage/xui/en/panel_toolbar_view.xml`, `skins/vintage/toolbars.xml`.

Deps: [[viewer-volume-panel]] (the audio cluster; AO / quick-prefs slots
degrade to hidden until their features land).

## Parity-audit addendum (2026-08-19)

The Vintage utility cluster (`utilitybar.cpp`) also hosts two voice
slots this body does not mention: the **talk button**
(`utility_talk_button`, press/release mic) and the **PTT-lock button**
(`utility_push_to_talk_lock_button`). The controls themselves are
[[viewer-voice-controls]] scope; the vintage cluster should reserve
and host the two slots, wired when voice-controls lands.
