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

## Not measured

- The edit right in use: editing a friend's object
  ([Building](building.md)).
- A friendship between avatars in different regions, or on different grids.
- The calling card's description and permissions.
