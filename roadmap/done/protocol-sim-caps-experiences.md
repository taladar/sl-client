---
id: protocol-sim-caps-experiences
title: Server-side experience caps
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 3
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The experience cap cluster, server side, over a small experience fixture
set: `GetExperienceInfo`, `FindExperienceByName`, `GetExperiences`,
`AgentExperiences`, the admin/creator/group experience lists,
`ExperiencePreferences`, `IsExperienceAdmin` / `IsExperienceContributor`,
`UpdateExperience`, `RegionExperiences`.

Inverse-pairing with the client-direction support in
`sl-wire/src/experience*`; verified against it in-memory.

## Done (2026-08-20)

All twelve `REQUESTED_CAPABILITIES` rows flipped Pending → Served in the
pinned coverage table (42 → 54 granted caps). **No new codecs**: the
whole server-direction surface (eight request parsers, five reply
builders in `sl-wire/src/experience/server.rs`) already existed
inverse-paired from protocol-62; this task wired it into dispatch over a
new fixture.

Serving store: `SimExperiences` (`sl-proto/src/sim_experiences.rs`, the
`SimInventoryTree` stance — deterministic BTree collections, driver
population API, mutations observable by follow-up reads), held as
`SimSession::experiences[_mut]`. State: metadata records by public id,
agent allowed/blocked preference sets, owned/admin/creator sets,
per-group id lists, region allowed/blocked/trusted triple. Semantics:
search is case-insensitive substring over public records (invalid +
`PROPERTY_PRIVATE` hidden), 1-based `SEARCH_PAGE_SIZE` paging;
`IsExperienceAdmin` ⇔ admin-set membership, `IsExperienceContributor` ⇔
creator-set membership (Firestorm files `GetCreatorExperiences` under
its Contributor tab); `UpdateExperience` applies the editable fields
only (owner/quota/expiration server-controlled, as the reference strips
them) and 404s on an unknown id; `ExperiencePreferences` accepts any id
(agent-scoped entry, documented no-404 exception) and echoes the full
lists both verbs; `RegionExperiences` POST replaces wholesale and
echoes.

Dispatch: eight new `CapHandler` variants (`ExperienceInfo`,
`ExperienceSearch`, `ExperiencePermissions`, `ExperiencePreferences`,
the name-routed `ExperienceIdList` + `ExperienceStatus`,
`UpdateExperience`, `RegionExperiences`); the existing `ais_suffix`
helper feeds the sl-wire suffix parsers (the `/id/` sub-path and the
bare-query forms both round-trip). Mutations surface three new
`ServerEvent`s: `ExperiencePermissionSet`, `ExperienceUpdated`,
`RegionExperiencesSet`.

Verified by twelve new loopback tests driving the real client
builders/folds against `SimCaps::dispatch`
(`sl-proto/tests/sim_caps.rs`; the three caps whose replies don't echo
the queried id — `GroupExperiences`, `IsExperienceAdmin`,
`IsExperienceContributor` — parse out-of-band exactly as the runtimes
do) plus six `SimExperiences` unit tests; book coverage in the new "The
experience handlers" section of `book/src/comms/caps.md`.
