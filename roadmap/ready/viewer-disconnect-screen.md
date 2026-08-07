---
id: viewer-disconnect-screen
title: Disconnect screen (greyscale + error dialog) instead of closing the window
topic: viewer
status: ready
origin: user request (2026-08-07), during teleport live testing
refs: [viewer-ui-notification-host, viewer-teleport-flow-progress]
---

Context: [context/viewer.md](../context/viewer.md).

When the viewer loses its simulator connection it currently just **exits the
process / closes the window** (`session.rs`: "exit on any `LoggedOut` /
`Disconnected`"). The reference viewer instead **stays open**, drains the world
to a **greyscale (black-and-white) render** and shows a **"You have been
disconnected" error dialog** with the reason, letting the user read it and
choose to quit or (where possible) reconnect/relog.

Scope:

- On an **unexpected** `Disconnected` (not a user-initiated `LoggedOut`/quit):
  do **not** `AppExit`. Freeze the scene, apply a **full-screen desaturation**
  post-process (the reference's disconnect grey-out), and raise a modal error
  notification with the disconnect reason (carried by
  `SlSessionEvent::Disconnected(reason)`), via the notification host.
- Dialog actions: **Quit** (clean exit) and, if a relog path exists, **Log in
  again** (otherwise just Quit). Keep it distinct from the clean-logout path,
  which should still exit as today.
- Distinguish disconnect causes worth surfacing: a kicked/forced disconnect
  ([[missing-batch-3]] messages —
  `KickUser`/`LogoutReply`) vs. a timeout/socket loss.

The greyscale pass can reuse the existing post-process stack (tonemap/exposure
lives in `exposure.rs`/`tonemap.rs`); a global desaturation uniform toggled on
disconnect is the smallest hook.

Reference (Firestorm, read-only): `LLAppViewer::forceDisconnect` /
`LLViewerWindow` disconnect grey-out, the "you have been logged out" /
`LLAppViewer::disconnectViewer` notification.
