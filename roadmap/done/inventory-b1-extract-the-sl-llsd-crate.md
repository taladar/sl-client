---
id: inventory-b1
title: Extract the sl-llsd crate
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B1. Extract the `sl-llsd` crate (from A3)

Fully standalone (no inventory dependency); first because every later task
serialises or parses LLSD and B2 adds the binary codec here.

- [x] Add a new `sl-llsd` workspace member; move **only the core** (per the
      A3 boundary): the `Llsd` enum, the pure accessors (`get`/`index`/`as_*`/
      `kind`), the `field_*` / `require_*` accessors, the XML codec
      (`to_llsd_xml` / `parse_llsd_xml`, with `node_to_llsd` / `push_llsd_xml`),
      `push_escaped` (made `pub`), and the generic notation cursor (`Scan`) from
      `sl-wire/src/material/gltf.rs`. Dependencies: `sl-types`, `uuid`,
      `base64`, `roxmltree`, plus `thiserror` for `LlsdError` (`time` lands with
      B2, the binary date path). The `build_*` CAPS builders, the
      `AssetUploadResponse` / `ObjectMediaResponse` / `EventQueueResponse`
      types, the `llsd_*` helpers, and the GLTF-domain decode
      (`modify_material_update`) **stay** in sl-wire.
      Files: `sl-llsd/src/{lib,value,notation,error}.rs` + the per-crate aux
      (`README.md`, `CHANGELOG.md`, `cliff.toml`).
- [x] Introduce a crate-local `LlsdError` (`MalformedField { field, value }`,
      `MissingField { field }`) and re-type the moved `field_*` / `require_*` to
      it. **Error-architecture decision (user-directed, divergence from the
      drafted "mirror + identity-`From`" plan):** instead of mapping `LlsdError`
      back onto generic `WireError::MalformedField` / `MissingField`, those two
      `WireError` variants are **removed** and replaced by one transported type
      per format — LLSD faults flow through a transparent
      `WireError::Llsd(#[from] LlsdError)`, and the only genuinely non-LLSD use
      (text-scalar parsing in `sl-proto`'s `parse_u32_field` /
      `parse_mute_line`) moves to a new inline
      `WireError::InvalidScalar { field, value }` (sibling of `InvalidUuid` /
      `InvalidUrl`). This keeps structured-data faults distinguishable from
      text-scalar ones, and every LLSD parse site in sl-wire now
      produces/propagates `LlsdError` (via `?` / `.into()`) rather than
      inspecting `WireError` variants — making the retarget part of the
      extraction itself.
- [x] Keep sl-wire compiling: `sl-wire/src/llsd.rs`
      **stays a real `crate::llsd` module** — it opens with
      `pub use sl_llsd::{Llsd, LlsdError, parse_llsd_xml};` +
      `pub(crate) use sl_llsd::{Scan, push_escaped};` (re-export the moved core)
      **and** keeps the builders, so both `crate::llsd::Llsd` and
      `crate::llsd::build_seed_request` resolve at the sl-wire modules +
      downstream `sl-proto` / `sl-client-tokio` / `sl-client-bevy` call sites
      unchanged. `LlsdError` is re-exported at the sl-wire crate root.
- [x] Verify: full workspace builds + `cargo test` green, clippy-clean,
      rustdoc-clean (`-D warnings`). Split the tests by where their subject
      landed: the pure-LLSD cases + the inline `field_accessors_*` test move to
      `sl-llsd`; the builder/CAPS cases (`AssetUploadResponse`, `EventQueue`,
      `ObjectMedia`) stay in `sl-wire/tests/llsd.rs`.
