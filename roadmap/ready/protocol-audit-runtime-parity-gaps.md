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

**Re-exports.** `sl-client-tokio/src/lib.rs:55` promises "the core types a
consumer needs so they can depend on this crate alone", but re-exports 326
`sl_proto` items against bevy's 358. 41 are bevy-only, including **every**
directory-search result (`DirPeopleResult`, `DirPlaceResult`, `DirLandResult`,
`DirGroupResult`, `DirEventResult`, `DirClassifiedResult`) even though tokio
dispatches the corresponding queries, plus `AvatarAppearance`, `CoarseLocation`,
the `ViewerEffect` trio, `GroupNoticeReceived`, `GroupInvitationReceived`,
`EventInfo`, `EstateInfoUpdate`, `LoginFailure`, `EnvironmentAsset`,
`parse_landmark` and `landmark_to_wire`. Symmetrically `LandEdit`,
`TerraformArea`, `LandBrushAction`, `LandBrushSize`, `ChatSessionInfo`,
`ChatLifecycleView`, `InviteChannel`, `SessionMessage` and `j2c` are tokio-only,
so a bevy consumer cannot construct the `LandEdit` its own terraform command
accepts. Bevy also re-exports `sl_avatar`, `sl_bake`, `sl_prim`, `sl_sculpt` and
`sl_tree`; tokio none of them.

**Derived state.** `SlIdentity` (`sl-client-bevy/src/world.rs:40`) exposes ten
login-derived facts; tokio's `Client` exposes three. Missing on tokio:
`session_id`, `circuit_code`, `seed_capability`, `agent_appearance_service`
(without which a tokio client cannot fetch SL-baked avatar textures at all),
`map_server_url`, `openid_url` and `openid_token`.

**Derived models.** `SlParcelOverlay` (`world.rs:150` — parcel-overlay chunk
reassembly into a `ParcelOverlayGrid`) and `SlAgentParcel` (`world.rs:93` —
current parcel plus derived `can_fly` and seat state) are bevy-only; tokio
forwards raw chunks and every consumer must reimplement the sequencing.

**Config.** The plugin's `account_dirs` option has no tokio counterpart, and
`sl-account-dirs` is not even a dependency of `sl-client-tokio`.

Note this gap is *why* [[protocol-audit-runtime-shared-crate]]'s consumers reach
around: `sl-repl` depends on `sl-proto` directly because it could not get 41 of
those types from `sl-client-tokio` even if it wanted to. Moving the
parcel-overlay reassembly down so both runtimes share it also removes
`sl-repl-tokio.rs:16`'s lone `use sl_proto::Diagnostic;`.
