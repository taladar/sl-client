---
id: protocol-29
title: Profile & pick/classified editing (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**29. Profile & pick/classified editing (done) ✅ — `AvatarPropertiesUpdate`,
`AvatarInterestsUpdate`, `AvatarNotesUpdate`, pick/classified
create-update-delete, `pickinforequest`/`ClassifiedInfoRequest` detail · 5 pts.
(extends #4, Tier A.)** Item #4 delivered the read side
(`request_avatar_properties`/`picks`/`notes`); this finishes the deferred write
side and the per-item detail fetches. Implemented: **profile editing** —
`update_profile` (`AvatarPropertiesUpdate`, a `ProfileUpdate` builder: second/
first-life images + about text, allow/mature-publish flags, web URL),
`update_interests` (`AvatarInterestsUpdate`, an `InterestsUpdate`:
want-to/skills masks + free text, languages), and `update_avatar_notes`
(`AvatarNotesUpdate`). **The classifieds *list*** item #4 never had —
`request_avatar_classifieds` (the `GenericMessage` `avatarclassifiedsrequest` →
`Event::AvatarClassifieds`, the `AvatarClassifiedReply` siblings of #4's picks
list, each a header-only `AvatarClassified` id+name). **Detail fetches** —
`request_pick_info` (`pickinforequest` `GenericMessage`, params
`[creator_id, pick_id]` as the viewer sends → `PickInfoReply` →
`Event::PickInfo`, a full `PickInfo`: creator, parcel, name/desc, snapshot, sim
name, global position, sort order, enabled) and `request_classified_info`
(`ClassifiedInfoRequest` → `ClassifiedInfoReply` → `Event::ClassifiedInfo`, a
full `ClassifiedInfo`: creator, creation/expiration dates, category, name/desc,
parcel, snapshot, sim name, global position, flags, listing price) — the
picks/classifieds lists carry only summaries. **Pick CRUD** — `update_pick`
(`PickInfoUpdate`, a `PickUpdate` builder; the session fills `creator_id` with
the agent and never sets the god-only `TopPick` flag, as the viewer does —
supply a fresh id to create, an existing one to edit), `delete_pick`
(`PickDelete`), and the god-gated `god_delete_pick` (`PickGodDelete`).
**Classified CRUD** — `update_classified` (`ClassifiedInfoUpdate`, a
`ClassifiedUpdate` builder; the sim fills the parent estate),
`delete_classified` (`ClassifiedDelete`), and `god_delete_classified`
(`ClassifiedGodDelete`). New value types `ProfileUpdate`, `InterestsUpdate`,
`AvatarClassified`, `PickInfo`, `ClassifiedInfo`, `PickUpdate`,
`ClassifiedUpdate`, and events `AvatarClassifieds`/`PickInfo`/`ClassifiedInfo`,
all wired as `Command`/`SlCommand` variants through both runtimes (plus a new
`Client::agent_id()` accessor on the tokio runtime for self-directed requests).
Field layouts and the `pickinforequest` `[creator_id, pick_id]` param order were
cross-checked against the Firestorm viewer (`llavatarpropertiesprocessor.cpp`)
and OpenSim's `UserProfileModule` / `LLClientView`. Covered by six
`lifecycle.rs` tests (the classifieds-list generic message +
`AvatarClassifiedReply` decode, the `pickinforequest` params + `PickInfoReply`
decode, the `ClassifiedInfoRequest`→`Reply` round-trip, the
profile/interests/notes update encodings, and the pick/classified create+delete
encodings). *Live-verified against the local OpenSim with the profile module
enabled (`[UserProfilesService] Enabled = true` plus pointing `[UserProfiles]
ProfileServiceURL` at the standalone's own `:9000`, not the unbound ROBUST
`:8002`) via the new `profile_edit` tokio example: a full round-trip —
`update_profile` set the about text (confirmed persisted in the `userprofile`
SQLite row and read back cold as "Edited by sl-client #29"), a pick was created
(`PickInfoUpdate`), listed (`AvatarPicksReply`), its details fetched
(`pickinforequest` → `PickInfoReply` with parcel/sim/desc) and deleted
(`PickDelete`; the roster went 1 → 0), and the same create → detail → delete
cycle for a classified (`ClassifiedInfoUpdate`/`Request`/`Reply`/`Delete`). The
interests/notes updates and the two god-delete ops are unit-tested only (the
former need a second observer to see; the latter are god-gated). Test: local
OpenSim — `[UserProfilesService] Enabled = true` (off by default; the SQLite
`UserProfiles` realm auto-migrates) with `ProfileServiceURL` reachable.*
