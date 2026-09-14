---
id: viewer-audit-system-browser-scheme-allowlist
title: Any scheme a chat link or SLURL names is handed to xdg-open
topic: viewer
status: done
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

## Resolution (2026-09-14)

**`xdg-open` can no longer be handed a `String`.**
`sl_viewer_platform::system_browser::ExternalUrl` is the only type
`open_in_system_browser` accepts, and the only way to build one is
`parse` / `from_url`, which check the scheme against the new
`SYSTEM_BROWSER_SCHEMES` allowlist — `http` and `https`, nothing else. The
sibling-type option was taken: a platform leaf crate has no business depending
on the media stack for a two-entry allowlist, and the type is the evidence that
the check happened, exactly as `ValidatedMediaUrl` is at the engines.

All four sinks the item named now validate and report:

- **`linkified_text.rs`** — a link clicked in chat, a notice or an IM. What
  counts as a link there is the linkifier's regex over text anyone can send.
- **`slurl_dispatch.rs`** — the payload of a `secondlife:///` link that turned
  out to carry an ordinary URL.
- **`media_controls.rs`** / **`web_floater.rs`** — "open externally" on the
  page's *current* URL, which the page chose by navigating.

**The allowlist is narrower than the reference's on purpose.**
`gURLProtocolWhitelist` (`indra/llwindow/llwindow.cpp`) also admits
`secondlife:`, `ftp:`, `data:`, `mailto:` and, in Firestorm's Linux build,
`file:` — and matches with a substring `find()` anywhere in the URL rather than
against the scheme. `secondlife:` is dispatched inside the viewer long before
this sink; each of the rest is a scheme whose desktop handler is something
other than a browser, which is the whole hazard. The one visible consequence is
`ftp://`: the linkifier matches it, so such a link still renders and is still
copyable, but clicking it now logs a refusal instead of handing it to the
desktop's `x-scheme-handler/ftp`. Browsers themselves dropped FTP years ago, so
the branch was mostly a dud already.

**`normalize_web_url` was deliberately *not* the fold point**, contrary to the
item's last line. Every one of its callers is an **in-viewer** address bar (the
web floater, the media controls, the profile web panel) that then checks the
result against the wider `MEDIA_URL_SCHEMES`; none of them feeds the system
browser. Folding an `http`/`https` check into it would have rejected a typed
`rtsp://` stream in the media address bar — a regression in a path this item
does not touch. It stays a pure normaliser, now documented as one, with
`normalize_web_url_keeps_a_scheme_the_desktop_is_refused` pinning that split.

Unit-verified in `sl-viewer-platform`:
`schemes_the_desktop_dispatches_elsewhere_are_refused` walks a table of twelve
(`file://` twice, `mailto:`, `tel:`, `data:`, `javascript:`, `ftp:`, `rtsp:`,
`about:`, `chrome://`, `steam://`, `secondlife:`), plus
`web_schemes_are_accepted`, `scheme_matching_is_case_folded` and
`non_urls_are_refused`. The call sites need no test of their own: the refusal
there is a type error, not a runtime branch.
