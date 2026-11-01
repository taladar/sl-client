---
id: server-fake-grid-dynamic-textures
title: Dynamic textures — render a drawing list to a texture on the fake grid
topic: server
status: ideas
origin: server-lsl-lib-ossl (2026-11-01)
refs: [server-lsl-lib-ossl, server-lsl-lib-prim-state,
  viewer-fake-grid-render-catalogue]
---

Context: [context/lsl.md](../context/lsl.md).

OpenSim's `osSetDynamicTextureData` family is how nearly every scripted sign,
board and HUD label on an OpenSim region is drawn: a script builds a drawing
list (`osMovePen`, `osDrawText`, `osSetPenColor`, … — implemented by
[[server-lsl-lib-ossl]], which only writes the commands down), and the
region's `VectorRenderModule` renders it to an image, uploads it as a texture
asset and puts it on a face of the prim. `osSetDynamicTextureURL*` does the
same from a URL's image. `osGetDrawStringSize` measures text in the module's
font.

The fake grid has no renderer, so the eight functions are planned here
(`sl-lsl-runtime/src/library/opensim-scope.txt`), not in a library tranche:
the work is the grid's — a rasteriser for the drawing commands
(`VectorRenderModule.GetDrawStringSize`, `…Draw` are the reference, and their
font metrics are what `osGetDrawStringSize` answers), a JPEG2000 encode
(`sl-j2c`'s encoder exists), a texture asset the grid serves, and the face
change the viewer is sent ([[server-lsl-lib-prim-state]]'s
`llSetTexture`). The URL forms need outbound HTTP, which the determinism rule
keeps off by default.

Acceptance: a drawing list rendered onto a prim face that the viewer shows,
compared against the same list on the local OpenSim by screenshot, and
`osGetDrawStringSize` held to OpenSim's answer for a few strings and fonts.
