---
id: test-assets-remaining-class-audit
title: Decide what the remaining asset classes are worth
topic: test
status: ready
origin: asset-class audit while doing viewer-static-asset-library (2026-09-01)
points: 2
refs: [test-shared-test-assets, test-assets-object-asset-codec]
---

Context: [context/testing.md](../context/testing.md).

Auditing every `AssetType` against what the workspace can produce left
five classes with no fixture and no obvious consumer. They are grouped
here rather than given a task each, because the work is mostly *deciding*
whether each is worth anything — and for several the answer is probably
"no, and say so in the enum's doc comment".

- **`TextureTga` (12), `ImageTga` (18), `ImageJpeg` (19)** — pre-JPEG2000
  image classes. The viewer classifies them for a group-notice icon and
  nothing else; every texture path is J2C. Almost certainly vestigial on
  both grids: check whether anything on aditi still serves one, and if
  not, document them as decode-only legacy rather than build fixtures.
- **`ScriptBytecode` (11)** — compiled LSO / Mono bytecode. A viewer never
  decodes it (the *server* runs it); it is only ever an inventory class
  and an upload result. Likely nothing to do beyond confirming
  `sl-lsl` has no reason to want it.
- **`CallingCard` (2)** — a trivial body naming an agent. Cheap to
  synthesise if any test wants one; the question is whether the
  calling-card inventory paths are worth a fixture at all.
- **`Gltf` (58) / `GltfBin` (59)** — the newer whole-document glTF asset
  classes, distinct from `Material` (57), which `sl-material` already
  handles and `sl-test-assets::gltf_material_asset` already writes. Worth
  finding out whether Second Life actually serves these yet, or whether
  the classes are reserved: if they are live, they belong with the mesh
  fixtures; if reserved, say so in the doc comment so the next reader does
  not go looking.

The output of this task is a decision per class, recorded in
`sl-proto`'s `AssetType` doc comments (which are already the workspace's
reference for what each class *is*), plus a fixture only where the answer
is yes.

Acceptance: no `AssetType` variant is left without a one-line statement
of whether a fixture exists, is wanted, or is deliberately not.
