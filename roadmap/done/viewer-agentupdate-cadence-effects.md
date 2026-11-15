---
id: viewer-agentupdate-cadence-effects
title: Explore what the raised AgentUpdate/camera-interest cadence buys (and how the sim reacts)
topic: viewer
status: done
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

## Measured: a spinning camera over a static scene (2026-11-15)

Two identical headless aditi sessions (`--headless`, the `primary` landing in
Bonifacio, `--camera-position 175,102,35 --camera-look-at 128,128,30
--camera-spin 30`, `SL_VIEWER_INTEREST_CADENCE=cycle`, 330 s each, the first
40 readout seconds dropped as login streaming). Per-second means over ~90 s
per preset per run (run 1 / run 2):

| preset | reports | terse objects | full objects | busiest terse |
| --- | --- | --- | --- | --- |
| 2 | 2.4 / 2.4 | 5.3 / 5.0 | 18.5 / 17.4 | 1.3 / 1.2 |
| 45 | 8.3 / 8.2 | 4.7 / 4.8 | 18.4 / 16.6 | 1.2 / 1.2 |
| 125 | 17.7 / 17.3 | 3.2 / 4.0 | 16.7 / 16.4 | 0.8 / 1.0 |

- **Reports are frame-bound, not cap-bound.** The headless viewer ran at
  about 18 frames a second against aditi, so the 125 preset (a 0.1° threshold
  against 1.7° of turn per frame) sent once per frame; the 45 preset's 2.6°
  threshold sent every second frame. In a windowed session at 60+ fps the 125
  preset would send far more.
- **The object stream does not follow the cadence.** Full updates hold at
  ~17–18 objects/s under all three presets: what the simulator streams for a
  turning camera does not speed up when it is told about the turn 7× more
  often. Compressed and cached forms were zero throughout; kills negligible.
- **Terse updates are lower under 125, in both runs**: 3.2 and 4.0 objects/s
  against 5.0–5.3 at 2 Hz, and every one of the ten 20 s windows under 125
  sits below its run's 2 Hz mean. The terse stream here is a handful
  of slowly moving objects (local ids 112905530–37 carry most of it), so
  this is objects' own motion being reported *less* often while we send
  more. Why is not known — a guess is that each significant camera change
  makes the simulator re-prioritise the agent's update queue. It is a reason
  not to adopt the reference's 125 Hz thresholds without the vehicle case.
- **The region's `AgentUpdatesPerSecond` does not move at all** (320.1 /
  320.2 / 320.1, and 328 in run 2), even though our own rate goes from 2.4 to
  17.7 per second. About 7 avatars × `SimFps` 45 ≈ 320, so the stat looks
  like one processed update per agent per simulator frame, not a count of
  what arrives. If that is how the simulator works, anything above its 45 Hz
  frame rate is coalesced, and a cap above 45 buys nothing on the sim side.
- **Avatar resolution is unchanged** — the census held at 8 full / 8–9
  coarse throughout, but the camera never moved closer to or further from
  any avatar, so this case says nothing about approach or retreat.

Verdict for the static-scene case: going from 2 to 45 Hz changed nothing
measurable, and 125 coincided with fewer terse updates. The flight below does
not show that drop.

## Measured: flying through a crowd with the camera following (2026-11-15)

The vehicle case, driven by automation instead of a person. A headless viewer
from `sl-viewer-ctl launch` logged the `primary` in (it was in Mauve that
day). A small Python controller over `sl-viewer-ctl agent --json` and
`sl-viewer-ctl press <key> --hold <frames>` flew it round a loop of waypoints
at ~14 m above the ground, (40,50) → (100,62) → (112,72) → (40,90) →
(25,70), about 35 s a lap. The loop ran straight through four avatars standing
at ~(104–110, 63–73), with others nearby. The third-person camera follows the
avatar, so the viewpoint moved and turned the whole time, and the own avatar
is the moving object the camera follows, standing in for the vehicle. Two
details for repeating it: heading in the `agent` readout is `atan2(dy, dx)`
(east 0, `ArrowLeft` raises it), and a turn must be pressed a few frames at a
time with a re-read in between, or it overshoots.

The readout now also splits the full and compressed updates into
`arrived=new/returning/changed`. Those are objects the session never held,
objects re-sent after a kill earlier in the session (`StreamHistory`), and
changes to held ones. Without that split, a busier stream under a busier
cadence could not be told apart as fresher content or the same objects churned.

Eight minutes, ~150 readout seconds per preset (the first 16 s window, still
login streaming, dropped), per-second means:

| preset | reports | own terse | new | returning | changed | kills | avatars |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2 | 2.2 | 8.2 | 40.6 | 17.4 | 4.7 | 19.5 | 9.0 |
| 45 | 11.3 | 8.8 | 33.9 | 15.1 | 4.3 | 19.4 | 8.9 |
| 125 | 12.5 | 8.7 | 32.1 | 13.2 | 3.4 | 17.5 | 8.9 |

- **The followed object's motion stream does not follow the cadence.** The
  own avatar's terse rate stays at 8–9 a second under every preset, and its
  20 s windows overlap (7.5–9.8 at 2 Hz, 7.7–9.8 at 45 and 125). This agrees
  with the original observation that the driven kart's stream was ~14 Hz
  before and after the bump: the simulator sets a moving object's update
  rate, not our `AgentUpdate` rate.
- **What arrives does not follow it either.** Arrivals come in bursts of
  100–300 objects a second as the loop reaches dense parts of the scene, and
  the bursts land in 2, 45 and 125 windows alike. Over the run, new,
  returning and changed objects and kills are the same at all three presets
  within that burst noise. An earlier flight without the split (on the same
  path, 6 min) seemed to show full updates and kills rising with the cadence
  (3 / 28 / 39 full a second). This run does not reproduce that, and the
  per-window spread there (0.5–113) was the same bursts.
- **About a third of arrivals are re-sends.** Some 13–17 objects a second
  come back after a kill, under every preset. The simulator culls what leaves
  the agent's interest and sends it again when it comes back, however often
  it is told where the camera is.
- **Avatar resolution does not change.** The census held at 9 full / 6
  coarse under every preset, while the loop passed avatars a few metres away
  every lap.
- **45 and 125 send nearly the same number of reports while moving** (11.3 /
  12.5). A camera that translates crosses both presets' distance thresholds
  every frame, so both send once a frame at the headless viewer's frame rate
  against aditi. They differ only while the camera turns slowly in place.

## Outcome

Across the static scene and the flight, raising the cadence from 2 to 45 to
125 Hz changes nothing measurable in what aditi streams back: not the motion
stream of a moving object the camera follows, not what arrives or is re-sent,
not avatar resolution. The region's `AgentUpdatesPerSecond` does not even
register our rate. **45 Hz stays the default.** It is within the reference's
125 Hz cap, sends on movement only, and showed no downside. The smoothness
`viewer-physical-object-motion-not-smooth` was after has to come from the
viewer's own interpolation of the ~8–14 Hz stream the simulator sends, not
from asking it more often. The presets and the readout stay as tooling for
the next question about the stream.
