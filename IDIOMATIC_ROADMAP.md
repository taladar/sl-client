# sl-client idiomatic-safety road map

The protocol surface is broad and complete (client + server, both directions).
This road map closes a different kind of gap: places where the **type system
could prevent misuse** but currently does not, because raw integers, `Uuid`s,
`bool`s, and magic constants carry semantics the compiler can't see. The intent
is to make illegal states unrepresentable — the classic hardening pass for code
conceptually ported from a less type-safe origin (C++/C#).

**Out of scope (already enforced, verified clean):** memory/panic safety. Every
crate enforces ~175 restriction clippy lints (`unsafe_code = "forbid"`,
`unwrap_used`/`expect_used`/`panic` denied, `indexing_slicing` denied,
`as_conversions` denied, `arithmetic_side_effects` denied, `must_use_candidate`
denied, `unused_must_use` forbidden). There is zero `unsafe`, zero
`unwrap`/`expect`/`panic`, bounds-checked wire parsing, and capped allocations.
Do **not** spend effort re-deriving `#[must_use]` or hunting panics — the lints
already own that.

Work the phases top-to-bottom (high-ROI / low-risk first); tick a box only when
the step builds, is clippy-clean under the restriction lints, and
`cargo test --workspace` passes. Add sub-tasks as you discover them.

## Scope reminders

- Commit on the current branch only (never auto-create a feature branch).
- Keep `sl-client-tokio` and `sl-client-bevy` at feature parity (land mirrored
  changes together).
- `sl-types` is normally **consume-only** — new client wrappers live in
  `sl-proto`/`sl-wire`. The only sanctioned `sl-types` *additions* are general
  SL concepts (not client-only): `LindenBalance` (Phase 6) and any new union-key
  enums modelled on `OwnerKey` (Phase 5). List each such addition explicitly.
- SL (Linden Lab) is the primary target; OpenSim is only the safe test grid.

## The per-step refactor pattern

These are refactors, not new capabilities, so the 9-step per-capability pattern
collapses to a uniform sweep. For each type change:

1. **Change the type** in `sl-proto`/`sl-wire` (or consume the `sl-types` type).
2. **Fix every codec site** — encode/decode/conversion in
   `sl-proto/src/session/{conversions,methods,circuit}.rs`, `sim_session.rs`,
   and the wire layer. Wrap/unwrap only at the codec boundary so the wire bytes
   are byte-identical to before.
3. **Fix downstream** — `sl-repl/src/registry.rs` arg parsing and
   `sl-repl/src/format.rs` rendering; `sl-client-tokio/src/lib.rs` and
   `sl-client-bevy/src/lib.rs` (parity).
4. **Tests** — keep the lifecycle + `sim_session` round-trip suites green; add a
   focused unit test that the new type round-trips bit-identically to the old
   raw value.
5. **Book** — update any `book/src/content/*.md` that documents the changed
   field.

Expect to fight the usual restriction-lint gotchas already recorded in the
project memory (`indexing_slicing`, `arithmetic_side_effects`,
`must_use_candidate`, `float_cmp`).

## Phases

### Phase 1 — Permission & flag bitflags (low invasiveness, high ROI)

Caller-facing bitfields are currently raw integers. Follow the existing
`sl-wire/src/parcel_flags.rs` / `control_flags.rs` pattern
(`union`/`contains`/`from_bits`/`bits`; no external `bitflags` crate).

- [x] New `Permissions` bitflags type (the SL `PERM_*` set:
  MODIFY/COPY/TRANSFER/MOVE/…) plus a `Permissions5 { base, owner, group,
  everyone, next_owner }` grouping struct (both in `sl-wire/src/permissions.rs`,
  following the `parcel_flags`/`control_flags` pattern). The five raw `u32`
  masks (`base_mask`/`owner_mask`/`group_mask`/`everyone_mask`/
  `next_owner_mask`) are replaced by one `permissions: Permissions5` field in
  `ObjectProperties`, `InventoryItem`, `RestoreItem` (the `ObjectOwnershipData`
  the roadmap named), and `ObjectPropertiesFamily` (the condensed sibling of
  `ObjectProperties`, which carries the identical five-mask block). Codec sites
  wrap/unwrap at the boundary so the wire bytes are byte-identical. The
  partial-grant masks that are *not* a full `LLPermissions` block —
  `NewInventoryItem.next_owner_mask`, `NotecardRez`'s three masks,
  `Command::UploadAsset`'s three masks, and the `i32` `ObjectPermMasks`
  preferences block — were intentionally left raw (a different concept; not
  five-mask blocks).
- [x] Compressed object-update flags → typed flag newtype
      (`object_update/compressed.rs:10-40`), `.contains()` in place of
      `& MASK != 0`. Added a private `CompressedFlags` newtype (named
      `SCRATCHPAD`…`HAS_PARTICLES_NEW` consts,
      `from_bits`/`bits`/`contains`/`BitOrAssign`, following the
      `parcel_flags`/`permissions` pattern) replacing the eleven module-private
      `COMPRESSED_*` masks; every `& MASK != 0` is now `.contains()` and the
      flags word builds via `|=`. Kept private to the codec (never part of the
      public API); wire bytes are byte-identical (`bits()`/`from_bits` are
      transparent). Unit tests cover the constant values, raw round-trip, and
      `contains`/union.
- [x] Extra-params type flags → typed flag newtype (`extra_params.rs:22-40`).
      Replaced the eight module-private `PARAMS_*` `u16` type-code consts with a
      private `ExtraParamType(u16)` newtype carrying the same named constants
      (`FLEXIBLE` through `REFLECTION_PROBE`) plus a transparent
      `from_code`/`code`. Unlike `CompressedFlags` these codes are mutually
      exclusive (one tag per container entry, not OR-able), so the newtype has
      no `contains`/union: the decoder matches by name
      (`ExtraParamType::SCULPT | ExtraParamType::MESH`) and the encoder writes
      codes by name, dropping the scattered `0x10`/`0x20`/... literals. Kept
      private to the codec (the codes
      only appear inside the raw `ExtraParams` blob); wire bytes are
      byte-identical. A unit test asserts every named code wraps/unwraps to its
      exact wire value.
- [x] Physics flags → typed flag newtype (`extra_params.rs:364-366` →
  `types/object.rs:483-488`), replacing the three reconstructed bools. Added a
  public `ReflectionProbeFlags(u8)` bitflags newtype in
  `sl-wire/src/reflection_probe_flags.rs` (named `BOX_VOLUME`/`DYNAMIC`/`MIRROR`
  consts matching the viewer's `LLReflectionProbeParams::EFlags`, with
  `from_bits`/`bits`/`contains`/`is_empty`/`union`/`difference` +
  `BitOr`/`BitOrAssign`/`BitAnd`/`Not`, following the
  `permissions`/`parcel_flags` pattern). The three
  `is_box`/`is_dynamic`/`is_mirror` bools on `ReflectionProbe` collapse to one
  `flags: ReflectionProbeFlags` field. Unlike the old 3-bool form, the byte
  newtype is byte-identical on round trip even for bits the viewer does not yet
  name (the decode/encode now copy the raw `u8` through). Re-exported via
  `sl-proto`/`sl-client-tokio`/`sl-client-bevy`; example + lifecycle test query
  with `.contains()`. +3 unit tests (named-bit values, raw round-trip incl.
  un-named bits, contains/combinators). **Phase 1 COMPLETE.**

### Phase 2 — Constructor invariants (low invasiveness, caller-facing)

- [x] `Camera::new` (`types/session.rs`): axes must be unit-length and
      orthonormal but it was unchecked. Did the maximal version of both options:
      the old `new` became the `const` `new_unchecked` (the codec-boundary
      constructor — the inbound `AgentUpdate` decode in `sim_session.rs` keeps
      whatever basis the peer sent, so it must reconstruct verbatim, not
      reject), and a *new* validating `Camera::new` returns
      `Result<Self, CameraError>` checking each axis unit-length, the three
      mutually orthogonal, and `at × left = up` (right-handed) — all within a
      small `f32` tolerance (`AXIS_TOLERANCE = 1e-3`). New public `CameraError`
      enum (`NotUnitLength`/`NotOrthogonal`/`NotRightHanded`, `thiserror`),
      re-exported through `sl-proto`/`sl-client-tokio`/`sl-client-bevy`.
      `looking_at`/`region_center` now build via `new_unchecked` (their bases
      are already valid by construction). The REPL `build_camera`
      (`sl-repl/src/registry.rs`) uses the validating `new`, mapping a
      `CameraError` to `ReplError::InvalidArg`. Added module-level
      `dot`/`length` helpers (dedup'd with the test module). +4 unit tests
      (accepts a valid basis; rejects non-unit / non-orthogonal / left-handed).
      Wire bytes unchanged (decode path still uses the unchecked constructor).
- [x] `Throttle::new` (`types/session.rs`): seven positional `f32`s in a
      fixed wire order — easy to transpose. Did the maximal version of every
      option. New public `Kilobits(f32)` newtype (validating `new` →
      `Result<_, ThrottleError>` rejecting NaN/infinite/negative, `const
      new_unchecked` codec-boundary ctor, `ZERO`, `get`). The seven `Throttle`
      fields are now **private** `Kilobits` with `resend()`…`asset()` accessors,
      so a negative/NaN bandwidth can't be set post-construction. The old `new`
      became validating (`Result<_, ThrottleError>`, mirroring the `Camera`
      pattern); a new `const new_unchecked` (used by the presets and the
      `from_bits_per_second` wire-decode) reconstructs verbatim. Added a
      `ThrottleBuilder` (named per-category setters taking already-validated
      `Kilobits`, infallible `const build`) reachable via `Throttle::builder` —
      this is what fixes the transposition hazard. New `ThrottleError`
      (`NotFinite`/`Negative`, `thiserror`). All re-exported through
      `sl-proto`/`sl-client-tokio`/`sl-client-bevy`. REPL `build_throttle`
      (`sl-repl/src/registry.rs`) uses validating `Throttle::new`, mapping a
      `ThrottleError` to `ReplError::InvalidArg`. Wire bytes byte-identical
      (`bits_per_second`/`from_bits_per_second` unchanged in value). +5 unit
      tests (accessor layout, builder == positional `new`, bps round-trip,
      `Kilobits::new` rejects NaN/inf/negative, `new` rejects a bad category).
- [x] `LoginRequest.start` (`sl-wire/src/login.rs`): a `String` constrained to
  `"last" | "home" | "uri:Region&x&y&z"`. Introduced a public `StartLocation`
  enum (`Last`/`Home`/`Region { region: String, position: [f32; 3] }`) with a
  parse-don't-validate `FromStr` (rejecting out-of-grammar values with a public
  `StartLocationParseError` — `Unrecognized`/`MalformedUri`), a
  `to_wire_string()` inverse, and a `StartLocation::region` constructor. The
  `uri:` parser splits the three trailing `&`-coordinates off the right so a
  legal region name survives, and the renderer formats floats exactly as
  Firestorm's `construct_start_string` does (`128.0` → `128`), so wire bytes are
  byte-identical. `LoginRequest.start` is now `StartLocation`; `LoginRequest`,
  `LoginParams`, and `ParsedLoginRequest` drop their `Eq` derive (the float
  position breaks `Eq`, matching `LoginSuccess`/`HomeLocation`). The server-side
  `ParsedLoginRequest.start` is `Result<StartLocation, String>` — parsed into a
  typed location when the (untrusted) client value matches the grammar,
  otherwise the raw string is preserved verbatim (`Err`), so nothing is lost and
  a malformed `start` can't masquerade as a valid location. Re-exported through
  `sl-proto`/`sl-client-tokio`/`sl-client-bevy`; REPL/survey CLIs parse the
  `--start` arg straight into `StartLocation` via clap, examples `.parse()?`
  the `SL_START` env, and the survey relog builds `StartLocation::region`. +4
  unit tests (three wire forms, wire round-trip, `&`-in-region-name,
  out-of-grammar rejection). **Phase 2 COMPLETE.**

### Phase 3 — Intent enums replacing bool / magic-int params (low-medium)

Replace ambiguous `bool`s and magic ints with named enums.

- [x] attachment `add: bool` → `AttachmentMode { Add, Replace }`
  (`command.rs:1492`, `sim_session.rs:291`). Did the maximal version: a new
  public `AttachmentMode` enum in `sl-proto/src/types/appearance.rs`
  (`Add`/`Replace`, `is_add`/`from_add_flag`) replaces the `add: bool` flag on
  **every** attachment carrier — `Command::AttachObject`,
  `ServerEvent::AttachObject`, and `RezAttachment` (the field renamed `add` →
  `mode`) — plus the `AttachmentPoint` helpers (`with_add(bool)` →
  `with_mode(AttachmentMode)`, `split_code` now returns
  `(AttachmentPoint, AttachmentMode)`). `Session::attach_object` /
  `send_object_attach` take `AttachmentMode`; codec wraps at the boundary
  (`with_mode`/`split_code`) so the wire byte (`ATTACHMENT_ADD` `0x80`) is
  byte-identical. Re-exported through `sl-proto`/`sl-client-tokio`/
  `sl-client-bevy`; both runtimes updated at parity. REPL gains a
  `parse_attachment_mode` (accepts `add`/`replace` plus the legacy
  `true`/`false` boolean spelling); `attach_object`/`rez_attachment` take
  `mode=add|replace`, `rez_attachments` records take `[:add|replace]`. Book
  `content/attachments.md` updated. +2 unit tests (mode↔add-flag mapping,
  `with_mode`/`split_code` bit-identical round-trip) and the lifecycle +
  `sim_session` round-trip suites updated. NO sl-types touched (a client
  wire-protocol concept).
- [x] `always_run: bool` → `MovementMode { Walk, AlwaysRun }`
      (`command.rs:1450`, `sim_session.rs:340`). New public `MovementMode` enum
      (`Walk`/`AlwaysRun`, `is_always_run`/`from_always_run_flag`) in
      `sl-proto/src/types/session.rs` (next to `Reliability`) replaces the
      `always_run: bool` on both `Command::SetAlwaysRun` and
      `ServerEvent::SetAlwaysRun` (field renamed `always_run` → `mode`).
      `Session::set_always_run` takes `MovementMode`; the codec wraps at the
      boundary (`mode.is_always_run()` on encode,
      `MovementMode::from_always_run_flag(..)` on decode) so the `SetAlwaysRun`
      wire byte is byte-identical. Re-exported through
      `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (both runtimes updated at
      parity). REPL gains `parse_movement_mode` (accepts `run`/`walk` plus the
      legacy `true`/`false` boolean spelling); `set_always_run` usage is now
      `<mode:run|walk>`. Book `content/appearance.md` updated. +1 unit test
      (mode↔always-run-flag mapping + round-trip) and the lifecycle +
      `sim_session` round-trip suites updated. NO sl-types touched (a client
      wire-protocol concept).
- [x] `first_detach_all: bool` → `DetachOrder` (`command.rs:1531`,
  `sim_session.rs:315`). New public `DetachOrder` enum
  (`DetachAllFirst`/`Keep`, `detaches_all_first`/`from_first_detach_all`) in
  `sl-proto/src/types/appearance.rs` (next to `AttachmentMode`) replaces the
  `first_detach_all: bool` on both `Command::RezAttachments` and
  `ServerEvent::RezAttachments` (field renamed `first_detach_all` → `detach`).
  `Session::rez_attachments` and the `send_rez_multiple_attachments` codec take
  `DetachOrder`; the codec wraps at the boundary (`detach.detaches_all_first()`
  on encode, `DetachOrder::from_first_detach_all(..)` on decode) so the
  `RezMultipleAttachmentsFromInv` `FirstDetachAll` wire bool is byte-identical.
  Re-exported through `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (both
  runtimes updated at parity). REPL gains `parse_detach_order` (accepts
  `detach`/`keep` plus the legacy `true`/`false` boolean spelling);
  `rez_attachments` usage is now `[detach=detach|keep]`. Book
  `content/attachments.md` updated. +1 unit test (mode↔first-detach-all-flag
  mapping + round-trip) and the lifecycle + `sim_session` round-trip suites
  updated. NO sl-types touched (a client wire-protocol concept).
- [x] script-control `take: bool` → `ScriptControlAction { Take, Release }`
  (`types/script.rs:171`). The roadmap's proposed name
  (`ScriptPermissionResponse { Granted, Denied }`) was a misnomer — the cited
  field is `ScriptControl.take`, the `TakeControls` flag on a
  `ScriptControlChange.Data` block (`llTakeControls`/`llReleaseControls`), which
  is take/release of movement controls, *not* a permission grant/deny (the real
  permission answer, `ScriptAnswerYes`, carries a granted-subset *mask*, no
  bool). Renamed (user-approved) to a public `ScriptControlAction` enum in
  `sl-proto/src/types/script.rs` (`Take`/`Release`,
  `takes_controls`/`from_take_controls`) replacing the `take: bool` field on
  `ScriptControl` (renamed `take` → `action`). Codec wraps at the boundary
  (decode `from_take_controls`, encode `action.takes_controls()`) so the
  `ScriptControlChange` `TakeControls` wire bool is byte-identical. Re-exported
  through `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (added `ScriptControl`
  itself to the tokio/bevy re-exports too — it was previously absent there yet
  is needed to name the enum). The REPL only renders
  `Event::ScriptControlChange` as a label (never touches the field), so no REPL
  change. Book `content/appearance.md` updated. +1 unit test
  (action↔take-controls-flag mapping + round-trip); lifecycle + `sim_session`
  round-trip suites updated. NO sl-types touched (a client wire-protocol
  concept).
- [x] magic ints → enums: map-layer constant (`session.rs:43`); consolidate the
  `TELEPORT_FLAGS_*` constants into the existing `TeleportFlags` newtype
  (`types/editing.rs:490`). Two changes. **Map-layer flag:** the bare
  `const MAP_LAYER_FLAG: u32 = 2` (and its sibling `MapBlockRequest` `0`) became
  a new *public* `MapRequestFlags(pub u32)` newtype in `types/map.rs`, modelled
  on `TeleportFlags` (named consts `LAYER` = `2` and `RETURN_NULL_SIMS` =
  `0x0001_0000`, both matching the reference viewer's
  `llworldmapmessage.cpp` `LAYER_FLAG`/`MAP_SIM_RETURN_NULL_SIMS`, plus a
  `contains`). The internal `MapBlockRequest`/`MapNameRequest`/`MapItemRequest`/
  `MapLayerRequest` senders now write `MapRequestFlags::LAYER` (the
  `MapBlockRequest` keeps its `flags: 0` unchanged); the **server-side** surface
  is now typed end-to-end: the four `ServerEvent::Map*Requested { flags }`
  fields, the `SimSession::send_map_{block,item,layer}_reply` params, and the
  `build_map_{block,item,layer}_reply` conversions all take `MapRequestFlags`,
  wrapping at the codec boundary (decode `MapRequestFlags(raw)`, encode
  `flags.0`) so the agent-block `Flags` word is byte-identical. Re-exported
  through `sl-proto` (`types.rs` + `lib.rs`); +3 unit tests (constant values,
  raw round-trip, `contains`). **Teleport flags:** the standalone
  `const TELEPORT_FLAGS_VIA_LURE: u32 = 4` in `session.rs` was deleted and
  folded into the existing `TeleportFlags` newtype — `accept_teleport_lure`
  passes `TeleportFlags(TeleportFlags::VIA_LURE)` and
  `Circuit::send_teleport_lure_request` now takes a `TeleportFlags` (unwrapping
  `.0` at the `TeleportLureRequest` boundary), so the `1 << 2` wire value is
  unchanged (lifecycle test still asserts
  `teleport_flags == TeleportFlags::VIA_LURE`, i.e. `4`). NO sl-types touched
  (both are client wire-protocol concepts). The
  REPL/runtimes are unaffected (the map-reply helpers are server-only and
  `accept_teleport_lure`'s public signature did not change).
- [x] `#[non_exhaustive]` — applied **case-by-case** to 49 public data/value/
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

### Phase 4 — Domain ID newtypes (medium-high invasiveness)

New newtypes in `sl-proto`/`sl-wire` (derive `Copy,Clone,Debug,Eq,Hash`; mirror
`sl-types::Key` ergonomics). Public/caller-facing first:

- [x] `RegionHandle(u64)` with a `grid_coordinates()` decode — public in
  `types/object.rs:57`, `types/terrain.rs:93`, `types/map.rs:181`, plus the
  teleport/`HandoverPending` paths. (Pairs with `map::GridCoordinates` in
  Phase 6.) New public `RegionHandle(pub u64)` newtype in
  `sl-wire/src/region_handle.rs` (`Copy`/`Eq`/`Hash`/`Ord`/`Default`, mirroring
  the `sl-types` key ergonomics) carrying `new`/`get`, the
  `from_global`/`global_coordinates` (metres) and `from_grid`/`grid_coordinates`
  (region indices) packers/decoders, plus `Display`/`LowerHex`/`UpperHex`.
  **Pulled the `map::GridCoordinates` pairing forward from Phase 6 at the user's
  request:** `impl From<GridCoordinates> for RegionHandle` (total) and
  `impl TryFrom<RegionHandle> for GridCoordinates` (fallible — a new public
  `RegionHandleError::GridCoordinateOutOfRange` when a decoded index exceeds the
  `u16` a `GridCoordinates` holds). Replaced the raw `u64` region handle on
  **every** carrier: `Object`, `TerrainPatch`, `MapRegionInfo`, `NeighborInfo`,
  `RegionIdentity`, the six `Event` variants (`TeleportFinished`/
  `RegionChanged`/`TimeDilation`/`ObjectRemoved`/`GltfMaterialOverride`/
  `SoundTrigger`), `RemoteParcelRequest` (sl-wire), the three `Command` variants
  (`Teleport`/`RequestRemoteParcelId`/`RequestMapItems`), the
  `ServerEvent::MapItemRequested` field, `SimSession` (+`new`), and the private
  `Session` state
  (`HandoverPending`, the `regions` map, `teleport_target`).
  `MapItem::region_handle()` now returns `RegionHandle`; the public `Session`
  methods `teleport_to`/`objects_in_region`/`terrain_patches_in_region`/
  `request_map_items` take it. Codec wraps/unwraps at the boundary (decode
  `RegionHandle(raw)`, encode `.0`) so the wire bytes are byte-identical. The
  legacy free functions `handle_to_grid`/`grid_to_handle`/`handle_to_global`/
  `global_to_handle` (public, still raw `u64`) now delegate to the newtype.
  Re-exported through `sl-proto`/`sl-client-tokio`/`sl-client-bevy`; downstream
  `sl-repl` (`SessionContext.region_handle`, the teleport/map/remote-parcel arg
  parsers) and `sl-survey` (unwraps `.0` at the event boundary; its JSON
  `RegionRecord` keeps a raw `u64`) updated. +7 unit tests on the newtype
  (grid/global round-trips, raw packing, the unknown-`0` sentinel,
  `GridCoordinates` round-trip, out-of-range rejection); lifecycle +
  `sim_session` round-trip suites updated. NO sl-types touched (consumed
  `GridCoordinates` only; `RegionHandle` is a client wire concept living in
  `sl-wire`).
- [x] `LocalId(u32)` — public in `types/object.rs:60`, `types/editing.rs:587`;
  re-key `objects: BTreeMap<_, BTreeMap<u32, Object>>` (`session.rs:818`);
  reconcile the parcel `local_id: i32` inconsistency (`types/parcel.rs:170`).
  **Renamed (user-approved) for a self-describing, symmetric pair** — the bare
  `LocalId` was under-descriptive, so two public newtypes live in
  `sl-wire/src/region_local_id.rs` (mirroring `RegionHandle`/`sl-types` key
  ergonomics: `Copy`/`Eq`/`Hash`/`Ord`/`Default`, `new`/`get`, `Display`):
  `RegionLocalObjectId(pub u32)` (the object id, `LLViewerObject::mLocalID`) and
  `RegionLocalParcelId(pub i32)` (the parcel id, `LLParcel::mLocalID`). Keeping
  them as **distinct types with the wire's own signedness** (`u32` vs `i32`) is
  what reconciles the historical `local_id: u32` / `local_id: i32`
  inconsistency, and makes "passed a parcel id where an object id was expected"
  (and vice-versa) a compile error. Maximal scope: replaced **every** object
  region-local id with `RegionLocalObjectId` — `Object.local_id`/`parent_id`,
  `TerseUpdate`, `ObjectBuyItem`, `GltfMaterialOverride` (sl-wire), the object
  cache's inner `BTreeMap` key, `task_local_id`, the `object_local_id` telehub
  fields, `parse_object_physics_properties`'s `(RegionLocalObjectId, _)` pairs,
  the `Event`/`ServerEvent`/`Command` object-id fields **including the plural
  `Vec<u32>`/`&[u32]` lists** (delete/link/delink/select/deselect/duplicate/
  detach/drop/derez/…), and the public `Session`/`SimSession` methods that take
  them — and **every** parcel region-local id with `RegionLocalParcelId`
  (`ParcelInfo`, `ParcelVoiceInfo` + `VoiceProvisionRequest` (sl-wire),
  `ParcelScriptResources` (sl-wire), the parcel-management `Command`/`Event`/
  `ServerEvent` fields, and all the `request_parcel_*`/`reclaim_parcel`/
  `release_parcel`/`buy_parcel_pass`/… method params). Codec wraps at the
  boundary (decode `RegionLocal*Id(raw)`, encode `.0`) so wire bytes are
  byte-identical; the *sequence-number* ack arrays (`record_acks`) and the
  terrain DCT `decopy: &[u32]` were left raw (not ids). Re-exported through
  `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (both runtimes at parity); REPL
  arg parsers parse the raw int then wrap, survey unwraps `.0` for its raw-int
  JSON record. +4 unit tests on the newtypes (raw round-trip incl. the negative
  parcel sentinel, `Display`, the `0` object sentinel); lifecycle +
  `sim_session` round-trip suites updated. NO sl-types touched (client concepts
  in `sl-wire`).
- [x] **Circuit-scoped id wrappers for the user-facing API.** Added two public
  scoped-id structs in `sl-proto/src/scoped_id.rs`:
  `ScopedObjectId { circuit: CircuitId, id: RegionLocalObjectId }` and
  `ScopedParcelId { circuit, id: RegionLocalParcelId }`, plus a new opaque
  **`CircuitId(u64)`** that is the chosen circuit key (user-approved over
  `SocketAddr`/`RegionHandle`). `CircuitId` is a per-establishment **instance
  token** minted from a monotonic `Session` counter every time a circuit is
  established (root at login, each child at `EnableSimulator`, a fresh root on a
  teleport `retarget`); a child promoted to root across a border **keeps** its
  id (same connection). It is deliberately *not* derived from address/region, so
  a reconnect to the same address/region mints a *different* `CircuitId` and a
  stale scoped id fails to resolve — capturing the session/connection scope the
  user correctly identified (a region-local id is only reliably valid for the
  lifetime of the one circuit it was learned on). The four per-circuit caches
  (`objects`/`terrain`/`regions`/`time_dilation`) were **re-keyed from
  `SocketAddr` to `CircuitId`** (user-approved), so the address-reuse-after-
  reconnect hazard is structurally impossible. The wire codec still encodes only
  the bare `RegionLocalObjectId`/`RegionLocalParcelId` (the scope is never
  serialized). Surfacing: the `Object` struct gained a `circuit: CircuitId`
  field (stamped at cache `upsert`) + `Object::scoped_id()`/`scoped_parent_id()`
  accessors; the id-bearing `Event`s now carry the scoped form (`ObjectRemoved`/
  `GltfMaterialOverride`/`ObjectPhysicsProperties`/`ParcelDwell`/
  `ParcelAccessList`), and `Event::CircuitEstablished`/`RegionChanged` gained a
  `circuit: CircuitId` so a caller can track the current circuit. Consuming: the
  ~44 object/parcel `Session` methods and `Session::object` take the scoped form
  and resolve it via `circuit_for_scope` (→ `Error::NoCircuit` if not logged in,
  new `Error::UnknownCircuit` if the circuit is gone/stale; new
  `Error::MixedCircuits` for a batch slice spanning circuits); the matching
  `Command` enum fields are scoped too, with the runtimes forwarding verbatim.
  New `Session::root_circuit_id()` lets a driver build a scoped id from a raw
  id. Re-exported through `sl-proto`/`sl-client-tokio`/`sl-client-bevy`
  (parity); the
  REPL `SessionContext` tracks the current circuit (`$circuitid`, fed from the
  two circuit events) and `registry.rs` scopes freshly typed ids via
  `scoped_object`/`scoped_parcel`/`scoped_objects(ctx, …)`; examples use
  `Object::scoped_id()`. Book `content/world.md` documents the scoping. +5
  scoped-id unit tests and a focused lifecycle test
  (`scoped_object_id_is_circuit_bound`: the right circuit resolves and sends, a
  foreign/stale circuit returns `None` / `Error::UnknownCircuit`); lifecycle +
  `sim_session` suites updated. NO sl-types touched (client concepts in
  `sl-proto`/`sl-wire`).

Then internal bookkeeping IDs (lower misuse surface, do last):

- [x] `CircuitCode(u32)`, `SequenceNumber(u32)` (wrapping helpers),
  `TransferId(Uuid)`/`XferId(u64)`, `PingId(u8)`, `InventoryCallbackId(u32)`.
  Six bookkeeping/correlation id newtypes. Two are wire concepts and live in
  `sl-wire`: **`CircuitCode(pub u32)`** (`sl-wire/src/circuit_code.rs`, the
  login server's per-session code reused by every circuit — explicitly *not* the
  same as the local per-connection `CircuitId`; a separate type) and
  **`SequenceNumber(pub u32)`** (`sl-wire/src/sequence_number.rs`, `FIRST` +
  `wrapping_next` helper). `SequenceNumber` was taken to **full depth** (the
  user-approved maximal option): `ParsedDatagram.sequence`, `.acks`, and the
  `encode_datagram`/`parse_datagram` framing primitives are typed, as is all
  the session-layer ack bookkeeping (`next_sequence`, `pending_acks`, the
  `unacked` `BTreeMap` key, the `SeenWindow` set/queue) in both `Session` *and*
  `SimSession`, plus the public `Diagnostic::ExpectedReplyMissing.sequence`. The
  other four are session correlation ids in
  **`sl-proto/src/bookkeeping_ids.rs`** (all public, re-exported):
  **`PingId(pub u8)`** (`wrapping_next`; `ServerEvent::PingRequested`,
  `SimSession::start_ping_check`), **`XferId(pub u64)`** (the legacy
  file-transfer id; `mute_xfers`/`upload_xfers` keys, the
  `send_*_xfer_*`/`advance_upload` params), **`TransferId(pub Uuid)`** —
  wrapping the **actual wire `LLUUID`** (the `u128` `next_transfer_id` is only
  the minting counter, so the roadmap's literal `(u128)` was wire-incorrect;
  user-approved `TransferId(Uuid)` with a `from_u128` minting helper keys the
  `asset_transfers` map) — and **`InventoryCallbackId(pub u32)`**
  (`Event::InventoryItemCreated.callback_id`, the `InventoryBulkUpdate`
  `(item_id, callback_id)` pairs, and the `create`/`copy_inventory_item` return
  values). The public `Session::circuit_code` accessor now returns
  `Option<CircuitCode>`. Codec wraps/unwraps `.0`/`.get()` at every boundary so
  the wire bytes are byte-identical. Re-exported through
  `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity, including the runtime
  `circuit_code()` accessor + bevy `SlIdentity.circuit_code`); `sl-repl`
  `SessionContext` keeps a typed `circuit_code` and the `set_identity` arg.
  +3 unit tests in `circuit_code`/`sequence_number`, +4 in `bookkeeping_ids`
  (round-trips, wrapping, the `from_u128` mint); lifecycle + `sim_session`
  round-trip suites updated. The wire-spec prose in `book/` is unchanged (these
  are representation-only wrappers — a circuit code / sequence number is still
  the same concept). NO sl-types touched (all client wire/session concepts).
  **Phase 4 COMPLETE — next effort = Phase 5 typed UUID keys from `sl-types`.**

### Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)

`sl-types` exports `AgentKey`, `GroupKey`, `ObjectKey`, `InventoryKey`,
`InventoryFolderKey`, `TextureKey`, `ParcelKey`, `ClassifiedKey`, `EventKey`,
`ExperienceKey`, `FriendKey`, and the `OwnerKey` enum — all `Key(pub Uuid)`
wrappers, so wire conversions are mechanical. Replacing the ~196 raw
`pub …: Uuid` fields across `types/*.rs` with the correct typed key makes
"passed a group id where an agent id was expected" a compile error. Split per
type-family across several commits.

- [x] `AgentKey` sweep (`agent_id`, `prey_id`, `creator` agents, …). Replaced
  every raw `Uuid` field that is unambiguously an **avatar** with
  `sl_types::key::AgentKey`, wrapping at the codec boundary only (wire bytes
  byte-identical). **sl-types change (user-approved via AskUserQuestion, the
  only edit to the shared crate):** added `Copy, Hash` to `Key`, all the `*Key`
  newtypes and `OwnerKey`, plus `pub const fn uuid(&self) -> Uuid` and
  `impl From<Uuid>` on each key (`OwnerKey` got `uuid()` only). `Ord` was
  deliberately *not* added
  (nothing in this family keys a `BTreeMap`/set by an agent; arbitrary on a
  random UUID / variant-first on `OwnerKey`; deferred to the inventory/texture
  families that genuinely need it). Construction idiom `AgentKey::from(uuid)`,
  extraction `key.uuid()`. Converted fields: own agent (`Circuit.agent_id`,
  `SimSession.agent_id`, the `Session`/`SimSession` `agent_id()` accessors,
  `LoginSuccess.agent_id`); IM/chat (`InstantMessage.{from,to}_agent_id`,
  `InventoryOffer.from_agent_id`, the `to_agent_id` `Command`s, `OutgoingIm`);
  presence (`CoarseLocation.agent_id`, `ViewerEffect.agent_id`,
  `ViewerEffectData::{LookAt,PointAt}.source` — *not* `Spiral.source`/any
  `target`, which are objects); tracking (`prey_id`); group
  (`ActiveGroup.agent_id`, `GroupMember.agent_id`,
  `GroupRoleMember`/`GroupRoleMemberChange.member_id`,
  `GroupProfile.founder_id`, `vote_initiator`, the `member_ids:
  Vec`/`&[AgentKey]`); creators (`creator_id`/`creator` on
  inventory/object/editing/pick/classified/event); profile
  (`AvatarProperties.{avatar_id,partner_id}`, `AvatarInterests.avatar_id`);
  directory (`DirPeopleResult.agent_id`, `AvatarPickerResult.avatar_id`); the
  agent-bearing `Event`/`ServerEvent` variants; `ExperienceInfo.agent_id`; the
  server-side `build_map_*_reply`/`send_viewer_effect` agent params;
  `compute_im_session_id`. **Left for later families (deliberately not
  touched):**
  `owner_id`/`last_owner_id`, money `source_id`/`dest_id` (agent-or-group →
  OwnerKey), all `group_id`/`role_id` (GroupKey), object/task ids (ObjectKey),
  `item_id`/`folder_id` (InventoryKey), texture/`insignia`/`snapshot` ids
  (TextureKey), `parcel_id` (ParcelKey), `classified_id` (ClassifiedKey),
  `Friend.id` (FriendKey), chat `source_id`. `ExperienceInfo` lost its `Default`
  derive (AgentKey has no `Default`) → equivalent hand-written impl. Re-exported
  `AgentKey`/`Key` through `sl-proto`, `AgentKey` through
  `sl-client-tokio`/`sl-client-bevy` (parity; `Client::agent_id()` and bevy
  `SlIdentity.agent_id` now `Option<AgentKey>`); REPL parses the raw `Uuid` then
  wraps, survey unwraps `.uuid()` for its raw-`Uuid` records. +1 focused unit
  test (AgentKey↔Uuid bit-identical round-trip; IM `from_agent_id` survives an
  `InventoryOffer` extraction). Build + clippy (--workspace --all-targets) + 678
  tests green.
- [x] `GroupKey` sweep (`group_id`, group membership/role ids). Replaced every
  unambiguous **group** id with `sl_types::key::GroupKey` (already had
  `Copy`/`Hash`/`uuid()`/`From<Uuid>` from the `AgentKey` sweep — no `sl-types`
  change needed). For the **role** ids the roadmap bundled here (`role_id`,
  `owner_role`) a new public **`GroupRoleKey(pub Uuid)`** newtype was added in
  `sl-proto/src/types/group.rs` (user-approved, kept **client-only** in this
  repo rather than `sl-types` — group-role ids never surface in the non-client
  tooling `sl-types` serves; mirrors the `*Key` shape:
  `From<Uuid>`/`uuid()`/`Display`, `Copy`/`Eq`/`Hash`). Keeping group vs role as
  distinct types makes a role↔group mix-up a compile error. Maximal scope:
  converted the `group.rs` carriers (`ActiveGroup.active_group_id`,
  `GroupMembership`/`GroupProfile`/`GroupAccountSummary`/`GroupAccountDetails`/
  `GroupAccountTransactions` `group_id`; `GroupRole`/`GroupRoleMember`/
  `GroupTitle`/`GroupRoleEdit`/`GroupRoleMemberChange` `role_id`;
  `GroupProfile.owner_role`), the `group_id` fields on `AvatarGroupMembership`/
  `DirGroupResult`/`InventoryItem`/`NotecardRez`/`RestoreItem`/object/parcel
  types, **every** group-bearing `Command` (incl. the tuple variants
  `ActivateGroup`/`JoinGroup`/… and the `InviteToGroup` invitees now
  `Vec<(AgentKey, GroupRoleKey)>`), `Event`, and `ServerEvent` variant, and the
  ~30 `Session`/circuit-sender/`SimSession` method params. **Left raw
  (deliberately):** `RequestGroupNotice(Uuid)`/`notice_id` (a notice id),
  `request_id`/`vote_id`/`candidate_id` (proposal/correlation ids),
  `GroupNoticeAttachment.{item_id,owner_id}` (Inventory/Owner families),
  `StartConference.invitees` (agents — `AgentKey` family). Codec wraps at the
  boundary (decode `GroupKey::from`/`GroupRoleKey::from`, encode `.uuid()`) so
  the wire bytes / LLSD `GroupID` fields are byte-identical. The internal
  `OutgoingIm.to_agent_id` was **reverted from `AgentKey` to a plain `Uuid`**
  (it is `pub(crate)`, never public): the `ImprovedInstantMessage` `ToAgentID`
  field is dialog-discriminated — an agent for a 1:1 IM, a group for a group
  notice / group-session message, an ad-hoc session id for a conference message
  — so no single typed key fits, and the prior `AgentKey` typing was a misnomer
  (`send_conference_message` did `AgentKey::from(session_id)`). Callers now
  pass the raw `Uuid` for their dialog (`group_id.uuid()` for the notice,
  `agent.uuid()` for real IMs, the session id verbatim for conferences); the
  public method params stay correctly typed (`group_id: GroupKey`,
  `session_id: Uuid`). `GroupKey`+`GroupRoleKey` re-exported through
  `sl-proto`/`sl-client-tokio`/`sl-client-bevy`; the CAPS group helpers
  (`fetch_group_members`/`fetch_group_experiences` + bevy mirrors) take
  `GroupKey` and unwrap only at the sl-wire `build_group_member_data_request`/
  `group_experiences_query` boundary; REPL parses the raw `Uuid` then wraps,
  survey unchanged. +2 unit tests (`GroupRoleKey`↔`Uuid` bit-identical
  round-trip incl. the nil "Everyone" role; group/role keys are distinct types);
  lifecycle + `sim_session` suites updated. NO sl-types touched.
- [x] `OwnerKey` sweep (`owner_id` — agent-or-group). Replaced the raw
      agent-or-group owner fields with `sl_types::key::OwnerKey`
      **only where the wire actually expresses the union** (a discriminator is
      present); discriminator-less owner ids and **every `last_owner_id`** stay
      raw `Uuid` (no agent/group tag on the wire — same precedent as the
      GroupKey-sweep dialog-discriminated IM field). NO sl-types change
      (OwnerKey already had `Copy`/`Hash`/`uuid()`/`From<Uuid>`/ `is_group()`
      from the AgentKey sweep's 0.4.0). Two wire shapes, both collapsed to
      **one `OwnerKey` field ⇄ the two wire fields on encode** (user-directed —
      no double storage). **Type X — explicit `*_is_group` bool, the id itself
      holds the group when set** (Firestorm: parcel
      `mGroupOwned // true if mOwnerID is a group_id`):
      `MoneyTransaction.source`/`dest` (`is_source_group`/`is_dest_group`),
      `ParcelInfo.owner`, `ParcelObjectOwner.owner`, `LoadUrlRequest.owner`, and
      the sl-wire `ScriptedObjectInfo.owner` (`is_group_owned`) — decode
      `owner_key_from_wire(id, flag)`, encode `owner.uuid()`/`owner.is_group()`,
      no group slot, zero redundancy. **Type Y — group-owned signals via a
      *null* `OwnerID` with the owning group in a separate `GroupID`** (objects
      via the null-convention, inventory via an explicit `GroupOwned` flag):
      `ObjectProperties`, `ObjectPropertiesFamily`, `InventoryItem`,
      `RestoreItem` — `owner: OwnerKey` (the `Group` variant sourced from
      `GroupID`) plus the separate set-to group
      **`group: GroupKey` → `group: Option<GroupKey>`** (`None` = no group set,
      killing the `GroupKey(nil)` footgun — user-requested), the now-redundant
      `group_owned` bool removed. Codec helpers in `sl-proto/src/types.rs`
      (`object_owner_from_wire`/`inventory_owner_from_wire`/`object_owner_to_wire`/
      `group_from_wire`/`group_to_wire`) keep the wire bytes byte-identical,
      incl. the `inventory_item_crc` checksum (recomputed from the wire
      `(OwnerID, GroupID)` pair). `ScriptedObjectInfo` lost its `Default` derive
      → equivalent manual impl. Re-exported `OwnerKey` through
      `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity); REPL
      `build_inventory_item`/`RezRestoreToWorld` keep their keyword grammar and
      recombine into `owner`/`group`, survey unwraps `owner.uuid()`/
      `owner.is_group()` for its raw JSON record. +4 focused round-trip unit
      tests (`owner_codec_tests`) covering both shapes incl. the group-owned
      null path; lifecycle + `sim_session` suites updated. **Left for later
      families (deliberately, no discriminator / different family):**
      `Object.owner_id` (live ObjectUpdate, sound-only, no tag),
      `ScriptDialog.owner_id`, `SoundPreload.owner_id`,
      `RezAttachment.owner_id`, `ChatMessage.owner_id`,
      `DirEventResult`/`PlacesResult`/ `ParcelDetails.owner_id`,
      `GroupNoticeAttachment.owner_id`, `estate_owner_id` (an agent → AgentKey
      family), and `BuyParcel`'s `group_id`+`is_group_owned` (buyer intent, not
      an owner field).
- [x] `ObjectKey` sweep (object/task/`full_id`/`object_id`/`from_task_id`).
  Replaced every raw `Uuid` field that is unambiguously an **in-world object /
  task** (LL's `mFullID`/`ObjectID`/`TaskID`) with `sl_types::key::ObjectKey`,
  wrapping at the codec boundary only (wire bytes byte-identical). NO sl-types
  change needed — `ObjectKey` already had `Copy`/`Hash`/`uuid()`/`From<Uuid>`
  from the AgentKey sweep's 0.4.0. Converted carriers: `Object.full_id`,
  `ObjectProperties`/`ObjectPropertiesFamily` `object_id` + `from_task_id`
  (the `from_task_id` "task" = the object an item was rezzed out of),
  `ParticleSystem.target_id` (documented "target object"), `ScriptDialog`/
  `LoadUrlRequest` `object_id` and `ScriptPermissionRequest.task_id`,
  `NotecardRez` `from_task_id`/`ray_target_id`/`object_id`,
  `AvatarAnimationSource.source_id` (`Option<ObjectKey>`),
  `SoundPreload.object_id`,
  `AvatarAttachment.id`, `LandStatItem.task_id`, `TelehubInfo.object_id`, the
  `ViewerEffectData` `LookAt`/`PointAt` `target` and `Spiral` `source`/`target`
  (explicitly deferred to here by the AgentKey sweep), every object-bearing
  `Event` (`SetFollowCamProperties`/`ClearFollowCamProperties`/`SitResult`
  `sit_object`/`PayPriceReply`/`ScriptRunning`/`ObjectMedia`/`SoundTrigger`
  `object_id`+`parent_id`/`AttachedSound`/`AttachedSoundGainChange`) and the
  cost/physics reply keys (`Event::ObjectCosts`/`ObjectPhysicsData` →
  `Vec<(ObjectKey, _)>`), `Command`/`ServerEvent` object fields (script
  dialog/permissions, cost/physics/selected-cost `object_ids: Vec<ObjectKey>`,
  parcel return/select/disable `task_ids`/`object_ids`, grab-update,
  buy-inventory, pay-price, properties-family, spin, duplicate-on-ray
  `ray_target_id`, the three script-running variants, the three object-media
  variants), the `AbuseReport`/`ObjectMediaResponse`/`MaterialOverrideUpdate`
  sl-wire structs, and the sl-wire
  `build_get_object_cost_request`/`parse_get_object_cost`/`…_response`/
  `build_resource_cost_selected_request`/`parse_resource_cost_selected_request`/
  `build_get_object_physics_data_*`/`build_object_media_*_request` helper
  signatures. The ~40 `Session`/`SimSession`/circuit-sender method params that
  take a persistent object id (not a region-local id) now take `ObjectKey`.
  **Left raw (deliberately):** `ChatMessage.source_id` (agent-*or*-object union,
  discriminated by `source_type` → deferred to the union-key item),
  `DerezObjects.destination_id` (folder-*or*-task union), and the non-object
  families already named for later sweeps (`owner_id`/`last_owner_id`,
  inventory `item_id`/`folder_id`, `texture_id`/`sound`/`asset_id`, `parcel_id`,
  agent/group ids). REPL gains `req_object`/`object_or_nil`/`vec_object` arg
  helpers (parse the raw UUID then wrap); `SessionContext.last_object` is now
  `Option<ObjectKey>`; both runtimes' object-media fetch take `ObjectKey`
  (parity). Book `content/region.md` updated. +1 focused unit test
  (`object_key_round_trips_raw_uuid`: wrap/unwrap is the identity, default
  `full_id` is the nil object key); lifecycle + `sim_session` round-trip suites
  updated. NO sl-types touched.
- [x] `InventoryKey` / `InventoryFolderKey` sweep (`item_id`, `folder_id`).
  Replaced every unambiguous inventory **item** id (LL `mItemID`/`InventoryID`)
  with `sl_types::key::InventoryKey` and every inventory **folder**/category id
  (`mFolderID`, incl. the nil-parent root) with `InventoryFolderKey`,
  wrapping at the codec boundary only (wire bytes byte-identical, incl. the
  `inventory_item_crc` checksum, which unwraps `.uuid()` before `uuid_crc`). NO
  sl-types change — both keys already carry `Copy`/`Hash`/`uuid()`/`From<Uuid>`/
  `Display` from the AgentKey-sweep 0.4.0, so sl-types stayed clean (no version
  bump). Maximal scope across `sl-proto` + `sl-wire`: the type structs
  (`InventoryItem`, `InventoryFolder`, `NewInventoryItem` — which lost its
  `Default` derive → equivalent manual impl, `GestureActivation`,
  `ObjectProperties`, `RestoreItem`, `NotecardRez`, `ScriptPermissionRequest`,
  `GroupNoticeAttachment.item_id` [the Inventory half deferred here by the
  GroupKey sweep], `Wearable`, `RezAttachment`); the id-bearing `Event`
  variants (`InventoryDescendents.folder_id`, `ScriptRunning.item_id`, the
  `InventoryBulkUpdate` `item_callbacks` `Vec<(InventoryKey, _)>`); ~35
  `Command` variants (folder/item CRUD, the `Ais3*` REST ops,
  `BuyObjectInventory`, the three script-running variants, `RemoveAttachment`,
  `UpdateInventoryAsset`, `GiveInventory`/`GiveInventoryFolder`,
  `Accept`/`DeclineInventoryOffer` folders,
  `AcceptFriendship.calling_card_folder`, and the plural `Vec` lists); the
  **inventory/library root folder** chain (`LoginSuccess.inventory_root`/
  `library_root`, `LoginAccount.library_root`, the `Session` state +
  `inventory_root()` accessor → `Option<InventoryFolderKey>`); and the `sl-wire`
  helper signatures (`login.rs` `SkeletonFolder`; `inventory.rs` every AIS3
  URL/body builder+parser, `CreateInventoryCategoryRequest`, and `AisUpdate`'s
  eight folder/item id-lists; `llsd.rs` `build_fetch_inventory_request`/
  `build_group_notice_bucket`/`build_update_item_asset_request`/
  `build_new_file_agent_inventory_request`). Builders stay wire-identical via
  the keys' `Display`; parsers wrap `Key::from`. **Left raw (deliberately):**
  `InventoryOffer.item_id` (an item-*or*-folder union discriminated by
  `asset_type == Folder` → deferred to the union-key item, the same precedent as
  `ChatMessage.source_id` / the dialog-discriminated IM field); every `asset_id`
  (TextureKey/asset family); `transaction_id` (TransferId); the Owner-family
  owner ids (`GroupNoticeAttachment.owner_id`, `RezAttachment.owner_id`, no
  discriminator); and `DerezObjects.destination_id` (a folder-or-task union,
  left raw). Re-exported `InventoryKey`/`InventoryFolderKey` through
  `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity); REPL parses the raw
  `Uuid` then wraps, both runtimes mirrored, examples typed their folder
  trackers.
  +2 focused unit tests (the keys round-trip bit-identically and are distinct
  types over the same uuid; an `InventoryFolder`'s ids survive a round trip
  incl. the nil-parent root). NO sl-types touched.
- [x] `TextureKey` sweep (texture/asset image ids). Replaced every raw `Uuid`
      field that is unambiguously a **texture/image asset** with
      `sl_types::key::TextureKey`, wrapping at the codec boundary only (decode
      `TextureKey::from(..)`, encode `.uuid()`) so the wire bytes are
      byte-identical. NO sl-types change — `TextureKey` already carries
      `Copy`/`Hash`/`uuid()`/`From<Uuid>`/`Display` from the AgentKey-sweep
      0.4.0, so sl-types stayed clean (no version bump). Converted carriers:
      avatar profile imagery (`AvatarProperties`/`ProfileUpdate`
      `image_id`+`fl_image_id`,
      `PickInfo`/`PickUpdate`/`ClassifiedInfo`/`ClassifiedUpdate` `snapshot_id`,
      `AvatarGroupMembership.group_insignia_id`); parcel media/snapshot
      (`ParcelInfo`/`ParcelUpdate` `media_id`+`snapshot_id`,
      `ParcelMediaUpdateInfo.media_id`, the directory-land result
      `snapshot_id`); group insignia (`GroupMembership.group_insignia_id`,
      `GroupProfile`/`CreateGroupParams` `insignia_id`); object surface/light
      textures (`LightImage.texture` projected light,
      `ParticleSystem.texture_id`,
      `ObjectProperties.texture_ids: Vec<TextureKey>`, `TextureFace.texture_id`
      + the `TextureEntry::texture_id()` accessor → `Option<TextureKey>`);
      directory (`PlacesResult.snapshot_id`); map tiles
      (`MapRegionInfo.map_image_id`, `MapLayer.image_id`); script dialog icon
      (`ScriptDialog.image_id`); EEP environment (`SkySettings`
      sun/moon/cloud/bloom/halo/rainbow textures, `WaterSettings`
      `normal_map`+`transparent_texture`); the fetched `Texture.id`; the texture
      pipeline (`Event::TextureNotFound`,
      `Command::RequestTexture`/`FetchTexture`, `Session::request_texture` + the
      `send_request_image` codec, both runtimes' HTTP `GetTexture` fetch fns);
      and the sl-wire `LegacyMaterial.normal_map`+`specular_map` (the
      `RenderMaterials` capability's explicit per-map "texture id"s).
      `ProfileUpdate` lost its `Default` derive (`TextureKey` is not `Default`)
      → equivalent manual impl. **Left raw (deliberately):**
      `SculptData.texture` (a mesh-*or*-texture union discriminated by
      `sculpt_type`'s `MESH` bit — typing it `TextureKey` would be *wrong* when
      it holds a mesh asset; deferred to the union-key item), the GLTF/legacy
      *material* asset ids
      (`RenderMaterialRef`/`TextureFace`/`RenderMaterialEntry` `material_id`,
      `MaterialOverrideUpdate.asset_id` — a material is not a texture), every
      generic `asset_id` and `Asset.id` (variable asset class), and the
      `RegionHandshake` `terrain_detail0..3` (only nil placeholders in the
      generated message blocks, no hand-written typed surface). The
      `texture_downloads` map stays keyed by `Uuid` (`TextureKey` has no `Ord`).
      Re-exported `TextureKey` through
      `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity); REPL parses the
      raw `Uuid` then wraps, examples wrap at the texture-id definition;
      `sl-survey` unaffected (no texture handling). Also folded in a
      user-requested AgentKey-sweep fix: `AvatarAppearance.avatar_id` `Uuid` →
      `AgentKey`. +1 focused unit test (`types::asset`
      `texture_key_round_trips_raw_uuid`: wrap/unwrap is the identity, incl. the
      nil default); lifecycle + `sim_session` round-trip suites updated. NO
      sl-types touched.
- [x] `ParcelKey` / `ClassifiedKey` / `EventKey` / `ExperienceKey` /
      `FriendKey` for the remaining role-specific id fields. Replaced every raw
      role-specific id with the matching typed newtype, wrapping at the codec
      boundary only (decode `Key::from`/`EventId::new`, encode `.uuid()`/
      `.get()`; builders stay wire-identical via `Display`) so wire bytes are
      byte-identical. NO sl-types change — all five `sl-types` keys already
      carry `Copy`/`Hash`/`uuid()`/`From<Uuid>`/`Display` from the AgentKey
      sweep's 0.4.0.
      **`EventKey` was wire-inapplicable → a new repo-local `EventId(pub u32)`
      newtype instead (user-directed):** SL events-directory ids are a numeric
      `u32` (`DirEventResult`/`EventInfo` `event_id`, the three `Event*Request`
      commands), not a UUID, so the `Key(Uuid)`-shaped `EventKey` fits nothing.
      Added a public `EventId` (`new`/`get`/`Display`, modelled on
      `RegionLocalObjectId`) **in `sl-proto`, not `sl-types`**, typing every
      `event_id` across the client `Command`s, `EventInfo`/`DirEventResult`, the
      `Session` event methods + circuit senders, and the server-side
      `ServerEvent::Event*Request` + `SimSession` decode/encode.
      **ParcelKey:** `parcel_id` on `PickInfo`/`ClassifiedInfo`/`PickUpdate`/
      `ClassifiedUpdate`/`DirPlaceResult`/`DirLandResult`/`ParcelDetails`,
      `Event::ParcelDwell`/`RemoteParcelId`, `Command::RequestParcelInfo`/
      `RequestLandResources`, `ServerEvent::RequestParcelInfo`, the
      `request_parcel_info` method + circuit sender, and the sl-wire
      remote-parcel / land-resources codec helpers. **ClassifiedKey:**
      `classified_id` on `AvatarClassified`/`ClassifiedInfo`/`ClassifiedUpdate`/
      `DirClassifiedResult`, the `Command::RequestClassifiedInfo`/
      `DeleteClassified`/`GodDeleteClassified` trio, and the three Session +
      circuit-sender pairs.
      **ExperienceKey:** `ExperienceInfo.public_id`,
      `ExperienceUpdate.public_id` (lost its `Default` derive → manual impl),
      the experience `Event`s + `Command`s, and the full sl-wire experience
      cap codec (client + server helpers); `group_experiences_query(group_id)`
      stays raw (a group id). **FriendKey:** `Friend.id`,
      `Event::FriendsOnline`/`FriendsOffline`/`FriendRightsChanged.friend_id`,
      `Command::GrantUserRights.target`/`TerminateFriendship`, the two Session +
      circuit-sender pairs.
      **Nil-sentinel fields on user-exposed structs became `Option` (and an
      agent-or-group owner an `OwnerKey`), per a user-stated rule:**
      `ExperienceInfo`'s `agent_id`+`group_id` collapsed to
      `owner: Option<OwnerKey>` (`None` = placeholder; the codec splits it back
      to the two wire fields); `ScriptPermissionRequest.experience_id` →
      `Option<ExperienceKey>`; `AvatarProperties.partner_id` →
      `Option<AgentKey>`; `PickUpdate`/`ClassifiedUpdate.parcel_id` →
      `Option<ParcelKey>` (`None` = use the agent's current parcel). Also fixed
      the GroupKey-sweep miss `ExperienceInfo.group_id`.
      Re-exported the keys + `EventId` through `sl-proto`/`sl-client-tokio`/
      `sl-client-bevy` (parity); REPL / runtimes / examples updated. +3 focused
      unit tests; lifecycle + `sim_session` + sl-wire round-trip suites updated
      (691 tests green). NO sl-types touched.
- [x] **Union keys (kept client-local, *not* `sl-types`).** Four discriminated-
  union UUID fields whose referent is one-of-several key kinds. **Scope change
  (user, this session, via AskUserQuestion):** the new unions were kept
  **client-only in `sl-proto`**, NOT added to `sl-types` — the general ones
  (`AgentOrObjectKey`, `InventoryItemOrFolderKey`) may move to `sl-types` later
  bundled with other changes to avoid a `sl-types` release churn; the wire-codec
  ones (`MeshKey`, `SculptOrMeshKey`) are client details. **NO `sl-types` change
  at all.** New `sl-proto/src/types/union_key.rs`: `MeshKey(pub Uuid)`,
  `SculptOrMeshKey { Sculpt(TextureKey), Mesh(MeshKey) }`,
  `AgentOrObjectKey { Agent(AgentKey), Object(ObjectKey) }`,
  `InventoryItemOrFolderKey { Item(InventoryKey), Folder(InventoryFolderKey) }`
  (each with `uuid()`/`is_*`/`Display`, modelled on `OwnerKey`). Per-field:
  (1) **`SculptData.texture`** (`types/object.rs`) `Uuid` → `SculptOrMeshKey`,
  discriminated by the low bits of `sculpt_type` (`== LL_SCULPT_TYPE_MESH` →
  mesh asset, else sculpt texture) in `extra_params.rs` decode/encode; wire
  byte-identical. (2) **`InventoryOffer.item_id`** (`types/chat.rs`) `Uuid` →
  `InventoryItemOrFolderKey`, discriminated by `asset_type == AssetType::Folder`
  in the IM-bucket decode (and the REPL builder). (3) **`ChatMessage`** — the
  `source_id: Uuid` + `source_type: ChatSourceType` pair FOLDED into one
  `source: ChatSource { System, Agent(AgentKey), Object(ObjectKey), Unknown {
  source_type, source_id } }` (with `from_wire`/`source_id`/`source_type_byte`/
  `agent_or_object()→Option<AgentOrObjectKey>`); the server encoder
  `SimSession::send_chat_from_simulator` now takes `ChatSource` (one fewer arg).
  Round-trip lossless (Unknown preserves both bytes; System ⇒ nil id).
  (4) **`DerezObjects.destination_id`** — FOLDED the id INTO the
  `DeRezDestination` variants (user picked this over a union+Option):
  `SaveIntoAgentInventory(InventoryKey)`/`AcquireToAgentInventory`/`Take…`/
  `ForceToGod…`/`Trash(InventoryFolderKey)` carry a folder/item,
  `SaveIntoTaskInventory(ObjectKey)` a task, the rest none; new
  `DeRezDestination::destination_id() -> Uuid` (nil for the id-less ones) feeds
  the wire, and `Command::DerezObjects`/`Session::derez_objects` dropped their
  `destination_id` field/param. Semantics verified against Firestorm
  `derez_objects` call sites + OpenSim `DeRezAction`. Re-exported all five new
  types through `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity); REPL
  derez grammar keeps `<destination> <destination_id>` and recombines.
  +4 focused unit tests (union round-trips; sculpt mesh-vs-texture
  discriminator; folder-vs-item offer; `ChatSource` wire round-trip;
  `DeRezDestination` code+id). Build + clippy (`--workspace --all-targets`) +
  699 tests + `cargo doc` (`-D warnings`) + mdbook green. **NO `sl-types`
  change.**

### Phase 6 — Adopt `sl-types` non-key value types (low-medium)

Already in use: `sl_types::{lsl::Vector, lsl::Rotation, money::LindenAmount,
attachment::*}`. Adopt these more, selectively by semantic role:

- [x] `chat::ChatChannel(i32)` — replaced every raw chat-channel `i32` in
      the typed layer with `sl_types::chat::ChatChannel`, wrapping at the codec
      boundary only (decode `ChatChannel(raw)`, encode `.0`) so wire bytes are
      byte-identical. NO sl-types change — `ChatChannel` already carries
      `Copy`/`Eq`/`Ord`/`Display`/`FromStr` (consumed via the existing path dep,
      no version bump). Converted fields: `Command::Chat.channel`,
      `Command::ReplyScriptDialog.chat_channel`, `ScriptDialog.chat_channel`
      (`types/script.rs`), `ServerEvent::Chat.channel` (`sim_session.rs`), and
      the matching codec/method params (`Session::say` +
      `send_chat_from_viewer`, `Session::reply_script_dialog` +
      `send_script_dialog_reply`, the `script_dialog` conversion decode, the
      `set_typing` channel-`0` call). The sl-wire *generated* message blocks
      (`ChatFromViewerChatDataBlock.channel`,
      `ScriptDialogDataBlock`/`ScriptDialogReplyDataBlock.chat_channel`) stay
      raw `i32` (the wire representation). Left raw (not chat channels):
      `LoginRequest.channel` (the viewer-version string),
      `ReplyScriptDialog.button_index`, voice-channel fields. Re-exported
      `ChatChannel` through `sl-proto`/`sl-client-tokio`/`sl-client-bevy`
      (parity; the runtimes forward the typed `Command` field verbatim, no
      signature change). REPL `chat`/`reply_script_dialog` parse the raw `i32`
      then wrap. Book `content/chat.md` updated. +1 focused unit test
      (`chat_channel_round_trips_raw_i32`, incl. the negative hidden channels
      and `i32::MIN`/`MAX`); lifecycle + `sim_session` round-trip suites
      updated.
      Build + clippy (`--workspace --all-targets`) + tests + `cargo doc`
      (`-D warnings`) + mdbook green. NO sl-types touched.
- [x] `money::LindenAmount` — extended to the non-negative L$ *price*
  fields (maximal scope: every carrier of the named fields, not just the named
  files). Converted: the `EconomyData` price block (`price_energy_unit`,
  `price_object_claim`, `price_public_object_decay`/`_delete`,
  `price_parcel_claim`, `price_upload`, `price_rent_light`,
  `teleport_min_price`, `price_parcel_rent`, `price_group_create` — the `f32`
  multipliers/exponents and the object counts stay raw); `ownership_cost` +
  `sale_price` on
  `ObjectProperties`/`ObjectPropertiesFamily`; `sale_price` on `InventoryItem`,
  `ObjectBuyItem`, `RestoreItem`, `ParcelInfo`, `ParcelUpdate`, `ParcelDetails`,
  `DirLandResult`; `price_per_meter` on `RegionLimits`; `price_for_listing` on
  `ClassifiedInfo`, `ClassifiedUpdate`, `DirClassifiedResult`; the
  `Command::SetObjectForSale.sale_price` field + `Session::set_object_for_sale`
  param. **Design divergence (user-directed, this session):** the roadmap said
  "wrap/unwrap at the codec boundary so wire bytes are byte-identical", which
  implied an *infallible* wrap. Instead the codec is **fallible and does not
  mask** — the user rejected the `LindenAmount(u64::try_from(raw).unwrap_or(0))`
  pattern (it would silently rewrite a malformed negative price to `0`). New
  `WireError::ValueOutOfRange { field, value }` in `sl-wire`; new
  `crate::types::linden_from_wire`/`linden_to_wire` boundary helpers. A negative
  wire L$ value (which a conforming peer never sends) now *rejects* the message:
  the `Result`-returning struct decoders (`economy_data`, `region_limits`,
  `parcel_info`, `classified_info`, `object_properties`, the three
  `inventory_item*` builders) propagate the error so the datagram is dropped
  (and
  surfaced as a `DecodeFailed` diagnostic on the normal path), and the
  `Option`-returning LLSD decoders (`parcel_info_from_llsd`,
  `inventory_item_from_llsd`, `bulk_update_item_from_llsd`) reject via `None`
  (a `CapsDecodeFailed` / dropped item). `try_dispatch_object` became
  `Result<bool, Error>`. On the encode side the same helper rejects a value
  above
  the signed-32-bit wire range rather than clamping, so the public server-side
  `*_to_llsd` encoders (`parcel_info_to_llsd`, `inventory_descendents_to_llsd`,
  `bulk_update_inventory_to_llsd`, `ais_inventory_update_to_llsd`,
  `inventory_item_to_llsd`, `bulk_update_item_to_llsd`) now return
  `Result<Llsd, WireError>`. Wire bytes remain byte-identical for all valid
  (non-negative, in-range) values. **Scope widened (user follow-up): also
  converted the non-negative L$ fields the roadmap didn't name explicitly** —
  parcel `claim_price`/`rent_price`/`pass_price` (`ParcelInfo`/`ParcelUpdate`),
  group `membership_fee` (`GroupProfile`/`CreateGroupParams`),
  `PlacesResult.price`, and the `GroupAccountSummary` non-negative block
  (`total_credits`/`total_debits`/the four `*_tax_current`/the four
  `*_tax_estimate`/`parcel_dir_fee_current`/`parcel_dir_fee_estimate`) — making
  `group_membership`/`group_member` stay infallible but
  `group_profile`/`group_account_summary`/
  `parcel_info`-LLSD fallible and the `PlacesReply`/`GroupAccountSummaryReply`
  server encoders + `parcel_info_to_llsd` encode them via `linden_to_wire`.
  **Left raw (deliberately):** the genuinely *signed* group `balance` (both
  `GroupProfile.money` and `GroupAccountSummary.balance`) and `amount`
  (`GroupAccountDetailsEntry`, `GroupAccountTransaction` — the latter doc'd
  "positive credit, negative debit") fields — those are the *next* roadmap item,
  `LindenBalance`. **Also corrected a pre-existing mislabel:** group
  `contribution` (`GroupMembership`/`GroupMember`) is *not* L$ at all — the wire
  `Contribution` is the member's **land-tier donation in square metres** (the
  viewer renders it as `[AREA]`, confirmed in Firestorm
  `llpanelgrouproles.cpp`/`llfloaterlandholdings.cpp`), so it stays `i32` and
  its "L$" doc comments were fixed. NO sl-types change (consume-only —
  `LindenAmount`
  already has the needed traits). REPL parses the raw `u64` then wraps;
  `sl-survey`'s JSON record `sale_price` is now a `u64` (`info.sale_price.0`);
  both runtimes + the tokio examples updated at parity. Book
  `content/economy.md` updated. +3
  focused unit tests (`linden_from_wire`/`linden_to_wire` round-trip
  bit-identical for non-negative values incl. `0`/`i32::MAX`; negative rejected;
  over-`i32` encode rejected); lifecycle + `sim_session` + conversions
  round-trip
  suites updated (clippy `--all-targets` clean, `cargo doc -D warnings` + mdbook
  green).
  **Follow-ups (same session, user-directed):** (1) **`LandArea(u32)` newtype**
  — the wire carries land *area* (square metres) in the same signed-32-bit slots
  L$ prices use, and group `contribution` was even doc-mislabelled "L$" when it
  is land tier in m² (viewer `[AREA]`, confirmed in Firestorm). Added a public
  `LandArea(pub u32)` (Display "N m²", `Add`/`Sub`, transparent serde) in
  `sl-proto/src/types/land_area.rs` with `land_area_from_wire`/`_to_wire`
  boundary helpers (reject negative, same as the L$ helpers). Typed every
  land-area field: group `contribution` (×2), `MoneyBalance`
  `square_meters_credit`/`_committed`, `ParcelInfo.area`,
  `ParcelDetails`/`PlacesResult`/`DirLandResult` `actual_area`/`billable_area`.
  **Kept client-local in `sl-proto` (NOT `sl-types`)** per the user, to be moved
  to `sl-types` with the other value types in one later batch (avoids version
  churn) — same precedent as the union keys. (2) **Sale prices →
  `Option<LindenAmount>`** (`None` = not for sale, gated on the companion
  `sale_type`/`FOR_SALE` flag/`for_sale` field; a for-sale item may still be
  free; wire `0` is the not-for-sale sentinel) on `ObjectProperties`/
  `ObjectPropertiesFamily`, `InventoryItem`, `RestoreItem`,
  `ParcelInfo`/`ParcelUpdate`/`ParcelDetails`, `DirLandResult`, and
  `Command::SetObjectForSale`; new `linden_price_from_wire`/`_to_wire` helpers
  do the gating. **`ObjectBuyItem.sale_price` was a latent `i32` the first sweep
  missed** (its doc said "advertised" not "the sale price") — fixed to plain
  `LindenAmount` (the bid you must match; lost its `Copy` derive). (3) **Added a
  `serde` dependency to `sl-proto`** (user-directed) so `LandArea` derives
  transparent serde and `sl-survey`'s JSON record carries the typed
  `area: LandArea` / `sale_price: Option<LindenAmount>` directly (was raw
  `u32`/`u64`); JSON output is unchanged (transparent newtypes). The free-fn
  boundary helpers stay `pub(crate)` (not inherent methods) precisely so the
  value types migrate to `sl-types` cleanly without dragging
  `sl_wire::WireError` along — same reason `LindenAmount`'s converter is a free
  fn. +2 focused unit
  tests (`LandArea` round-trip + reject-negative; sale-price for-sale gating).
- [x] **`LindenBalance` (new signed-money type)** — for the legitimately signed
      fields: group `balance`/`amount` and transaction deltas.
      **Kept client-local in `sl-proto` (NOT `sl-types`)** per the user (this
      session's standing rule: new types go local first, batch-migrated to
      `sl-types` later to avoid version churn) — same precedent as
      `LandArea`/the union keys, overriding the roadmap's original "add to
      `sl-types`" note. New public `LindenBalance` in
      `sl-proto/src/types/money.rs`: shape
      `{ negative: bool, magnitude: LindenAmount }` with **private** fields and
      a normalising `new` (zero is canonically non-negative → no negative-zero,
      so derived `Eq`/`Hash` stay consistent with the manual sign-aware
      `Ord`/`PartialOrd`). Arithmetic composes balances and amounts by type:
      `Add`/`Sub<LindenAmount>` and `Add`/`Sub<LindenBalance>` (+ the four
      assign variants), `Neg`, `From<LindenAmount>`, and
      `TryFrom<LindenBalance> for LindenAmount` (errors with a new
      `NegativeBalanceError` when negative). Wire codec is pure inherent methods
      (`from_i32`/`to_i32`/`from_i64`/ `to_i64`; decode is total, encode
      fallible on `i32` overflow) so the type migrates to `sl-types` cleanly
      without dragging `sl_wire::WireError`; the thin `WireError`-wrapping
      boundary helper `linden_balance_to_wire` lives in `sl-proto/src/types.rs`
      next to `land_area_to_wire`. Typed the three signed L$ fields —
      `GroupAccountSummary.balance`, `GroupAccountDetailsEntry.amount`,
      `GroupAccountTransaction.amount` — wrapping at the codec boundary only
      (decode `LindenBalance::from_i32`, encode `linden_balance_to_wire`) so the
      wire i32 is byte-identical. LEFT RAW (deliberately, NOT signed L$):
      the `MoneyTransaction` wire-block amount (the typed
      `MoneyTransaction.amount` is already `LindenAmount`; only the raw
      wire-block integer stays raw, like every wire field) and
      `ResourceAmount.amount` (script memory/url count, not money). Re-exported
      `LindenBalance`+`NegativeBalanceError` through
      `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity). REPL/survey only
      label these events (no field access) → no downstream change. Book
      `content/economy.md` updated (replaced the "awaiting a `LindenBalance`
      type" note with the realised description). +6 focused unit tests (i32 wire
      round-trip incl. `i32::MIN`/`MAX`, negative-zero normalisation, sign-aware
      ordering, by-type arithmetic, `LindenAmount` interconvert,
      out-of-`i32`-range encode → `None`); lifecycle + `sim_session` round-trip
      suites updated. Build+clippy(`--workspace --all-targets`, 0 warnings)+all
      tests+`cargo doc -D warnings`+mdbook green. NO `sl-types` touched.
      **Follow-up (same session, user-spotted `LindenAmount`-sweep MISS):**
      `EventInfo.amount` was raw `u32` but is documented as the event cover
      charge in L$ (wire `Amount` is `U32`, non-negative). Typed it
      `Option<LindenAmount>` gated on the companion `cover` flag (user picked
      the `Option`-gating shape, mirroring the `sale_price` precedent): `Some`
      iff `cover != 0`, `None` otherwise, `None` ⇒ the `0` no-cover wire
      sentinel. New `pub(crate)` boundary helpers
      `linden_cover_from_wire(cover, amount)` (total — `U32` is always in range)
      / `linden_cover_to_wire(field, amount)` (rejects an amount above the `u32`
      wire range) in `types.rs`; wire bytes byte-identical. Book
      `content/search.md` updated; +1 unit test
      (`linden_cover_gates_on_cover_flag`); lifecycle + `sim_session` suites
      updated. No downstream change (REPL/survey only label `EventInfoReply`).
- [x] `map::RegionName(String)` — only for genuine *region* name fields (region
  info / map block replies / teleport). Consumed the existing
  `sl_types::map::RegionName` nutype (validation `len 2..=35` after trim, the SL
  wiki limit — **no `sl-types` change**, consume-only). Audited every `name:
  String` / `sim_name` / `region_name` site and converted the **11 genuine
  region-identity fields** to **`Option<RegionName>`** (`None` = the empty
  "unknown region" sentinel — same precedent as the Phase-5 nil-sentinel
  `Option`s): `RegionIdentity.sim_name`, `RegionLimits.sim_name`,
  `MapRegionInfo.name`, `ParcelDetails.sim_name`, `PickInfo.sim_name`,
  `ClassifiedInfo.sim_name`, `PlacesResult.sim_name`, `EventInfo.sim_name`,
  `ScriptTeleportRequest.region_name`, plus the two sl-wire carriers
  `ParcelVoiceInfo.region_name` and `AbuseReport.abuse_region_name`.
  **Codec boundary is FALLIBLE + non-masking** (user-chosen via AskUserQuestion,
  mirroring the `LindenAmount` precedent): new public sl-wire helpers
  `region_name_from_wire(field, raw) -> Result<Option<RegionName>, WireError>`
  (empty/whitespace → `Ok(None)`; non-empty invalid → new
  `WireError::InvalidRegionName`) and `region_name_to_wire(Option<&RegionName>)
  -> String` (in `sl-wire/src/region_name.rs`), so wire bytes are byte-identical
  for valid names. **A non-empty invalid name is never silently dropped** (user
  requirement): the UDP struct decoders propagate the error up through
  `dispatch`/`handle_datagram` as a **hard error**; `map_region_info` was made
  fallible (`Result<Option<_>, WireError>`) so a bad map-block entry is a hard
  error too (empty/sentinel entries still skip via `Ok(None)`); the caps
  `ParcelVoiceInfo::from_llsd` `None` already routes to a
  `Diagnostic::CapsDecodeFailed`; the server-side caps `parse_send_user_report`
  became `Result<AbuseReport, WireError>`. `region_identity`/`pick_info` were
  made fallible; the empty-string `Default`s on `ParcelDetails`/`AbuseReport`/
  `ParcelVoiceInfo` became `None`. **Left raw (deliberately):** the polymorphic
  `MapItem.name` (region/parcel/event/avatar-hash) and the two *outbound search
  filters* `DirPlacesQuery.sim_name`/`PlacesQuery.sim_name` (possibly-partial
  query strings, not region identities), plus all person/object/inventory/
  estate/event/parcel names. Re-exported `RegionName` +
  `region_name_from_wire`/`region_name_to_wire` through
  `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity); REPL parses the raw
  arg then wraps (mapping a bad name to `ReplError::InvalidArg`); `sl-survey`
  renders the `Option<RegionName>` to its raw-`String` JSON record; examples use
  `{:?}`. +3 boundary unit tests (`region_name.rs`: empty→`None` round-trip,
  valid round-trip, non-empty invalid rejected); lifecycle + `sim_session` +
  voice/abuse round-trip suites updated. NO `sl-types` touched.
- [x] `map` geometry — pairs with `RegionHandle`. Consume-only (no `sl-types`
      change). **`GridCoordinates`:** the redundant `grid_x: u32` /
      `grid_y: u32` pair on `RegionIdentity`, `NeighborInfo`, and
      `MapRegionInfo` collapses to one typed `grid_coordinates: GridCoordinates`
      field. For the two handle-derived carriers a new private
      `grid_coordinates_from_handle` decodes the handle via the Phase-4
      `TryFrom<RegionHandle>` (falling back to the `(0,0)` unknown sentinel);
      for `MapRegionInfo` the wire `u16` pair is primary and the region handle
      is the typed `RegionHandle::from(grid_coordinates)` inverse. Codec wraps
      at the boundary (`map_region_info` decode /
      `map_region_info_to_data_block` encode use `.x()`/`.y()` directly — no
      more `u16::try_from` narrowing) so the `MapBlockReply` `Data` block is
      byte-identical. **`RegionCoordinates`:** the region-local teleport
      *position* (`Command::Teleport.position`, `Session::teleport_to`, and
      `ScriptTeleportRequest.position` — the look-at stays a direction
      `Vector`/tuple) is now `RegionCoordinates`; `teleport_to` unwraps it to
      the wire `Vector` at the `TeleportLocationRequest` boundary, the
      `ScriptTeleportRequest` decode wraps the wire vector, so wire bytes are
      unchanged. Both types re-exported through `sl-proto`/`sl-client-tokio`/
      `sl-client-bevy` (parity); REPL `teleport` wraps the parsed position
      (`RegionCoordinates::from`), `sl-survey` typed its `ARRIVAL_POSITION`
      const + `grid_coordinates.x()`/`.y()` reads (widened to its `u32` bounds).
      +2 focused unit tests (grid/handle consistency,
      `RegionCoordinates`⇄`Vector` round-trip); lifecycle + `sim_session` suites
      updated; `book/src/content/region.md` updated.
      **`map::Distance` (`draw_distance`/`far`): DONE** in the later batched
      `sl-types` migration (see "Batched `sl-types` migration" below) —
      `sl_types::map::Distance` gained a public `new`/`meters` constructor in
      `sl-types 0.5.0`, and `draw_distance` (`Session`/`Circuit` state,
      `set_draw_distance`, `Command::SetDrawDistance`) now carries it, converted
      to the wire `Far` `f32` at the single `AgentUpdate` encode site.
      **`map::Location` and `map::ZoomLevel`: NOT ADOPTED** (user decision, see
      "considered, not adopted") — no matching LLUDP wire field (no map-zoom
      field exists; `Location`'s integer-coord + mandatory-name shape matches
      neither the float region-local teleport positions nor the grid-coord map
      blocks).
- [x] `search::SearchCategory` — **NOT ADOPTED** (no matching wire field), but
      did the genuine adjacent hardening it pointed at: a new local
      `ClassifiedCategory` enum. `sl_types::search::SearchCategory`
      (`All`/`People`/`Places`/`Events`/`Groups`/`Wiki`/`Destinations`/
      `Classifieds`) is the *Search-floater tab* /
      `secondlife:///app/search/ <category>` viewer-URI concept; no single LLUDP
      field carries it (the directory queries express it implicitly through
      *which* message is sent — `DirFindQuery`+flags / `DirPlacesQuery` /
      `DirClassifiedQuery` / `DirLandQuery` — and via the web-search CAP). The
      queries are not even uniform in shape (Places adds a
      `ParcelCategory`+region filter, Land drops `query_text` and adds
      sale-type/price/area and has no `SearchCategory` variant at all, three
      variants — All/Wiki/Destinations — have no UDP query whatsoever), so a
      `SearchCategory`-dispatched API would buy nothing over the typed `Command`
      variants. Same situation as `viewer_uri::ViewerUri` / `map::Location` /
      `map::ZoomLevel` (see considered-not-adopted). The roadmap's parenthetical
      was right: parcel category → already `ParcelCategory`,
      `EventInfo.category` → free-text `String`. **The actual raw `category:
      u32` directory fields are the *classified-ad* category**
      (`Any=0, Shopping=1, Land Rental=2, … Personal=9` — the viewer's
      `panel_dir_classified.xml` combo, a *different closed code set*, not a
      `SearchCategory`), so (user decision: "Reject + ClassifiedCategory") added
      a **new public client-local `ClassifiedCategory` enum** in
      `sl-proto/src/types/avatar_profile.rs` next to `ClassifiedInfo` (mirroring
      `ParcelCategory`: `#[non_exhaustive]`, `#[default] AnyCategory`,
      `Unknown(u32)`, `from_u32`/`to_u32`). Typed every classified-category
      field: `ClassifiedInfo.category`, `ClassifiedUpdate.category`,
      `Command::DirClassifiedQuery.category`,
      `ServerEvent::DirClassifiedQuery.category`, and the
      `Session::dir_classified_query` param. Codec wraps at the boundary (decode
      `ClassifiedCategory::from_u32` in `conversions.rs` / `sim_session.rs`,
      encode `.to_u32()` in `circuit.rs`) so the `Category` U32 wire word is
      byte-identical. **Kept client-local in `sl-proto` (NOT `sl-types`)** per
      the standing rule (new types go local first, batch-migrated later to avoid
      version churn) — same precedent as `LandArea`/`LindenBalance`/the union
      keys. Left raw (deliberately, a different concept): the *object* category
      code (`ObjectProperties`/`ObjectPropertiesFamily.category`,
      `Command::SetObjectCategory`), `EventInfo.category` (free text), the abuse
      `category` (u8). Re-exported `ClassifiedCategory` through
      `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity; the runtimes
      forward the typed `Command` field verbatim). REPL
      `build_classified_update` / the `dir_classified_query` command parse the
      raw `u32` then wrap; the `profile_edit` example builds
      `ClassifiedCategory::Shopping` and prints via `.to_u32()`. Book
      `content/search.md` updated. +1 focused unit test
      (`classified_category_round_trips_raw_u32`: every named code ⇄ wire value,
      `Unknown` verbatim, default = `AnyCategory`/`0`); lifecycle +
      `sim_session` round-trip suites updated. Build + clippy
      (`--workspace --all-targets`) + tests + `cargo doc` (`-D warnings`) +
      mdbook green. NO `sl-types` touched.
- [x] `ChatVolume` ⇄ `ChatType` interop (we keep the richer `ChatType`, don't
  adopt `ChatVolume`). Implemented in `sl-proto/src/types/chat.rs`
  (orphan-rule-legal — `ChatType` is local): `impl
  From<sl_types::chat::ChatVolume> for ChatType` (total, lossless widening:
  `Whisper→Whisper`, `Say→Normal`, `Shout→Shout`, `RegionSay→Region`) and the
  fallible inverse `impl TryFrom<ChatType> for sl_types::chat::ChatVolume`
  (`Whisper→Whisper`, `Normal→Say`, `Shout→Shout`, `Region→RegionSay`; every
  non-volume type — the typing triggers, debug channel, owner, direct, and
  `Unknown(_)` — yields the new public `ChatTypeNotAVolume { chat_type }` error,
  modelled on `NegativeBalanceError`: `thiserror`, `#[non_exhaustive]`).
  `ChatTypeNotAVolume` re-exported through `sl-proto`/`sl-client-tokio`/
  `sl-client-bevy` (parity). +2 unit tests (the four volumes round-trip
  `ChatVolume → ChatType → ChatVolume` identically; the six non-volume types
  each narrow to `ChatTypeNotAVolume`). Consume-only — NO `sl-types` change. No
  downstream/book change (a pure conversion API, no wire field). **Phase 6
  COMPLETE.**
- [ ] Considered, not adopted: `chat::ChatVolume` (richer `ChatType` kept — see
      interop above), `search::SearchCategory` (Search-floater tab / search-URI
      concept with no LLUDP wire field; the raw `category` directory fields are
      the distinct classified-ad code set, now the local `ClassifiedCategory` —
      see the `SearchCategory` item above), `pathfinding::PathfindingType`,
      `viewer_uri::ViewerUri`,
      `radar::Area`, `map::Location` (integer-coord + mandatory-name shape
      matches no wire field — teleport positions are float region-local coords,
      map blocks carry grid coords), `map::ZoomLevel` (no map-zoom field in the
      LLUDP protocol) (no matching protocol field). `map::Distance`
      (`draw_distance`/`far`) was deferred, not rejected — it needed an
      `sl-types` constructor; that constructor was added and `Distance` adopted
      in the batched migration below.

### Batched `sl-types` migration (post-roadmap follow-up)

The value types created **client-local in `sl-proto`** during this pass (under
the standing "new types go local first, batch-migrate later to avoid version
churn" rule) were moved into the shared `sl-types` crate in one release
(`sl-types 0.5.0`), and `sl-proto` now consumes them:

- Moved to `sl-types`: `LindenBalance`/`NegativeBalanceError` (→ `money.rs`),
  `LandArea` (→ `map.rs`), `EventId` + `ClassifiedCategory` (→ `search.rs`),
  `GroupRoleKey`, `MeshKey`, and the three union keys `AgentOrObjectKey`/
  `InventoryItemOrFolderKey`/`SculptOrMeshKey` (→ `key.rs`). Each gained the
  conventions of its sibling (chumsky parsers; serde on `LindenBalance`; the
  single-UUID keys reshaped to wrap `Key`).
- `sl_types::map::Distance` gained a public `new(meters)`/`meters()`
  constructor, and `draw_distance` adopted it (the wire `Far` `f32` conversion
  lives at the single `AgentUpdate` encode site in `sl-proto`).
- Removed the misfit `sl_types::key::EventKey` (the SL event id is a numeric
  `U32`, not a UUID — verified against the viewer); `ViewerUri::EventAbout` now
  carries `EventId`.
- `sl-proto` keeps only the `sl_wire::WireError` codec boundary helpers
  (`land_area_*`/`linden_*`); it re-exports the moved types via
  `pub use sl_types::…` so the flat `sl_proto::…` surface and downstream crates
  are unchanged. `sl-proto`'s `serde` dependency was dropped (its only user,
  `LandArea`, left).

### Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)

A fresh audit after the Phases 1–6 sweeps found three remaining classes of the
same gaps, pursued under three user decisions (via `AskUserQuestion`): (A) raw
ids still in user-facing APIs become typed newtypes — **new ones kept
client-local in `sl-proto`** per the standing rule; (B) **maximal** in-band-nil/
`0`-sentinel → `Option`, with a documented exception list; (C) silently-masked
decode sites become **always a hard `WireError`** on a present-but-malformed
value (absence stays `Option`/default, never an error).

**A — domain-id newtypes (DONE):**

- [x] Name-lookup ids the Phase-5 sweeps missed: `AvatarName.id` → `AgentKey`,
      `GroupName.id` → `GroupKey`, `DisplayName.id` (sl-wire) → `AgentKey`.
      Codec wraps at the boundary (LLSD/UDP byte-identical); `DisplayName` lost
      its derived `Default` (no `AgentKey::default`) → equivalent manual impl.
      (commit "Phase 7 A1")
- [x] New client-local UUID newtypes (mirror the `sl-types` key ergonomics
      `From<Uuid>`/`uuid()`/`Display`): **`PickKey`** (avatar_profile.rs — the
      picks-side parallel of `ClassifiedKey`;
      `AvatarPick`/`PickInfo`/`PickUpdate` `pick_id`, the
      `RequestPickInfo`/`DeletePick`/`GodDeletePick` commands + methods),
      **`GroupNoticeKey`** (`GroupNotice.notice_id` + `RequestGroupNotice`),
      **`ProposalVoteId`** (`GroupActiveProposalItem`/`GroupVoteHistoryItem`
      `vote_id`, the ballot `proposal_id`), **`ProposalCandidateId`**
      (`GroupVote.candidate_id`, a distinct type from `ProposalVoteId`).
      Re-exported through both runtimes; REPL parses raw `Uuid` then wraps; +3
      unit tests. **Left raw (deliberately):** the
      `*_request_id`/`query_id`/`transaction_id` correlation ids and session
      tokens (no entity identity). (commit "Phase 7 A2")

**B — in-band sentinel → `Option` (maximal; IN PROGRESS):**

- [x] EEP textures (`SkySettings` sun/moon/cloud/bloom/halo/rainbow,
      `WaterSettings` normal_map/transparent_texture) → `Option<TextureKey>`;
      new `optional_texture_member`/`optional_texture_to_llsd` LLSD boundary
      helpers. Viewer effects (`ViewerEffectData::{LookAt,PointAt}`
      source/target, `Spiral` source/target) →
      `Option<AgentKey>`/`Option<ObjectKey>`; module-local
      `optional_agent`/`optional_object` decode helpers +
      `map_or_else(Uuid::nil,..)` encode. The REPL gained reusable
      `opt_agent`/`opt_object` arg helpers (absent/nil → `None`) for the rest of
      the sweep. +1 unit test. (commit "Phase 7 B part 1")
- [x] `ParticleSystem.texture_id`/`target_id` → `Option<TextureKey>`/
  `Option<ObjectKey>` (nil → `None` in the `PSYS` blob codec). (commit "Phase 7
  B part 2")
- [ ] **Remaining nil-key fields** (apply the same nil ⇄ `None` boundary
      pattern; wire byte-identical): parcel
      `media_id`/`snapshot_id`/`auth_buyer_id`
      (`ParcelInfo`/`ParcelUpdate`/`ParcelMediaUpdateInfo`/`ParcelDetails` —
      note `snapshot_id`/`media_id` are shared with `PickInfo`/`ClassifiedInfo`/
      `DirLandResult`/`PlacesResult` and read by `sl-survey`, so convert all
      carriers together); `ObjectProperties` `folder_id`/`from_task_id`;
      `TextureFace.material_id`; `Wearable.asset_id`; group
      `ActiveGroup .active_group_id`/`GroupRole.role_id`/`GroupProfile.insignia_id`;
      map `TelehubInfo.object_id`/`MapItem.id`; `ScriptDialog.owner_id` (raw
      `Uuid`); `ChatSource::Object.owner_id`/`InstantMessage.region_id` (raw
      `Uuid`); `InventoryFolder.parent_id` (nil = root); editing
      `RezObjectRequest.group_id`/ `ray_target_id`. (`MuteEntry.id` is
      genuinely-keyed-or-name — see exceptions.)
- [ ] **Remaining numeric `0`/`-1`-means-unset fields** → `Option`:
  `InstantMessage.timestamp`/`Event::ConferenceInvited.timestamp` (0 = unset),
  `ParcelMediaUpdateInfo` `media_width`/`media_height` (0 = native), and the
  `InventoryCallbackId` `0`-no-callback call sites.
- **Exceptions (kept in-band — sentinel is in the value domain):** open enums
  preserving `Unknown(raw)`; the polymorphic `MapItem.name`; outbound search
  filters (`DirPlacesQuery.sim_name`) that are partial query strings, not
  identities.

**C — non-masking decode (always hard error; IN PROGRESS):**

- [x] New `WireError::{InvalidUuid, MalformedField}` + strict
      `parse_uuid_field`/`parse_optional_uuid_field`/`parse_u32_field` helpers.
      Hardened the user-cited masking sites: `estate_info_from_params` (→
      `Result<Option<EstateInfo>, WireError>`; a malformed owner/id/flags/sun/
      parent/covenant-timestamp / a non-empty invalid covenant id rejects the
      whole `EstateInfo`; `EstateInfo`+`EstateCovenant` `covenant_id` became
      `Option<Uuid>` — the B half); `parse_mute_list`/`parse_mute_line` (→
      fallible; a bad UUID/flags line is a hard error, blank still `Ok(None)`);
      `parse_uuid_string` (EventInfoReply `Creator`); `inventory_offer_bucket`
      (rejects an out-of-byte- range asset code instead of writing `0`). +2 unit
      tests. (commit "Phase 7 C part 1")
- [ ] **Verify-then-fix leads** (read to separate real masking from defensive/
      unreachable code, then fix only genuine masking): `terrain.rs`
      `unwrap_or(0)` on a coefficient size / zigzag index (a size/index mismatch
      that silently flattens or reads the wrong coefficient should fail the
      patch decode; the procedural-math intermediates stay clamped);
      `appearance.rs` `decode_texture_entry` per-face `unwrap_or_default`
      (confirm it is LL's intentional default-fill — if so leave it; only a
      genuine length-shortfall errors); the `sl-wire` caps LLSD
      `.unwrap_or_default()` sweep (keep an *absent* optional key lenient, but
      make a *present key of the wrong LLSD type* a hard `CapsDecodeFailed`).
- **Confirmed non-issues (do not touch):** `uuid_crc` `chunks_exact(4)`
  `unwrap_or(0)` (unreachable, for the indexing lint); the documented-sentinel
  `grid_coordinates_from_handle`/`parse_lure_region_handle`; `login.rs` `.ok()`
  → `Option` fields (correct optional handling); the server-side
  `build_map_block_reply` `u16::try_from` clamp (encode of our own data).
