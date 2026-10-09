---
id: viewer-automation-set-a-setting
title: Let a test change a viewer setting through the automation
topic: viewer
status: done
origin: gridspec-object-update-stream (2026-10-09)
refs: [gridspec-object-update-stream, gridspec-terrain]
---

Context: [context/automation.md](../context/automation.md).

A test that wants the viewer at another draw distance has to drive the
Preferences window to it, slider and all. Nothing in the automation
protocol reads or writes a setting by its key.

Two checks were left on the session's side for the lack of it:

- [[gridspec-object-update-stream]]: on Second Life a linkset beyond the
  draw distance is killed by its root alone and comes back when the
  distance does. The session drops and restores it whole; nobody watched
  the viewer's scene do the same.
- [[gridspec-terrain]]: whether either grid holds terrain back at a small
  draw distance.

## What

A request to read a setting and one to write it, by the key the settings
store uses (`ViewerSettings`), refused for a key that is not registered and
for a value of the wrong kind; the driver's `Viewer::setting` /
`set_setting`; `sl-viewer-ctl setting get|set`. A written value goes
through the store, so everything that follows the setting follows the
write, and the stage's per-run settings directory keeps it out of the
operator's own.

Then the draw-distance leg in `e2e_objects` on aditi: objects counted, the
distance taken to 32 m, fewer objects and none hanging off a root that is
gone, the distance brought back, the count restored.

## Done (2026-10-09)

Inside [[gridspec-object-update-stream]], when its two viewer checks were
asked for through the automation instead of by eye.
`RequestBody::ReadSetting` / `WriteSetting` answered with
`ResponseBody::Setting` (the key, the kind as the store spells it, the value
as bare JSON); the executor coerces a written value to the setting's
declared kind, refuses an unknown key and a wrong kind as invalid, and
writes to the account's scope when the account overrides the setting and to
the machine-wide one otherwise. `Viewer::setting` / `set_setting`;
`sl-viewer-ctl setting <KEY> [VALUE]`, a bare word read as a string.

The draw-distance leg is `e2e_objects`'
`objects_out_of_range_leave_whole_and_come_back`, run on both fake flavours
and both live grids. The terrain question of [[gridspec-terrain]] — whether
either grid holds ground back at a small draw distance — can now be asked
the same way and has not been.
