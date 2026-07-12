---
id: test-estate-info
title: request estate info / covenant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

`estate-info` — request estate info / covenant. `1av` (estate owner).
**Green on OpenSim.** Runs as the **estate-owner** avatar
(`--avatar estate-owner`): OpenSim gates `EstateOwnerMessage`/`getinfo` behind
`CanIssueEstateCommand`, so a non-manager gets *no* reply — a reply at all
proves the rights. The case drives two round-trips over the estate channel:
[`Command::RequestEstateInfo`] (`getinfo`) → an `estateupdateinfo`
[`Event::EstateInfo`] (name/owner/id/flags/sun/parent/covenant-id+timestamp
/abuse email) trailed by one `setaccess` [`Event::EstateAccessList`] per list
(managers, allowed agents, allowed groups, bans — OpenSim emits one *even when
empty*, via `SendEstateList`'s `do…while`); and
[`Command::RequestEstateCovenant`] (`EstateCovenantRequest`) → an
`EstateCovenantReply` [`Event::EstateCovenant`] (covenant notecard id
+timestamp, estate name, owner). Asserts a non-empty estate name and that
**both** replies agree the estate owner is the logged-in avatar; the trailing
access lists are drained to a quiet gap and their count / total membership
recorded (contents are the next case's job). Records both reply latencies plus
the estate id (`101`), flags, parent estate, covenant presence, and the
access-list count (`4`, all empty on the local grid). **No new client code** —
the `Command`/`Event`/session surface (`request_estate_info`,
`request_estate_covenant`, `estate_info_from_params`,
`estate_access_from_params`) already existed; only the runtime crates gained a
re-export of `EstateCovenant` (added to both `sl-client-tokio` and
`sl-client-bevy` for parity). `[both]`; the aditi run is deferred with the
batch (SL answers the same `getinfo`/covenant round-trips to an estate
manager/owner).
