---
id: viewer-recent-people
title: Recent People — the list of residents you just dealt with, and its tab
topic: viewer
status: ready
origin: the reference's `LLRecentPeople::add` had no counterpart here, found
  wiring the friendship-offer prompt ([[viewer-add-friend-offers-silently]],
  2026-09-11)
points: 3
refs: [viewer-social-people-panel, viewer-add-friend-offers-silently,
  viewer-avatar-radar, viewer-people-lists-multi-select, viewer-contact-sets]
---

Context: [context/viewer.md](../context/viewer.md).

The reference keeps a **Recent People** list: every resident the agent has just
had something to do with — IM'd, been spoken to by in nearby chat, offered
friendship or a teleport, given or accepted inventory from, called. It is the
People floater's **Recent** tab, and the answer to "who was that person I was
just talking to", which neither the friends list (they are usually not friends)
nor the radar (they may have walked away, or never been in range) can give.

We have no such model. Every surface that the reference files a resident from
files nothing here.

## What it is in the reference

`LLRecentPeople` (`llrecentpeople.h`) is a singleton `map<LLUUID, LLSD>`, where
the payload is `{"date": <when>}` — an id plus the time of the **last**
interaction, not the first (a repeat interaction replaces the date rather than
being ignored). It refuses the agent's own id and any id that is a **group**.
It is **session-only**: nothing writes it to disk, so it starts empty at login.

`LLRecentPeople::add` is called from:

| Reference site | The interaction |
| --- | --- |
| `llavataractions.cpp:169` | offering friendship (the Add Friend dialog) |
| `llviewermessage.cpp` (×2) | **accepting** a friendship offer |
| `llviewermessage.cpp:1734` | accepting / declining an inventory offer |
| `llviewermessage.cpp:8051` | sending a teleport offer |
| `llgiveinventory.cpp` (×2) | giving inventory (item, and category) |
| `llimview.cpp:1987` | an IM arriving |
| `llimview.cpp:2340`/`:2360` | starting a 1:1 / ad-hoc conference session |
| `llimview.cpp:2377` | a participant speaking in an ad-hoc session |
| `llimview.cpp:4270` | an incoming voice call |
| `llfloaterimnearbychathandler.cpp:693` | an agent speaking in nearby chat |
| `llvoicechannel.cpp` (×2) | a voice channel's other party |

Group chat is deliberately **not** a trigger (the header says so: 1:1 and ad-hoc
only), which is what keeps the list to people rather than to everyone in a busy
group.

Two of the sites are RLV-gated (`RlvActions::canShowName`) so a restricted
session does not accumulate names the user is not allowed to see — we have
`sl-viewer-rlv` and should honour the same gate.

## The surface

`panel_people.xml`'s `recent_panel`: an `avatar_list` with
`show_last_interaction_time="true"` — each row shows the resident and **how long
ago** the interaction was (`llavatarlist.cpp:779` renders `now - date`) — over a
button row of **filter**, a **gear** menu of the per-resident actions, a
**view/sort** menu (`menu_people_recent_view.xml`: *Sort by Most Recent* /
*Sort by Name*, plus a people-icons toggle), **Add Friend**, and **Remove
Friend**.

Firestorm's own Contacts floater (`floater_fs_contacts.xml`) has no Recent tab —
it carries Friends / Groups / Contact Sets — so the Recent list is a **default
skin** surface. Our People pane already departs from Firestorm here by carrying
Blocked as a fourth sub-tab, so the natural home is a **fifth sub-tab**:
`people.rs` spawns the strip (`FRIENDS_TAB_KEY` … `CONTACT_SETS_TAB_KEY`) and a
content slot per tab, each filled by its own module
([[viewer-social-people-panel]] left this tab unbuilt on purpose).

## Shape here

- **A pure model.** `RecentPeople` in `sl-viewer-world-api` beside `MuteModel`
  and `FriendsModel`: `AgentKey → last interaction (seconds)`, refusing the own
  agent, newest-wins on a repeat, unit-tested. It belongs in the world-api crate
  because the writers are spread across features (chat, conversations,
  inventory, the friendship path) exactly the way `RequestBlock`'s writers are,
  and a feature must not have to depend on the tab that displays it.
- **One way in.** A `NoteRecentInteraction` message (or a `&mut` on the resource
  where the writer already has one) rather than each feature reaching for the
  map: the RLV gate and the own-agent / group refusal then live in one place,
  the way [[viewer-add-friend-offers-silently]] put the offer guards behind one
  `RequestFriendship`.
- **The triggers we can honour today**: the friendship offer
  (`sl_viewer_people::add_friend`), an accepted / declined offer and an accepted
  inventory offer (`offers_invites.rs`), a teleport offer, a give-inventory
  (`Command::GiveInventory`), an arriving or sent IM and a started conference
  (`conversations.rs`), and an agent speaking in nearby chat (`chat.rs`
  `ChatReceived`, agent sources only — not objects). The **voice** triggers
  arrive with the voice work ([[viewer-voice-controls]]).
- **The tab**: a virtualized table like the Friends list's, sorted by most
  recent by default with a Name alternative, a name filter, and the row actions
  the other people lists already route
  ([[viewer-people-lists-multi-select]] — profile, IM, offer teleport, add
  friend, block), plus the "how long ago" column the reference shows.

## Not in scope

- **Persistence.** The reference's list is session-only; keep it so. (If we
  later want it across relogs, that is a separate decision and an account-scoped
  settings file, not a silent addition here.)
- **`updateAvatarsArrivalTime` / `getArrivalTimeByID`.** That half of
  `LLRecentPeople` serves the *Nearby* tab's "sort by recent arrival", which is
  the radar here — and `radar_model.rs` already keeps its own `first_seen` per
  avatar ([[viewer-avatar-radar]]). Nothing to port.
- Group chat participants, per the reference.

## How to verify

Unit: the model refuses the own agent, replaces (not ignores) a repeat
interaction's timestamp, and orders newest-first; each trigger's system files
the resident it names and nobody else; a nearby-chat line from an **object**
files nothing.

Live (OpenSim or aditi, with a second avatar): say something in nearby chat,
send an IM, offer a teleport and offer friendship from the other account, and
watch each of the four appear in the Recent tab with a plausible "moments ago",
in most-recent-first order — and that neither avatar's own id ever appears in
its own list.

Reference (Firestorm, read-only): `llrecentpeople.{h,cpp}`,
`llpanelpeople.cpp` (`LLRecentListUpdater`, `mRecentList`, the
`People.Recent.ViewSort` handlers), `panel_people.xml` (`recent_panel`),
`menu_people_recent_view.xml`, `llavatarlist.cpp` (the last-interaction column).
