# Profiles and display names

A profile is read over UDP with `AvatarPropertiesRequest` — answered by
`AvatarPropertiesReply`, `AvatarInterestsReply` and `AvatarGroupsReply` —
and with three generic messages: `avatarpicksrequest`, `avatarnotesrequest`
and `avatarclassifiedsrequest`. It is written with `AvatarPropertiesUpdate`,
`AvatarInterestsUpdate` and `AvatarNotesUpdate`; picks and classifieds have
messages of their own. Second Life adds `AgentProfile` (the whole profile in
one GET, and PUTs of any part of it), `UploadAgentProfileImage`,
`SearchStatRequest` (a classified's click counts) and `SetDisplayName`;
both grids serve `GetDisplayNames`.

Measured on 2026-10-15 by the conformance cases `profiles` (`2av`: avatar
**A** reads and writes, avatar **B** is the other avatar) and
`display-names` (`2av`). Two runs on aditi (Mauve) and two on the local
OpenSim (`UserProfilesModule`) per case; no answer changed between two runs
of a grid, and every run is held to them (**held**). The fake grid imitates
the display names of each (`DisplayNameStyle`), and `display-names` holds
both of its flavours to them offline; it serves no profiles yet
(`server-fake-grid-profiles`), so `profiles` runs on the live grids only.

The avatars' texts are named by what the census wrote, and an avatar's own
text only by its length; dates are written `d/dd/dddd`.

## A generic request is NUL-terminated

Both grids answered **none** of the three generic requests until the
census sent them as the reference packs them: the method name and every
parameter NUL-terminated (`addString`). Without the terminator neither grid
answered; with it, both answer every one. The old belief that OpenSim
ignores the picks, notes and classifieds queries was this.

## Reading a profile

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| A asks for B's profile over UDP (**held**) | properties (`BornOn` as `d/dd/dddd`, `CharterMember` one byte `1`, flags `0x11`: online and published), interests (all empty) and groups (B's, in one or more `AvatarGroupsReply`) | properties (`CharterMember` empty, flags `0x10`) and interests; **no groups** |
| … B is told (**held**) | nothing | nothing |
| A reads B's profile over `AgentProfile` (**held**) | `200` and every key: `allow_publish`, `allowed_maturity`, `charter_member` (an integer), `customer_type` ("Base"), `display_name`, `display_name_next_update`, `fl_about_text` and `sl_about_text` (`undef` where never written), both image ids, **`groups` empty** although UDP lists them, `home_page`, `id`, `identified`, `is_display_name_default`, the legacy names, `mature_profile`, `member_since` (a date), `notes`, **`partner_id` as a string**, `picks`, `preferred_maturity`, `title`, `transacted`, `username`; **no `online`** and **no `hide_age`** | no capability |
| A reads its own over `AgentProfile` (**held**) | the same keys **and `online`** | no capability |
| the web profile URL in the login response (**held**) | none | none |

## Writing one's own profile

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| `AvatarPropertiesUpdate`: A is told (**held**) | nothing | **the profile and interests again**, unasked |
| … read back over UDP and the capability (**held**) | both texts as sent, both roads | as sent |
| a `PUT` of one key (`sl_about_text`) (**held**) | `200`, no body; read back as sent over both roads | no capability |
| a `PUT` of two keys (**held**) | `200`; **both taken** | no capability |
| a `PUT` of `hide_age` (**held**) | `200`, but **never read back**: the answer still has no `hide_age`, to A or to B | no capability |
| a text of 1000 bytes over the capability (**held**) | kept whole over the capability; **cut to 500 bytes** over UDP | no capability |
| `AvatarInterestsUpdate` (**held**) | taken, the mask and the text as sent; nothing told | the same |

## Notes

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| `avatarnotesrequest` for B (**held**) | `AvatarNotesReply`, empty | the same |
| `AvatarNotesUpdate`: read back (**held**) | as sent, over both roads | as sent |
| a `PUT` of `notes` to `{cap}/{B}` (**held**) | `200`, no body; read back over both roads | no capability |
| B is told of either (**held**) | nothing | nothing |

## Picks

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| `avatarpicksrequest`, nobody's picks (**held**) | an empty `AvatarPicksReply` | the same |
| `PickInfoUpdate` of a new pick (**held**) | the list and the pick's `PickInfoReply`, **unasked**; the pick's `User` is the parcel's owner ("Governor Linden"), its position as sent (zero), no region name | the same two; `User` is the parcel's **name**, the position the agent's, a region name |
| `pickinforequest` (**held**) | the `PickInfoReply` | the same |
| the pick in `AgentProfile` (**held**) | `{description, enabled, grid_x, grid_y, id, name, parcel_id, parcel_name, region_name, region_x/y/z, slurl, snapshot_id}` | no capability |
| an edit (**held**) | the list and the pick again, unasked | the same |
| B asks for A's picks and the pick (**held**) | both | both |
| `PickDelete` (**held**) | **nothing** said; a later list is empty | **the empty list, unasked** |

## Classifieds

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| `avatarclassifiedsrequest` (**held**) | the `AvatarClassifiedReply` list | the same |
| a listing for L$0 (**held**) | **refused**: alert "Invalid price for listing, classified not published."; never readable | taken; nothing said |
| a listing for L$50 (**held**) | taken: **charged L$50** (two balance updates "You paid Commerce Linden L$50.", type 1103), its `ClassifiedInfoReply` and a `classifiedclickthrough` (counts 0), **unasked** | taken, free; nothing said |
| `ClassifiedInfoRequest` (**held**) | the record (estate 1, no region name, no parcel name, flags `0x0`, **no expiry**) and a `classifiedclickthrough` | the record (estate 0, region and parcel names, flags `0x4`, an expiry after the creation) |
| `SearchStatRequest` (**held**) | `200`, the six counters at 0 | no capability |
| an edit (**held**) | the record and the counts again, unasked | nothing |
| `ClassifiedDelete` (**held**) | nothing said; the record is gone | **alert "/Error deleting classified"**; the record stays (a data-layer error in OpenSim's profiles service) |

## Display names

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `GetDisplayNames` for A, B and an unknown id (**held**) | `200`, `agents` and **`bad_ids` naming the unknown id** | `200`, `agents` only: **the unknown id is left out** | each flavour as its grid |
| a record's keys (**held**) | no `display_name_expires` | every key | each as its grid |
| the user name (**held**) | the SLID (a Resident's first name, else `first.last`, lower case) | **the legacy name**, space and capitals | each as its grid |
| the default display name (**held**) | a Resident's first name, anyone else's legacy name | the legacy name | each as its grid |
| `SetDisplayName` (**held**) | granted | no capability | Second Life's flavour grants it |
| setting a display name | **not measured**: Second Life lets an avatar change it once a week, and the test avatars are not renamed | — | from the reference: `409` "Conflict" over the event queue for a stale old name, `400` for an empty name or one over 31 characters, else `200` with the name, a `DisplayNameUpdate` to the agent and to everybody else in the region |

## The profile image

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `UploadAgentProfileImage`, first step (**held**) | `200`, `{state: "upload", uploader}` | no capability | Second Life's flavour hands out an uploader |
| … the image (**held**) | `200`, `{state: "complete", new_asset}`; nothing else said | no capability | the same; the image is stored |
| the profile afterwards (**held**) | the field names the new asset over both roads | — | (no profiles yet) |

## What the client does

`Command::RequestAvatarProperties`, `RequestAvatarPicks` and
`RequestAvatarNotes` read the profile over `AgentProfile` where the region
grants it, one GET for all three (a read already out is not sent again),
and over UDP elsewhere; `UpdateProfile` and `UpdateAvatarNotes` PUT. The
capability's answer reads as the UDP replies do: `member_since` as
`M/D/YYYY`, the caption index as the one-byte `CharterMember`, the flags
from the booleans, and `hide_age`, `customer_type` and `online` (left out
where the answer leaves them out). The classified's click counts arrive as
`Event::ClassifiedClickStats`, over `SearchStatRequest` or from Second
Life's `classifiedclickthrough`; a profile image uploads with
`Command::UploadProfileImage`; `Command::SetDisplayName` POSTs a change.
