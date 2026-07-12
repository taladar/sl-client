---
id: test-simulator-features
title: request simulator features
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

`simulator-features` — request simulator features. `1av`.
**Green on OpenSim.** The runtime already fetches the `SimulatorFeatures`
capability automatically on region arrival (surfacing
[`Event::SimulatorFeatures`]); the case additionally drives it on demand with
`Command::RequestSimulatorFeatures` and asserts a decodable reply arrives
carrying at least one advertised feature, plus (OpenSim only) the
`OpenSimExtras` subtree that Second Life omits. Records the reply latency, the
count of advertised top-level fields (12 on this grid), and the mesh-upload /
physics-materials flags and the max-attachment/texture limits. **Surfaced and
fixed a client parser bug:** OpenSim encodes `ExportSupported` inside
`OpenSimExtras` as an LLSD **string** `"true"` (its
`SimulatorFeaturesModule.GetGridExtraFeatures` stores every grid-wide extra as
a string, and the `GridService` default for the key is the literal `"true"`),
whereas the Second Life-style path sends a boolean — the strict `field_bool`
decode rejected the whole reply (`field ExportSupported carried malformed
value "string"`). `map_export_supported` now accepts either encoding
(boolean/integer or a case-insensitive `"true"`/`"false"` string, matching
OpenSim's own `bool.TryParse`), with unit tests for the string, `false`, and
garbage cases. No other new client code — the whole command/event/CAPS surface
already existed. `[both]`; the aditi run is deferred with the batch (SL sends
a boolean, so the parser fix is not needed there, but the case exercises the
same flow).
