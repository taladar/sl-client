---
id: test-baked-texture-upload
title: upload a client-baked avatar texture over the UploadBakedTexture capab
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 13 — Asset & texture pipeline `[both]`
---

Context: [context/test.md](../context/test.md).

`baked-texture-upload` — upload a client-baked avatar texture over
the `UploadBakedTexture` capability. `1av`. Same two-step CAPS uploader as
`asset-upload` but for a bake: an empty LLSD body to the capability returns an
`uploader` URL, the raw JPEG-2000 codestream goes there, and the completion
carries a *temporary* asset UUID with **no** inventory item
(`new_inventory_item = None`) — the outcome the legacy (client-side-bake)
appearance path relies on. The `UploadBakedTexture` command and its two-step
run-loop were already built during the texture work; this case is the first to
exercise them, and it re-exports `AssetFetcher`/`FetchChunk`/`TextureFetcher`
from both runtime crates so the case can pull raw codestream bytes. It uploads
a real J2C: the plywood texture's `GetTexture` codestream (the asset
`texture-fetch-http` drives, present on both grids) is refetched and
re-uploaded as the bake, so the payload is a valid codestream on SL — which
validates it — without a client-side
JPEG-2000 encoder (the decode-only `sl-texture` crate has none). **Grid
divergence:** `complete` on OpenSim, which registers the capability and caches
the ~79 KB bake verbatim as a temporary texture (upload ≈ 14 ms loopback);
`partial` on aditi, whose regions do **not** advertise `UploadBakedTexture`
(the client requests it in its seed-cap list, but modern SL uses server-side
"Sunshine" baking so the legacy client-upload capability is absent), recorded
with the reason. `[both]`.
