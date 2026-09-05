---
id: viewer-audit-settings-write-race
title: Settings are written non-atomically from unordered detached tasks
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/viewer.md](../context/viewer.md).

Three compounding defects in the settings-persistence path:

- **Not atomic.** `sl-settings/src/store.rs:402` (`save_scope`) and
  `sl-viewer-settings/src/lib.rs:238` write with `fs_err::write` (O_TRUNC).
  There is no temp-and-rename or lock helper anywhere in either crate, so a
  crash mid-write leaves a truncated or empty `viewer-settings.toml`.
- **Unordered.** `sl-viewer-settings/src/lib.rs:227` — `save_async` serializes
  on the frame thread then `IoTaskPool::spawn(...).detach()`s the write, with no
  sequence number and no read-modify-write. Ten call sites can fire in adjacent
  frames (`preferences.rs:901`, `floater_persist.rs:387`, `ui_table.rs:1390`,
  four in `sl-viewer-people`, `derender.rs:580`, `menu_bar.rs:908`,
  `inventory_actions.rs:1803`), and two detached tasks have no ordering
  guarantee, so an older serialization can land last.
- **Exit save happens at the wrong time.**
  `sl-viewer-world-view/src/session.rs:367` — `save_settings_on_logout` calls
  the *synchronous* `save()` the frame `quit_deadline` is armed, guarded by a
  `Local<bool>`. Any `save_async` write still in flight can complete after it;
  any setting changed between logout-request and process exit is lost; and the
  `Local<bool>` means a second logout in the same process never saves.

Together these are the mechanism behind the recorded settings-clobber hazard.

**The helper now exists.** `sl-settings/src/atomic_file.rs` —
`write_atomically` (sibling temp file, `sync_all`, rename, directory sync) and
`move_aside` — was built for
[[viewer-audit-notification-store-overwrite]], which had the same
overwrite hazard in the notification store. `save_scope` and
`sl-viewer-settings`' writes still call `fs_err::write`, so the first bullet is
now a matter of routing them through it rather than of writing it. The other two
defects are untouched.

Scope: one serialized writer (a monotonic version stamp, or funnel every save
through one channel), the existing atomic temp-and-rename helper wired into both
write sites, and move the final save to actual exit. `sl-viewer-settings` has
**zero tests**;
`from_store_for_test` / `declared_for_test` already exist, so a round-trip test
over every `SettingValue` variant needs no filesystem.
