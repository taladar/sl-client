---
id: missing-out-batch-2
title: object prim editing
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 2 — object prim editing.** `ObjectShape` (prim geometry),
`ObjectExtraParams` (sculpt/flexi/light/mesh extra params), `ObjectImage`
(per-face textures / TE) — the edit-tool prim-update messages keyed by the
region-local object id.

Implemented as
`Session::set_object_shape(local_id: ScopedObjectId, shape: &PrimShapeParams)`,
`Session::set_object_image(local_id, media_url: Option<&str>, texture_entry: &TextureEntry)`,
and `Session::set_object_extra_params(local_id, params: &ObjectExtraParams)`
(mirroring the existing `set_object_*` edit methods, all `ScopedObjectId`-keyed
via `circuit_for_scope`). Each reuses an existing domain struct rather than raw
wire fields: `ObjectShape` carries the inbound `PrimShapeParams` (the same
quantized path/profile values an `ObjectUpdate` decodes to); `ObjectImage`
carries a `TextureEntry` packed with the existing `encode_texture_entry` (a new
`TextureFace::new` builds a neutral face — one face retextures every face, since
the wire run-length default applies to all); `ObjectExtraParams` carries the
inbound `ObjectExtraParams` bag and is serialised by a new
`extra_param_message_blocks` helper (factored out of `encode_extra_params`'s
entry builder) that emits **one block per known subtype** with `ParamInUse`
reflecting presence — mirroring the reference viewer's `sendExtraParameters`, so
a subtype absent from `params` is *cleared* on the object and
`ObjectExtraParams::default` clears them all. Wired as
`Command::{SetObjectShape, SetObjectImage, SetObjectExtraParams}` through the
tokio and bevy runtimes, the `command_name` formatter, and the
`set_object_shape` / `set_object_image` / `set_object_extra_params` REPL tokens.
The extra-params token now covers **all** subtypes — the OpenSim-handled
flexi/light/sculpt plus the (largely SL-only)
light-image/extended-mesh/render-material/reflection-probe ones (the projector
texture+params, extended-mesh flags, reflection-probe ambiance/clip/flags, and
the per-face GLTF material faces+ids as two parallel lists). Covered by three
pack-the-wire tests plus two REPL parse tests for the extra-params subtypes;
object edit (shape/texture) is OpenSim-testable.
