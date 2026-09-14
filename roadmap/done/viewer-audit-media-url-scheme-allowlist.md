---
id: viewer-audit-media-url-scheme-allowlist
title: Parcel media URLs reach CEF and GStreamer with no scheme allowlist
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-media/src/lib.rs:517` — `classify_url` matches `STREAM_SCHEMES`, then falls
through to `MediaKind::Web` for **everything** unrecognised, including
`file://`, `data:` and `javascript:`. There is no filter downstream either:
`SurfaceConfig::initial_url` (`:113`) and `MediaSurface::navigate` (`:308`) take
a bare `String`, `sl-cef/src/chromium.rs:563` passes it to CEF with an
unhardened `BrowserSettings::default()` (`:922`), and `sl-gst/src/stream.rs:142`
sets it as `playbin3`'s `uri` (GStreamer's `uridecodebin` opens `file://`
happily — `sl-gst/src/surface.rs:782` even tests with a `file://` URL).

Parcel and prim media URLs are supplied by any land or object owner.

**Scoped honestly:** Chromium's `allow_file_access_from_file_urls` defaults to
false, so a `file://` page cannot read *other* local files and exfiltrate them.
The real impact is that a land or object owner can make your viewer open and
render a local file on a prim face, visible only to you. This is a hardening
gap, not a data-exfiltration hole.

Scope: a scheme allowlist in `sl-media`, made unbypassable with a
`ValidatedMediaUrl` newtype so the type system carries the evidence — matching
the workspace's existing typed-newtype convention. The 4 existing `classify_url`
tests are all happy paths; the table test to add asserts `file://`, `data:`,
`javascript:`, `about:` and `chrome://` are each **rejected**.

## Resolution (2026-09-14)

**A media engine can no longer be handed a `String`.**
`sl_media::ValidatedMediaUrl` is the only type `SurfaceConfig::initial_url`,
`MediaSurface::navigate`, `AudioStreamPlayer::play` and `classify_url` accept,
and the only way to build one from remote data is `parse` / `from_url`, which
check the scheme against the new `MEDIA_URL_SCHEMES` allowlist (`http`,
`https`, and the five streaming schemes `classify_url` already dispatched on).
Making `classify_url` take the validated type is what keeps this honest: the
dispatch that used to *fall through* to `MediaKind::Web` for anything
unrecognised now cannot be reached by an unrecognised scheme at all.

Two constructors sit beside the allowlist, named for the claim they make:
`blank()` (the engine's own `about:blank`, so `SurfaceConfig::default` stays
infallible — `about:` is otherwise refused) and `viewer_authored()` (the one
`data:` URL in the tree, the gallery's offline specimen page, plus the `file://`
URL the GStreamer error-path test authors for itself).

Every URL entering the engines was tracked down and routed through the check:

- **In-world prims** (`media_prim.rs`) — the surface start, the server-side
  navigation follow, and the bounce-back destination. A refused URL starts no
  surface. The white-list enforcement now applies *both* checks to the URL the
  page itself navigated to, so a page that redirects itself to `file://` is
  bounced exactly like one that leaves its entry's white-list.
- **Parcel music** (`parcel_audio.rs`) — the land owner's music URL. The raw
  URL is kept alongside as the change detector so validation, and its log line,
  happen once per parcel switch rather than once per frame; a refused URL reads
  as "this parcel has no stream".
- **Search web tab** (`search.rs`) — on OpenSim the base URL comes from the
  grid's `SimulatorFeatures`.
- **Profile web panel** (`avatar_profile.rs`) — another avatar's homepage field.
- **Web floater** (`web_floater.rs`) — the address bar, the `OpenWebBrowser`
  message (chat / SLURL openers), and the page's own popup request.
- **Media controls** (`media_controls.rs`) — the typed URL, which also goes to
  the grid for every other agent, and the "home" button's entry URL.

Unit-verified in `sl-media`: `disallowed_schemes_are_rejected` walks a table of
ten (`file://` twice, `data:`, `javascript:`, `about:blank`, `about:config`,
`chrome://`, `chrome-devtools://`, `blob:`, `ftp:`), plus
`scheme_matching_is_case_folded`, `non_urls_are_rejected`,
`blank_is_its_own_door` and `stream_schemes_are_all_allowed` (which pins that
every scheme the video dispatch arm matches is one the allowlist admits —
otherwise that arm would be dead code). `media_prim.rs` grew
`a_media_url_the_allowlist_refuses_opens_nothing`, at the spot the audit named.

Two adjacent findings were deliberately **not** folded in and are filed instead:
[[viewer-audit-system-browser-scheme-allowlist]] (the same class of hole at a
different sink — `xdg-open`, which is not a browser) and
[[viewer-audit-cef-browser-settings-hardening]] (the
`BrowserSettings::default()` half of this audit item's own text, which is a
capability question rather than a URL question).

One thing seen while tracing the call sites, not acted on: the profile web
panel spawns its view with `isolated: false`, i.e. in the **shared** request
context that carries the grid's web-session cookie, for a URL any avatar can
put in their homepage field. The reference's `LLPanelProfileWeb` may well do
the same — that needs checking against Firestorm before it is called a defect,
so it is recorded here rather than filed.
