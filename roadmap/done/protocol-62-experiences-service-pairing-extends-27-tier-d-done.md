---
id: protocol-62
title: Experiences service pairing (extends #27, Tier D). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**62. Experiences service pairing (extends #27, Tier D). ✅ Done.**
`sl-wire/src/experience.rs` had the client-side query/body builders + the
`parse_experience_*` response parsers. Added the full server-side inverse:
**URL-suffix parsers** (inverse of the query builders) — `parse_experience_`
`info_query` (collecting every `public_id`), `parse_find_experience_query`
(percent-decoding the `query` text via a new `percent_decode`),
`parse_group_experiences_query` / `parse_forget_experience_query` (the shared
bare-`?<uuid>` form), and `parse_experience_id_query` (`?experience_id=<id>`);
**request-body parsers** (inverse of the body builders, via #52's
`parse_llsd_xml`, lenient defaults) — `parse_set_experience_permission_request`
→ `Option<(Uuid, ExperiencePermission)>` (with a new
`ExperiencePermission::from_wire`), `parse_update_experience_request` →
`ExperienceUpdate`, `parse_region_experiences_request` (delegating to
`parse_region_experiences`); and **response builders** (server output on #52's
`Llsd::to_llsd_xml`, inverse of the response parsers) —
`build_experience_infos_response` (routing `missing` records to `error_ids`, the
rest to `experience_keys` via a new `ExperienceInfo::to_llsd`),
`build_experience_ids_response`, `build_experience_permissions_response`,
`build_region_experiences_response`, `build_experience_status_response`. All
re-exported from `sl-wire` and `sl-proto`, same private-intra-doc-link gotcha
as #54–#61. *Test: 10 unit round-trips in `experience.rs` (URL/body builders →
parsers, response builders re-parsed via `parse_llsd_xml`); stock OpenSim ships
no experience module so this is unit-tested only.*
