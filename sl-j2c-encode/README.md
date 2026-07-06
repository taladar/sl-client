# sl-j2c-encode

In-memory JPEG-2000 (`.j2c`) encoder for canonical RGBA8 images, built on the
OpenJPEG C library ([`openjpeg-sys`](https://crates.io/crates/openjpeg-sys)) —
deliberately the same backend `jpeg2k` decodes with (via `sl-texture`), so only
one OpenJPEG implementation is ever linked into a binary. (Using the pure-Rust
`openjp2` port here instead would export duplicate `#[no_mangle]` `opj_*` C
symbols that collide with this library at link time and corrupt the decode
path.)

It turns tightly packed 8-bit RGBA pixels into the raw JPEG-2000 codestream
Second Life / OpenSim stores textures as — the byte form the
`UploadBakedTexture` capability accepts. It exists so a client that composites
its own avatar bake (the client-side / legacy bake path) can publish the result
to the grid so the simulator and other viewers see it.

A fully-opaque image is encoded as three (RGB) components; one with any
transparency keeps its alpha as a fourth component, so an alpha-masked bake
round-trips its cut-outs. Encoding is lossy (the reference viewer's bake path is
too) and runs entirely in memory (no temp file).

This is the **only** crate in the `sl-client` workspace that owns `unsafe` FFI:
the OpenJPEG bindings and the raw-pointer bookkeeping they require are isolated
here behind the single safe `encode_rgba8` function, so the rest of the
workspace can keep the `unsafe_code = "forbid"` policy.
