//! Pure decoder / encoder for the Second Life / OpenSim **inventory object
//! asset** — the nested-block text a simulator writes when an object is taken
//! into inventory, and reads back when it is rezzed.
//!
//! It mirrors `sl-notecard`, `sl-prim` and the other format crates: **Bevy-free
//! and I/O-free**, so it can be tested, fuzzed and reused with no session and
//! no grid. Beyond the format itself its only ties are `sl-proto` and `sl-prim`
//! for the wire types the [`bridge`] converts to and from.
//!
//! An object asset is *not* the `ObjectUpdate` wire form. It is one text block
//! per prim, children first and the root last:
//!
//! ```text
//! {'task_id':u1fd77b79-a8e7-25a5-9454-02a4d948ba1c}
//! {
//!     name    Object|
//!     permissions 0
//!     {
//!         base_mask   7fffffff
//!         …
//!     }
//!     local_id    10284
//!     type    1
//!     pos 0   0   0
//!     rotation    0   0   0   1
//!     scale   0.28    0.28    0.28
//!     shape 0
//!     {
//!         path 0
//!         {
//!             curve   16
//!             …
//!         }
//!         profile 0
//!         {
//!             curve   1
//!             …
//!         }
//!     }
//!     faces   6
//!     {
//!         imageid 89556747-24cb-43ed-920b-47caed15465f
//!         colors  1 1 1 1
//!         …
//!     }
//!     …
//!     sale_info   0
//!     {
//!         sale_type   not
//!         sale_price  10
//!     }
//!     linked  child
//!     default_pay_price   -2  1   5   10  20
//! }
//! ```
//!
//! # Where the format came from
//!
//! Second Life's simulator is closed, and **neither reference implementation
//! this workspace has ever carried a reader for this format**.
//!
//! In the reference viewer that is not "it was removed": the repository's
//! history runs unbroken from the 2007 open-source drop, no commit in it ever
//! touched an `importFileLegacy` / `exportFileLegacy`, and the one prim-level
//! keyword unique to this format (`sandboxhome`) has exactly three commits —
//! the 2007 initial import, where it lived only inside a comment block in
//! `indra/test/io.cpp`, and two 2009 commits (DEV-41175, DEV-41352) that split
//! the legacy TUT tests into `llcommon/tests/commonmisc_test.cpp` and
//! `lluri_test.cpp`.
//!
//! Only **one** of those three sites is even compiled, and it does not parse
//! the asset: `lluri_test`'s "do some round-trip tests with very long strings"
//! pushes it through `LLURI::escape` / `unescape` and checks it survives, as a
//! long punctuation-dense stress payload — the only other string in that test
//! is a paragraph of the Community Standards. The `commonmisc_test` copy sits
//! inside `#if 0` and the
//! `io.cpp` one inside a comment block. So nothing in the reference has ever
//! *read* this format — not even the tests that carry it.
//!
//! That cuts both ways, and the second half is why the captures can be trusted
//! anyway: nothing validated them either, so a truncated paste would have gone
//! unnoticed — but the `lluri_test` copy is prefixed `'asset_data':b(12100)`,
//! an LLSD binary field declaring its own length, and the bytes unescaped out
//! of it are exactly 12100. The capture is complete, and it came out of a real
//! LLSD asset payload. (The file in `tests/data` is five bytes shorter: one
//! name in it is redacted, as its own README records.)
//!
//! OpenSim does not use the format at all: it stores
//! `SceneObjectSerializer.ToOriginalXmlFormat` (or
//! `CoalescedSceneObjectsSerializer.ToXml` for a multi-object take) as its
//! `AssetType.Object` body (`InventoryAccessModule.cs`), which is XML and
//! shares nothing with this. So `AssetType::Object` is **two different formats
//! on the two grids**, and this crate is the Second Life one.
//!
//! The grammar is therefore reconstructed from three sources, and the crate
//! says which parts rest on which:
//!
//! - the **sub-block writers that survive** in the reference viewer —
//!   `LLPermissions::exportLegacyStream`, `LLSaleInfo::exportLegacyStream` and
//!   `LLPathParams` / `LLProfileParams::exportLegacyStream` — which give the
//!   `permissions`, `sale_info`, `path` and `profile` blocks exactly;
//!   their `importLegacyStream` twins give the reading rules (a keyword
//!   switch, `{` skipped, `}` ending the block);
//! - **two complete assets** carried verbatim in the reference's own test
//!   sources: a single attachment prim (`commonmisc_test.cpp`) and a four-prim
//!   linkset (`lluri_test.cpp`). Both are in this crate's `tests/data`, and
//!   `tests/reference.rs` pins what each field decodes to;
//! - `LLTextureEntry`'s LLSD field names, which are the same names the `faces`
//!   blocks use (`imageid`, `colors`, `scales`, `bump`, `media_flags`, …) and
//!   which say what each one means.
//!
//! What no source establishes is recorded as such rather than guessed at: the
//! simulator-internal fields (`task_valid`, `travel_access`, `displayopts`,
//! `gpw_bias`, …) are carried verbatim with no claim about their meaning, the
//! `scratchpad` block is kept as raw lines because every known asset's is
//! empty, and a keyword this crate does not know is preserved rather than
//! dropped ([`PrimBlock::unknown`]).
//!
//! # What a viewer can actually see (measured)
//!
//! Neither grid makes this format a viewer's business, and the two do not agree
//! with each other — measured 2026-09-06 by `sl-conformance`'s
//! `object-asset-format` case:
//!
//! | grid | asset id exposed to a viewer? | body |
//! | --- | --- | --- |
//! | OpenSim | yes — every object item names one | `<SceneObjectGroup>` XML |
//! | Second Life | **no** | unobservable |
//!
//! Second Life answers an object inventory item with a **nil** `asset_id`, in
//! the AIS3 folder listing and in the per-item fetch alike, and it does so even
//! for items that are full-perm to their owner — so a viewer cannot fetch an
//! object asset there at all, whatever the bytes would have been. That is
//! consistent with neither reference viewer ever having carried a reader.
//!
//! So this crate is **not** on any viewer's critical path, and its fidelity to
//! the 2005 text buys nothing a viewer can observe. What it is for is below.
//!
//! # What it is for
//!
//! `AssetType::Object` was the one inventory class the workspace could neither
//! read nor write. Two uses survive the measurement above:
//!
//! - **the fake grid needs a serialisation.** Its take has to write the object
//!   down somewhere, or nothing can drag it back out of inventory. **Only the
//!   grid reads these bytes back** — which is now true by construction rather
//!   than by convention: since [[test-fake-grid-object-asset-id-divergence]]
//!   the fake grid imitates Second Life by default
//!   (`sl_fake_grid::ObjectAssetPolicy::Withheld`), files a take under a nil
//!   asset id and keeps the body where no capability reaches it. A grid asked
//!   for OpenSim's side (`Served`) names the asset and serves it, and that is
//!   the one configuration where these bytes cross the wire. The choice of
//!   format is therefore free; this one is chosen because it is the format
//!   Second Life is known to have written.
//! - **the two reference captures are readable.** They are the only public
//!   examples of the format, and a decoder that reads them field by field is
//!   how the workspace can say what it is at all, rather than repeating
//!   folklore about it.

pub mod bridge;
pub mod decode;
pub mod encode;
pub mod model;
#[cfg(test)]
pub(crate) mod test_support;

pub use bridge::{RezTarget, rendered_face_count};
pub use decode::ObjectAssetError;
pub use model::{
    DEFAULT_DISPLAY_TYPE, DEFAULT_PAY_PRICE, IDENTITY_ROTATION, LegacyFace, LegacyPathParams,
    LegacyPermissions, LegacyProfileParams, LegacySaleInfo, LegacySaleType, LegacyShape, LinkState,
    ObjectAsset, PrimBlock, PrimBookkeeping, PrimFlags, PrimPlacement, PrimSound, Scratchpad,
    UnknownField, ZERO_VECTOR,
};
