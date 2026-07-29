---
id: viewer-dialog-script-load-url
title: Script web-page request dialog (llLoadURL)
topic: viewer
status: done
origin: script-interface survey (2026-07-23)
blocked_by: [viewer-ui-notification-host]
refs:
  [
    viewer-media-prim-browser,
    viewer-slurl-parse-dispatch,
    viewer-load-url-body-links,
  ]
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

Done (2026-07-29): `src/load_url.rs` — a bespoke card adopted into the shared
toast channel (an "Open a web page?" heading, the `'Object' owned by Owner`
title, the script message, and the target URL rendered verbatim so the user can
vet it), plus **Load** / **Block** / **Ignore** actions. Load writes
`OpenWebBrowser` to open the URL in the embedded web floater
([[viewer-media-prim-browser]]) — never auto-opened, only on the click; Block
mutes the object; Ignore / × dismisses. The `LoadURL` message carries only the
owner *key*, so the owner name is requested (`RequestAvatarNames` /
`RequestGroupNames`) and the title rewritten in place when the reply lands, with
the pending state riding on the title entity (`PendingOwnerName`) so a dismissed
card drops it cleanly. A gallery specimen and unit tests for the owner-name
lookup + request routing. **Links in the message are deferred** like chat's —
follow-up [[viewer-load-url-body-links]]. The **anti-spam** flood-throttle
integration ([[viewer-anti-spam-filter]]) stays that task's hook, not wired
here.
