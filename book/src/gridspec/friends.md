# Friends and presence

A friendship is offered as an instant message (dialog 38), answered with
`AcceptFriendship` or `DeclineFriendship` and ended with
`TerminateFriendship`; each friend then grants the other rights with
`GrantUserRights` (see online 1, see on the map 2, edit objects 4) and is told
of the other's with `ChangeUserRights`; presence arrives as
`OnlineNotification` and `OfflineNotification`. A calling card is offered,
accepted and declined by three messages of its own. What each grid makes of
them shows in what each of the two avatars is then sent.

Measured on 2026-10-14 by the conformance case `friends-presence`: avatar
**A** offers, grants and ends; avatar **B** answers and goes away four times.
Two runs on aditi (Mauve) and two on the local OpenSim; no answer changed
between two runs of a grid, and every run is held to them (**held**). The
fake grid imitates each (`sl-fake-grid`'s `social` module, a row of
`FriendsPolicy` per divergence), and the same case holds both of its flavours
to these answers offline.

## An offer and its answer

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| the offer as B is sent it (**held**) | dialog 38 from A under A's name, **an id of its own** (the transaction), **no region**, not kept, the text as sent, a bucket of one zero byte | dialog 38 from A under A's name, **A's own id** for the transaction, a region, not kept, the text as sent, no bucket |
| the offerer is told (**held**) | nothing | nothing |
| B declines: A is told (**held**) | **nothing** — the reference no longer tells the offerer either | `FriendshipDeclined` (dialog 40) from B, its id the session of the two avatars, its text an id |
| an offer after a decline (**held**) | passed on | passed on |
| B accepts: A is told (**held**) | `FriendshipAccepted` (dialog 39) from B, **id nil, no region, no text**, a bucket of one zero byte; and B is online | `FriendshipAccepted` from B, id the session of the two avatars, a region, its text an id; and **a calling card of B's** in A's calling-card folder |
| … and B is told (**held**) | nothing | A is online; and **a calling card of A's** in the calling-card folder B named |
| an offer to a friend (**held**) | **passed on**, as any offer | **refused**: A is sent the alert "This person is already your friend. Please delete it first if you want to reestablish the friendship."; B is sent nothing |
| ending a friendship that is not there (**held**) | both avatars are told it ended | the same |
| ending one: who is told (**held**) | both, each naming the other | the same |
| ending one while the friend is away (**held**) | the one who ends it is told; the friend's next login has no entry for it, and nothing says so | the same |

So Second Life makes no calling card at all when a friendship forms, and
OpenSim makes one for each side whatever folder the accepter named.

## Rights

A friendship starts with seeing each other online, both ways.

| A grants B | A is told (**held**) | B is told on Second Life (**held**) | B is told on OpenSim (**held**) |
| --- | --- | --- | --- |
| everything (7) | 7, as an echo of its own | **6** | 7 |
| nothing (0) | 0 | A is offline; 0 | A is offline; 0 |
| seeing it online (1) | 1 | **A is online — and no change of rights** | A is online; 1 |

B granting A seeing it online and on the map (3) is told to A as **2** on
Second Life and as 3 on OpenSim. **Second Life never tells a grantee that it
may see the grantor online**: the bit is masked out of every
`ChangeUserRights` the grantee is sent, and a grant of that bit alone sends
the grantee no change at all, only the presence it now may see. The login
buddy list does not mask it: B logging in after A granted it everything and
it granted A 3 reads "given 3, has 7" on both grids (**held**).

| the map right in use | Second Life | OpenSim |
| --- | --- | --- |
| `FindAgent` for B without the right (**held**) | no answer | no answer |
| … with the right (**held**) | an answer, at one place | the same |
| `TrackAgent` for B with the right: the coarse feed's `Prey` (**held**) | **points at B** (without the right it never did: [Avatars](avatars.md)) | never set: its encoder does not |

## Presence

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| B logs out: A is told (**held**) | B is offline, within seconds | the same |
| B vanishes without logging out: A is told (**held**) | B is offline — about **100 s** later | B is offline — about **60 s** later |
| B comes back (**held**) | A is told B is online; B's login brings its buddy list and then A online | the same |
| a login after a vanishing | taken at once; nothing refused | the same |
| a stranger logs out (**held**) | nothing | nothing |

## Calling cards

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| A offers its card: B is sent (**held**) | the offer, under the transaction A sent | the offer under **the new card's item id**, and the card itself, **already** in B's calling-card folder |
| B accepts: who is told (**held**) | A, under A's transaction; B's card arrives in the folder it named | nobody: the card is already there |
| B declines: who is told (**held**) | A, under A's transaction | nobody; B's card goes to its Trash |

## An offer kept for an avatar that is away

A ends the friendship, B logs out, and A offers again while B is away; A is
told nothing on either grid (**held**). B then comes back.

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| what B is sent unasked (**held**) | nothing | **the offer**, at once, as if live: not kept, A's id for the transaction, a region, and the text "Will you be my friend?" — not A's |
| asked with `RetrieveInstantMessages` (**held**) | the offer, kept, its transaction id, no region, A's text, a bucket of one zero byte | nothing more (it was already handed over) |
| read over `ReadOfflineMsgs` (**held**) | the offer, `transaction-id` **nil**, `asset_id` the session of the two avatars, `offline` 1 and every other field as for a message | no such capability |
| accepted over UDP with the kept transaction id (**held**) | taken: A is told as for a live accept | taken |
| read as our client reads (`RequestOfflineMessages`) and accepted as it answers (**held**) | read over the capability: no id, a region; accepted through `AcceptFriendship`: taken, A is told as for a live accept | read over UDP (no capability), accepted over UDP: taken |
| declined through `DeclineFriendship` (**held**) | `200`, `{success: true}`; A is told nothing, and nothing is kept for B afterwards | no such capability |

So an offer read over the capability can only be answered through the
capabilities, and the capabilities take an offer however it was read — the
first runs accepted one read over UDP through `AcceptFriendship` too: a
viewer can answer every kept offer that way, as the reference does
(`friendship_offer_callback`), where the region grants them.

## What the viewer does with it

- **A kept offer is answered over the capabilities** where the region grants
  them, as the reference does: `Command::AcceptFriendship` and
  `DeclineFriendship` carry `use_offline_cap`, the viewer sets it for an offer
  that arrived kept, and the client POSTs `?from=<offerer>&agent_name=` or sends
  a DELETE of `?from=<offerer>`, logging a refusal; without the capability it
  answers over UDP as before.
- **The kept messages are read over `ReadOfflineMsgs`** where the region
  grants it and both answer capabilities, and the census's client round
  reads and accepts as the viewer does.
- **The Friends list no longer claims to show whether a friend lets you see
  them online.** Second Life never says, and a friend who hides simply looks
  offline: the list shows the reference's five rights columns.
- **No calling card is waited for.** Neither the friendship nor the card
  offer waits on the other side's confirmation, which OpenSim never sends.

## Mute list

The mute (block) list is kept by the grid and read whole: the viewer sends
`MuteListRequest` with the CRC of the copy it has cached (0 for none), and the
grid answers with `UseCachedMuteList`, with a `MuteListUpdate` naming a file
the viewer then fetches over [Xfer](../comms/xfer.md), or — for an empty list —
with the `emptymutelist` [generic message](../comms/generic-messages.md). An
entry is added or changed with `UpdateMuteListEntry` (id, name, type, the
per-aspect *exception* flags) and removed with `RemoveMuteListEntry` (id,
name); neither is answered.

Measured on 2026-10-16 by the conformance case `mute-list`: avatar **A** keeps
the list, avatar **B** is the resident it mutes. Runs on aditi (Mauve) and on
the local OpenSim with its mute store in MariaDB (see the note below); every
run is held to these answers (**held**). The fake grid keeps no mute list yet
(`server-fake-grid-mute-list`).

### Reading the list

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| an empty list, asked with CRC 0 (**held**) | `emptymutelist` | `emptymutelist` |
| an empty list, asked with any CRC (**held**) | `emptymutelist` | `emptymutelist` |
| a list, asked with CRC 0 (**held**) | `MuteListUpdate`, then the file over Xfer | the same |
| a list, asked with the CRC of the file as the viewer caches it (**held**) | `UseCachedMuteList` | `UseCachedMuteList` |
| a list, asked with another CRC (**held**) | the file again | the file again |
| the list emptied by the removals (**held**) | `emptymutelist`, not an empty file | the same |

The CRC is the IEEE CRC-32 of the file as the reference writes its cache —
one `<type> <id> <name>|<flags>` line an entry — so both grids write the file
byte for byte as a viewer would, apart from the line below.

### What is kept

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| a resident, by id under the user name (**held**) | kept as sent: agent, the id, the name, flags 0 | the same |
| a by-name entry (nil id) (**held**) | kept, **written `0 <nil> <name>\|` — the flags field empty** | kept, written `…\|0` |
| an agent id that is nobody, an object, a group, an external entry (**held**) | each kept as sent | the same |
| the order of the file (**held**) | the order the entries were added | the same |
| a second mute of the same resident with other flags (**held**) | the entry's flags change; one entry | the same |
| the same resident muted under another name (**held**) | **the entry is renamed**: one entry, the new name, flags 0 | **a second entry**: both names, each with its own flags |
| a removal under a name the resident was never muted under (**held**) | **removes the entry** | removes nothing |
| a removal under the first name, after the rename (**held**) | — (already gone) | removes that name's entry only |
| a mute of oneself (**held**) | not kept | not kept |
| the reply to an edit | none | none |

So Second Life keys an entry by its id and OpenSim by its id *and* name: a
viewer that names the same resident two ways files two entries on OpenSim,
and must remove an entry under exactly the name it was filed under. The
reference names a resident by the **user name** (`getUserName()`: `First
Last`, or `First` for a Resident — not lower case).

**OpenSim's store.** The local grid first ran the stock standalone SQLite
store, whose migration declares `PRIMARY KEY(AgentID)` beside the
`UNIQUE(AgentID, MuteID, MuteName)` the MySQL migration has: its `replace
into` keeps **one entry per avatar**, each mute replacing the last (a census of
five kinds kept only the last). A read taken in the same instant as an edit
could also list the old row and the new one together. The grids people run use
the MySQL store, so the local grid's `[MuteListStore]` now points at the
MariaDB the groups module already uses, and the table above is that store's
answer.

### What a muted resident can still send

A mutes B outright, with text chat excepted (flags 1), and with every aspect
but text chat excepted (flags 14); in each round, and once before the mute, B
sends A an instant message, a typing notice, a friendship offer, a teleport
offer and a shout.

| behaviour | Second Life | OpenSim |
| --- | --- | --- |
| what reaches A, in every round (**held**) | **all of it** | all of it |
| what B is told (**held**) | nothing | nothing |

**Neither grid enforces a mute.** Everything a muted resident sends is
delivered, and the sender is told nothing; filtering is the viewer's alone.

### What the viewer does with it

- **The mute list is asked for once the region has taken the agent in**
  (`AgentMovementComplete`), as the reference asks in `STATE_INVENTORY_SEND`.
  The viewer asked as soon as the login named the agent; Second Life does not
  answer a `MuteListRequest` that early, so on Second Life the list never
  loaded and no block was enforced after a login (found by the live e2e stage
  below; OpenSim answers at once).
- **A fresh login's mute list is served on OpenSim.** OpenSim keeps its
  pending `Xfer`s by id across every client of a region, and our session
  numbered every login's transfers from 1, so after a relog the list request
  met an earlier session's unfinished transfer and was never answered (found by
  the live e2e stage below). A login now numbers its transfers from its session
  id; the reference draws a random id per transfer.
- **A mute list with a by-name entry is read.** The parser refused Second
  Life's `name|` line as a malformed flags field and with it the *whole* list,
  so one by-name block on Second Life left every block unenforced. An empty
  field now reads as no flags, as the reference's `" %d %254s %254[^|]| %u"`
  does.
- **A resident is muted under the user name**, whatever name the asking surface
  showed (a display name, an instant message's `First Resident`): the name the
  grid resolved for the id, else the given one with a `Resident` surname
  dropped. A request for someone already on the list updates that entry under
  its own name, so OpenSim does not file a second.
- **An unblock names the entry as the list keeps it** (`RequestUnblock`): the
  radar's and a `secondlife:///…/unmute` link's removals used the name the
  surface knew, which OpenSim ignores when it is not the entry's.
- **Everything is filtered locally**, as neither grid filters: the instant
  message, its typing notice, local chat, offers (friendship declined,
  teleport ignored, inventory declined), group and conference lines. Now also a
  group invitation from a resident blocked by id (matched by the inviter's
  name in any form, as `LLMuteList::isMuted(username)` does), a conference
  invitation from a by-name entry, and a group notice a blocked resident posted
  even when asked for. The session's own conversation record no longer opens a
  conversation or counts unread for a blocked sender. The muted auto-reply uses
  the same test as the message filter.

Held by the e2e stage
`a_live_block_is_kept_by_the_grid_and_the_radar_unblocks_it` (two viewers,
either live grid): Alpha blocks Beta from the radar and a name
from the Blocked tab, a relog reads both back from the grid, the radar's
Unblock removes Beta, and a second relog shows the grid kept the name and let go
of Beta. The fake-grid radar stage holds the name both messages go out under.

## Not measured

- The edit right in use: editing a friend's object
  ([Building](building.md)).
- The mute list's limit (the reference's `MuteListLimit`, 1000): filling it
  costs a thousand edits on aditi.
- Muting a Linden or a god: OpenSim refuses an administrator by re-sending the
  list (`MuteListModule`); not tried on aditi.
- Offline messages from a muted resident: what is kept for an avatar that is
  away was not tried with a mute in place.
- A friendship between avatars in different regions, or on different grids.
- The calling card's description and permissions.
