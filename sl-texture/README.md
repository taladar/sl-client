# sl-texture

A higher-level texture fetch API and cache for Second Life / OpenSim clients,
built on top of the low-level `GetTexture` HTTP fetch in `sl-client-tokio` /
`sl-client-bevy`.

It fetches JPEG-2000 (`.j2c`) texture codestreams, **decodes** them to RGBA8
(via the `jpeg2k` crate / OpenJPEG, off the calling thread on a `rayon` pool),
and keeps the results in a **level-of-detail-aware store**:

- **Weak-reference store** — the store holds only `Weak<TextureEntry>`, so a
  texture is collectible as soon as the last external `Arc` to it drops
  (garbage collection follows pointer counts).
- **LOD upgrade/downgrade in place** — one logical texture object upgrades to a
  finer [`DiscardLevel`](sl_proto::DiscardLevel) by fetching more of the
  codestream and re-decoding, or downgrades to a coarser one by downsampling the
  existing pixels (no re-decode). A usage lease guards against freeing pixels
  that are mapped to the GPU or otherwise in use.
- **Never fetch or decode twice while referenced** — single-flight requests
  share one fetch and one decode; an in-memory + on-disk cache avoids repeat
  network fetches.
- **Priority scheduler** — non-blocking, observable, cancellable, re-priorit=
  izable requests; a texture wanted by several users takes their combined
  priority.
- **Firestorm-compatible on-disk cache** — reads and writes the Second Life
  viewer's `texture.entries` / `texture.cache` / body-file format in a dedicated
  cache directory.

The OpenJPEG C dependency is behind the default-on `decode` feature.
