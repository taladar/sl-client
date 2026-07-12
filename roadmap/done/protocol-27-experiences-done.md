---
id: protocol-27
title: Experiences (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**27. Experiences (done) ✅ — the CAPS experience APIs · 5 pts.** Permission
grants and experience-keyed scripts — an experience-permission client. The UDP
half was already in place: a script's `llRequestExperiencePermissions` arrives
in the `ScriptQuestion` `Experience` block, surfaced by #8 as
`ScriptPermissionRequest.experience_id` with the `ScriptPermissions::EXPERIENCE`
bit, and granted via the existing `answer_script_permissions`. This item adds
the full **CAPS** surface in a new `sl-wire/src/experience.rs`, faithfully
ported from the viewer's `llexperiencecache.{h,cpp}` /
`llfloaterexperiences.cpp`:

- **Read** — `RequestExperienceInfo` (`GetExperienceInfo`, batching every id as
  a `…/id/?page_size=N&public_id=<id>&…` GET → `Event::ExperienceInfo`, with
  unresolved `error_ids` folded in as `missing` placeholders), `FindExperiences`
  (`FindExperienceByName`, a paged `?query=` GET →
  `Event::ExperienceSearchResults`), `RequestExperiencePermissions`
  (`GetExperiences` → `Event::ExperiencePermissions` `{ allowed, blocked }`),
  `RequestOwnedExperiences` / `RequestAdminExperiences` /
  `RequestCreatorExperiences` (`AgentExperiences` / `GetAdminExperiences` /
  `GetCreatorExperiences` → `Event::{Owned,Admin,Creator}Experiences`),
  `RequestGroupExperiences` (`GroupExperiences`, `?<group_id>` →
  `Event::GroupExperiences`, the runtime echoing the queried group),
  `RequestExperienceAdmin` / `RequestExperienceContributor` (`IsExperienceAdmin`
  / `IsExperienceContributor`, `?experience_id=` →
  `Event::Experience{Admin,Contributor}Status`, the runtime echoing the queried
  experience), and `RequestRegionExperiences` (`RegionExperiences` GET →
  `Event::RegionExperiences` `{ allowed, blocked, trusted }`).
- **Write** — `SetExperiencePermission` (`ExperiencePreferences`: an
  `Allow`/`Block` PUT of `{ "<id>": { permission } }`, or a `Forget` DELETE of
  `?<id>`; the reply echoes the updated `{ experiences, blocked }`),
  `UpdateExperience` (`UpdateExperience` POST of the editable metadata →
  `Event::ExperienceUpdated`), and `SetRegionExperiences` (`RegionExperiences`
  POST of the three id lists, estate-gated).

New value types `ExperienceInfo` (public/agent/group ids, name, description,
`ExperienceProperties` bitfield, quota, expiration, maturity, slurl, extended
metadata, `missing`), `ExperienceProperties` (the `PROPERTY_*` bits —
`INVALID`/`PRIVILEGED`/`GRID`/`PRIVATE`/`DISABLED`/`SUSPENDED`, with `is_grid`/…
helpers), `ExperiencePermission` (`Allow`/`Block`/`Forget`), and
`ExperienceUpdate`; sl-wire builders/parsers for each cap body and reply. Twelve
caps join the seed; the self-describing replies route through
`Session::handle_caps_event` (decoded once in `sl-proto`), while the three
context-needing GETs (group/admin/contributor) build their event in the runtimes
so they can echo the queried id. All wired as `Command`/`SlCommand` variants
through both runtimes (the cap GET/PUT/DELETE/POSTs run on a background
task/thread, like the #19/#23/#26 caps). Covered by seven `sl-wire` unit tests
(the info batch query + decode incl. `error_ids`, the search escaping, the
id-list and permission decodes, the permission PUT body, the `UpdateExperience`
round-trip, the `RegionExperiences` round-trip, and the status/properties
helpers) and three `lifecycle.rs` tests (`GetExperienceInfo`, `GetExperiences`,
`RegionExperiences` through `handle_caps_event`), plus a new `experiences` tokio
example. *Test: stock local OpenSim ships **no** experience module, so the caps
are absent there — the new `experiences` example logs in, fires the queries
(which no-op as the caps are not in the map) and logs out cleanly with no
protocol error, which is what was live-verified; real data needs a Second Life
region (or an OpenSim grid with an experience module). Deferred (out of scope,
as with #19/#23/#25/#26's asset-bytes and signalling): experience *event/asset*
contents beyond the metadata records, and the experience key-value store an
experience-keyed script uses server-side.*
