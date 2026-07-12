---
id: test-parcel-info-dwell
title: parcel info and dwell
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 10 — Parcel & land `[both]`
---

Context: [context/test.md](../context/test.md).

`parcel-info-dwell` — parcel info and dwell. `1av`. Exercises the two
distinct "tell me about this parcel" request/reply pairs against the
region-centre parcel. First learn the parcel's *region-local* id from a
`ParcelPropertiesRequest` reply (as in `parcel-properties`), then: (1) request
its **dwell** with a UDP `ParcelDwellRequest`
([`Command::RequestParcelDwell`]) keyed on a [`ScopedParcelId`] — the
region-local id paired with the **root circuit id** — and await the matching
[`Event::ParcelDwell`]; and (2) fetch the condensed **info listing** by
resolving the region-centre location to a *grid-wide* parcel id through the
`RemoteParcelRequest` **capability** ([`Command::RequestRemoteParcelId`] →
[`Event::RemoteParcelId`]), then feeding that id to a UDP `ParcelInfoRequest`
([`Command::RequestParcelInfo`]) and awaiting the [`Event::ParcelDetails`]
whose echoed id matches. Asserts the dwell reply echoes the requested
region-local id, the resolved grid-wide id is non-nil, and the info listing
carries a region name. New harness plumbing: the runtime's `Client` now
exposes `root_circuit_id()` (mirroring the existing `region_handle()`
accessor) and the conformance `Session` seeds/exposes `circuit_id()` from it,
so a case can build the [`ScopedParcelId`] the scoped parcel commands
take — infrastructure the later Phase 10 scoped-parcel cases
(`parcel-access-list`, `parcel-object-owners`, `parcel-divide-join`) reuse.
Also re-exported `ParcelDetails`/`ParcelKey` from `sl-client-tokio` (both
appear in public `Event` variants but were missing from the re-export).
Green on OpenSim's Default Region: the region-wide parcel ("Your Parcel",
`local_id` 1, area 65536) answers all three requests, dwell tracked by the
default `DefaultDwellModule`, `RemoteParcelRequest` cap and the two UDP
replies all RTT ≈ 0.5–1.1 s. `dwell_parcel_id` == `parcel_id` on OpenSim
(both the FakeID), but the case does not assert that cross-grid. `[both]`;
the aditi run is deferred with the batch.
