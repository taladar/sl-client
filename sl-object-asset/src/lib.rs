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
//! Second Life's simulator is closed and the viewer's own reader for this
//! format was removed years ago, so the grammar here is reconstructed from
//! three sources, and the crate says which parts rest on which:
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
//! # What it is for
//!
//! `AssetType::Object` was the one inventory class the workspace could neither
//! read nor write, which left every path that goes *through* the asset
//! untestable: rezzing an object from inventory, taking one back, a coalesced
//! object, an object offered in an IM, and the object embedded in a notecard.
//! A fake grid can now author the asset a take mints an id for, and a test can
//! assert that what came back is the object that was taken.

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
