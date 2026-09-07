//! The typed object-asset model: an [`ObjectAsset`] of [`PrimBlock`]s, and the
//! nested blocks (permissions, sale info, shape, faces) each one carries.
//!
//! Field names follow the asset text rather than the reference viewer's C++
//! member names, because the text is what this crate reads and writes and what
//! a reader will have in front of them when they come here to check a keyword.

use sl_types::lsl::{Rotation, Vector};
use uuid::Uuid;

/// A decoded inventory object asset: the prims of one taken object, in the
/// order the asset lists them.
///
/// A linkset is several [`PrimBlock`]s in one asset. The reference's own
/// serialisation writes the **children first and the root last**, and marks
/// each with a [`link`](PrimBlock::link) state rather than by position, so
/// [`root`](Self::root) reads the marker instead of trusting the order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ObjectAsset {
    /// The prims, in asset order.
    pub prims: Vec<PrimBlock>,
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the model owns its `impl ObjectAsset` block, apart from decode's canonical impl"
)]
impl ObjectAsset {
    /// An asset holding the single prim `prim`.
    #[must_use]
    pub fn single(prim: PrimBlock) -> Self {
        Self { prims: vec![prim] }
    }

    /// An asset holding a whole linkset: `children` first and `root` last —
    /// the order the reference writes them — each marked with the
    /// [`link`](PrimBlock::link) state that says which it is.
    ///
    /// With no children this is [`single`](Self::single): a solitary prim
    /// carries no `linked` line at all, and one that claimed to be a root would
    /// be a linkset of one, which the reference never writes.
    #[must_use]
    pub fn linkset(mut root: PrimBlock, mut children: Vec<PrimBlock>) -> Self {
        if children.is_empty() {
            root.link = None;
            return Self::single(root);
        }
        root.link = Some(LinkState::Root);
        for child in &mut children {
            child.link = Some(LinkState::Child);
        }
        children.push(root);
        Self { prims: children }
    }

    /// The linkset's root prim: the one marked [`LinkState::Root`], or — for an
    /// asset holding one unlinked prim — that prim. `None` for an empty asset,
    /// or for one whose prims are all marked as children (which a simulator
    /// never writes, but a hand-built asset could).
    #[must_use]
    pub fn root(&self) -> Option<&PrimBlock> {
        self.prims
            .iter()
            .find(|prim| prim.link == Some(LinkState::Root))
            .or_else(|| match self.prims.as_slice() {
                [only] if only.link.is_none() => Some(only),
                _linked_or_several => None,
            })
    }

    /// The child prims of the linkset, in asset order.
    pub fn children(&self) -> impl Iterator<Item = &PrimBlock> {
        self.prims
            .iter()
            .filter(|prim| prim.link == Some(LinkState::Child))
    }
}

/// One prim of an object asset: everything between its `{'task_id':u…}` header
/// and the `}` that closes its block.
///
/// Every field the reference writes is carried, including the ones whose
/// meaning is the simulator's own business (`task_valid`, `gpw_bias`,
/// `ps_next_crc`, …). They are kept rather than dropped because an asset that
/// loses fields on a re-save is a lossy editor, and because a field nobody here
/// understands is still one a real grid may act on.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimBlock {
    /// The prim's own id — the `task_id` of the header line, which is the
    /// object key the prim had in the region it was taken from.
    pub task_id: Uuid,
    /// The prim's name (`name`, written with a `|` terminator).
    pub name: String,
    /// The prim's description (`description`), absent when the prim has none —
    /// which is how the reference writes an empty one.
    pub description: Option<String>,
    /// The ownership and permission masks (`permissions` block).
    pub permissions: LegacyPermissions,
    /// The region-local id the prim had when it was serialised.
    pub local_id: u32,
    /// The simulator's object CRC at serialisation time (`total_crc`).
    pub total_crc: u32,
    /// The object class byte (`type`) — a `LL_PCODE_*` value.
    pub pcode: u8,
    /// The simulator's `task_valid` field.
    pub task_valid: u32,
    /// The simulator's `travel_access` field (the parcel access byte).
    pub travel_access: u32,
    /// The simulator's `displayopts` field.
    pub display_options: u32,
    /// The simulator's `displaytype` field (a single letter, `v` in every
    /// reference asset seen).
    pub display_type: String,
    /// The prim's position (`pos`): region-relative for a root, and for a child
    /// the *world* position it had, with [`placement`](Self::placement)
    /// carrying the offset from the root that a rez actually uses.
    pub position: Vector,
    /// The simulator's `oldpos` field — the position before the last move.
    pub old_position: Vector,
    /// The prim's rotation (`rotation`).
    pub rotation: Rotation,
    /// What the prim states about its motion or its place in the linkset — the
    /// `velocity`/`angvel` pair for a root, `childpos`/`childrot` for a child.
    pub placement: PrimPlacement,
    /// The prim's size along each axis (`scale`).
    pub scale: Vector,
    /// The seat offset (`sit_offset`, `llSitTarget`'s position).
    pub sit_offset: Vector,
    /// The camera eye offset for a seated avatar (`camera_eye_offset`).
    pub camera_eye_offset: Vector,
    /// The camera focus offset for a seated avatar (`camera_at_offset`).
    pub camera_at_offset: Vector,
    /// The seat rotation (`sit_quat`, `llSitTarget`'s rotation).
    pub sit_rotation: Rotation,
    /// The simulator's `sit_hint` field.
    pub sit_hint: u32,
    /// The object `state` byte — the attachment point for an attachment (with
    /// its nibbles swapped, as on the wire), the species for a tree.
    pub state: u8,
    /// The prim's material code (`material`).
    pub material: u8,
    /// The attached sound (`soundid` and friends).
    pub sound: PrimSound,
    /// The floating-text colour (`textcolor`), RGBA in `0.0..=1.0`.
    pub text_color: [f32; 4],
    /// Whether the prim was selected when it was serialised (`selected`).
    pub selected: bool,
    /// Who had it selected (`selector`); nil when nobody had.
    pub selector: Uuid,
    /// The prim's behaviour toggles (`usephysics`, `phantom`, `temporary`, …).
    pub flags: PrimFlags,
    /// The `llRemoteLoadScriptPin` access pin (`remote_script_access_pin`).
    pub remote_script_access_pin: u32,
    /// The sandbox origin the prim is returned to (`sandboxhome`).
    pub sandbox_home: Vector,
    /// The path and profile parameters (`shape` block).
    pub shape: LegacyShape,
    /// The per-face texture entries (`faces` block).
    pub faces: Vec<LegacyFace>,
    /// The simulator's own bookkeeping fields (`birthtime`, `tax_rate`, …).
    pub bookkeeping: PrimBookkeeping,
    /// The prim's name-value pairs (`namevalue` lines), each kept as the
    /// verbatim declaration text.
    ///
    /// Verbatim rather than parsed because the reference's own reader
    /// (`LLNameValue`) *defaults* an omitted access class or send-to token, so
    /// re-writing a parsed pair would add tokens the asset did not carry.
    /// [`PrimBlock::to_object`](crate::bridge) hands them to
    /// [`sl_proto::Object::name_values`] where a typed read is wanted.
    pub name_values: Vec<String>,
    /// The `scratchpad` block, kept verbatim.
    pub scratchpad: Scratchpad,
    /// The sale terms (`sale_info` block).
    pub sale_info: LegacySaleInfo,
    /// The asset the prim was last rezzed from (`orig_asset_id`), when the
    /// reference wrote one.
    pub orig_asset_id: Option<Uuid>,
    /// The inventory item the prim was last rezzed from (`orig_item_id`).
    pub orig_item_id: Option<Uuid>,
    /// The task the prim was rezzed by (`from_task_id`), for an object a script
    /// rezzed.
    pub from_task_id: Option<Uuid>,
    /// The simulator's `correct_family_id` field.
    pub correct_family_id: Uuid,
    /// Whether the prim has been rezzed since it was created (`has_rezzed`).
    pub has_rezzed: bool,
    /// The base mask the prim had before it was linked (`pre_link_base_mask`),
    /// which is what an unlink restores.
    pub pre_link_base_mask: u32,
    /// The prim's place in the linkset (`linked`), absent for a solitary prim.
    pub link: Option<LinkState>,
    /// The five pay-button amounts (`default_pay_price`); `-2` is "use the
    /// default", `-1` is "hide this button".
    pub default_pay_price: [i32; 5],
    /// Keywords this crate does not know, kept in encounter order so a re-save
    /// does not silently drop a field a newer simulator wrote.
    ///
    /// They are re-emitted at the **end** of the block rather than where they
    /// were found: the reference's reader is a keyword switch that does not
    /// care about order, so this preserves the content without pretending to
    /// preserve a position nothing depends on.
    pub unknown: Vec<UnknownField>,
}

/// What a prim states about its motion, which the reference writes as one of
/// two mutually exclusive pairs.
#[derive(Debug, Clone, PartialEq)]
pub enum PrimPlacement {
    /// A root (or solitary) prim: its linear and angular velocity.
    Free {
        /// The linear velocity (`velocity`).
        velocity: Vector,
        /// The angular velocity (`angvel`).
        angular_velocity: Vector,
    },
    /// A child prim: its offset and rotation relative to the linkset root,
    /// which is what a rez rebuilds the linkset from.
    Child {
        /// The offset from the root (`childpos`).
        position: Vector,
        /// The rotation relative to the root (`childrot`).
        rotation: Rotation,
    },
}

impl Default for PrimPlacement {
    fn default() -> Self {
        Self::Free {
            velocity: ZERO_VECTOR,
            angular_velocity: ZERO_VECTOR,
        }
    }
}

/// The zero vector, as a constant the defaults here can share.
pub const ZERO_VECTOR: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// The identity rotation, as a constant the defaults here can share.
pub const IDENTITY_ROTATION: Rotation = Rotation {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    s: 1.0,
};

/// A prim's place in a linkset (`linked`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// `linked linked` — the root of a linkset of two or more prims.
    Root,
    /// `linked child` — a child of the linkset's root.
    Child,
}

impl LinkState {
    /// The keyword's value text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Root => "linked",
            Self::Child => "child",
        }
    }

    /// The link state a `linked` line's value names, or `None` for a value this
    /// crate does not know.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "linked" => Some(Self::Root),
            "child" => Some(Self::Child),
            _unknown => None,
        }
    }
}

/// The ownership and permission masks of a prim (`LLPermissions`, the
/// `permissions` block).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LegacyPermissions {
    /// The base mask, the ceiling every other mask is clamped to.
    pub base_mask: u32,
    /// What the current owner may do.
    pub owner_mask: u32,
    /// What the owning group may do.
    pub group_mask: u32,
    /// What everyone may do.
    pub everyone_mask: u32,
    /// What the next owner will receive on transfer.
    pub next_owner_mask: u32,
    /// Who created the prim.
    pub creator_id: Uuid,
    /// Who owns it now.
    pub owner_id: Uuid,
    /// Who owned it before.
    pub last_owner_id: Uuid,
    /// The group it is shared with (or owned by).
    pub group_id: Uuid,
    /// Whether the group owns it rather than an individual (`group_owned`,
    /// written only when true).
    pub group_owned: bool,
}

/// The sale terms of a prim (`LLSaleInfo`, the `sale_info` block).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LegacySaleInfo {
    /// How the prim may be bought.
    pub sale_type: LegacySaleType,
    /// The asking price in L$.
    pub sale_price: i32,
}

/// How a prim is offered for sale (`LLSaleInfo::EForSale`), by the keyword the
/// asset text carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LegacySaleType {
    /// Not for sale (`not`).
    #[default]
    NotForSale,
    /// The object itself is sold (`orig`).
    Original,
    /// A copy is sold (`copy`).
    Copy,
    /// The object's contents are sold (`cntn`).
    Contents,
}

impl LegacySaleType {
    /// The keyword the asset text uses for this sale type
    /// (`LLSaleInfo::lookup`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotForSale => "not",
            Self::Original => "orig",
            Self::Copy => "copy",
            Self::Contents => "cntn",
        }
    }

    /// The wire `SALE_TYPE_*` code for this sale type — what an
    /// `ObjectProperties` record and an inventory item carry where the asset
    /// text carries [`as_str`](Self::as_str).
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::NotForSale => 0,
            Self::Original => 1,
            Self::Copy => 2,
            Self::Contents => 3,
        }
    }

    /// The sale type a wire `SALE_TYPE_*` code names. A code this crate does
    /// not know is read as not for sale, which is what the reference's own
    /// viewer shows for one.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Original,
            2 => Self::Copy,
            3 => Self::Contents,
            _not_for_sale => Self::NotForSale,
        }
    }

    /// The sale type a keyword names, or `None` for one this crate does not
    /// know.
    #[must_use]
    pub fn parse(keyword: &str) -> Option<Self> {
        match keyword {
            "not" => Some(Self::NotForSale),
            "orig" => Some(Self::Original),
            "copy" => Some(Self::Copy),
            "cntn" => Some(Self::Contents),
            _unknown => None,
        }
    }
}

/// The attached-sound fields of a prim (`soundid`, `soundgain`, `soundradius`,
/// `soundflags`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PrimSound {
    /// The sound asset the prim plays; nil when it plays none.
    pub sound_id: Uuid,
    /// The playback gain.
    pub gain: f32,
    /// The cutoff radius, in metres.
    pub radius: f32,
    /// The `LL_SOUND_FLAG_*` playback flags.
    pub flags: u8,
}

/// A prim's behaviour toggles, each its own `0`/`1` line in the asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one field per independent toggle line the asset carries; a bitfield here would \
              have to be un-packed again to read or write the text"
)]
pub struct PrimFlags {
    /// The prim is physical (`usephysics`).
    pub use_physics: bool,
    /// The prim's X axis is free to rotate under physics (`rotate_x`).
    pub rotate_x: bool,
    /// The prim's Y axis is free to rotate under physics (`rotate_y`).
    pub rotate_y: bool,
    /// The prim's Z axis is free to rotate under physics (`rotate_z`).
    pub rotate_z: bool,
    /// The prim has no collisions (`phantom`).
    pub phantom: bool,
    /// The prim detects volume entry (`volume_detect`).
    pub volume_detect: bool,
    /// Grabbing the prim is blocked (`block_grabs`).
    pub block_grabs: bool,
    /// The prim dies when it leaves the region (`die_at_edge`).
    pub die_at_edge: bool,
    /// The prim is returned when it leaves the region (`return_at_edge`).
    pub return_at_edge: bool,
    /// The prim is temporary (`temporary`).
    pub temporary: bool,
    /// The prim is sandboxed to [`sandbox_home`](PrimBlock::sandbox_home)
    /// (`sandbox`).
    pub sandbox: bool,
}

/// The simulator's own bookkeeping fields, carried verbatim.
///
/// None of these is a viewer's business: they are the region's timers, its
/// object CRC bookkeeping and its land-impact accounting. They are modelled
/// only so a re-save writes back what it read.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PrimBookkeeping {
    /// The next particle-system CRC (`ps_next_crc`).
    pub ps_next_crc: u32,
    /// The prim's land-impact bias (`gpw_bias`).
    pub gpw_bias: f32,
    /// The simulator's `ip` field.
    pub ip: u32,
    /// The simulator's `complete` flag, written as `TRUE` / `FALSE`.
    pub complete: bool,
    /// The simulator's `delay` field, in microseconds.
    pub delay: u32,
    /// The simulator's `nextstart` stamp, in microseconds.
    pub next_start: u64,
    /// When the prim was created (`birthtime`), in microseconds.
    pub birth_time: u64,
    /// When the prim was last rezzed (`reztime`), in microseconds.
    pub rez_time: u64,
    /// When the prim last changed parcel (`parceltime`), in microseconds.
    pub parcel_time: u64,
    /// The prim's tax rate (`tax_rate`).
    pub tax_rate: f32,
}

/// The `scratchpad` block: a counted blob the reference writes for a prim's
/// scratch data, kept as the verbatim lines between its braces.
///
/// Verbatim because every asset seen carries a count of zero and a single empty
/// line, so there is no example of the encoding to model — and inventing one
/// would be a guess that a real asset would then contradict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scratchpad {
    /// The declared count on the `scratchpad` line.
    pub count: u32,
    /// The lines between the braces, without their terminating newline.
    pub lines: Vec<String>,
}

impl Default for Scratchpad {
    fn default() -> Self {
        Self {
            count: 0,
            // What the reference writes for an empty scratchpad: one line
            // holding a single tab.
            lines: vec!["\t".to_owned()],
        }
    }
}

/// A prim's path and profile parameters (`shape` block), dequantized — the
/// asset text carries the float values, not the wire's quantized bytes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LegacyShape {
    /// The extrusion path (`path` block).
    pub path: LegacyPathParams,
    /// The swept profile (`profile` block).
    pub profile: LegacyProfileParams,
}

/// The extrusion path of a prim (`LLPathParams`, the `path` block).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LegacyPathParams {
    /// The path curve byte (`LL_PCODE_PATH_*`).
    pub curve: u8,
    /// The path cut start, `0.0..=1.0`.
    pub begin: f32,
    /// The path cut end, `0.0..=1.0`.
    pub end: f32,
    /// The top-size X ratio.
    pub scale_x: f32,
    /// The top-size Y ratio.
    pub scale_y: f32,
    /// The shear along X.
    pub shear_x: f32,
    /// The shear along Y.
    pub shear_y: f32,
    /// The twist at the path end, in revolutions.
    pub twist: f32,
    /// The twist at the path start, in revolutions.
    pub twist_begin: f32,
    /// The radius offset (a torus's hole size).
    pub radius_offset: f32,
    /// The taper along X.
    pub taper_x: f32,
    /// The taper along Y.
    pub taper_y: f32,
    /// The number of revolutions, `1.0..=4.0`.
    pub revolutions: f32,
    /// The skew.
    pub skew: f32,
}

/// The swept profile of a prim (`LLProfileParams`, the `profile` block).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LegacyProfileParams {
    /// The profile curve byte — the profile in the low nibble and the hole type
    /// in the high one, exactly as the wire packs it.
    pub curve: u8,
    /// The profile cut start, `0.0..=1.0`.
    pub begin: f32,
    /// The profile cut end, `0.0..=1.0`.
    pub end: f32,
    /// The hollow fraction, `0.0..=1.0`.
    pub hollow: f32,
}

/// One face of a prim (`LLTextureEntry`, a block of the `faces` list).
///
/// The asset text carries fewer fields than the wire `TextureEntry` does: there
/// is no `glow` and no legacy material id, and `bump` is the bump-and-shiny
/// half of the wire's packed byte with `fullbright` split out beside it. A
/// conversion in either direction is therefore lossy in one direction, which
/// [`LegacyFace::from_texture_face`] documents.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LegacyFace {
    /// The face's texture asset id (`imageid`).
    pub image_id: Uuid,
    /// The tint colour (`colors`), RGBA in `0.0..=1.0`.
    pub color: [f32; 4],
    /// Horizontal texture repeats (`scales`).
    pub scale_s: f32,
    /// Vertical texture repeats (`scalet`).
    pub scale_t: f32,
    /// Horizontal texture offset (`offsets`).
    pub offset_s: f32,
    /// Vertical texture offset (`offsett`).
    pub offset_t: f32,
    /// Texture rotation in radians (`imagerot`).
    pub rotation: f32,
    /// The packed bump-and-shiny byte (`bump`, LL's `getBumpShiny`).
    pub bump: u8,
    /// Whether the face is unlit (`fullbright`).
    pub fullbright: bool,
    /// The packed media / texture-generation byte (`media_flags`).
    pub media_flags: u8,
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the model owns its `impl LegacyFace` block, apart from the bridge's conversions"
)]
impl LegacyFace {
    /// A face showing `image_id` with neutral values: an opaque white tint, one
    /// un-offset un-rotated texture repeat, and no bump, shine, full-bright or
    /// media. The asset-text counterpart of [`sl_proto::TextureFace::new`].
    #[must_use]
    pub const fn new(image_id: Uuid) -> Self {
        Self {
            image_id,
            color: [1.0, 1.0, 1.0, 1.0],
            scale_s: 1.0,
            scale_t: 1.0,
            offset_s: 0.0,
            offset_t: 0.0,
            rotation: 0.0,
            bump: 0,
            fullbright: false,
            media_flags: 0,
        }
    }
}

impl Default for LegacyFace {
    /// A neutral face showing nothing — [`LegacyFace::new`] with a nil texture.
    ///
    /// Written out rather than derived: a derived default would be a *black,
    /// zero-scale* face, which is not "no texture set" but a face that renders
    /// wrong, and the difference only shows up in a picture.
    fn default() -> Self {
        Self::new(Uuid::nil())
    }
}

/// A keyword the decoder did not recognise, kept so a re-save writes it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownField {
    /// The keyword, as written.
    pub keyword: String,
    /// Everything after the keyword's separator, verbatim.
    pub value: String,
}

impl Default for PrimBlock {
    fn default() -> Self {
        Self {
            task_id: Uuid::nil(),
            name: String::new(),
            description: None,
            permissions: LegacyPermissions::default(),
            local_id: 0,
            total_crc: 0,
            pcode: 0,
            task_valid: 0,
            travel_access: 0,
            display_options: 0,
            display_type: DEFAULT_DISPLAY_TYPE.to_owned(),
            position: ZERO_VECTOR,
            old_position: ZERO_VECTOR,
            rotation: IDENTITY_ROTATION,
            placement: PrimPlacement::default(),
            scale: ZERO_VECTOR,
            sit_offset: ZERO_VECTOR,
            camera_eye_offset: ZERO_VECTOR,
            camera_at_offset: ZERO_VECTOR,
            sit_rotation: IDENTITY_ROTATION,
            sit_hint: 0,
            state: 0,
            material: 0,
            sound: PrimSound::default(),
            text_color: [0.0, 0.0, 0.0, 1.0],
            selected: false,
            selector: Uuid::nil(),
            flags: PrimFlags::default(),
            remote_script_access_pin: 0,
            sandbox_home: ZERO_VECTOR,
            shape: LegacyShape::default(),
            faces: Vec::new(),
            bookkeeping: PrimBookkeeping::default(),
            name_values: Vec::new(),
            scratchpad: Scratchpad::default(),
            sale_info: LegacySaleInfo::default(),
            orig_asset_id: None,
            orig_item_id: None,
            from_task_id: None,
            correct_family_id: Uuid::nil(),
            has_rezzed: false,
            pre_link_base_mask: 0,
            link: None,
            default_pay_price: DEFAULT_PAY_PRICE,
            unknown: Vec::new(),
        }
    }
}

/// The `displaytype` value every reference asset carries.
pub const DEFAULT_DISPLAY_TYPE: &str = "v";

/// The pay-button amounts a prim has before anyone sets them: "use the
/// viewer's default" for the free-entry button and the reference's own four
/// preset amounts.
pub const DEFAULT_PAY_PRICE: [i32; 5] = [-2, 1, 5, 10, 20];

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::{LinkState, ObjectAsset, PrimBlock};

    /// A prim keyed by `index`, so an asset's order is readable in an assert.
    fn prim(index: u128) -> PrimBlock {
        PrimBlock {
            task_id: uuid::Uuid::from_u128(index),
            ..PrimBlock::default()
        }
    }

    /// [`ObjectAsset::linkset`] writes the children first and the root last,
    /// and marks each — which is what [`ObjectAsset::root`] and
    /// [`ObjectAsset::children`] read back. The two ends of one convention, so
    /// they are asserted against each other rather than against the order.
    #[test]
    fn a_linkset_writes_children_first_and_marks_its_root() {
        let asset = ObjectAsset::linkset(prim(1), vec![prim(2), prim(3)]);
        assert_eq!(
            asset
                .prims
                .iter()
                .map(|prim| (prim.task_id.as_u128(), prim.link))
                .collect::<Vec<_>>(),
            vec![
                (2, Some(LinkState::Child)),
                (3, Some(LinkState::Child)),
                (1, Some(LinkState::Root)),
            ]
        );
        assert_eq!(asset.root().map(|root| root.task_id.as_u128()), Some(1));
        assert_eq!(
            asset
                .children()
                .map(|child| child.task_id.as_u128())
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    /// A linkset of one is a solitary prim, not a root: the reference writes no
    /// `linked` line for an unlinked prim, and an asset that claimed a linkset
    /// of one would be one no simulator ever wrote.
    #[test]
    fn a_linkset_with_no_children_is_a_solitary_prim() {
        let asset = ObjectAsset::linkset(
            PrimBlock {
                link: Some(LinkState::Root),
                ..prim(1)
            },
            Vec::new(),
        );
        assert_eq!(asset.prims.len(), 1);
        assert_eq!(asset.prims.first().and_then(|prim| prim.link), None);
        assert_eq!(asset.root().map(|root| root.task_id.as_u128()), Some(1));
        assert_eq!(asset.children().count(), 0);
    }
}
