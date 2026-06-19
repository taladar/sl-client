# Profiles, Picks & Classifieds

Every avatar has a **profile**: a public page of self-description, interests,
the groups they show, plus two kinds of user-authored listings — **picks**
(favourite places) and **classifieds** (paid ads). They are read for any avatar
and written for your own.

## Profile

The core **properties** are the profile image, partner, "about" text, the
account's *born-on* date, a profile web URL, and charter-member status. Separate
**interests** describe what the avatar wants, their skills, and languages.

| Read command | Result event |
|--------------|--------------|
| `RequestAvatarProperties` | `AvatarProperties` (+ `AvatarInterests`) |
| `RequestAvatarPicks` | `AvatarPicks` |
| `RequestAvatarClassifieds` | `AvatarClassifieds` |
| `RequestAvatarNotes` | `AvatarNotes` |
| `RequestPickInfo` | `PickInfo` |
| `RequestClassifiedInfo` | `ClassifiedInfo` |

The groups shown on a profile arrive as `Event::AvatarGroups`. Per-avatar
private **notes** are also part of the profile system (only you see your notes
about someone).

## Picks

A **pick** is a saved favourite location with a name, description, snapshot, the
sim name, and a global position — the entries on the "Picks" tab of a profile.
The list comes from `RequestAvatarPicks`; one pick's detail from
`RequestPickInfo`.

## Classifieds

A **classified** is a paid listing shown in search: it has a category, a
snapshot, a target parcel/position, a listing price, and creation/expiry dates.
The list comes from `RequestAvatarClassifieds`; detail from
`RequestClassifiedInfo`.

## Editing your own

For the local avatar, the write commands are `UpdateProfile`, `UpdateInterests`,
`UpdateAvatarNotes`, `UpdatePick` / `DeletePick`, and `UpdateClassified` /
`DeleteClassified`. (There are also god/admin delete variants for moderation.)

> **Testing note.** OpenSim only serves profiles/picks/classifieds when its user
> profiles service is enabled and the profile service URL is configured; without
> that, these requests return nothing.

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/avatar_profile.rs`: `AvatarProperties`,
>   `AvatarInterests`, `AvatarGroupMembership`, `AvatarPick`, `PickInfo`,
>   `AvatarClassified`, `ClassifiedInfo`, and the update structs
>   (`ProfileUpdate`, `InterestsUpdate`, `PickUpdate`, `ClassifiedUpdate`).
> - The `Request*`/`Update*`/`Delete*` commands are in
>   `sl-proto/src/command.rs`; the matching events in
>   `sl-proto/src/types/event.rs`.
> - Worked example: `sl-client-tokio/examples/profile_edit.rs`.
