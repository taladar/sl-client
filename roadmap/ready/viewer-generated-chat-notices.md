---
id: viewer-generated-chat-notices
title: Viewer-originated nearby-chat notices (settings-gated)
topic: viewer
status: ready
origin: debug-settings/chat-lines survey (2026-07-23)
refs:
  [
    viewer-region-restart-schedule,
    viewer-money-economy-ui,
    chat-b1,
    viewer-conversation-log,
    viewer-dialog-offers-invites,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer writes many *viewer-originated* informational lines
into nearby chat, each behind a setting. We currently emit only received
chat plus a few structural `ChatSource::System` lines — there is no
shared "report to nearby chat" helper. This task introduces that helper
and the individual gated notices:

- **Region restart to chat** (`FSReportRegionRestartToChat`, announce
  channel `FSRegionRestartAnnounceChannel`) plus the no-screen-shake
  option — complements the countdown floater in
  [[viewer-region-restart-schedule]].
- **Landmark created** notification (`FSLandmarkCreatedNotification`).
- **Payment lines** — "You paid X" / "X paid you" in chat
  (`FSPaymentInfoInChat`) and the confirm-above-threshold guard
  (`FSConfirmPayments`, `FSPaymentConfirmationThreshold`); pairs with
  [[viewer-money-economy-ui]].
- **Friend online/offline to nearby chat**
  (`OnlineOfflinetoNearbyChat`, `ChatOnlineNotification`) — presence is
  modelled ([[chat-b1]]); this adds the emitter.
- **Auto-accepted inventory logged to chat**
  (`FSLogAutoAcceptInventoryToChat`).
- **Muted-group-chat / block reports**
  (`FSReportMutedGroupChat`, `FSReportBlockToNearbyChat`,
  `FSReportIgnoredAdHocSession`).
- **Server version change** notice on region cross
  (`FSShowServerVersionChangeNotice`).

Reference (Firestorm, read-only): `reportToNearbyChat` callers across
`llviewermessage.cpp` / `fs*` floaters, the named settings.

Builds on: the chat transcript/system-line plumbing (done); each notice
hooks its owning event source as that system exists.
