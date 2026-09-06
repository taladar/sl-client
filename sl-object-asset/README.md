# sl-object-asset

Decoder / encoder for the Second Life / OpenSim **inventory object asset** —
the nested-block text a simulator writes when an object is taken into
inventory, and reads back when it is rezzed.

`AssetType::Object` is what an inventory object item points at, and it is not
the `ObjectUpdate` wire form: the two are unrelated encodings of the same prim.
Everything that goes *through* the asset — rezzing from inventory, taking back,
a coalesced object, an object offered in an IM, the object embedded in a
notecard — needs this one, and nothing in this workspace could read or write it
before.

Like `sl-notecard` and `sl-prim` it is **Bevy-free and I/O-free**; its only
substantive dependencies are `sl-proto` and `sl-prim`, for the wire types the
`bridge` module converts to and from.

## What it looks like

One text block per prim, children first and the root last, each headed by an
LLSD-notation line naming the prim:

```text
{'task_id':u1fd77b79-a8e7-25a5-9454-02a4d948ba1c}
{
    name    Object|
    permissions 0
    {
        base_mask       7fffffff
        …
    }
    local_id        10284
    type    1
    pos     0       0       0
    shape 0
    {
        path 0
        {
            curve   16
            …
        }
        profile 0
        {
            curve   1
            …
        }
    }
    faces   6
    {
        imageid 89556747-24cb-43ed-920b-47caed15465f
        colors  1 1 1 1
        …
    }
    …
    linked  child
    default_pay_price       -2      1       5       10      20
}
```

## Where the format came from

Second Life's simulator is closed and the reference viewer's own reader for
this format was removed years ago, so the grammar is reconstructed — and the
crate is explicit about which part rests on what:

- the sub-block writers that **do** survive in the viewer
  (`LLPermissions::exportLegacyStream`, `LLSaleInfo::exportLegacyStream`,
  `LLPathParams` / `LLProfileParams::exportLegacyStream`), which give the
  `permissions`, `sale_info`, `path` and `profile` blocks exactly, and their
  `importLegacyStream` twins, which give the reading rules;
- **two complete assets** carried verbatim in the reference's own test sources
  — a single worn attachment prim and a four-prim linkset. Both are in
  `tests/data`, and `tests/reference.rs` pins field by field what they decode
  to;
- `LLTextureEntry`'s LLSD field names, which are the names the `faces` blocks
  use and say what each one means.

Fields no source explains — `task_valid`, `travel_access`, `displayopts`,
`gpw_bias` and the rest of the simulator's own bookkeeping — are carried
verbatim with no claim about their meaning, and a keyword this crate has never
seen is preserved rather than dropped, so a re-save is not lossy against a grid
that grew a field.

## Two departures from the reference

- **A malformed value is an error.** The reference's `sscanf` leaves the field
  at whatever it held, so a bad `pos` line silently leaves a prim at the origin.
- **The round trip is semantic, not byte-level.** The reference writes each
  number at whatever precision its own `ostream` had — six significant digits
  for a position, a float's full exact decimal expansion for a rotation, inside
  one prim — and this crate writes the shortest text that round-trips. Decode,
  encode and decode again yields the same model; it does not yield the same
  bytes.

## The bridge

`PrimBlock::from_object` is what a **take** needs (a live `sl_proto::Object`
becomes an asset prim) and `PrimBlock::to_object` what a **rez** needs (the
region minting the ids the asset cannot know, via `RezTarget`).

The text cannot carry everything a live prim has: there is no field for a
face's glow or legacy material id, none for `ExtraParams` (flexi, light,
sculpt, mesh, reflection probe), none for floating text — only its colour —
none for a media URL, and none for a texture animation or particle system. That
is the format's limit rather than the crate's, and it is documented rather than
papered over with invented keywords.
