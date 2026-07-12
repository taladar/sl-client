---
id: test-animation-play-stop
title: play and stop an animation
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 14 — Appearance, attachments & animations `[both]`
---

Context: [context/test.md](../context/test.md).

`animation-play-stop` — play and stop an animation. `1av`. A viewer
starts/stops an avatar's animations with `AgentAnimation` (a list of
`(anim_id, start)` pairs); the simulator folds each into that avatar's set
and broadcasts the *complete* authoritative set back to every viewer in view
— the agent's own included — as an `AvatarAnimation`, so a stopped animation
simply drops out of a later update rather than being signalled individually.
The case plays a built-in gesture ([`Command::PlayAnimation`],
`ANIM_AGENT_CLAP` — a gesture rather than a locomotion/posture default like
`STAND`, so it is not in the baseline set) and waits for the
[`Event::AvatarAnimation`] describing *this* agent (matched by the event's
avatar id, filtering out any nearby avatars also animating) whose set now
lists the played id; then stops it ([`Command::StopAnimation`]) and waits for
the next such update whose set no longer lists it (the drop-out). Because
[`Session::wait_for`] consumes events in order, the channel position is
already past the play confirmation, so an "absent" set is genuinely
post-stop, not a stale pre-play baseline. The play is proved by finding the
id in the authoritative set *and* a positive per-avatar `sequence_id` (the
simulator bumps it on each (re)start); RTTs for both steps are recorded.
**Complete on BOTH grids, no divergence** (OpenSim: `animations_while_playing
= 2` = baseline `STAND` + `CLAP`, `animations_after_stop = 1`, play RTT ~34
ms / stop ~15 ms; aditi: identical `2 → 1`, play RTT ~1.3 s / stop ~0.2 s).
Self-contained on either grid — no inventory, permission or peer avatar; the
agent drives its own set and observes its own broadcast. On OpenSim the
avatar is forced into the "Default Region" so it is a *root* presence —
`ScenePresenceAnimator` refuses to add or broadcast animations for a child
agent, and `AddAnimation` otherwise accepts an arbitrary animation UUID.
**Case-only:** the command/event and both runtimes' handling all pre-existed
(no re-export or library gap this time). `[both]`.
