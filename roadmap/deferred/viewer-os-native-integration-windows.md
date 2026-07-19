---
id: viewer-os-native-integration-windows
title: Windows native OS integration (file dialogs, OpenURI, notifications)
topic: viewer
status: deferred
origin: raised during viewer-i18n-fluent-scaffold (2026-07)
blocked_by: [viewer-os-portals-linux]
---

Context: [context/viewer.md](../context/viewer.md).

The Windows equivalent of the Linux desktop-portal work
([[viewer-os-portals-linux]]): the same viewer needs — file open/save, opening a
URL/SLURL in the browser, desktop notifications, and reading the OS light/dark
preference — implemented against Windows' native APIs instead of XDG portals.

**Deferred: the dev environment is Linux, so this cannot be built or tested
here.** Do the Linux task first, land the async request → pending → result
plumbing on the shared bus in a platform-neutral way, then fill in the Windows
backend behind the same interface.

Windows equivalents to wire:

- **File dialogs** — the Common Item Dialog (`IFileOpenDialog` /
  `IFileSaveDialog`), with the same filters and multi-select as the portal
  FileChooser.
- **Open URI** — `ShellExecute` / `ShellExecuteEx` on the URL.
- **Notifications** — toast notifications (`ToastNotificationManager`), or a
  tray-icon balloon fallback on older systems.
- **Colour scheme** — `AppsUseLightTheme` in the registry
  (`HKCU\...\Themes\Personalize`) + a change subscription, to drive the skin's
  default theme.
- **SLURL handler registration** — the inbound counterpart
  ([[viewer-os-slurl-handler-linux]]): register the `secondlife` / `hop` URL
  protocols under `HKCR` (a `URL Protocol` key with a `shell\open\command`), and
  supply the Windows single-instance transport (a named pipe / `WM_COPYDATA`)
  behind the shared `IncomingSlurl` command.

**Prefer a cross-platform crate if one fits:** `rfd` already provides native
Windows file dialogs behind the same API it uses for the Linux portal, so if the
Linux task adopts `rfd` for the file-dialog half, the Windows file-dialog half
is largely free and this task shrinks to OpenURI + notifications + theme.

Reference (Firestorm, read-only): `LLFilePicker` (its Win32 `GetOpenFileName`
path), `LLWeb::loadURL`.
