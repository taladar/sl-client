# Friends & Presence

Friendship is a mutual relationship between two avatars, each side of which
grants the other a set of **rights**. The friends list (the "buddy list") is
part of the [login](login.md) response, and presence (who is online) is pushed
afterward.

## The friends list and rights

Each **friend** entry records the friend's id and two rights masks — the rights
*you* have granted them and the rights *they* have granted you. The rights are:

- **see online status** — whether they can tell when you are logged in,
- **see on map** — whether they can locate you on the world map,
- **modify objects** — whether they may edit your in-world objects.

The initial list arrives as `Event::FriendList` (seeded from the login buddy
list); a later change to rights surfaces as `Event::FriendRightsChanged`.

## Presence

Once logged in, the region pushes online/offline transitions for your friends:
`Event::FriendsOnline` and `Event::FriendsOffline` carry the ids that just came
online or went offline. A client merges these into the friends list to show
presence.

To locate an agent in-world (a friend on the map, or any avatar nearby), see
[Nearby Avatars & Viewer Effects](nearby.md): the coarse-location feed, plus the
`TrackAgent` / `FindAgent` lookups.

## Managing friendships

- **Offer / respond** — `Command::OfferFriendship`, then the recipient's
  `Command::AcceptFriendship` or `DeclineFriendship`. The offer itself travels
  as an [instant message](chat.md#instant-messages) with the friendship dialog.
- **Remove** — `Command::TerminateFriendship`. When a friendship ends — whether
  the *other* party removed you or a removal you requested is confirmed — the
  simulator pushes `Event::FriendshipTerminated { other }`. A client mirroring
  the buddy list should drop `other` on this event.
- **Change rights** — `Command::GrantUserRights` adjusts what a friend may do
  (see online, see on map, modify objects).

## Calling cards

A **calling card** is a reference card to another avatar, filed in your *Calling
Cards* inventory folder. It is **not** a friendship request — it is just a saved
pointer to someone, with no rights attached. The exchange is a small
offer/accept handshake correlated by a `TransactionId`:

- **Offer** — `Command::OfferCallingCard { to_agent_id, transaction_id }` offers
  *your* calling card to another agent. They see an
  `Event::CallingCardOffered { offering_agent, transaction }`.
- **Respond** — the recipient replies with `Command::AcceptCallingCard {
  transaction_id, calling_card_folder }` (filing the card in the named inventory
  folder) or `Command::DeclineCallingCard(transaction_id)`, echoing the offer's
  `transaction_id` so the simulator can match the reply.
- **Outcome** — the original offerer learns the result as
  `Event::CallingCardAccepted { agent, transaction }` or
  `Event::CallingCardDeclined { agent, transaction }`. The accepter's
  destination folder is theirs, not the offerer's, so it is not surfaced.

Calling cards are largely a Second Life feature; OpenSim does not surface
calling-card offers.

---

> **In this codebase**
>
> - Types `Friend` and `FriendRights` are in
>   `sl-proto/src/types/avatar_profile.rs`.
> - Commands `OfferFriendship`, `AcceptFriendship`, `DeclineFriendship`,
>   `TerminateFriendship`, `GrantUserRights`, and the calling-card trio
>   `OfferCallingCard`, `AcceptCallingCard`, `DeclineCallingCard` are in
>   `sl-proto/src/command.rs` (`Session` helpers `offer_calling_card` /
>   `accept_calling_card` / `decline_calling_card`).
> - Events `FriendList`, `FriendsOnline`, `FriendsOffline`,
>   `FriendRightsChanged` are in `sl-proto/src/types/event.rs`; the initial list
>   comes from the login buddy list (see [Login](login.md)). The friendship/
>   calling-card pushes `FriendshipTerminated`, `CallingCardOffered`,
>   `CallingCardAccepted`, and `CallingCardDeclined` are there too (each calling
>   card carries a `TransactionId`).
> - Server events: the sim-side inverses are
>   `SimSession::send_terminate_friendship`, `send_offer_calling_card`,
>   `send_accept_calling_card`, and `send_decline_calling_card`; the decoded
>   client→server calling-card commands surface as
>   `ServerEvent::{CallingCardOffered, CallingCardAccepted, CallingCardDeclined}`
>   (`sl-proto/src/sim_session.rs`).
