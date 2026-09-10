//! The **other** `AssetType::Object` body: OpenSim's `<SceneObjectGroup>` XML.
//!
//! The rest of this crate is the Linden text a Second Life simulator writes.
//! OpenSim has never written that format for anything. What it stores under
//! `AssetType.Object` is `SceneObjectSerializer.ToOriginalXmlFormat` —
//! `InventoryAccessModule.cs:527-587` picks it for a single-object take, and
//! `CoalescedSceneObjectsSerializer.ToXml` for a multi-object one — so the two
//! grids genuinely disagree about what the class *is*, and this module is the
//! OpenSim half of that disagreement.
//!
//! # What it buys over the text
//!
//! Everything the text has no keyword for. The `Shape` block carries the
//! packed `TextureEntry` and `ExtraParams` blobs **verbatim**, so a face's glow
//! and legacy material id, and the whole flexi / light / sculpt / **mesh** /
//! light-image / extended-mesh / render-material / reflection-probe set, cross
//! this format untouched. Floating text, its colour, a media URL, a texture
//! animation and a particle system each have an element of their own. The
//! crate docs' "Do not rez out of these bytes" is a statement about the *text*;
//! it is not true of this one, which is exactly why OpenSim rezzes from it.
//!
//! # What it is written from, and how far that goes
//!
//! The writer here is a transcription of OpenSim's own — the element names,
//! their order, which ones are omitted when they hold a default, the
//! base64-encoded blobs, the `PrimFlags` names with their commas stripped —
//! from `SceneObjectSerializer.SOPToXml2` / `WriteShape` /
//! `WriteTaskInventory`. The reader is the same file's processor table read the
//! other way, and is order-independent because OpenSim's is: it dispatches each
//! child element through a name → handler dictionary.
//!
//! Five nested sub-documents OpenSim can write are **not modelled**: a
//! vehicle's parameters, a physics-inertia block, dynamic attributes
//! (`DynAttrs`), serialised object animations (`SOPAnims`) and a group's
//! keyframe motion. Nothing in this workspace produces any of them, and each is
//! another serialiser's format rather than this one's. They are not *lost*,
//! though: an element this module does not model is captured verbatim
//! ([`UnknownElement`]) and written back out, the same way
//! [`PrimBlock::unknown`](crate::PrimBlock::unknown) keeps an unknown keyword
//! of the text.
//!
//! # A quirk worth knowing before it surprises you
//!
//! Hover text's alpha is **inverted** between this format and the wire.
//! OpenSim stores `Color.A` as opacity (`llSetText`'s `alpha * 0xff`) and sends
//! `0xFF - Color.A` in the object update (`SceneObjectPart.GetTextColor`);
//! the reference viewer inverts it straight back
//! (`llviewerobject.cpp`, `coloru.mV[3] = 255 - coloru.mV[3]`). The
//! [`bridge`] does that inversion, so a fully opaque hover text is `255` here
//! and `0` in an [`Object::text_color`](sl_proto::Object::text_color).

pub mod bridge;
pub mod decode;
pub mod encode;
pub mod model;

pub use decode::SceneObjectXmlError;
pub use model::{
    SceneObjectGroup, SceneObjectPart, SceneShape, SceneTaskInventoryItem, UnknownElement,
    prim_flags,
};
