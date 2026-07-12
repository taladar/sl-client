---
id: test-profile-edit-roundtrip
title: update profile / interests; read back
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 7 — Avatar profile & social `[both]`
---

Context: [context/test.md](../context/test.md).

`profile-edit-roundtrip` — update profile / interests; read back. `1av`.
Where `avatar-properties` reads a **different** avatar's profile, this is the
"edit my own profile floater" round-trip: it reads the agent's current
profile and interests, writes a changed copy back, and confirms a fresh read
reflects the edit. Both `Command::UpdateProfile` (`AvatarPropertiesUpdate`)
and `Command::UpdateInterests` (`AvatarInterestsUpdate`) *replace the whole
record*, so the case reads the current values first and edits from there
rather than blanking unrelated fields (the publish/mature booleans are
reconstructed from the read profile `flags`). Neither update carries an ack,
so the edit is verified by polling a fresh `RequestAvatarProperties` read
until the new value appears; the about-text and interests markers *toggle*
between two fixed values keyed off what was just read, so every re-run is a
real, detectable change and an interrupted run self-heals. After asserting the
edit, the case writes the originals back so it leaves the profile as it found
it. Live finding: a single `RequestAvatarProperties` yields the properties
**and** the interests reply that follows, so the read-back must consume both
together — reading the interests apart races the queued replies and reads a
stale value. `1av`, `[both]`. When a grid omits interests that half is
recorded `partial`, not failed. OpenSim needs the UserProfiles module enabled
(appendix). Green on OpenSim: about-text + interests edits both reflected,
re-read RTT ≈ 60 ms loopback, profile restored. The aditi run is deferred with
the batch (no aditi record this session).
