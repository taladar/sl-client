---
id: viewer-os-slurl-handler-linux
title: Register as the SLURL / hop URI handler (Linux)
topic: viewer
status: ready
origin: raised during viewer-i18n-fluent-scaffold (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The **inbound** half of URL integration, opposite to the OpenURI portal
([[viewer-os-portals-linux]], viewer → browser): register the viewer as the
desktop's handler for the **`secondlife://`** scheme (and OpenSim's `hop://`),
so clicking a SLURL in a browser, chat app, or `xdg-open`
**routes into the viewer** — teleport to the region/coords, open an agent/group
profile, show a parcel, run an app command. This is the testable platform;
Windows and macOS registration are folded into their deferred native-integration
tasks ([[viewer-os-native-integration-windows]],
[[viewer-os-native-integration-macos]]).

Three parts, only the first is Linux-specific:

- **Scheme registration (Linux).** Ship a `.desktop` file declaring
  `MimeType=x-scheme-handler/secondlife;x-scheme-handler/hop;` and register it
  as the default handler (`xdg-mime default …`, `update-desktop-database`). The
  OS then launches the viewer with the URL as `argv`. (Firestorm does exactly
  this via `register_secondlifeprotocol.sh` / its `.desktop` — the mechanism to
  mirror.)
- **Single-instance routing (cross-cutting, lives here).** A SLURL click while
  the viewer is already running must hand the URL to the **existing** instance,
  not spawn a second login. Detect the running instance and forward the URL over
  IPC — a D-Bus name / activation on Linux, a named pipe on Windows, Apple
  Events on macOS — so the deferred per-OS tasks only supply their transport
  behind a shared `IncomingSlurl` command on the bus. If no instance is running,
  the URL becomes the start location of a fresh login.
- **SLURL parse → action (platform-neutral).** Parse `secondlife://` (region
  teleport `secondlife:///…/128/128/30`, `app/agent/<uuid>/about`,
  `app/group/…`, `app/teleport/…`, the `/app/` command family) into a typed
  action and dispatch it — the same routing a clicked in-world SLURL uses, so
  the parser is shared, not duplicated per entry point. (`sl-types` likely
  already has SLURL types; reuse them.)

Security note: an inbound `/app/` command can be hostile (a malicious page
firing a teleport or a purchase). Gate the dangerous commands behind the same
confirmation the reference viewer's "trusted browser" / `secondlife:///app`
throttle uses — this is a real attack surface, not a convenience.

Reference (Firestorm, read-only): `LLURLDispatcher`, `LLCommandDispatcher` /
the `/app/` handlers (`LLCommandHandler`), `register_secondlifeprotocol.sh`, and
the `-url` / `--url` command-line path in `LLAppViewer`.
