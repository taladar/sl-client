---
id: test-parcel-properties
title: request parcel properties (note the CAPS EventQueue path on SL vs UDP)
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 10 — Parcel & land `[both]`
---

Context: [context/test.md](../context/test.md).

Edits need the estate-owner avatar.

`parcel-properties` — request parcel properties (note the CAPS
EventQueue path on SL vs UDP). `1av`. A single request/reply: send a UDP
`ParcelPropertiesRequest` ([`Command::RequestParcelProperties`]) for a 4×4 m
square at the region centre (128, 128) with a distinctive sequence id, then
await the [`Event::ParcelProperties`] whose *echoed* sequence id matches — so
the reply is our query's answer, not an unsolicited on-entry one. The reply
does **not** come back over UDP on a modern region: OpenSim (whenever the
region has an event queue, its default) and Second Life both enqueue
`ParcelProperties` on the **CAPS EventQueue**, decoded by the runtime's
event-queue task via `parcel_info_from_llsd` into the event — the UDP
request is only the trigger, the UDP `ParcelProperties` message is
deprecated. So this also exercises the CAPS decode path, not a plain UDP
round-trip. The query rectangle is region-relative and independent of the
avatar's exact position, so no `start_location` override is needed. Asserts
the reply carries real data (`request_result` ≠ `NoData`) with a positive
area. No new client code — the
`RequestParcelProperties`/`ParcelProperties`/`ParcelInfo` surface (and its
CAPS decode) all already existed from the sl-survey parcel work; only the new
case. Green on OpenSim against the Default Region's single region-wide parcel
("Your Parcel", `local_id` 1, area 65536, max_prims / sim_wide_max_prims
15000), RTT ≈ 48 ms over the event queue. `[both]`; the aditi run is deferred
with the batch (no aditi record this session).
