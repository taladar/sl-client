---
id: protocol-sim-caps-assets
title: Server-side asset delivery caps — GetTexture, GetMesh, ViewerAsset
topic: protocol
status: blocked
origin: user request (2026-07) — complete simulator protocol surface
points: 8
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The server side of `GetTexture`, `GetMesh`, `GetMesh2` and `ViewerAsset`:

- HTTP Range request parsing and 206 partial-content responses (the
  client fetches mesh LODs and progressive JPEG2000 by byte range);
- correct content types per asset kind;
- an asset-store fixture trait (in-memory + directory-backed) so the fake
  grid and loopback tests can serve real texture/mesh bytes.

Mirrors the client fetch paths in `sl-client-tokio/src/assets.rs` /
`sl-client-bevy/src/assets.rs`; verified by round-tripping against them
in-memory.
