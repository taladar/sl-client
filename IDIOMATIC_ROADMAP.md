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
- [ ] `InventoryKey` / `InventoryFolderKey` sweep (`item_id`, `folder_id`).
- [ ] `TextureKey` sweep (texture/asset image ids).
- [ ] `ParcelKey` / `ClassifiedKey` / `EventKey` / `ExperienceKey` /
  `FriendKey` for the remaining role-specific id fields.
- [ ] **Union keys → `sl-types`.** Where a field is one-of-several key kinds
  (e.g. a chat/transaction source that is an agent *or* an object), prefer the
  existing `OwnerKey`; when a *new* union is required, add it to
  `sl-types/src/key.rs` next to `OwnerKey` (same `TryFrom`/accessor shape + the
  `OwnerIs*Error` pattern), not as a client-only enum in `sl-proto`. Record each
  new union here as an `sl-types` addition.

### Phase 6 — Adopt `sl-types` non-key value types (low-medium)

Already in use: `sl_types::{lsl::Vector, lsl::Rotation, money::LindenAmount,
attachment::*}`. Adopt these more, selectively by semantic role:

- [ ] `chat::ChatChannel(i32)` — replace raw `channel: i32` /
      `chat_channel: i32` at `command.rs:39,548`, `sim_session.rs:269`,
      `session/circuit.rs:401,1711`, `session/methods.rs:2880,4515`,
      `types/script.rs:25`. (Lowest-risk, high-value; may be pulled forward.)
- [ ] `money::LindenAmount` — extend to the non-negative L$ `i32` fields:
  `economy.rs` price block (`price_*`, `teleport_min_price`),
  `object.rs`/`inventory.rs` `sale_price`/`ownership_cost`, `region.rs:103`
  `price_per_meter`, `avatar_profile.rs` `price_for_listing`.
- [ ] **`LindenBalance` (new signed-money type in `sl-types/src/money.rs`)** —
      for legitimately signed fields: group `balance`/`amount`
      (`group.rs:304,344`) and any transaction delta/refund. Do not leave these
      raw `i32`. Shape: a sign + a `LindenAmount` magnitude
      (`struct LindenBalance { negative: bool, magnitude: LindenAmount }`) with
      standard arithmetic traits so balances and amounts compose by type:
      `Add`/`Sub<LindenAmount>` (+ assign) → `LindenBalance`,
      `Add`/`Sub<LindenBalance>`, `Neg`, `PartialOrd`/`Ord`,
      `From<LindenAmount>`, `TryFrom<LindenBalance> for LindenAmount` (errors
      when negative), plus a signed `i32`/`i64` wire codec. Add the type + tests
      in `sl-types` (minor version bump), then consume it here. (Sanctioned
      `sl-types` addition — general money concept, not client-only.)
- [ ] `map::RegionName(String)` — only for genuine *region* name fields (region
  info / map block replies / teleport). Audit each `name: String` site first; do
  NOT touch person/object/inventory names.
- [ ] `map` geometry — pairs with `RegionHandle`: decode handles to
  `GridCoordinates`/`RegionCoordinates`; `map::Location` for map-block/teleport
  coordinates; `map::Distance` for `draw_distance`/`far` metres
  (`session.rs:692,745`, `sim_session.rs:221`); `map::ZoomLevel` for map zoom.
- [ ] `search::SearchCategory` — for directory category *codes* (note
      `directory.rs` already uses the local `ParcelCategory`;
      `EventInfo.category` is free text and stays `String`).
- [ ] `ChatVolume` ⇄ `ChatType` interop (we keep the richer `ChatType`, don't
  adopt `ChatVolume`). In `types/chat.rs` (orphan-rule-legal — `ChatType` is
  local): `impl From<sl_types::chat::ChatVolume> for ChatType` (total:
  `Whisper→Whisper`, `Say→Normal`, `Shout→Shout`, `RegionSay→Region`) and
  `impl TryFrom<ChatType> for sl_types::chat::ChatVolume` (fallible; non-volume
  types → a new `ChatTypeNotAVolume` error). Round-trip test: `ChatVolume →
  ChatType → ChatVolume` is identity for all four volume variants.
- [ ] Considered, not adopted: `chat::ChatVolume` (richer `ChatType` kept — see
  interop above), `pathfinding::PathfindingType`, `viewer_uri::ViewerUri`,
  `radar::Area` (no matching protocol field).
