---
id: test-parcel-divide-join
title: divide then join parcels
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 10 — Parcel & land `[both]`
---

Context: [context/test.md](../context/test.md).

`parcel-divide-join` — divide then join parcels. `1av`. Runs as the
estate-owner avatar (`--avatar estate-owner`), who owns the region-wide parcel
on the local grid, since `ParcelDivide`/`ParcelJoin` need land-divide/join
rights. A divide-verify-join-verify cycle that leaves the region with exactly
the single parcel it started with: `ParcelDivide`
([`Command::DivideParcel`]) chops a metre `west/south/east/north` rectangle —
a strict subsection of one parcel — out into a brand-new parcel;
`ParcelJoin` ([`Command::JoinParcels`]) merges every owned parcel within a
rectangle back into the largest (survivor). Neither has a direct reply, so
the case reads the reshaped layout back with `ParcelPropertiesRequest`
queries (as in `parcel-properties`, each with a distinct echoed sequence id).
Flow: (0) defensively join the whole region to a single-parcel baseline —
a no-op if already single, and it heals any parcels a prior interrupted run
left behind; (1) learn the region-centre parcel's local id, owner (confirm we
own it), and area `A0`; (2) divide out the SW 64×64 m corner, then assert a
point inside the corner now resolves to a **new** parcel id whose area is the
corner's (4096 m²), the region centre still resolves to the **original** id
with a reduced area, and the two areas sum back to `A0`; (3) join the whole
region, then assert the region centre is the original id with `A0` restored
and the corner now resolves to that same id (a single parcel again). No new
client code — the `ParcelDivide`/`ParcelJoin` command surface all existed;
only the new case. **Green on OpenSim's Default Region:** single region-wide
parcel (`local_id` 1, area 65536, owned by the estate owner), corner divides
out as `local_id` 4 area 4096 leaving 61440, join restores the full 65536
under `local_id` 1. A fixed ~2 s settle after each edit is needed: the edit
has no reply and the readback otherwise races the simulator applying it (a
no-settle readback saw the pre-divide layout). `[both]`; the aditi run is
deferred with the batch — but note it likely needs a **full owned region**
we do not have on aditi (the fixed SW-corner chop assumes we own the region
origin), so the aditi leg may be infeasible without a suitable owned parcel
and dynamic coordinates, unlike the other Phase 10 land cases.
