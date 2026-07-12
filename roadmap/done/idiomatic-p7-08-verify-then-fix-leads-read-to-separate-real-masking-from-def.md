---
id: idiomatic-p7-08
title: Verify-then-fix leads** (read to separate real masking from defensive/
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

**Verify-then-fix leads** (read to separate real masking from defensive/
    unreachable code, then fix only genuine masking). Outcome of the audit:
    - `terrain.rs` `unwrap_or(0)` on the coefficient size / zigzag index
    (`decompress_patch` line 288-289, `decode_patch_data` line 250):
    VERIFIED defensive/unreachable, LEFT AS-IS.
    `decopy`/`dequantize`/`coefficients` are all sized to
    `total = size*size`, the zigzag step `count` is bounded by `total`, and
    `coefficients` is `resize`d to `total`, so the index can never exceed
    the slice and `i32::try_from(unpack(word_bits<=17))` never overflows —
    the `unwrap_or` only satisfies the no-`as`/no-indexing lints. No real
    size/index mismatch is representable, so there is nothing to make fail.
    (The procedural-math `unwrap_or(0.0)` intermediates stay clamped.)
    - `appearance.rs` `decode_texture_entry` per-face `unwrap_or_default`
    (lines 112-122): VERIFIED LL's intentional default-fill, LEFT AS-IS. The
    eleven per-face arrays are each `vec![default; count]` and the final
    `(0..count).map(|index| …get(index)…)` only ever indexes in range, so
    the `unwrap_or` is unreachable (indexing-lint only). The genuine
    default-fill for a truncated blob happens inside `unpack_field`
    (documented LL behaviour), not at these sites.
    - The **`sl-wire` caps LLSD `.unwrap_or_default()` sweep**: DONE (the real
    masking). New fallible `Llsd` accessors `field_i32`/`f64`/`f32`/`bool`/
    `uuid`/`str`/`binary`/`array`/`map(key, label) -> Result<Option<T>,
    WireError>` + `Llsd::kind()` in `llsd.rs`: an absent/`Undef` key stays
    lenient (`Ok(None)` → the caller's existing default), but a
    **present key of the wrong LLSD kind** is now
    `Err(WireError::MalformedField{field, value})` instead of a silent
    default. Swept every
    `…get(key).and_then(Llsd::as_T) .unwrap_or(default)` site across
    `sim_features`, `agent_preferences`, `object_cost`, `object_physics`,
    `resource_report`, `remote_parcel`, `voice`, `display_name`,
    `inventory`, `material/{legacy,gltf}`, `llsd.rs`
    (`MediaEntry`/`ObjectMediaResponse` for `CAP_OBJECT_MEDIA` + the
    `llsd_bool`/`llsd_int`/`llsd_string`/`llsd_perm` helpers), the inline
    `CAP_MODIFY_MATERIAL_PARAMS` handler, and the `experience/*` parsers
    (≈30 sites); the ≈27 affected `parse_*`/`from_llsd` functions became
    `Result<_, WireError>`. The client `handle_caps_event` dispatch routes
    each `Err` to `caps_decode_failed` (the `Diagnostic::CapsDecodeFailed`
    the roadmap named); the runtimes' direct callers
    (experiences/land-resources) skip on `Err`. The absent-vs-required
    distinction is the *caller's* (see the required-field pass below).
    **Scope
    note:** the shared helpers made the *server-side* XML request parsers
    fallible too, so `experience/server.rs`, `voice.rs`, and the
    `inventory.rs` AIS parsers changed their error type from
    `roxmltree::Error` to `WireError` (malformed XML now surfaces as
    `WireError::MalformedField` carrying the parse error). The
    `EventQueueGet`/seed XML envelope parsers (`parse_event_queue_response`,
    `parse_seed_response`) were LEFT on `roxmltree::Error` — transport
    envelopes, not caps-body decoders, and converting them would only widen
    the `roxmltree → WireError` churn. Wire bytes / lenient-on-absent
    behaviour unchanged; +1 focused `llsd.rs` unit test (absent → `Ok(None)`,
    right kind reads, wrong kind → `MalformedField`). NO `sl-types` change.
    - **Required-field hardening (strict, user-directed follow-up):** absent
    is no longer *always* lenient — a **spec-mandatory** field that is
    absent is now a hard error too. New `WireError::MissingField{field}` +
    nine `Llsd::require_T(key, label) -> Result<T, WireError>` accessors
    (absent → `MissingField`, present-wrong-kind → `MalformedField`). Three
    tiers per decoder: (a) an absent field that means the WHOLE reply is "no
    result" keeps the `Result<Option<Struct>>` shape and yields `Ok(None)` —
    never a half-populated struct (`ParcelVoiceInfo` no-voice,
    `parse_remote_parcel_ reply` unresolved); (b) a present struct missing a
    mandatory field → `require_T` (`MissingField`); (c) a present struct
    missing an optional field → `field_T?.unwrap_or(default)`. Each field
    was classified strict but **evidence-based**, cross-checking the
    Firestorm reader and the OpenSim emitter; "no evidence it is always
    sent" → left optional. Promoted to required: `ObjectCost` cost quartet +
    `SelectedResourceCost` physics/streaming/simulation;
    `ObjectPhysicsData.PhysicsShapeType` (+ the
    density/friction/restitution/gravity group when `Density` is present)
    and the event's `ObjectData`/`LocalID`; `DisplayName`
    id/username/legacy-first/legacy-last; conditional voice fields
    (`jsep.type`/`sdp`/`viewer_session` when a WebRTC answer is present,
    Vivox `password` when `username` is present, `channel_uri` when
    `voice_credentials` is present); `ExperienceInfo.public_id` (via a
    string-or-uuid `require` that preserves the historical tolerance);
    `create_inventory_category` folder_id/parent_id/name;
    `ObjectMediaResponse.object_id` (its `from_llsd` became `Result<Self>` —
    an absent object id is a malformed reply, not "no result"); the
    `resource_report` identity/amount fields (ScriptedObjectInfo
    id/owner_id, ParcelScriptResources id/local_id, ResourceAmount
    type/amount). No field was made required in the feature-advertisement /
    partial-echo maps (`sim_features` + `OpenSimExtras`, `agent_preferences`,
    `MediaEntry`) — a grid legitimately omits any of those. **Loud drop:** the
    client dispatch now routes a decode `Err` through a new
    `caps_decode_error(message, &error)` that `tracing::warn!`s the offending
    field and carries it in `Diagnostic::CapsDecodeFailed { message, reason:
    Some(..) }` (the legacy `Option`-returning conversions still use
    `caps_decode_failed` with `reason: None`). +11 negative unit tests (each
    asserts the precise `MissingField`). NO `sl-types` change.
    - **Absent-vs-advertised modelling for the legitimately-optional maps:**
    the feature-advertisement scalars that previously collapsed an absent
    key to a *lossy default* (`false`/`0`/`0.0`/`""`) are now `Option<T>` so
    the caller can distinguish "grid advertised it disabled" (`Some(false)`)
    from "grid did not advertise it" (`None` → apply the true default, e.g.
    the 20/100/10 m chat ranges rather than `0`). `SimulatorFeatures` (every
    flag/limit, `lsl_syntax_id`, and the nested
    `physics_shape_types`/`animated_objects` containers) and every
    `OpenSimExtras` scalar became `Option<_>`; the server encoder emits only
    the `Some` keys, so a build→parse round-trip preserves the
    advertised-vs-absent distinction. `agent_preferences` was already all
    `Option`; `MediaEntry` keeps the viewer's documented `LLMediaEntry` ctor
    defaults (its non-`Option` defaults are the *correct* per-key defaults,
    not a lossy zero). Consumers only matched the `Event` variant, so the
    only downstream churn was the two field-reading tests. NO `sl-types`
    change.

- **Confirmed non-issues (do not touch):** `uuid_crc` `chunks_exact(4)`
`unwrap_or(0)` (unreachable, for the indexing lint); the documented-sentinel
`grid_coordinates_from_handle`/`parse_lure_region_handle`; `login.rs` `.ok()`
→ `Option` fields (correct optional handling); the server-side
`build_map_block_reply` `u16::try_from` clamp (encode of our own data).

**D — geometry tuples → typed `sl-types` coordinate/vector types (consume-only
for `RegionCoordinates`; two NEW client-local types; COMPLETE 2026-06-24):**

A fresh-eye audit (2026-06-24, user-spotted) found `(f32, f32, f32)` position
tuples that the Phase 6 map-geometry sweep never revisited — it only typed the
teleport positions (`Command::Teleport`/`teleport_to`/`ScriptTeleportRequest`)
with `sl_types::map::RegionCoordinates`. These are the same concept
(region-local metres) and should adopt the same type; conversion is consume-only
and wire byte-identical (wire `Vector` is f32, `From<Vector> for
RegionCoordinates` already exists). The tuples split by concept — only the
**region-local positions** become `RegionCoordinates`:
