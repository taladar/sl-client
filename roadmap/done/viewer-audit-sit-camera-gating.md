---
id: viewer-audit-sit-camera-gating
title: The scripted sit camera arms on any SitResult and never clears forced mouselook
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

Two defects in `sl-viewer-world-view/src/sit_camera.rs`:

- `:120` — the scripted sit camera arms on **any** `SitResult`, with no
  "actually sitting" gate (no `autopilot` check, though `SitResult` carries one;
  the reference conjoins `isSitting()` on both branches).
  `clear_sit_camera_on_stand` (`:156`) only fires on a seated-to-unseated edge,
  so a **cancelled** sit welds the camera to the seat indefinitely.
- `:139` — `forced_mouselook` is write-only: set inside `if *force_mouselook {`
  with no `else` clearing it. A sit-to-sit hand-off (A forces, B does not, no
  stand between) leaves it armed, and standing from B then steals the user's own
  mouselook choice.

`sit_camera.rs` has zero tests; `ingest_sit_result` is testable as-is with
`MinimalPlugins` + `add_message::<SlEvent>()`, the pattern `session.rs:548`
already uses.

## Fixed (2026-09-14)

**A reply is not a sit.** `ingest_sit_result` now only *arms*: it records the
reply's offsets and, in `force_mouselook_seat`, which seat asked for mouselook.
Nothing moves the camera there. That mirrors the reference, whose
`process_avatar_sit_response` likewise only records both (`setSitCamera` /
`setForceMouselook`) and leaves acting on them to `LLVOAvatar::sitOnObject` and
the two `isSitting()`-conjoined consumption sites.

**What "actually sitting" is here.** The new `seated_on` helper conjoins two
signals, and both are load-bearing. `SlAgentParcel::seated_on` names the seat
but is the session's own optimism — `SitState::Seated` is set the moment the
`AvatarSitResponse` arrives (`session/methods.rs:3496`), before any walk and
whether or not the simulator ever honours the sit. `AvatarState::is_seated` is
the avatar's own object stream (a non-zero `ParentID` — the very signal that
draws the avatar on the seat), so it says the sit happened but not, for the
camera's purposes, on what. Together they answer "sitting, on this seat", which
is strictly more than the reference's `isSitting()`: a reply for seat B cannot
engage while the avatar rides seat A. The reply's `autopilot` flag is *not* the
gate — it says the sit needs a walk first, not whether the walk arrived.

`engage_sit_camera` (new, between the ingest and the stand systems) puts the
armed camera in force and enters the forced mouselook when that holds; a
cancelled sit, a sit still being walked to, and a sit the simulator never
honoured all stay out of it, and the stand path now also fires when a script
unseats the avatar under a session that still believes it is seated.

**The mouselook claim lasts as long as the mouselook.** Two fields where there
was one write-only bool: `mouselook_applied_for` (the seat already dropped into
first person, so it happens once per sit and a user who leaves mouselook while
seated is not shoved back in) and `forced_mouselook` (whether the mouselook
*currently* on screen is ours), which lapses the moment the camera leaves it by
any other route. Standing therefore restores third person only for a mouselook
this module put the user in — a mouselook they chose themselves, before or after
a hand-off, is theirs to keep.

Unit-verified, six tests in `sit_camera.rs` (it had none):
`a_reply_without_a_sit_moves_no_camera`,
`taking_the_seat_engages_the_camera_and_standing_clears_it`,
`an_unhonoured_sit_does_not_engage`, `another_seat_does_not_engage_this_reply`,
`a_handoff_does_not_steal_the_users_mouselook` and
`a_seat_without_a_scripted_camera_engages_nothing`.
