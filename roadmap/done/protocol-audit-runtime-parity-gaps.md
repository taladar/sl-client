---
id: protocol-audit-runtime-parity-gaps
title: Re-exports and derived login state reach only the bevy runtime
topic: protocol
status: done
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

## Done (2026-11-26)

- **Re-exports.** One list, `sl_client_common::runtime_reexports!`, expanded
  at both runtimes' crate roots: the union of what the two exported, 706 names
  from `sl_proto`, the three stores, `sl_material`, and the geometry, avatar
  and bake crates. Those crates are light (roxmltree, a TGA-only `image`), so
  tokio carries them too. It is a macro rather than a glob-imported module
  because the Bevy runtime also glob-imports `bevy::prelude`, and two globs
  naming `Camera` or `Color` would be ambiguous. Only `Event` stays per
  runtime: `Event` in tokio, `SlSessionEvent` in Bevy.
- **Derived models.** `ParcelOverlays` (the per-region overlay reassembly
  and its sequencing over the event stream, `apply`) and `AgentParcel` (the
  current parcel, fly permission and seat, `refresh_from` saying whether
  anything changed) live in `sl-client-common`. Bevy's `SlParcelOverlay` and
  `SlAgentParcel` are now resource newtypes that dereference to them. Tokio
  publishes `AgentParcel` over `Client::set_agent_parcel_reporter` (a `watch`
  channel); a tokio consumer folds forwarded events through `ParcelOverlays`.
- **Config.** `AccountDirsConfig` and `resolve_account_directories` moved to
  `sl-client-common`; `Client::set_account_dirs` is the tokio counterpart of
  the plugin's `account_dirs`. `sl-repl-tokio` uses it instead of its own
  copy of the reconcile, and no longer depends on `sl-proto`.
- One behaviour change: a root left `None` in `AccountDirsConfig` now keeps
  that feature's fixed directory, as the field's documentation always said.
  The Bevy code had disabled the feature instead. No host relied on it: the
  viewer sets both roots or neither, and `sl-repl-bevy` leaves the inventory
  cache off.
