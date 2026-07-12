---
id: protocol-3
title: Agent movement & control
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**3. Agent movement & control · 5 pts. ✅ Done.** Promoted the stubbed
`AgentUpdate` into a real control surface. Implemented: a `ControlFlags`
bitfield (walk/run/fly/turn/jump/up/down/…); `Session::set_controls` and
`Session::set_rotation` (persisted and re-sent on every keep-alive, so the sim
keeps moving the agent); one-shot `Session::stand` and `Session::sit_on_ground`;
`Session::sit_on` (the `AgentRequestSit` → `AvatarSitResponse` → `AgentSit`
handshake, surfaced as `Event::SitResult`); and `Session::autopilot_to`
(server-side walk-to-coordinates via a `GenericMessage` `autopilot`, so a bot
can navigate without any scene knowledge). Wired as
`Command::{SetControls, SetRotation, Stand, SitOnGround, Sit, Autopilot}`
through both runtimes; verified live (the avatar walked +14.5 m forward under
`AT_POS`). Camera stays at region centre — true camera control waits on position
tracking from the object/scene graph (#16). *Test: local OpenSim — needs a real
physics engine (ubODE/BulletSim); the default BasicPhysics does not move
avatars.*
