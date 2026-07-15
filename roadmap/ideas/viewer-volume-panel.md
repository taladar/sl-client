---
id: viewer-volume-panel
title: Volume panel (master + per-category sliders)
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-ui-widget-scaffold, viewer-audio-backend]
---

Context: [context/viewer.md](../context/viewer.md).

The volume popup behind the speaker button: a master slider and mute, plus one
slider and mute per audio category — sound effects (in-world), ambient /
environment, UI, streaming music, media, and voice — with the per-category
levels persisted and applied live.

It is the user-facing face of the mixer that [[viewer-audio-backend]] builds:
the categories are exactly that task's volume buses, so this panel should read
and write them rather than inventing a parallel notion of "volume". Each
producer then simply plays on its bus — [[viewer-in-world-sounds]],
[[viewer-ui-sound-effects]], [[viewer-streaming-audio]] and
[[viewer-voice-audio]] — which is also why this is worth a task of its own: it
is the one place where all four meet, and it should exist before the fourth
one lands so nobody grows a private volume control.

Scope: the panel and its button, the category set and its mapping onto the
mixer's buses, master vs. per-category interaction, mute semantics (mute is not
"volume 0" — it must restore the previous level), persistence through the
settings store, and the mute-on-focus-loss / mute-on-minimise behaviour the
reference viewer offers. The sliders also want a home in
[[viewer-quick-preferences]] and the full [[viewer-preferences-ui]] audio tab —
same store, three views.

Reference (Firestorm, read-only): `llfloatervolumepulldown`,
`llpanelvolumepulldown`, the `llui` audio settings group.

Deps: [[viewer-ui-widget-scaffold]] (the pulldown panel),
[[viewer-audio-backend]] (the mixer buses the sliders drive).
