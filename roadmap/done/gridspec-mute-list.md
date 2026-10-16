---
id: gridspec-mute-list
title: Mute (block) list storage and enforcement on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-mute-entry-name-is-not-the-username,
  viewer-calling-card-offer-unanswered]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Works fully on OpenSim with `MuteListModule`; never run on aditi; the fake grid
has nothing.

## Discover

`mute_list` on aditi; server-side IM enforcement with two avatars.

## Document

`book/src/gridspec/friends.md` § Mute list.

## Fake grid

Medium — [[server-fake-grid-mute-list]].

## Viewer

Mute name is the username; local filtering where the grid does not filter.

## Done (2026-10-16)

**Discover**: `mute-list` (`sl-conformance/src/cases/mute_list.rs`) is now a
two-avatar census, held to a measured table: the read's road for an empty
list, a list, a cached copy (the IEEE CRC-32 of the file as the reference
caches it) and a stale one; B muted by id under the user name; flag edits, a
rename and removals under a wrong, the first and the second name; every kind
of entry and the file's order; a self-mute; and what reaches A from B (IM,
typing, friendship and teleport offers, a shout) unmuted, muted, with text
excepted and with all but text excepted. Two recording runs per grid, then a
verification run. An `sl-repl` probe on aditi read the raw file.

Findings beyond the plan:

- **Neither grid enforces a mute**: everything a muted resident sends reaches
  the avatar, and the sender is told nothing.
- Second Life keys an entry by id (a rename replaces it, a removal under any
  name removes it); OpenSim by id and name (two names, two entries; removal
  needs the exact name).
- Second Life writes a by-name entry's flags field **empty** (`name|`), which
  our parser refused — failing the whole list, so one by-name block on Second
  Life left every block unenforced.
- The standalone OpenSim's SQLite mute store keeps **one entry per avatar**
  (`PRIMARY KEY(AgentID)` in its migration; MySQL's keys
  `(AgentID, MuteID, MuteName)`), and a read in the same instant as an edit can
  list both rows. The local grid's `[MuteListStore]` now uses the os_groups
  MariaDB, and the book's OpenSim column is that store.

**Document**: `book/src/gridspec/friends.md` § Mute list, linked from
`comms/xfer.md` (the content chapter already links the gridspec chapter).

**Fake grid**: medium gap — [[server-fake-grid-mute-list]] is ready, with the
measurements.

**Viewer**: the parser reads an empty flags field as none (`sl-proto`); a
resident is muted under the user name and an update keeps the entry's own
name (`sl-viewer-people` `wire_name`); `RequestUnblock` removes an entry under
the name it is filed under (radar, `secondlife:///…/unmute`); local filtering
gaps closed: a group invitation from a resident blocked by id
(`MuteList::agent_named`), a conference invitation from a by-name entry, a
requested group notice from a blocked poster, the muted auto-reply's test, and
the session's conversation record no longer opens a conversation or counts
unread for a blocked sender. Unit tests cover each. The live e2e stage
`a_live_block_is_kept_by_the_grid_and_the_radar_unblocks_it` (radar block,
by-name block, relog read-back, radar unblock, second relog) found that a
relogged session's mute list was never served on OpenSim — our `Xfer` ids
restarted at 1 per login and OpenSim keys pending transfers by id across
clients — so a login now numbers its `Xfer`s from its session id (`sl-proto`).
On aditi the same stage found the viewer asking for the list before the
region had taken the agent in — Second Life never answered, so no block was
enforced after a login there — and the request now waits for `AgentArrived`
(`sl-viewer-people` `request_mute_list`), as the reference waits for
`STATE_INVENTORY_SEND`. The fake-grid radar stage holds the names the block and
unblock go out under. Closed
[[viewer-mute-entry-name-is-not-the-username]] (its premise misread
`getUserName`); filed [[viewer-calling-card-offer-unanswered]] (calling-card
offers are never shown or answered, mute or not).

### What could not be measured

- The list's limit (1000 entries on the reference) — a thousand edits on aditi.
- Muting a Linden or a god on aditi; offline messages from a muted resident.
- The fake flavours: the fake grid keeps no mute list until
  [[server-fake-grid-mute-list]].
