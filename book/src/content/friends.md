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

## Managing friendships

- **Offer / respond** — `Command::OfferFriendship`, then the recipient's
  `Command::AcceptFriendship` or `DeclineFriendship`. The offer itself travels
  as an [instant message](chat.md#instant-messages) with the friendship dialog.
- **Remove** — `Command::TerminateFriendship`.
- **Change rights** — `Command::GrantUserRights` adjusts what a friend may do
  (see online, see on map, modify objects).

---

> **In this codebase**
>
> - Types `Friend` and `FriendRights` are in
>   `sl-proto/src/types/avatar_profile.rs`.
> - Commands `OfferFriendship`, `AcceptFriendship`, `DeclineFriendship`,
>   `TerminateFriendship`, `GrantUserRights` are in `sl-proto/src/command.rs`.
> - Events `FriendList`, `FriendsOnline`, `FriendsOffline`,
>   `FriendRightsChanged` are in `sl-proto/src/types/event.rs`; the initial list
>   comes from the login buddy list (see [Login](login.md)).
