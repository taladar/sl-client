---
id: test-e2e-sweep-two-avatars
title: End-to-end tests for what two avatars see of each other
topic: test
status: ready
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
