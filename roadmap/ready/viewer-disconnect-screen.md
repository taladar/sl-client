---
id: viewer-disconnect-screen
title: Disconnect screen (greyscale + error dialog) instead of closing the window
topic: viewer
status: ready
origin: user request (2026-08-07), during teleport live testing
refs: [viewer-ui-notification-host, viewer-teleport-flow-progress]
---

Context: [context/viewer.md](../context/viewer.md).

When the viewer loses its simulator connection it currently just
**exits the process / closes the window**
(`sl-viewer-world-view/src/session.rs`: "exit on any `LoggedOut` /
`Disconnected`"). The reference viewer instead **stays open**, drains the world
to a **greyscale (black-and-white) render** and shows a
**"You have been disconnected" error dialog** with the reason, letting the user
read it and choose to quit or (where possible) reconnect/relog.

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
lives in `sl-viewer-world-scene/src/exposure.rs` / `tonemap.rs`); a global
desaturation uniform toggled on disconnect is the smallest hook.

Reference (Firestorm, read-only): `LLAppViewer::forceDisconnect` /
`LLViewerWindow` disconnect grey-out, the "you have been logged out" /
`LLAppViewer::disconnectViewer` notification.

## Measured kick texts (2026-10-06, [[gridspec-login-refusals]])

A second login of the same account kicks the session already in world on both
grids, as a `KickUser` (`Disconnected(Kicked { reason })`):

- Second Life: "The system has logged you out because you are attempting to
  log in from another location."
- OpenSim: "New login detected".

`sl-client-bevy-viewer`'s `e2e_login` tests
`a_second_life_flavoured_second_login_kicks_the_viewer` and its OpenSim twin
pin today's clean exit; this task changes them to expect the dialog with the
text.

## Kick reasons measured by gridspec-estate (2026-10-17)

OpenSim kicks with "You have been kicked out" (`kickestate`), "Your access to
the region was revoked and TP home failed - you have been logged out." (an
estate ban on a resident with no home) and "The simulator is going down." (a
region restart); each arrives as `DisconnectReason::Kicked`.

## Logging in again after a restart (2026-10-26, gridspec-lsl-persistence)

OpenSim takes logins for a restarting region again a few seconds after it
went down, but a login in between does not fail: the login service cannot
find the region, falls back to its defaults and puts the avatar in a
neighbour. A **Log in again** offered after `The simulator is going down.`
should wait for the region, or say where the login landed.
