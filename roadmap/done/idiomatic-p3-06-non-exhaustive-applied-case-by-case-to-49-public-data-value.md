---
id: idiomatic-p3-06
title: `#[non_exhaustive]` applied case-by-case to 49 public types
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 3 — Intent enums replacing bool / magic-int params (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`#[non_exhaustive]` — applied **case-by-case** to 49 public data/value/
error enums in `sl-proto`/`sl-wire` that model open-ended protocol value sets
(LL may add values) or are caller-facing error types, so downstream crates
must add a `_` arm and a future variant is a non-breaking change.
**Included:** the chat enums
(`ChatType`/`ChatSourceType`/`ChatAudible`/`ImDialog`),
`Diagnostic`, asset/inventory (`AssetType`/`InventoryType`/`ImageCodec`/
`TransferStatus`), `WearableType`/`AttachmentPoint`, editing
(`ClickAction`/`Material`/`SaleType`/`DeRezDestination`/`PermissionField`/
`Maturity`/`ProductType`), parcel (`ParcelRequestResult`/`ParcelStatus`/
`LandingType`/`ParcelMediaCommand`/`ParcelCategory`/`LandStatReportType`),
`TerrainLayerType`, `MoneyTransactionType`, map
(`EstateAccessDelta`/`EstateAccessKind`/`MapItemType`), nearby
(`ViewerEffectType`/`LookAtType`/`PointAtType`/`ViewerEffectData`),
`GroupRoleUpdateType`, script (`FollowCamProperty`/`MuteType`),
`MeanCollisionType`, `DisconnectReason`, sl-wire (`PhysicsShapeType`/
`SelectedCostKind`/`AbuseReportType`/`ExperiencePermission`),
and the caller-facing error enums (`Error`, `WireError`, `ThrottleError`,
`CameraError`, `StartLocationParseError`, `LoginParseError`). **Excluded
(case-by-case):** `Command`/`Event`/`ServerEvent` (dispatch enums deliberately
matched exhaustively — `sl-repl/src/format.rs` for the first two — as a
compile-time safety net); `LoginResponse` (matched exhaustively in
`sl-proto::handle_login_response` — a new login outcome *should* fail to
compile, not silently fall through a `_`); the closed wire-spec structural
enums `Llsd`/`MessageId`/`StartLocation`; and the closed binary client-intent
enums `Reliability`/`MovementMode`/`AttachmentMode`/`DetachOrder`/
`ScriptControlAction`/`ParcelAccessScope`/`GroupRoleChange` (a 2-value set
will not grow). Only three external match sites needed a new `_` arm
(`sl-repl/src/format.rs` `write_diagnostic`, `sl-survey` `maturity_str`/
`product_str`); `wildcard_enum_match_arm` is not among the enabled restriction
lints so `_` arms are clippy-clean. NO sl-types touched. **Phase 3 COMPLETE.**
