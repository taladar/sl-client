---
id: idiomatic-p7-10
title: Direction** tuples (ScriptTeleportRequest.look_at, ParcelInfo.user_loo
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

**Direction** tuples (`ScriptTeleportRequest.look_at`,
    `ParcelInfo.user_look_at`) → NEW public client-local `Direction` newtype
    (`sl-wire/src/geometry.rs` — see the second-pass note): a full 3-D `f32`
    facing vector (verified vs
    the viewer — `LLAgent::resetAxes` uses the look-at as the agent *at*-axis
    including its vertical component, so it is a 3-D direction, not a
    horizontal-only or position vector; not forcibly normalised so the raw
    components round-trip byte-identically). `new`/`x()`/`y()`/`z()`/`ZERO`
    + `length()`/`normalized() -> Option<Direction>`. Re-exported through
    `sl-proto`/`sl-client-tokio`/`sl-client-bevy`; REPL `global_or_zero`
    unaffected. (Name chosen by the user 2026-06-24.)
