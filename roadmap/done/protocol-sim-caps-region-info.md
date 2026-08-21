---
id: protocol-sim-caps-region-info
title: Server-side region/object-info caps
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 5
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The region- and object-information cluster, server side:

- `SimulatorFeatures` (paired with `sl-wire/src/sim_features.rs`);
- `LSLSyntax` (paired with `sl-wire/src/lsl_syntax.rs`);
- `ExtEnvironment` — EEP get/put;
- `RemoteParcelRequest` (paired with `sl-wire/src/remote_parcel.rs`);
- `GetObjectCost` / `GetObjectPhysicsData` / `ResourceCostSelected`
  (paired with `object_cost.rs` / `object_physics.rs` /
  `resource_report.rs`);
- `AttachmentResources` / `LandResources`.

Inverse-pairing per the convention; verified against the client-direction
builders/parsers in-memory.

## Done (2026-08-20)

All nine `REQUESTED_CAPABILITIES` rows flipped Pending → Served in the
pinned coverage table (33 → 42 granted caps). Serving stores: inline
driver-populated `SimSession` fields — the `SimulatorFeatures` + `LslSyntax`
documents (`set_lsl_syntax` owns the `lsl_syntax_id` consistency
invariant), a per-parcel `environments` map (`-1` = seeded region entry,
parcel fallback to region), three per-object maps (cost / physics /
selection-cost, summed component-wise for `ResourceCostSelected`), a
region id + `SimParcel` cover-rectangle list resolving
`RemoteParcelRequest` (miss ⇒ `200 {}`), and the attachment/land resource
reports. `LandResources` is two-stage: the POST mints `summary`/`detail`
sub-path URLs (the screenshot-uploader pattern).

EEP got both directions of the previously-missing **PUT** (maximal
scope): new `EnvironmentUpdate` type,
`build_environment_update_request` / `environment_update_from_llsd`
(Firestorm `coroUpdateEnvironment` shape, `?parcelid=`/`?trackno=`
queries), `Command::SetEnvironment` wired through both runtimes
(`put_caps_llsd` / `run_put_caps_llsd`) + repl `set_environment`; the
success reply folds through the existing `Event::Environment`. The store
merge bumps `env_version`, surfaces the new
`ServerEvent::EnvironmentUpdated`, and pairs with
`enqueue_windlight_refresh` for other-client re-fetch; a `day_asset`-only
update answers the reference's graceful `200 { success: false, message }`
(no settings-asset store). Per-track splicing deferred to
[[viewer-region-environment-panel]] (whose sl-proto write pairing this
task supplied).

The only other net-new codec was `parse_get_object_cost_request`
(everything else already existed client-paired). Verified by twelve new
loopback tests driving the real client builders/folds against
`SimCaps::dispatch` (`sl-proto/tests/sim_caps.rs`) plus the
`environment_update_round_trips` codec unit test; book coverage in the
new "The region-information handlers" section of `book/src/comms/caps.md`.
