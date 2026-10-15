---
id: protocol-audit-runtime-parity-gaps
title: Re-exports and derived login state reach only the bevy runtime
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/protocol.md](../context/protocol.md).

The two `Command` dispatchers cover 350 shared variants and every `CAP_*`
constant is referenced by both, so the command table is at parity. The drift is
in the edges.

**Re-exports** (recounted 2026-10-15). `sl-client-tokio/src/lib.rs` promises
"the core types a consumer needs so they can depend on this crate alone", but
re-exports roughly 420 `sl_proto` names against bevy's roughly 490. Still
bevy-only: **every** directory-search result (`DirPeopleResult`,
`DirPlaceResult`, `DirLandResult`, `DirGroupResult`, `DirEventResult`,
`DirClassifiedResult`) even though tokio dispatches the corresponding queries,
plus `AvatarAppearance`, `AvatarName`, `GroupNoticeReceived`,
`GroupInvitationReceived`, `EventInfo`, `EnvironmentAsset` and the sky/water
helpers, `MuteList`, `TelehubInfo`, `parse_landmark` and `landmark_to_wire`, and
a dozen `CAP_*` constants. Tokio-only: `ChatSessionInfo`, `ChatLifecycleView`,
`InviteChannel`, `SessionMessage`, `LoginSuccess`, `LoginResponse`,
`REQUESTED_CAPABILITIES`, `RegionStats` and `j2c`. (`LandEdit`,
`TerraformArea`, `CoarseLocation`, `EstateInfoUpdate` and `LoginFailure` are on
both now.) Bevy also re-exports `sl_avatar`, `sl_bake`, `sl_prim`, `sl_sculpt`
and `sl_tree` items; tokio none of them.

**Derived state.** No longer a gap: tokio's `Client` now exposes `session_id()`,
`root_circuit_id()`, `login_account()` and the whole parsed response through
`login_success()` (circuit code, seed capability, appearance service, map
server, OpenID),
which covers what `SlIdentity` (`sl-client-bevy/src/world.rs:40`) derives.

**Derived models.** `SlParcelOverlay` (`world.rs:150` — parcel-overlay chunk
reassembly into a `ParcelOverlayGrid`, per region) and `SlAgentParcel`
(`world.rs:94` —
current parcel plus derived `can_fly` and seat state) are bevy-only; tokio
forwards raw chunks and every consumer must reimplement the sequencing.

**Config.** The plugin's `account_dirs` option has no tokio counterpart, and
`sl-account-dirs` is not even a dependency of `sl-client-tokio`.

Note this gap is *why* [[protocol-audit-runtime-shared-crate]]'s consumers reach
around: `sl-repl` depends on `sl-proto` directly because it could not get the
bevy-only types from `sl-client-tokio` even if it wanted to. Moving the
parcel-overlay reassembly down so both runtimes share it also removes
`sl-repl-tokio.rs:16`'s lone `use sl_proto::Diagnostic;`.
