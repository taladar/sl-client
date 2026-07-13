---
id: viewer-p31-9
title: Typing animation & sound
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.9. Typing animation & sound.** When an avatar types in nearby
chat the reference viewer plays `ANIM_AGENT_TYPE` (the hands-on-keyboard
gesture) plus the typing UI sound, and advertises the state so others see
it. For **other** avatars this should already flow through the P31.6-fixed
path — the simulator plays `ANIM_AGENT_TYPE` (a downloadable keyframe, now
correctly classified) and broadcasts it over `AvatarAnimation`, which Phase
18 plays — so the first task is to **verify** a typing neighbour animates
(a second `sl-repl` login sending `StartTyping`). For the **own** avatar,
drive it from local chat-entry state: on start-typing play the animation
locally *and* send `ChatFromViewer` `StartTyping` (the `ChatTyping` P11.1
already ingests for others), clear it on stop-typing; optionally the typing
sound. A sibling of P31.6 — an activity-driven state animation, not a
procedural adjuster. Reference: `LLAgent::startTyping` / `stopTyping`,
`ANIM_AGENT_TYPE`, `gAgent.sendAnimationRequest`.

**Done.** New `typing.rs`: `TypingState` + `drive_own_typing`. On the typing
edge it sends **both** wire signals the reference viewer does — an
`AgentAnimation` request (`Command::PlayAnimation` / `StopAnimation` of
`ANIM_AGENT_TYPE`, built-in `"type"`) *and* the `ChatFromViewer` "is typing"
indicator (`Command::Typing`) — and plays `ANIM_AGENT_TYPE` locally for
immediate own-avatar feedback. Typing is an **overlay**, so the local play uses
a dedicated `client_typing` slot on `AnimationPlayback` (parallel to the P31.6
`client_locomotion` slot), priority-blended against stand/walk rather than
replacing it.

Protocol correction: the plan assumed "the simulator plays `ANIM_AGENT_TYPE`
and broadcasts it" from `StartTyping`. It does **not** — OpenSim's `ChatModule`
only *relays* `StartTyping` / `StopTyping`. What makes *other* viewers animate
is the typist's own `AgentAnimation` request, which the sim rebroadcasts as an
`AvatarAnimation` the Phase 18 path plays. So the `ChatFromViewer` signal is the
chat "is typing" indicator only; the `AgentAnimation` request is the animation
trigger — hence sending both. Receiving a neighbour's typing needs no new code
(Phase 18 already plays the broadcast).

Deviations from the plan:

- **No chat-entry box exists yet** (a separate ui-framework task), so the `T`
  key toggles the typing state as a stand-in; `TypingState::set()` is the hook
  a future chat input drives instead, unchanged driver.
- **Typing sound deferred** — the viewer has no sound-effect playback (its own
  roadmap task), so only the animation + wire signals are implemented.
- **Not yet run live** — needs a live run (own typing on `T`; a second login
  typing to confirm the neighbour animates). `SL_VIEWER_LOG_TYPING=1` logs the
  own edge.
