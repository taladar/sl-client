---
id: protocol-21
title: Animations
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**21. Animations — `AgentAnimation` (send/trigger), `AvatarAnimation` (receive)
· 5 pts. ✅ Done.** Play/stop built-in and custom animations and observe others'
— a dance/gesture bot, or motion in a renderer. **Send:**
`Session::set_animations(&[(anim_id, start)])` is the batch surface
(`AgentAnimation`: each pair starts/stops one animation; the message always
carries the single empty `PhysicalAvatarEventList` block the reference viewer
appends), with `play_animation`/`stop_animation` single-animation convenience
wrappers. `anim_id` is a built-in animation UUID or an uploaded animation asset
(custom anims are fetched via #19). **Receive:** incoming `AvatarAnimation` is
surfaced as `Event::AvatarAnimation { avatar_id, animations }` carrying a new
`PlayingAnimation` value type (`anim_id`, the simulator's per-avatar
`sequence_id`, and the optional triggering `source_id` from the
positionally-correlated `AnimationSourceList`, matching the viewer's
`process_avatar_animation`). The list is the *complete* current set, not a delta
— a stopped animation simply drops out of a later update — so consumers treat
each event as authoritative state. Wired as
`Command`/`SlCommand::{SetAnimations, PlayAnimation, StopAnimation}` through
both runtimes. Covered by three `lifecycle.rs` tests (the `AgentAnimation` send
encoding for batch start/stop and the single-animation wrapper, plus the
`AvatarAnimation` decode with source correlation and nil-vs-missing source
slots). *Live-verified against the local OpenSim via the
`tokio_login_hold_logout` tokio example: `PlayAnimation(ANIM_AGENT_CLAP)`
round-tripped — the simulator echoed an `Event::AvatarAnimation` for the agent
listing the default stand plus the triggered clap animation. Test: local
OpenSim.*
