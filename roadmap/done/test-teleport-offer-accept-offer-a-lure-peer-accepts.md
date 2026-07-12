---
id: test-teleport-offer-accept
title: offer a lure, peer accepts
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 12 — Teleport (state machine) `[both]`
---

Context: [context/test.md](../context/test.md).

`teleport-offer-accept` — offer a lure, peer accepts. `2av`. **Green on
OpenSim.** The *invited* teleport, complementing the three self-initiated
Phase 12 cases: one avatar offers a lure and another accepts it, driving the
same teleport handover but provoked by the offer rather than a
`TeleportLocationRequest` the accepter chose. A lure offer is a `StartLure`
([`Command::OfferTeleport`]) the grid delivers to the target as an
`ImprovedInstantMessage` with the [`ImDialog::LureUser`] dialog; the offer
IM's `id` is the lure id — on OpenSim a *fake parcel id* encoding the
offerer's region handle + position (`LureModule.OnStartLure` builds it from
the offerer's `AbsolutePosition`) — which the target quotes back in a
`TeleportLureRequest` ([`Command::AcceptTeleportLure`]) to accept. OpenSim's
`OnTeleportLureRequest` parses that fake parcel id back into a handle +
position and calls `RequestTeleportLocation`, so the accepter teleports to the
*offerer's* location. Sequence: the primary offers with a distinct per-run
message; the secondary — a separate session — observes the matching
[`Event::InstantMessageReceived`] (`LureUser`, attributed to the primary,
carrying the exact message verbatim), takes the lure id, accepts, collects the
teleport phases, and the case asserts the sequence opens with *Starting* and
ends at a terminal arrival — tolerating both `TeleportLocal` (avatars sharing
a region) and a `RegionChanged` handover (different regions) — then confirms
the accepter's current region handle is now the primary's, the point the lure
id encodes. OpenSim sends the offerer nothing back on accept (no
`IM_LURE_ACCEPTED`), so the acceptance is observable only on the accepter side
as the completed teleport. **No new client code** — the
`Command::OfferTeleport` / `Command::AcceptTeleportLure` surface and the
lure-accept teleport handover (`parse_lure_region_handle` + the CAPS
`TeleportFinish` handover) already existed from earlier IM and teleport work.
Recorded green with the two avatars in *different* regions of the 2×2 block,
so the accepted lure crossed a boundary: `phase_sequence =
"started,region-changed"`, `arrival = "region-changed"`, `progress_updates =
0`; offer deliver RTT ≈ 6.7 ms, teleport ≈ 0.30 s loopback. `[opensim]` only;
the Aditi variant is deferred to Phase Z pending a second Aditi avatar (SL
answers a lure offer the same way, its own `TeleportFinish` handover to the
offerer's simulator).
