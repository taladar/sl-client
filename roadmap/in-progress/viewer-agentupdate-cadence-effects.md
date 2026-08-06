---
id: viewer-agentupdate-cadence-effects
title: Explore what the raised AgentUpdate/camera-interest cadence buys (and how the sim reacts)
topic: viewer
status: in-progress
origin: raised committing the cadence bump in viewer-physical-object-motion-not-smooth (2026-08-06)
refs: [viewer-physical-object-motion-not-smooth]
---

Context: [context/viewer.md](../context/viewer.md).

While chasing vehicle-motion smoothness
([[viewer-physical-object-motion-not-smooth]]) the viewer's interest-camera
`AgentUpdate` cadence was raised from **2 Hz to ~45 Hz** (`session.rs`
`report_camera_interest`, `CAMERA_INTEREST_MIN_PERIOD_SECS = 1/45`,
send-on-camera-movement, 1 Hz keep-alive floor). The reference viewer sends
`AgentUpdate` at up to `MAX_AGENT_UPDATES_PER_SECOND` (125) on camera / control
change with a 1 Hz floor (`indra/newview/llviewermessage.cpp`), so 2 Hz was
plainly under the reference and the bump is reference-faithful — it is committed
as-is.

**But we do not actually know what it buys us, or how the simulator reacts** —
the change was kept on faith + a subjective "seemed smoother", not a
measurement. Open questions:

- **Object-update rate.** The measured driven-vehicle stream stayed ~14 Hz
  irregular *both* before and after the bump — i.e. the higher cadence did
  **not** visibly densify the object stream in our test. Is the sim's per-object
  update rate genuinely independent of our `AgentUpdate` rate, or does it
  respond under other conditions (distance, interest priority, bandwidth
  headroom)? Compare against Firestorm in the same region: does its higher
  cadence correlate with a denser object stream, or is ~14 Hz just this
  region/object?
- **Interest list / avatar resolution.** The stated benefit (from R22) is that
  the camera viewpoint drives which objects/avatars the sim streams as full
  updates. Measure: does 45 Hz vs 2 Hz change how fast a distant avatar resolves
  on approach, or coarsens on retreat?
- **Simulator reaction / cost.** Does a high `AgentUpdate` rate change the sim's
  behaviour — object prioritisation, the region `AgentUpdatesPerSecond` stat,
  throttling, or sim load — and is there any downside (upstream bandwidth, being
  rate-limited, packet flood)? Confirm the camera-only updates never induce
  unintended agent *movement* (they carry camera fields only, controls
  unchanged).
- **The right cap.** Is ~45 Hz correct, or should it track the display rate up
  to the reference's 125 with tighter change thresholds?

Deliverable: a measured account (ideally an aditi A/B with the object/interest
diagnostics) of what the cadence actually changes, feeding back into whether 45
Hz is the right value and whether it belongs on for all sessions.
