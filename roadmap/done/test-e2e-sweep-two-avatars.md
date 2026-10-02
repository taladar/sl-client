---
id: test-e2e-sweep-two-avatars
title: End-to-end tests for what two avatars see of each other
topic: test
status: done
origin: test-e2e-live-verify-sweep (2026-09-30)
points: 8
refs: [test-e2e-live-verify-sweep, server-fake-grid-agent-avatars-shared,
  server-fake-grid-im-relay, test-e2e-sweep-live-grid]
---

Context: [context/automation.md](../context/automation.md).

The pending checks that need a second avatar. Two stage viewers on the fake
grid do not see each other's avatars
([[server-fake-grid-agent-avatars-shared]]) and their IMs and offers are not
relayed ([[server-fake-grid-im-relay]]), so each of these either waits for
that grid work or, where the grid's side can be scripted through
`FakeAgent::with_sim` (as the do-not-disturb test does), is written that way
now; the rest run on a live grid meanwhile.

- [[viewer-social-modify-rights-confirm]]: with a friend in the list, the
  empty checkbox asks, Cancel does nothing, Grant sends `GrantUserRights`.
- [[viewer-add-friend-offers-silently]]: the typed message arrives with the
  offer.
- [[viewer-contact-set-presence-extras]]: a contact set's own auto-reply is
  the one sent.
- [[viewer-do-not-disturb-away]]: closing the conversation re-arms the
  busy reply.
- [[viewer-p31-9]]: one's own typing, and a second
  avatar's typing animating it.
- [[viewer-avatar-radar]]: sort persistence, the row context menu (Track,
  Teleport To, Mute), toast mode, the alert sound, a neighbour region's
  avatars in the coarse style.
- [[viewer-derender-blacklist]]: derendering an avatar with attachments.
- [[viewer-floaters-never-reread-after-a-push]]: two residents on one
  parcel, one changes it, the other's open window updates.
- [[viewer-p15-4]]: an observer sees the
  published bake.
- [[viewer-minimap-avatar-dot-color]]: a nearby resident's dot beside a
  beacon.
- [[viewer-table-widget-remaining]]: the friends-rights toggles.
- [[test-fake-grid-concurrent-edits]]: the live-grid half — whether
  anything arbitrates two editors, and whether the loser is told.

## Done

`tests/e2e_two_avatars.rs`. On the fake grid the other resident is a friend
account that never logs in, a catalogue NPC or a message sent through the
viewer's own session; what only a grid relaying between its sessions can show
declares `Need::LiveGrid` (and the bake, `Need::OpenSim`) and runs with
`SL_E2E_GRID=opensim|aditi`.

- **Friendship rights** (fake grid): ticking the edit box asks, Cancel sends
  nothing, Grant sends the grant; the map and edit boxes are taken back
  without asking; a right the friend grants ticks the read-only box.
- **Busy replies** (fake grid): closing an Unavailable conversation re-arms
  the reply, and a contact set's own reply is the one a member hears.
- **Typing** (fake grid): the indicator, the typing animation and the chirp
  reach the grid; an NPC's typing shows a `T` on the radar, and its
  animation reaches the viewer.
- **Radar** (fake grid): Track, Teleport To, Add Friend (the typed message
  rides the offer), Block and Derender (the avatar's body and attachment go,
  the radar keeps it); the sort survives a relog; a region arrival raises a
  `RadarAlert` toast and the radar sound; a neighbour region's coarse-only
  resident is drawn as an approximate position.
- **Minimap** (fake grid): a resident's dot and the tracking beacon are told
  apart by colour, in the minimap window's own pixels.
- **About Land** (fake grid, two viewers): an open window follows the
  owner's rename.
- **Bakes**: the published bakes are stored by the grid (fake grid), and on
  OpenSim the other viewer is told exactly those.
- **A fresh read after a lost write** (fake grid): a prim renamed by
  somebody the grid does not tell this viewer about shows the grid's name
  once selected afresh.
- **Live** (OpenSim, both backends): two residents befriend each other from
  the radar with a message, trade rights both ways, contest one prim's name,
  and part; one sees the other type.

For the tests: a `Sound` stream in the event log (each UI sound by name,
forwarded by the viewer's assembly), `AgentReadout::published_bakes`,
`WorldNode::bakes`, names on the friends list's rights boxes and on the radar's
position mark, and the offer cards' buttons addressed by their response
(`offer-invite-action:Accept`) rather than their caption.

The tests found these viewer bugs, each fixed with a test:

- **Ticking then unticking a right opened an IM.** The rights box's press
  bubbled to the row, whose double-click opens the conversation.
- **A derendered avatar kept its name tag**, drawn where its hidden
  placeholder sat. A tag now hides with its avatar's anchor.
- **A right-click on a window over a name tag opened the avatar's pie**
  behind the window too: the tag test ran before the blocking-UI one.
- **An offer that arrived with the login showed raw Fluent keys** for good:
  it was carded before the locale bundles had loaded. The session's reports
  now wait (`SlEventHold`) until they have (`LocaleSettled`), ten seconds at
  most.
- **A friendship formed mid-session never reached the Friends list**: the
  session adds the friend, but no friend event says so. Accepting, or an
  acceptance arriving, re-reads the buddy list.
- **A world query waited for good on a linkset's child prims**: a grid
  answers a child's family request with its root's record, so the query
  waits only on roots.
- **A Build field re-sent its unchanged text when the focus left it**, which
  overwrote another editor's later rename. A blur now commits only an edit
  (the reference's `mPrevText` check).

Left open: the concurrent-edit contest on Second Life. On aditi the live test
gathers the two avatars, befriends them and trades rights, but the rez it
contests never landed — both viewers ran at about 3 fps on the busy `Morris`
region and the ground aim never held still (see
[[viewer-automation-host-thread-per-viewer]]). It is listed with the other
real-grid checks in [[test-e2e-sweep-live-grid]].
