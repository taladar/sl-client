---
id: test-estate-access
title: update estate access list
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

`estate-access` — update estate access list. `1av` (estate owner).
**Green on OpenSim.** Runs as the **estate-owner** avatar
(`--avatar estate-owner`): editing an estate list needs estate-owner/god
rights, which OpenSim rechecks (`IsEstateManager`/`CanIssueEstateCommand`) on
every `estateaccessdelta`. A read-modify-verify-restore cycle over *two* lists
that leaves the estate as it found it: read the current lists (`getinfo`,
[`Command::RequestEstateInfo`]) and record the allowed-agents/banned-agents
membership; then add a known **other** avatar to the allowed-agents list
([`Command::UpdateEstateAccess`] with [`EstateAccessDelta::AllowedAgentAdd`]),
assert it lands in the `setaccess` [`Event::EstateAccessList`] reply, remove
it and assert the list is back to its start size; repeat the add/remove
round-trip against the banned-agents list. The target is never the estate
owner (OpenSim short-circuits `_user == EstateOwner`, so the case asserts they
differ up front); it need not be online (the lists are pure id sets) and the
ban round-trip has no eject side effect because the target is not in the
region. Two wire subtleties shaped the drain: OpenSim **defers** the
`setaccess` replies, flushing only once its delta queue drains (~500 ms
batch), and an allowed/banned change replies with *both* the allowed list and
the ban list together — so after each delta the case drains every
[`Event::EstateAccessList`] to a quiet gap and takes the **latest** membership
per [`EstateAccessKind`], rather than matching the first event of a kind
(which could be a stale reply from the previous step). Records the
read/allowed/banned latencies, estate id+name, the target id, and the initial
vs after-add counts (`0`→`1` for each list on the local grid). **No new client
code** — the `Command`/`Event`/`Session` surface (`update_estate_access`,
`EstateAccessDelta`, `EstateAccessKind`, `estate_access_from_params`) already
existed; the case reuses `fixtures::opensim_secondary_avatar` (`Friend
Tester`) as the target, mirroring `parcel-access-list`. `[both]`; the aditi
run is deferred with the batch (SL enforces the same estate-owner gating and
`estateaccessdelta` flow).
