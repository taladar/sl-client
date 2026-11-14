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
`report_camera_interest`, a 1/45 s minimum period (now
`CadencePreset::Current45Hz` in `interest_cadence.rs`),
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

## Measurement tooling (2026-11-14)

`sl-viewer-world-view/src/interest_cadence.rs` makes the cadence a preset and
reads out what comes back, so the A/B is one session on one scene. That matters
because separate logins mix the cadence up with whatever the region was doing
in each:

- `SL_VIEWER_INTEREST_CADENCE` picks the preset: `2` (the old 0.5 s cadence),
  `45` (the default, unchanged behaviour), `125` (the reference's
  `send_agent_update`: 125 Hz cap, 1 cm `TRANSLATE_THRESHOLD`, a 0.1°
  `CAMERA_AT_THRESHOLD`; ours was 2 cm and ~2.6°), or `cycle`, which steps
  2 → 45 → 125 every 20 s.
- `SL_VIEWER_LOG_INTEREST_CADENCE=1` logs one `interest_cadence` line per
  second: the active preset, the interest reports sent, the object-update
  stream per form (messages/objects for terse, full, compressed, cached and
  kill), how many objects got a terse update, the three busiest by terse count,
  and the region's own `AgentUpdatesPerSecond` and `SimFps` from `SimStats`.
- Avatar resolution is the existing `SL_VIEWER_LOG_AVATAR_INTEREST=1` census.

A smoke run on aditi showed the line working. Bonifacio reported
`AgentUpdatesPerSecond` ~180 at `SimFps` 45 with the camera mostly still, so
that stat is region-wide and cannot isolate our own rate. The rest of the
readout is per-session and is what the comparison rests on.

**Still to do, needs someone at the controls:** one aditi session with

```sh
RUST_LOG=info SL_VIEWER_INTEREST_CADENCE=cycle \
  SL_VIEWER_LOG_INTEREST_CADENCE=1 SL_VIEWER_LOG_AVATAR_INTEREST=1 \
  ./target/release/sl-client-bevy-viewer \
  --credentials credentials.aditi.toml --grid aditi --avatar primary
```

with the camera kept moving for 3+ minutes: ideally driving a vehicle (the
case the 45 Hz bump was for, read off the busiest object's terse rate per
preset), and passing another avatar (resolve time per preset). The cadence
changes nothing while the camera is still, because then only the 1 Hz
keep-alive is sent under every preset. Then compare the windows per preset and
decide the default (45, or the reference's 125 with its finer thresholds).
