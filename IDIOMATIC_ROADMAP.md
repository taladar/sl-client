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
- [ ] `LoginRequest.start` (`sl-wire/src/login.rs`): a `String` constrained to
  `"last" | "home" | "uri:Region&x&y&z"`. Introduce a `StartLocation` enum
  (parse-don't-validate) with `to_wire_string()`.

### Phase 3 — Intent enums replacing bool / magic-int params (low-medium)

Replace ambiguous `bool`s and magic ints with named enums.

- [ ] attachment `add: bool` → `AttachmentMode { Add, Replace }`
  (`command.rs:1492`, `sim_session.rs:291`).
- [ ] `always_run: bool` → `MovementMode { Walk, AlwaysRun }`
  (`command.rs:1450`, `sim_session.rs:340`).
- [ ] `first_detach_all: bool` → `DetachOrder` (`command.rs:1531`,
  `sim_session.rs:315`).
- [ ] script `take: bool` → `ScriptPermissionResponse { Granted, Denied }`
  (`types/script.rs:171`).
- [ ] magic ints → enums: map-layer constant (`session.rs:43`); consolidate the
  `TELEPORT_FLAGS_*` constants into the existing `TeleportFlags` newtype
  (`types/editing.rs:490`).
- [ ] `#[non_exhaustive]` — apply **case-by-case** to data enums external
  consumers match (`Diagnostic`, `ChatType`, …). **Do NOT** add it to
  `Command`/`Event`: `sl-repl/src/format.rs` matches them exhaustively (no `_`
  arm) on purpose so a new variant fails to compile; `#[non_exhaustive]` would
  defeat that safety net.

### Phase 4 — Domain ID newtypes (medium-high invasiveness)

New newtypes in `sl-proto`/`sl-wire` (derive `Copy,Clone,Debug,Eq,Hash`; mirror
`sl-types::Key` ergonomics). Public/caller-facing first:

- [ ] `RegionHandle(u64)` with a `grid_coordinates()` decode — public in
  `types/object.rs:57`, `types/terrain.rs:93`, `types/map.rs:181`, plus the
  teleport/`HandoverPending` paths. (Pairs with `map::GridCoordinates` in
  Phase 6.)
- [ ] `LocalId(u32)` — public in `types/object.rs:60`, `types/editing.rs:587`;
  re-key `objects: BTreeMap<_, BTreeMap<u32, Object>>` (`session.rs:818`);
  reconcile the parcel `local_id: i32` inconsistency (`types/parcel.rs:170`).

Then internal bookkeeping IDs (lower misuse surface, do last):

- [ ] `CircuitCode(u32)`, `SequenceNumber(u32)` (wrapping helpers),
  `TransferId(u128)`/`XferId(u64)`, `PingId(u8)`, `InventoryCallbackId(u32)`.

### Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)

`sl-types` exports `AgentKey`, `GroupKey`, `ObjectKey`, `InventoryKey`,
`InventoryFolderKey`, `TextureKey`, `ParcelKey`, `ClassifiedKey`, `EventKey`,
`ExperienceKey`, `FriendKey`, and the `OwnerKey` enum — all `Key(pub Uuid)`
wrappers, so wire conversions are mechanical. Replacing the ~196 raw
`pub …: Uuid` fields across `types/*.rs` with the correct typed key makes
"passed a group id where an agent id was expected" a compile error. Split per
type-family across several commits.

- [ ] `AgentKey` sweep (`agent_id`, `prey_id`, `creator` agents, …).
- [ ] `GroupKey` sweep (`group_id`, group membership/role ids).
- [ ] `OwnerKey` sweep (`owner_id`, `last_owner_id` — agent-or-group).
- [ ] `ObjectKey` sweep (object/task/`full_id`/`object_id`/`from_task_id`).
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
