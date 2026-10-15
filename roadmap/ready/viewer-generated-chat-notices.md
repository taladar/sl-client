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
into nearby chat, each behind a setting. A shared viewer-originated line
now exists: the `LocalChatNotice` message
(`sl-viewer-intents/src/requests.rs`), written by the build tools,
inventory drag and panorama capture and rendered by `update_chat_overlay`
(`sl-viewer-chat/src/chat.rs`). Audit 2026-10-15: it reaches only the
chat overlay, not the nearby-chat transcript in
`sl-viewer-people/src/conversations.rs`, so a reference-style "report to
nearby chat" still needs that half. None of the gated notices below
exist yet:

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
  modelled ([[chat-b1]]); this adds the emitter. Once it exists it also owes
  the **per-set** half (`FSContactSetsNotificationNearbyChat`): one call to
  `ContactSets::notifies`, the same gate the toast path already uses
  ([[viewer-contact-set-presence-extras]]).
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

## Parity-audit addendum (2026-08-19)

Parity-audit extensions — five more settings-gated notice lines: a
notice when the simulator version changes on region change
(`FSShowServerVersionChangeNotice`), a notice when a nearby avatar's
display name changes (`FSShowDisplayNameUpdateNotification`), the saved
snapshot filename logged to local chat (`FSLogSnapshotsToLocal`), a
clickable back-SLURL to the departure point after teleport
(`FSShowBackSLURL`), and radar enter/exit alerts emitted on a private
script channel for scripted consumers (`RadarEnterChannelAlert` /
`RadarLeaveChannelAlert` — the radar itself is done; only the
script-channel announce is missing).
