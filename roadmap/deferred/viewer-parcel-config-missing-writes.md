---
id: viewer-parcel-config-missing-writes
title: About Land — the controls that have no protocol write path yet
topic: viewer
status: deferred
origin: About Land floater follow-up (2026-07-28)
refs: [viewer-parcel-options-general, viewer-parcel-options-access-media]
---

Context: [context/viewer.md](../context/viewer.md).

The nine-tab About Land floater ([[viewer-parcel-options-general]] /
[[viewer-parcel-options-access-media]]) shows every reference control, but a
handful have **no write path in the current protocol surface**, so they are
rendered **read-only** (a disabled control reflecting the grid's value) rather
than editable. This task is to add the missing wire support and then make them
editable:

- **Media type / description / size (width × height) / loop** — these live in
  `ParcelMediaUpdateInfo` (read, via `Event::ParcelMediaUpdate`) but not in
  `ParcelUpdate`; there is no media-update command. Needs the
  `ParcelMediaCommandMessage` / media-update write path.
- **Avatar-sound restrictions** (`any_av_sounds` / `group_av_sounds`) — present
  on `ParcelInfo` (read) but absent from `ParcelUpdate`.
- **`ObscureMOAP`** (restrict media-on-a-prim to the parcel) — neither read nor
  written in the surface.
- **Experiences tab** — there is **no per-parcel experience allow/block
  message**: `ParcelAccessScope` only encodes access / ban, not the experience
  sub-lists (`ParcelAccessFlags::ALLOW_EXPERIENCE` / `BLOCK_EXPERIENCE`), and no
  parcel-experience request exists. The tab is a stub note today.
- **Environment tab editing** — `RequestEnvironment { parcel_id }` reads a
  parcel's EEP, but there is no `Set`/`UpdateEnvironment` command, and
  per-parcel EEP editing is the separate environment-editor subsystem anyway.
  The tab shows a read-only summary today.

Reference (Firestorm, read-only): `llpanellandmedia`, `llpanellandaudio`,
`panel_region_experiences.xml`, `panel_region_environment.xml`.

## Parity-audit addendum (2026-08-19)

Parity-audit addition: the Options tab's **SeeAvatarsCheck** ("Avatars
on other parcels can see and chat with avatars on this parcel"). We
have no such control at all in `sl-client-bevy-viewer/src/about_land.rs`;
the read exists (`ParcelInfo.see_avs`, `sl-proto/src/types/parcel.rs`)
but `ParcelUpdate` lacks the field — the same caps-side
ParcelPropertiesUpdate class as the avatar-sound toggles this task
already lists, so it belongs here.
