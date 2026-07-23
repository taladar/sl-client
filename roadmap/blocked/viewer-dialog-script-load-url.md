---
id: viewer-dialog-script-load-url
title: Script web-page request dialog (llLoadURL)
topic: viewer
status: blocked
origin: script-interface survey (2026-07-23)
blocked_by: [viewer-ui-notification-host]
refs: [viewer-media-prim-browser, viewer-slurl-parse-dispatch]
---

Context: [context/viewer.md](../context/viewer.md).

`llLoadURL` sends the viewer a `LoadURL` message: "Object 'X' owned by Y
wants to take you to a web page", with the script's message and the URL.
`sl-proto` decodes it (`Event::LoadUrl`) but no viewer system consumes
it, so scripted web links (vendor pages, info kiosks) silently vanish.

Scope:

- A toast on the notification host showing object name, owner, and the
  script message, with **Load** / **Ignore** (and the reference's
  block-object option).
- Load opens the URL in the embedded browser
  ([[viewer-media-prim-browser]]'s web floater); never auto-open, and
  show the target URL so the user can vet it.
- Owner-mute and anti-spam integration hook (`viewer-anti-spam-filter`
  throttles floods).

Reference (Firestorm, read-only): `process_load_url`
(`llviewermessage.cpp`), the `LoadWebPage` notification in
`notifications.xml`.

Builds on: the notification host (the toast surface this dialog needs).
