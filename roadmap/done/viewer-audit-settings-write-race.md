---
id: viewer-audit-settings-write-race
title: Settings are written non-atomically from unordered detached tasks
topic: viewer
status: done
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

## Outcome (2026-09-05): nobody writes but the flush, and it writes one at a time

No version stamp in the end, because there is nothing to stamp: `save_async` no
longer writes. It sets an `AtomicBool` and returns, and a single
`flush_settings` system in `PostUpdate` — after every system that can change a
setting — does the serializing and starts the write. The stamp existed to order
writes that were allowed to race; one writer that starts no second write while
the first is running orders them by construction, and is the same shape the
notification store took in [[viewer-audit-notification-store-overwrite]].

That also makes a burst cheaper rather than merely safe. The ten call sites fire
within a few frames of each other and each used to serialize both scopes and
spawn a task; now the first one to run in a frame costs a `store(true)` and the
other nine cost nothing, and the whole burst becomes one write of the newest
state. What [[viewer-perf-settings-save-offthread]] bought is kept — the disk
write is still off the frame thread, it is just started from one place.

### The exit save is keyed off the exit

`save_settings_on_logout` became `save_settings_on_exit`: an `AppExit` reader in
`Last`, not a `quit_deadline` probe in `Update`. The deadline is armed the frame
a *logout is requested* and the process then lives on for up to the whole grace
period, so everything changed in between was lost; `AppExit` is written by every
way out (an acknowledged logout, the forced deadline exit, a login-outcome
restart, a window close) and Bevy checks for it only once the whole schedule has
run, which makes `Last` the final point at which the newest state can still
reach disk. The `Local<bool>` guard that made a second logout in one process
save nothing is gone with it. `save` waits for the flush in flight before
writing, so no older serialization can complete after it.

### `write_atomically`, and a directory that was never made

Both write sites go through it now — `SettingsStore::save_scope` for anyone
holding a store, `write_scope_file` for the viewer's own two scopes.

The viewer's helper creates the parent directory first, which is a *fourth*
defect the finding did not name: nothing makes the platform config directory
except `sl_account_dirs` at login, so a first run that quit before logging in
wrote `viewer-settings.toml` into a directory that did not exist and lost
everything it had been asked to remember. Pinned by
`a_save_creates_the_directory_it_writes_into`.

### Pinned by

One test in `sl-viewer-world-view`, `the_exit_save_runs_before_the_app_stops`,
for the assumption the whole exit fix rests on and the settings crate cannot
check on its own: it drives a **real** `App` (`MinimalPlugins`, an `Update`
system writing `AppExit`, `save_settings_on_exit` in `Last`) to its own exit and
then reads the file back. If a `Last` system did not run in the frame the exit
was requested there would be no file at all.

Six more in `sl-viewer-settings`, in a crate that had none:
`every_value_type_round_trips_through_the_saved_file` (all ten `SettingValue`
variants, each declared with a *different* default so a round trip cannot pass
by falling back to it),
`a_save_writes_the_global_scope_and_the_account_scope_once_resolved`,
`a_burst_of_saves_coalesces_into_one_serialized_write`,
`the_exit_save_writes_the_state_as_of_the_exit` (a value changed after an
in-session flush had already started a write of the older one — the ordering
defect itself), `a_save_creates_the_directory_it_writes_into`, and
`an_empty_path_writes_nothing` (an unresolved path means no persistence, not a
file called nothing — what both `_for_test` constructors hand out).

Each was checked against the defect it names by putting the defect back: without
the wait, `the_exit_save_writes_the_state_as_of_the_exit` fails; with a flush
that starts a write regardless of one in flight,
`a_burst_of_saves_coalesces_into_one_serialized_write` fails; without the
directory creation, two fail.
