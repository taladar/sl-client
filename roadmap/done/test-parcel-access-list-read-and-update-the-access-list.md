---
id: test-parcel-access-list
title: read and update the access list
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 10 — Parcel & land `[both]`
---

Context: [context/test.md](../context/test.md).

`parcel-access-list` — read and update the access list. `1av`. A
read-modify-verify-restore cycle on the region-centre parcel's **allow**
(AL_ACCESS) list, run as the estate-owner avatar (`--avatar estate-owner`),
who owns the parcel — the first case to use the estate-owner credentials
label. Learns the parcel's region-local id from a `ParcelPropertiesRequest`
reply (and asserts the owner is the logged-in avatar), reads both the allow
and ban lists ([`Command::RequestParcelAccessList`] →
[`Event::ParcelAccessList`] per [`ParcelAccessScope`]), adds a known other
avatar to the allow list ([`Command::UpdateParcelAccessList`]), re-reads to
assert it landed, then restores the list to its original entries and re-reads
to assert the entry is gone. Surfaced and fixed **two client issues** the
round-trip needs: (1) `ParcelAccessListUpdate` hard-coded a nil transaction
id, so the reference simulator (OpenSim `LandObject.UpdateAccessList`) only
clears-before-adds on the *first* update per list and *appends* thereafter —
the runtime now mints a fresh transaction id per update
(`Session::update_parcel_access_list` gained a `transaction_id` param, wired
through both `sl-client-tokio` and `sl-client-bevy`); and (2) an empty list
comes back as a single nil-agent placeholder block, which the decode now
drops (as the reference viewer's `LLParcel::unpackAccessEntries` does), so an
empty list surfaces as zero entries. Green on OpenSim's Default Region
("Your Parcel", `local_id` 1) owned by the estate owner: empty allow/ban
lists initially, the add leaves one entry, the restore clears back to empty,
read/update RTT ≈ 4–90 ms / 15 ms. `[both]`; the aditi run is deferred with
the batch (needs `other_avatar` in `fixtures.aditi.toml`).
