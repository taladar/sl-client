---
id: missing-out-batch-8
title: user info & sound
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 8 — user info & sound.** `UserInfoRequest` / `UpdateUserInfo`
(read/write the IM-forwarding & directory-visibility prefs — the outbound
side of the inbound batch-6 `UserInfoReply`), `SoundTrigger` (trigger a
one-shot spatial sound).

Implemented as `Session::request_user_info` (polls the agent's contact
prefs; the reply arrives as the already-handled [`Event::UserInfo`]),
`Session::update_user_info(im_via_email: bool, directory_visibility: &str)`
(the email address is *not* settable over UDP — the `UpdateUserInfo` wire
block carries no email field, only the writable IM-forwarding flag and the
directory-visibility string, so it mirrors the writable subset of
`Event::UserInfo`), and `Session::trigger_sound(sound: AssetKey, gain: f32,
region_handle: RegionHandle, position: RegionCoordinates)` (the viewer→sim
counterpart of the inbound `Event::SoundTrigger`; owner/object/parent ids
are left nil for the simulator to fill in, and it is sent **unreliably** as
the reference viewer's `send_sound_trigger` does — sound triggers are
best-effort). New typed [`DirectoryVisibility`] enum (`Default`/`Hidden`,
`to_wire`/`from_wire`) replaces the free-string search-visibility field — the
reference viewer only ever uses `"default"`/`"hidden"` (one "hide my online
status" toggle), and `from_wire` maps any unrecognised value to `Hidden`
exactly as the viewer's conservative fallback does; it is applied on **both**
the outbound `update_user_info` and the inbound `UserInfo` / `Event::UserInfo`
(the batch-6 inbound field was migrated off `String` for symmetry). The
remaining payloads reuse the typed `AssetKey` / `RegionHandle` /
`RegionCoordinates` for the sound trigger. Wired as
`Command::{RequestUserInfo, UpdateUserInfo, TriggerSound}` through the tokio
and bevy runtimes, the `command_name` formatter, and the `request_user_info`
/ `update_user_info` / `trigger_sound` REPL tokens. Covered by two
pack-the-wire lifecycle tests and two REPL parse tests; user-info read/write
is OpenSim-testable, sound triggering exercises against either grid.
