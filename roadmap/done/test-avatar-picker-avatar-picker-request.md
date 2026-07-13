---
id: test-avatar-picker
title: avatar picker request
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 16 — Directory & search `[both]`
---

Context: [context/test.md](../context/test.md).

`avatar-picker` — avatar picker request. `1av`.

The case runs a name-autocomplete lookup: it sends an `AvatarPickerRequest`
for the agent's *own* first name (taken from the login credentials at runtime,
so no avatar name is baked into the source or the record) and awaits the
`AvatarPickerReply` correlated by the minted `QueryID`. It records the raw and
real (non-nil-keyed) match counts, the named count, and whether the agent's own
id appeared — never the names.

The grids diverge and the case is grid-aware. On **OpenSim**
(`UserManagementModule.HandleAvatarPickerRequest`, searching the user-account
service, which includes the requester) the reply carries one real match and it
is the querying agent — asserted, complete. On **Aditi** the beta grid's people
directory is sparse: a self-name query returns no real match, only SL's
nil-keyed empty-named "no results" sentinel row (`result_count = 1`,
`real_count = 0`), so the run is marked partial rather than failed. Passes green
on both grids: complete on OpenSim, partial on Aditi.
