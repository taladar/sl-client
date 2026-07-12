---
id: viewer-p31-9
title: Typing animation & sound
topic: viewer
status: ready
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
