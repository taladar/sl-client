---
id: idiomatic-p7-09
title: Region-local **position** tuples → RegionCoordinates (consume-only):
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

Region-local **position** tuples → `RegionCoordinates` (consume-only):
    `ChatMessage.position`, `InstantMessage.position`,
    `Event::ConferenceInvited.position`, `ParcelInfo` (parcel.rs)
    `user_location` + `aabb_min`/`aabb_max`. Codec wraps at the boundary
    (`RegionCoordinates::new(x, y, z)` on decode, `.x()/.y()/.z()` on encode);
    LLSD helpers gained `region_coords_from_llsd`/`region_coords_to_llsd`,
    and the `offline_message_position`/`llsd_position` helpers now return
    `RegionCoordinates`. Wire bytes byte-identical. (The outbound
    `ParcelUpdate.user_location`/`user_look_at` — `lsl::Vector` — were ALSO
    converted in the second pass below, per the user's "Vector→typed"
    directive.)
