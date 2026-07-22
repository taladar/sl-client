---
id: viewer-voice-controls
title: Voice controls — talk button, PTT, participants, per-speaker volume
topic: viewer
status: blocked
origin: voice brought fully in scope for the viewer (user decision,
  2026-07-22)
blocked_by: [viewer-voice-audio]
refs: [viewer-name-tags-decorations, viewer-vintage-bottom-bar, viewer-social-im-conversations]
---

Context: [context/viewer.md](../context/viewer.md).

The voice **UI** over the transport [[viewer-voice-audio]] provides (that
task owns WebRTC/`SLData`, mic capture, AEC, and emits per-agent RMS /
voice-activity plus gain/mute commands):

- **Talk button** — the speak toggle (bottom bar; Vintage's voice cluster
  slot — [[viewer-vintage-bottom-bar]]), **push-to-talk** key via the
  input action map with a PTT-lock mode, and the mic-level meter while
  transmitting.
- **Voice controls floater** — current channel name (nearby / group / ad
  hoc / P2P), the participant list with speaking indicators (green-wave
  levels from the RMS data) and **per-speaker volume slider + mute**
  (sent as `SLData` gain/mute), and the input/output **device pickers**
  (capture device selection lands here; output device belongs to the
  audio backend's settings).
- **Speaking indicators elsewhere** — the same per-agent activity data
  drives indicators in the conversations floater's participant lists
  ([[viewer-social-im-conversations]]) and the voice dot near the name
  tag ([[viewer-name-tags-decorations]] hooks it in).

Reference (Firestorm, read-only): `fsfloatervoicecontrols`,
`floater_fs_voice_controls.xml`, `llvoicechannel`,
`floater_sound_devices.xml`, `llspeakers`.

Deps: [[viewer-voice-audio]] (transport, activity data, gain control).
