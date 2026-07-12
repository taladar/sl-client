---
id: protocol-15
title: Bandwidth throttle (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**15. Bandwidth throttle (done) ✅ — `AgentThrottle` · 2 pts.** Tell the sim how
to allocate the seven throttle categories
(resend/land/wind/cloud/task/texture/asset); without it the sim's conservative
defaults starve the object/terrain/texture firehose the rest of this tier needs.
Implemented: a `Throttle` value type holding the seven per-category rates in
kilobits per second (with `preset_300`/`preset_500`/`preset_1000` presets
mirroring the reference viewer's bandwidth tables, a `total`, and the wire
`bits_per_second` conversion), and `Session::set_throttle`, which packs the
rates as seven little-endian `f32` bits-per-second values into the
`AgentThrottle` `Throttles` byte array (`GenCounter` 0, as the viewer does) and
sends it reliably on the root circuit. The throttle is **remembered and re-sent
automatically on every region change** (each new root region starts with the
sim's defaults until re-told) — the re-send is funnelled through
`complete_arrival`, the single point where a new root region becomes active
(login *and* handover). Wired through both runtimes
(`Command::SetThrottle(Throttle)`). *Live-checked against the local OpenSim: the
example advertises `Throttle::preset_1000` at handshake and the session runs a
full clean lifecycle (login → throttle sent reliably → neighbours enabled →
clean logout) with no protocol error; the exact 28-byte wire payload, the
agent/session/circuit fields, and the re-send-on-region-change are covered by
unit tests. (`AgentThrottle` has no reply, and OpenSim's `debug lludp throttles`
console commands aren't dispatchable over the REST console in this build, so the
applied rate can't be read back live.)*
