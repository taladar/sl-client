---
id: test-avatar-properties
title: request another avatar's properties
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 7 — Avatar profile & social `[both]`
---

Context: [context/test.md](../context/test.md).

OpenSim needs the UserProfiles fix (see appendix) for profile/picks paths.

`avatar-properties` — request another avatar's properties. `1av`.
Where `profile-edit-roundtrip` edits the agent's *own* profile, this reads a
**different** avatar's — the "open someone's profile floater" lookup.
`Command::RequestAvatarProperties(target)` draws an `AvatarPropertiesReply`
(`Event::AvatarProperties`) carrying that avatar's account-level facts
(account creation date, partner, about text, flags), with an
`AvatarInterestsReply` (`Event::AvatarInterests`) alongside on grids that send
it. The target need not be online — profile data is profile-service state, not
presence — so a single
logged-in avatar reads any account's profile (`1av`). The point (vs
`profile-edit-roundtrip`) is that the reply describes *that other avatar*, so
the case asserts the reply's `avatar_id` equals the requested target and
differs from the logged-in primary, and that the grid returned real account
data (a non-empty `born_on`) rather than the "profile not available"
placeholder. The "other avatar" id is resolved per grid: OpenSim falls back to
the local secondary test avatar (`avatar2`, a fixed-UUID account on the
workspace grid) so no configuration is needed; Second Life has no built-in
second avatar, so the aditi run reads the `other_avatar` configured in
`fixtures.aditi.toml` (a new fixtures field), recording `partial` when that
fixture is absent. Needs the OpenSim UserProfiles module enabled (appendix).
Green on OpenSim: properties RTT ≈ 44 ms loopback, interests reply received,
`born_on` present. `[both]`; the aditi run is deferred with the batch (needs
the `other_avatar` fixture; no aditi record this session).
