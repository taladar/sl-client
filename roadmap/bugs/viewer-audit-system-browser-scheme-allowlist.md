---
id: viewer-audit-system-browser-scheme-allowlist
title: Any scheme a chat link or SLURL names is handed to xdg-open
topic: viewer
status: bugs
origin: found while fixing [[viewer-audit-media-url-scheme-allowlist]] (2026-09-14)
points: 2
refs: [viewer-audit-media-url-scheme-allowlist]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-platform/src/system_browser.rs:12` — `open_in_system_browser(url:
&str)` spawns `xdg-open` with whatever string it is given, and its callers pass
remote data straight in:

- `sl-viewer-notices/src/linkified_text.rs:357` — a link clicked in chat, a
  notice or an IM (the linkifier's own regex decides what counts as a link).
- `sl-client-bevy-viewer/src/slurl_dispatch.rs:447` — a `secondlife:///` link
  that turns out to carry an ordinary URL.
- `media_controls.rs:915` / `web_floater.rs:327` — "open externally" on the
  *page's* current URL, which the page itself chose.

`xdg-open` is not a browser: it dispatches by scheme and MIME type, so a
`file://` URL opens the user's file manager or editor, and any scheme with a
desktop-file handler (`mailto:`, `tel:`, a game launcher, a package installer)
launches that handler. [[viewer-audit-media-url-scheme-allowlist]] closed the
same class of hole on the *in-viewer* engines with a `ValidatedMediaUrl`
newtype, and deliberately left this sink alone: it is a different sink with a
narrower allowlist (`http` / `https` only — a streaming scheme has no business
reaching the desktop either).

Fix: a validated newtype on `open_in_system_browser` too, mirroring
`sl_media::ValidatedMediaUrl` (either reuse it with a tighter check at the call,
or a sibling type in `sl-viewer-platform`, which does not depend on `sl-media`
today). Each caller then reports a refusal rather than silently launching
nothing. `normalize_web_url` in the same file is the natural place to fold the
check into, since every caller already goes through it or could.
