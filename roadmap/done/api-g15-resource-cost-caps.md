---
id: api-g15
title: Resource & cost CAPS
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G15 — Resource & cost CAPS

`GetObjectCost`, `ResourceCostSelected`, `GetObjectPhysicsData` +
`ObjectPhysicsProperties` (EventQueue), `AttachmentResources`, `LandResources`,
and `LandStatReply` (top scripts/colliders for a parcel/region). Mostly
SL-only; guard on cap presence.

- [x] G15 object/attachment/land resource costs and physics data. Three new
      hand-written LLSD codec modules in `sl-wire`: `object_cost.rs`
      (`GetObjectCost` → `ObjectCost`, `ResourceCostSelected` →
      `SelectedResourceCost` with a `SelectedCostKind` roots/prims selector),
      `object_physics.rs` (`GetObjectPhysicsData` + the
      `ObjectPhysicsProperties` EventQueue push, both decoding
      `ObjectPhysicsData` with a `PhysicsShapeType` enum), and
      `resource_report.rs` (the shared `ResourceSummary` / `ScriptedObjectInfo`
      building blocks behind `AttachmentResources` →
      `AttachmentResourcesReport`, plus the two-step `LandResources` POST → the
      `ScriptResourceSummary` / `ScriptResourceDetails` follow-up reports →
      `ParcelScriptResources`). New caps `CAP_GET_OBJECT_COST`,
      `CAP_RESOURCE_COST_SELECTED`, `CAP_GET_OBJECT_PHYSICS_DATA`,
      `CAP_ATTACHMENT_RESOURCES`, `CAP_LAND_RESOURCES` (all added to
      `REQUESTED_CAPABILITIES`), plus two follow-up-URL tags
      `LAND_RESOURCE_SUMMARY_TAG` / `LAND_RESOURCE_DETAIL_TAG`. Commands
      `RequestObjectCost`, `RequestSelectedCost { roots }`,
      `RequestObjectPhysicsData`, `RequestAttachmentResources`,
      `RequestLandResources`, and the UDP `RequestLandStat`; events
      `ObjectCosts`, `SelectedResourceCost`, `ObjectPhysicsData`,
      `ObjectPhysicsProperties`, `AttachmentResources`, `LandResourcesUrls`,
      `LandResourceSummary`, `LandResourceDetail`, and `LandStatReply`. The
      **`LandStatRequest` / `LandStatReply`** UDP pair (Low 421/422, the
      estate-tools "Top Scripts / Top Colliders" report) is the one non-CAPS
      member: client `Session::request_land_stat` (new `LandStatItem` /
      `LandStatReportType` in `types/parcel.rs`) and server
      `SimSession::send_land_stat_reply`. Both runtimes (the two-step
      `LandResources` flow lives in their `fetch_land_resources` /
      `run_land_resources`, which `GET` the follow-up URLs and forward them
      tagged for `handle_caps_event`) + REPL (eight new registry commands incl.
      `request_land_stat [report_type=scripts|colliders] …`) + format.rs
      event/command names. Tests: 9 wire round-trip (3 modules) + 4 proto
      `handle_caps_event` decode + 1 `SimSession`→client `LandStatReply`
      loopback + 8 REPL registry. Book: extended `content/region.md` with a
      "Resource & physics costs" section + "In this codebase". **Scope note:**
      the four cost/physics/attachment/land CAPS are CAPS-only (HTTP,
      out-of-band), so — like G3/G7/G14 — the server side is the
      `build_*_response` wire functions, with no UDP `SimSession` encoder;
      `LandStatReply` is the exception (a real UDP message, so it gets a
      `SimSession::send_land_stat_reply` encoder and a `ServerEvent`-free
      receive-side decode). Mostly SL-only and guarded on cap presence; OpenSim
      serves all of them too (the EQ `ObjectPhysicsProperties` and the UDP
      LandStat are OpenSim-testable), but NOT live-tested this session (wire +
      lifecycle + loopback round-trips cover both directions). G16 (abuse
      reports, postcards, map-layer tiles) is now done; **NEXT = G17** (the
      viewer freeze/thaw event).
