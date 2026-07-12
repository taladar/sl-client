---
id: viewer-p14-4
title: Refresh on rebake
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 14 — Server-published baked texturing (incl. alpha)
---

Context: [context/viewer.md](../context/viewer.md).

**P14.4. Refresh on rebake.** Re-request bakes on `RebakeAvatarTextures`
and on a newer `cof_version` in a later `AvatarAppearance`.

**Done.** Two refresh triggers were wired up. (1) *Our own avatar,
`RebakeAvatarTextures`:* `appearance.rs`'s `drive_server_bake` now tracks
whether the central-baking `UpdateAvatarAppearance` capability was ever
offered (`ServerBakeState.cap_available`), and on an
`Event::RebakeAvatarTextures` — the simulator telling us it lost one of our
baked textures — re-runs the one-shot server-bake handshake from `Done`
(re-query the COF version → re-POST the bake) so the grid re-composites and
re-publishes our appearance. A rebake arriving mid-handshake is ignored (the
in-flight bake satisfies it), and without the capability (OpenSim) it is
inert. (2) *Any avatar, newer `cof_version`:* `ingest_avatar_bakes` re-fetched
on every `AvatarAppearance` already; it now gates on the COF version
(`AvatarState.baked_cof_version` + `should_refetch_bakes`) so a later
appearance whose `cof_version` is *strictly older* — an out-of-order /
duplicate resend — is skipped and cannot clobber a newer bake, while a newer
*or equal* version still re-fetches (equal covers a same-outfit rebake
republishing new baked ids at the same version) and an appearance with no
`cof_version` (OpenSim / the older path) always ingests. Unit-tested
(`should_refetch_bakes` cases); no library-surface change (viewer-internal —
the `RebakeAvatarTextures` event and `cof_version` field already existed and
are re-exported wholesale). The triggers are sim-initiated / outfit-change
driven and cannot be forced deterministically, so the unit-tested gate is the
guarantee, as with P14.3.
