---
id: viewer-quit-progress
title: Show that the viewer is logging out while a quit waits for the grid
topic: viewer
status: ready
origin: gridspec-logout (2026-10-06)
refs: [gridspec-logout, viewer-disconnect-screen, viewer-teleport-flow-progress]
---

Context: [context/viewer.md](../context/viewer.md).

A quit sends a `LogoutRequest` and the window stays as it was until the
session reports the logout. On Second Life that is a fifth of a second. On
OpenSim the reply usually never comes (`book/src/gridspec/session.md`
§ Logout) and the session ends on its own five-second timeout — five
seconds in which the world keeps rendering, the avatar can still be driven,
and nothing says the quit was heard.

The reference viewer answers a quit at once: it stops taking input, shows
its progress screen with "Logging out..." (`LLAppViewer::sendLogoutRequest`,
`gLogoutInProgress`), and closes after the reply or after
`gLogoutMaxTime` (six seconds).

Do: a quit request puts up the progress view (the one
[[viewer-teleport-flow-progress]] uses) with the logging-out text, and stops
sending agent input, until the app exits. The e2e quit tests in
`e2e_login.rs` (both fake flavours) can hold the OpenSim-flavoured one to
showing it.
