---
id: test-avatar-notes
title: write and read avatar notes
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 7 — Avatar profile & social `[both]`
---

Context: [context/test.md](../context/test.md).

`avatar-notes` — write and read avatar notes. `1av`.
The private, per-account free-text note a viewer keeps about *another* avatar
(the profile floater's "My Notes" box) — profile-service state keyed on the
pair (viewing agent, target), never shown to the target and independent of
presence, so one logged-in avatar drives the whole round-trip (`1av`). A read
is `Command::RequestAvatarNotes` (the `avatarnotesrequest` `GenericMessage`)
answered by an `AvatarNotesReply` (`Event::AvatarNotes`); a write is
`Command::UpdateAvatarNotes` (`AvatarNotesUpdate`), which carries no ack, so
the edit is verified by polling a fresh read until the new text appears. The
note *toggles* between two fixed markers keyed off the last read, so every
re-run is a real, detectable change and an interrupted run self-heals; after
asserting, the case writes the original back to leave the profile as it found
it. The "other avatar" is resolved per grid like `avatar-properties`: OpenSim
falls back to the local secondary (`Friend Tester`, fixed UUID); Second Life
reads the `other_avatar` fixture, `partial` if absent. **Live OpenSim finding
(worked around, not fixed):** stock OpenSim leaves the `avatarnotesrequest`
query *unanswered* — the same unresponsive-`GenericMessage` class
`picks-classifieds` documented — and, unlike picks, `AvatarNotesUpdate`
volunteers no reply either, so the note is never readable back on OpenSim. The
case detects the silence (the initial read times out), still pushes a write so
the `AvatarNotesUpdate` encoding is exercised on the wire, and records
`partial`; the read-back round-trip is only assertable on a grid that answers
the query. Unlike every prior Phase 7 case this one is `partial` (not green)
on OpenSim — `notes_read_answered=false`, write exercised, no positive
read assertion. Needs the OpenSim UserProfiles module enabled (appendix).
`[both]`; the aditi run — where the full toggle → write → re-read → assert →
restore round-trip runs green — is deferred with the batch (no aditi record
this session).
