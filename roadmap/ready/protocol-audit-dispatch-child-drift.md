---
id: protocol-audit-dispatch-child-drift
title: dispatch_child is a hand-copied subset of dispatch
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 5
refs: [protocol-audit-region-handshake-mid-session]
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/methods.rs:1926` — `dispatch_child` mirrors the root
dispatcher by hand, and its own comment admits it (`:2038`, "Mirror the
root-circuit handlers so it animates"). Verified byte-identical pairs:
`AvatarAnimation` `2040` = `3871`, `ObjectAnimation` `2051` = `3885`,
`AvatarAppearance` `2070` = `3828`, `ParcelOverlay` `2022` = `2863`, plus
`SoundTrigger` / `AttachedSound` / `AttachedSoundGainChange` / `PreloadSound`
(`2081-2123` = `4083-4123`, differing only by a comment).

Every fix has to be made twice, and the `RegionHandshake` pair has **already
diverged** — see [[protocol-audit-region-handshake-mid-session]].

Scope: factor the shared handlers into functions parameterised by which circuit
raised the message, so the root and child arms call one implementation. The
mirror is the point; hand-copying is what makes it fragile.

Related, and probably the same change: `dispatch` itself is 2155 lines with 141
`AnyMessage::` arms (`methods.rs:2786`) and its server mirror
(`sim_session.rs:7534`) is 1494 lines with 132. `sim_caps.rs:622 handler_for`
already demonstrates the fn-pointer-table pattern that applies. Note the
workspace enables neither `clippy::too_many_lines` nor `cognitive_complexity`,
which is why these survive an otherwise very strict lint set.
