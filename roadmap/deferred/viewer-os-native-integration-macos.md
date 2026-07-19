---
id: viewer-os-native-integration-macos
title: macOS native OS integration (file dialogs, OpenURI, notifications)
topic: viewer
status: deferred
origin: raised during viewer-i18n-fluent-scaffold (2026-07)
blocked_by: [viewer-os-portals-linux]
---

Context: [context/viewer.md](../context/viewer.md).

The macOS equivalent of the Linux desktop-portal work
([[viewer-os-portals-linux]]): the same file open/save, open-URL/SLURL,
notification and light/dark-preference needs, against macOS' native (Cocoa)
APIs.

**Deferred: the dev environment is Linux, so this cannot be built or tested
here.** Land the Linux task and its platform-neutral async request → result
plumbing first, then fill in the macOS backend behind the same interface.

Cocoa equivalents to wire:

- **File dialogs** — `NSOpenPanel` / `NSSavePanel`, with the same filters
  (UTType-based) and multi-select.
- **Open URI** — `NSWorkspace openURL:` on the URL.
- **Notifications** — `UNUserNotificationCenter` (User Notifications
  framework).
- **Colour scheme** — the effective appearance (`NSApp.effectiveAppearance`,
  `AppleInterfaceStyle`) + a KVO/notification subscription, to drive the skin's
  default theme.
- **SLURL handler registration** — the inbound counterpart
  ([[viewer-os-slurl-handler-linux]]): declare the `secondlife` / `hop` schemes
  in `Info.plist` (`CFBundleURLTypes`) and handle the
  `kInternetEventClass`/`kAEGetURL` Apple Event, which is also the macOS
  single-instance transport (the already-running app receives the URL event)
  behind the shared `IncomingSlurl` command.

**Prefer a cross-platform crate if one fits:** `rfd` provides native macOS file
dialogs behind the same API as the Linux portal path, so if the Linux task
adopts `rfd`, the macOS file-dialog half comes largely free and this task
shrinks to OpenURI + notifications + theme. (Cocoa calls from Rust need
`objc2` / the `objc2-app-kit` bindings.)

Reference (Firestorm, read-only): `LLFilePicker` (its Cocoa `NSOpenPanel` path,
`llfilepicker_mac.mm`), `LLWeb::loadURL`.
