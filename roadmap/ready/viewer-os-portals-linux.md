---
id: viewer-os-portals-linux
title: Linux desktop-portal integration (FileChooser, OpenURI, …)
topic: viewer
status: ready
origin: raised during viewer-i18n-fluent-scaffold (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

Wire the viewer into the Linux **XDG Desktop Portals** — the sandboxed,
compositor-native way an application asks the desktop to do things it should not
do itself. This is the testable platform (the dev environment is Linux/Wayland),
so it leads; the Windows and macOS equivalents are separate, deferred tasks
([[viewer-os-native-integration-windows]],
[[viewer-os-native-integration-macos]]).

The portals a viewer actually needs:

- **FileChooser** — the big one. Every "upload from disk" (textures, meshes,
  animations, sounds), "save snapshot", "export", "save notecard / script to
  file", "import outfit" flow needs an **open** or **save** dialog. Under
  Wayland the app cannot pop its own native file dialog; it must go through the
  `org.freedesktop.portal.FileChooser` portal, which renders the compositor's
  own picker. Filters (`.j2c` / images, `.dae` / mesh, `.anim`, …), multi-select
  for batch upload, and a remembered last-directory.
- **OpenURI** — clicking a URL in chat / a profile / a parcel description, or a
  SLURL, opens it in the user's browser via
  `org.freedesktop.portal.OpenURI` rather than shelling out to `xdg-open`
  guesses.
- **Notification** — desktop notifications for IMs / group notices / friend
  online when the window is unfocused or minimized
  (`org.freedesktop.portal.Notification`).
- **Settings** — read the OS **colour-scheme** preference (dark / light /
  no-preference) from `org.freedesktop.portal.Settings` to drive the skin's
  default theme, and subscribe to changes. (Ties into
  [[viewer-i18n-cultural-color-meanings]] / the skin-token work.)
- **Inhibit** (idle) — optionally suppress the screensaver / idle while active
  in-world.

Implementation notes:

- **`ashpd`** is the idiomatic async Rust binding for XDG portals (zbus-based)
  and covers all of the above; it fits the tokio side. `rfd` is a lighter
  cross-platform file-dialog crate that uses portals on Linux and native
  dialogs on Win/Mac — worth evaluating if a *single* file-dialog abstraction
  across all three platforms is preferable to per-platform portal code (it would
  collapse the file-dialog half of the two deferred tasks).
- The dialogs are **async and out-of-process**; the result arrives later, so the
  viewer needs a request → pending → result flow (a command/event pair on the
  existing bus), not a blocking call on the Bevy frame.
- Falls back gracefully where no portal is running (a bare X11 session): a
  minimal in-viewer file browser, or `rfd`'s GTK backend.

Reference (Firestorm, read-only): `LLFilePicker` / `LLDirPicker` (its GTK file
dialog is *not* portal-based, so it breaks under a confined Wayland session — a
concrete reason to do this properly rather than copy it), and `LLWeb::loadURL`.
