---
id: protocol-4
title: Avatar profiles
topic: protocol
status: done
origin: ROADMAP.md
---

Context: [context/protocol.md](../context/protocol.md).

**4. Avatar profiles — `AvatarPropertiesRequest`/`Reply` + `GenericMessage`
picks/notes · 3 pts. ✅ Done.** A standalone profile/picks checker. Implemented:
`Session::request_avatar_properties` (UDP `AvatarPropertiesRequest`, answered by
`AvatarPropertiesReply` + `AvatarInterestsReply` + `AvatarGroupsReply`) plus
`request_avatar_picks` and `request_avatar_notes` (the `GenericMessage`
`avatarpicksrequest` / `avatarnotesrequest` calls OpenSim expects). Surfaced as
`Event::{AvatarProperties, AvatarInterests, AvatarGroups, AvatarPicks,
AvatarNotes}`
with value types (`AvatarProperties`, `AvatarInterests`,
`AvatarGroupMembership`, `AvatarPick`). Wired as
`Command::RequestAvatar{Properties, Picks,Notes}` through both runtimes;
verified live (own-profile round-trip returned born date, flags, about text, and
interests). Profile *editing* (`AvatarPropertiesUpdate`, pick/classified
create-update-delete) and pick/classified *detail* fetches are follow-ups.
*Test: local OpenSim — needs the profile module enabled (set `[UserProfiles]
ProfileServiceURL`); otherwise no reply is sent.*
