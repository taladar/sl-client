---
id: viewer-own-motion-timed-stops
title: The own avatar's animations never told the simulator they had finished
topic: viewer
status: done
origin: found investigating [[viewer-crossing-movement-locks-up]] (2026-09-15)
refs: [viewer-crossing-movement-locks-up, viewer-movement-quickjump-movelock]
---

Context: [context/viewer.md](../context/viewer.md).

A simulator starts and lists an avatar's animations but never plays them, so it
cannot know when a non-looping one has finished. The viewer that owns the
avatar does, and the reference says so:

- `LLMotionController::activateMotionInstance` gives a non-looping motion of
  non-zero length a send-stop timestamp one ease-out before its end, and on the
  first frame past it `LLAgent::requestStopMotion` sends an `AgentAnimation`
  **stop** for it.
- `LLAgent::onAnimStop` also raises the one-shot **`AGENT_CONTROL_FINISH_ANIM`**
  (0x8000) control bit when the motion is `standup`, and when it is `pre_jump`,
  `land` or `medium_land` unless the ascend key is held — and, for a landing,
  not within `RecentJumpThresholdSecs` (1 s) of a jump input, so a rapid second
  jump's pre-jump is not skipped (FIRE-34049). A Second Life simulator waits for
  that bit before leaving those states; Firestorm's own comment there notes that
  withholding it on a pre-jump "can stall" a quick jump.

sl-client had the `FINISH_ANIM` constant and sent neither signal. No symptom was
pinned on the gap — jumping worked on aditi before the change, aditi never
played `standup` even after a 200 m fall, and OpenSim ignores the bit (its
animator ends the landing state on a 1 s timer) — so this is parity with the
reference rather than a fix for an observed failure.

## Done (2026-09-15)

- `sl-proto`: `Session::finish_animation` (one-shot `FINISH_ANIM`, like `stand`)
  and `Command::FinishAnimation`, wired through both runtimes and `sl-repl`
  (`finish_animation`); `finish_animation_is_a_one_shot_control` pins that the
  bit is not left in the keep-alive.
- `sl-viewer-world-avatar`: `take_run_out` reports, once per activation, each
  animation in the own avatar's **simulator-signalled** set that passed its
  send-stop point (a re-trigger — a new sequence id — is reported again; one the
  simulator already dropped is not). `motion_stops::request_own_motion_stops`
  sends the `AgentAnimation` stop for each and `FinishAnimation` for the holding
  built-ins under the reference's rule.
- Ordered after `poll_animations` (a motion decoded this frame is checked this
  frame) and before the skeleton driver, whose pruning would otherwise drop a
  motion first seen past its end before it was reported. A schedule-edge test
  pins it and was confirmed to fail without the edge.
- One deliberate departure: a holding built-in whose asset can never arrive (a
  failed fetch) releases the hold at once, with a warning, where the reference
  would wait for ever.
- Book: `content/world.md`, next to `STAND_UP` and `SIT_ON_GROUND`.

**Live (aditi):** four jumps, each logging the `pre_jump` release; flying and
jumping worked. The `standup` release was never exercised live, because no grid
available here plays `standup` and also waits for the bit.
