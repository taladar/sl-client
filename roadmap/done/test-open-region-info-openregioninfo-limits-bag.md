---
id: test-open-region-info
title: OpenRegionInfo limits bag
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

`open-region-info` — OpenRegionInfo limits bag. `[opensim] 1av`.
**Partial on OpenSim (module not loaded).** `OpenRegionInfo` is an
OpenSim-specific CAPS event-queue push (Firestorm
`llpanelopenregionsettings.cpp`, `/message/OpenRegionInfo`): a bag of
per-region overrides beyond the standard SL protocol — prim/link/scale
limits, build bounds, the say/shout/whisper chat ranges, a UTC offset. It is
**unsolicited**, so the case waits for [`Event::OpenRegionInfo`] after region
arrival rather than issuing a command, and (when present) asserts the bag
advertises at least one limit, recording the advertised-limit count plus the
link/group/prim-scale and chat-range values. Every field is optional (the sim
sends only the keys it overrides), so an empty push decodes to all-`None`.
The push only appears when the optional `OpenRegionSettings` region module is
loaded; the local standalone OpenSim does not ship it (absent from the source
tree and the `bin/` module set), and Second Life never sends the event at all,
so no live grid available here emits it. The case therefore marks the run
**partial** with a note when the window elapses with no push (mirroring
`library-tree-fetch` and the other optional-config cases); the decode path
itself is covered by `sl-proto`'s `open_region_info_from_llsd` unit tests.
**No new client code** — the CAPS event, the `OpenRegionInfo` type, and the
parser already existed; only the runtime crates gained a re-export of
`OpenRegionInfo` (added to both `sl-client-tokio` and `sl-client-bevy` for
parity).
