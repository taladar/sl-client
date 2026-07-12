---
id: test-picks-classifieds
title: request and edit picks / classifieds
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 7 — Avatar profile & social `[both]`
---

Context: [context/test.md](../context/test.md).

`picks-classifieds` — request and edit picks / classifieds. `1av`.
The profile "Picks" and "Classifieds" tabs: a create → read → edit → delete
round-trip over the two per-account lists the profile service keeps, driven on
the agent's own profile. Picks use `Command::UpdatePick`/`DeletePick` and are
verified through the replies the simulator *volunteers* after each edit — a
`PickInfoUpdate` draws back both an `AvatarPicksReply` (the whole list) and a
`PickInfoReply` (the full record), a `PickDelete` a fresh list — so the case
asserts the created pick's volunteered detail, sweeps any marker pick a prior
interrupted run left behind (from that same list), confirms the edited
description on the next volunteered detail, then deletes and confirms it left
the list. Classifieds get no volunteered reply, so they are read back with the
typed `Command::RequestClassifiedInfo` (`ClassifiedInfoRequest`), using a
fixed id (re-runs edit one record, not piling up) and toggling the description
so each edit is a detectable change. **Two live OpenSim findings shaped this,
both worked around rather than fixed:** the `avatarpicksrequest` /
`avatarclassifiedsrequest` list *queries* (both `GenericMessage`s, correctly
encoded and session-matched on the wire) go unanswered by stock OpenSim for
the agent's own profile — hence the volunteered-reply and typed-detail paths
above, never a bare list query; and OpenSim's `classified_delete` throws a
data-layer SQLite error and leaves the record, so classified deletion is
best-effort (recorded, not asserted; the fixed id keeps the leftover to
one). A
classified listing costs L$ on Second Life (this case lists at L$0, which
OpenSim accepts and SL declines), so when the created classified never reads
back that half is recorded `partial`, not failed. Needs the OpenSim
UserProfiles module enabled (appendix). Green on OpenSim: pick create RTT
≈ 20 ms loopback, pick listed / edited / deleted, classified create RTT
≈ 15 ms, classified edited (delete records `false` — the OpenSim bug).
`[both]`; the aditi run is deferred with the batch (no aditi record this
session). Added
a `ClassifiedKey` re-export to both runtime crates (sibling of the existing
`PickKey`).
