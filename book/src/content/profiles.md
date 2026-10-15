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
| `RequestAvatarProperties` | `AvatarProperties` (+ `AvatarInterests` over UDP; + `AvatarGroups`, `AvatarPicks`, `AvatarNotes` over `AgentProfile`) |
| `RequestAvatarPicks` | `AvatarPicks` |
| `RequestAvatarClassifieds` | `AvatarClassifieds` |
| `RequestAvatarNotes` | `AvatarNotes` |
| `RequestPickInfo` | `PickInfo` |
| `RequestClassifiedInfo` | `ClassifiedInfo` |

The groups shown on a profile arrive as `Event::AvatarGroups`. Per-avatar
private **notes** are also part of the profile system (only you see your notes
about someone).

Where the region grants the **`AgentProfile`** capability (Second Life), the
properties, picks and notes reads are one GET of the whole profile — the
answer arrives as the same events the UDP replies bring, without interests —
and `UpdateProfile` / `UpdateAvatarNotes` are PUTs, which keep an about text
longer than the 500 bytes UDP holds. `UpdateProfile`'s `hide_age` travels only
over the capability. Elsewhere (OpenSim) the UDP messages are used. The picks,
notes and classifieds queries are generic messages whose method and parameter
must be NUL-terminated, or neither grid answers. How each grid answers all of
it is measured in [Profiles and display names](../gridspec/profiles.md).

| Other command | Result event |
|---------------|--------------|
| `RequestClassifiedClickStats` | `ClassifiedClickStats` (over `SearchStatRequest`; Second Life also pushes the older `classifiedclickthrough` with every classified's details) |
| `UploadProfileImage` | `ProfileImageUploaded` (over `UploadAgentProfileImage`; no asset where it failed or the region has no such capability) |
| `SetDisplayName` | `SetDisplayNameReply`, then `DisplayNameUpdate`, over the event queue |

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

## Account contact preferences (`UserInfo`)

Distinct from the *public* profile above, every account has a few private
**contact preferences**. `Command::RequestUserInfo` reads them; the reply is
`Event::UserInfo(UserInfo)`, carrying:

- **`im_via_email`** — whether offline instant messages are forwarded to the
  account's email;
- **`directory_visibility`** — a `DirectoryVisibility` (`Default` = the
  account's online status is shown in people search, or `Hidden`), driven by the
  "hide my online status" toggle;
- **`email`** — the email address on file.

`Command::UpdateUserInfo { im_via_email, directory_visibility }` writes the two
toggles. The email address is **not** settable over UDP — the `UpdateUserInfo`
message has no email field — so only the writable subset is exposed.

## Display name changes

An avatar's [display name](region.md#display-names) — the mutable, user-chosen
name layered over the legacy `First Last` — is resolved in bulk over the
`GetDisplayNames` capability (see
[Region & Estate Information](region.md#display-names)). Beyond that read path,
the simulator also **pushes** two display-name events over the
[CAPS event queue](../comms/caps.md):

- `Event::DisplayNameUpdate(Box<DisplayNameUpdate>)` — a cached display name
  changed (for this agent or another). It carries the previous
  `old_display_name` (handy for a "X is now known as Y" notice) and the full new
  `DisplayName` record, so a client mirroring the name cache can refresh its
  entry.
- `Event::SetDisplayNameReply(Box<SetDisplayNameReply>)` — the asynchronous
  result of *this* agent's own set-display-name request. The set is a `POST` to
  the `SetDisplayName` capability that returns immediately; this push later
  reports the outcome via an HTTP-like `status` (`200` success, `409` a
  stale-name conflict to re-fetch), a `reason` phrase, and either the
  `new_display_name` or an `error_tag`. A `succeeded()` helper checks for `200`.

Both pushes are Second-Life-only: OpenSim resolves display names but never
pushes these.

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/avatar_profile.rs`: `AvatarProperties`,
>   `AvatarInterests`, `AvatarGroupMembership`, `AvatarPick`, `PickInfo`,
>   `AvatarClassified`, `ClassifiedInfo`, the update structs
>   (`ProfileUpdate`, `InterestsUpdate`, `PickUpdate`, `ClassifiedUpdate`), and
>   the contact-preference types `UserInfo` and `DirectoryVisibility` (with its
>   `to_wire`/`from_wire` codec). The display-name push structs
>   `DisplayNameUpdate` and `SetDisplayNameReply` are in
>   `sl-proto/src/types/display_name.rs`.
> - The `Request*`/`Update*`/`Delete*` commands are in
>   `sl-proto/src/command.rs` (incl. `RequestUserInfo` / `UpdateUserInfo`); the
>   matching events in `sl-proto/src/types/event.rs` (incl. `UserInfo`,
>   `DisplayNameUpdate`, `SetDisplayNameReply`).
> - Server events: the sim-side inverses are
>   `SimSession::send_user_info_reply` (the `UserInfo` reply) and the
>   event-queue helpers `enqueue_display_name_update` /
>   `enqueue_set_display_name_reply`;
>   `UpdateUserInfo` / `RequestUserInfo` decode client-side as
>   `ServerEvent::UpdateUserInfo` / `RequestUserInfo`
>   (`sl-proto/src/sim_session.rs`).
> - Worked example: `sl-client-tokio/examples/profile_edit.rs`.
