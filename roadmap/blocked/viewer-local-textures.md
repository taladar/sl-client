---
id: viewer-local-textures
title: Local textures — file-backed textures with live reload
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-texture-picker]
refs: [viewer-local-mesh]
---

Context: [context/viewer.md](../context/viewer.md).

The texture picker's **Local** tab ([[viewer-ui-texture-picker]] scoped it
out): register image files from disk as local-only textures, apply them
anywhere a texture is picked (faces, wearables), and **live-reload on
file change** — the creator loop that skips the upload fee while
iterating (nobody else sees them; re-applying happens locally by swapping
the backing image). File watching + PNG/TGA/J2C load are all standard
pieces (`sl-texture` decodes, `notify`-style watcher); the interesting
part is the stand-in `TextureKey` plumbing so scene materials can point
at a non-asset image and update in place.

Reference (Firestorm, read-only): `lllocalbitmaps`, the texture picker's
Local tab.

Deps: [[viewer-ui-texture-picker]] (the tab lives in it).
