# Materials

What each grid keeps of the materials on a prim's faces, who may change
them, and how everybody else is told. The messages are described in
[Materials](../content/materials.md).

A face has up to three things beyond its texture, kept two ways:

- a **legacy material** — a normal and a specular map with their parameters.
  A viewer PUTs it to the `RenderMaterials` capability; the region gives it
  an id and writes the id into the face's texture entry, and a POST of the id
  answers with the material.
- a **PBR material** — a GLTF material asset — and an **override** of some of
  its fields. A viewer POSTs both to `ModifyMaterialParams`; the region names
  the asset in the object's extra parameters (`0x80`) and sends the overrides
  in a `GenericStreamingMessage` (method `0x4175`) of their own.

Measured on 2026-10-12 by the conformance case `materials`. One avatar rezzes
a cube on land it may build on; a second, no friend with edit rights and with
no group in common, stands by. The case sends each request to the capability
itself, so that it reads the status and the body, then listens on both
avatars for the cube sent again and for override messages. It ran four times
on aditi (Mauve, the public sandbox parcel) and four on the local OpenSim.
Every row below is checked on every run of both live grids.

## The capabilities

| capability | Second Life | OpenSim |
| --- | --- | --- |
| `RenderMaterials` | granted | granted |
| `ModifyMaterialParams` | granted | **not granted** |
| `UpdateMaterialAgentInventory`, `UpdateMaterialTaskInventory` | granted | not granted |

A stock OpenSim has legacy materials only. Nothing there takes a PBR
material or an override, and no object it sends names one.

## Legacy materials

### Reading

| question | Second Life | OpenSim |
| --- | --- | --- |
| the body of every answer | `{Zipped: <binary>}`: zlib over binary LLSD | the same |
| a POST of a material's id | an array of one `{ID, Material}` | the same |
| the keys of `Material` | `AlphaMaskCutoff`, `DiffuseAlphaMode`, `EnvIntensity`, `NormMap`, `NormOffsetX`, `NormOffsetY`, `NormRepeatX`, `NormRepeatY`, `NormRotation`, `SpecColor` (four integers), `SpecExp`, `SpecMap`, `SpecOffsetX`, `SpecOffsetY`, `SpecRepeatX`, `SpecRepeatY`, `SpecRotation` — the maps ids, the rest integers | the same |
| a material read back against the one set | every field as sent | the same |
| the other avatar's POST of the id | what the owner's states | the same |
| a POST of an id twice and of an id no region made | two entries | two entries |
| a POST of an id no region made | an empty array | the same |
| a POST of no id | an empty array | the same |
| an HTTP `GET` | **every material of the region**, the one just set among them | **always an empty array** |
| a POST or a PUT with an empty body, or one that is no zipped map | `400`, no body | `200`, no body |

OpenSim's `GET` is a stub (`RenderMaterialsGetCap` answers the canned empty
array, its listing commented out), and it answers a body it cannot read with
a `200`.

### Setting

The PUT is `{FullMaterialsPerFace: [{ID: <local id>, Face, Material}]}`,
zipped; an entry with no `Material` clears the face.

| question | Second Life | OpenSim |
| --- | --- | --- |
| the answer to a PUT | an array of one `{ID: <local id>, Face, MaterialID}` for each face it took | **always an empty array** |
| a material on a face | the face's texture entry states an id; a full update to both avatars | the same |
| the id | a hash of the material: the same material on another face gets the same id, another material another | the same — but **not the hash Second Life makes** of the same material |
| the same material on the same face again | nothing changes, nobody is told (the PUT is still answered with the entry) | the same |
| another material on a face that has one | the id changes; **a terse update** to both | the id changes; a full update to both |
| the viewer's default material (no maps, alpha blending) | kept like any other, with an id | **not kept: the face has no material**, nobody is told |
| two faces in one PUT | both set, two entries in the answer, a full update | both set, a full update |
| a face the prim has not got | an empty array; nothing changes, nobody is told | an empty array; nothing changes, **and a full update to both all the same** |
| a prim the region has not got | an empty array | the same |
| a PUT of no face at all | **`400`** | an empty array |
| the other avatar's set, and its clear | an empty array; nothing changes, nobody is told | the same |
| a clear | the face's id gone; a full update to both | the same |
| a clear of a face with no material | nothing changes, nobody is told | the same |
| an `ObjectImage` whose texture entry leaves a face's material id out, states another known id, or states an id no region made | ignored: nothing changes, nobody is told | the same |

A material id is the region's to give: neither grid takes one from a texture
entry a viewer sends, so an `ObjectImage` neither sets nor clears a material.
Both refuse another avatar's set silently — the capability answers `200` and
an empty array either way, which on OpenSim is also what a set that took is
answered with.

The two grids hash differently. The same material — a normal and a specular
map, placements, a tinted highlight, an alpha mask — is
`daa9877a-44db-df92-d24d-fedd34d65f3a` on Second Life and
`2516b40d-5fa0-e456-5321-ac92185cf727` on OpenSim (`FaceMaterial.genID`), so
an id means nothing off the grid that made it.

OpenSim does not keep a material that says nothing: alpha blending with
neither map is "no material" there (`RenderMaterialsPutCap`), while Second
Life gives it an id (`20ddc75e-772a-57b1-1585-3df4c69e3388`) like any other.

## PBR materials

Second Life only: OpenSim grants no `ModifyMaterialParams`.

The request is an array of `{object_id, side, asset_id?, gltf_json?}`;
`side` is a face, or `-1` for every face. The answer is
`{success, message}`.

### What a region cannot serve

| request | Second Life |
| --- | --- |
| an HTTP `GET` | `405` |
| an empty body, or a map in place of the array | `400`, `success` false, `missing or invalid required parameter object_id` |
| an empty array | `200`, `success` true |
| a prim the region has not got | `404`, `failed to find task` |
| an update of `object_id` and `side` alone | `200`, `success` true; nothing changes |
| a face the prim has not got | `404`, `Invalid side: 9` |
| `gltf_json` that is no JSON | `400`, `Warnings/errors parsing GLTF JSON:` and the parser's error |
| the other avatar's material, and its override | `403`, `permission denied`; nothing changes, nobody is told |

Unlike [media on a prim](media.md#media-on-a-prim), a refusal here has an
HTTP status of its own, and `success` false beside it.

### The material

| question | Second Life |
| --- | --- |
| an asset on a face | the extra parameters name it for that face; a full update to both avatars |
| the same asset again | nothing changes, nobody is told |
| an asset id no asset is under | **taken like any other**: the object names it |
| another asset on a face that has an override | the asset changes; **the override stays** |
| the nil id | the face's material gone; a full update to both |
| `side` `-1` | every face, in a full update |
| a legacy material on a face that has a PBR one | both kept: the texture entry states the legacy id, the extra parameters the asset |

### The override

An override is sent to everybody who has the object in a
`GenericStreamingMessage`: `{'id': <local id>, 'te': [faces], 'od':
[overrides]}` in notation LLSD, **naming every face of the object that has an
override**, not the one that changed. A face it does not name has none.

| question | Second Life |
| --- | --- |
| an override on a face | one override message to both avatars, the owner included, about 0.2 s after the request; **no object update** |
| the override as it comes back | only the fields that were set: `{'bc':[r1,r0,r0,r1],'rf':r0.5}` for a base colour and a roughness |
| every kind of field | `{'ac':r0.25,'am':i2,'bc':[r0.5,r0.25,r1,r0.75],'ds':1,'ec':[r0.5,r0.25,r0],'mf':r0.75,'rf':r0.25,'tex':[u8955…],'ti':[{'o':[r0.25,r0.5],'r':r0.5,'s':[r2,r3]}]}` — alpha cutoff, alpha mode, base colour, double-sided, emissive colour, metallic and roughness factors, the textures and their transforms, each as sent |
| the same override again | nothing changes, nobody is told |
| another override on the face | **replaces** the first: a metallic factor alone leaves `{'mf':r0.25}` |
| a material and an override in one update | a full update and an override message |
| an override on a face with **no material** | **kept and sent** like any other |
| an override that overrides nothing (a bare GLTF material) | the face's override gone: a message that no longer names it |
| an empty `gltf_json` | the same; with no face left, a message with an empty `te` |
| the material under an override cleared, or every face's | **the overrides stay**, and no message says otherwise |
| `side` `-1` | every face in one message |
| the other avatar asks for the object again (`RequestMultipleObjects`) | a full update, and an override message after it |
| the same once every face's material is cleared | a full update naming no material, and a message with all six overrides |

An override belongs to the face, not to its material. It is kept under a
face that has no material, survives the material being replaced or cleared,
and is sent with the object to whoever asks for it — so a viewer may hold an
override for a face it has nothing to apply it to, and must keep it for the
material that may come.

## What the fake grid does

Not what either grid does: it answers a PUT and a `ModifyMaterialParams`
POST with success and applies neither to the object, on both flavours, and
the OpenSim flavour withholds `ModifyMaterialParams` as OpenSim does
(`sl-fake-grid/src/imitates.rs`). The model above is
`server-fake-grid-material-edits`'s to build; until it lands the case lists
the live grids only.

## What the viewer does

- The build floater's **PBR** tab is greyed on a region that grants no
  `ModifyMaterialParams`, as the reference greys it
  (`LLMaterialEditor::capabilitiesAvailable`), since 2026-10-12. Before, it
  sent edits a stock OpenSim has nothing to take, and the client only logged
  that it could not.
- An override message replaces every override the viewer holds for the
  object, as the reference's `applyOverrideMessage` does. An override it
  cannot read is logged, since 2026-10-12, rather than taken for a clear.
- The overrides the viewer's encoder writes come back through its decoder
  with every field as sent (`materials`, every run).

## Not measured

A material on a child prim, on an attachment and on a mesh with more faces;
what a take and a rez, a copy and a link keep; the group bit and a friend
with edit rights; a no-modify prim; a material asset the agent has no right
to; the sixteen-face and the per-transaction limits
(`MaxMaterialsPerTransaction`); how Second Life hashes a material; and what
a script sets (`gridspec-lsl-prim-state`).
