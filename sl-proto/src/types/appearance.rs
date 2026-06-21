//! Wearables and avatar appearance: textures, attachments, animations.

use sl_types::lsl::Vector;
use uuid::Uuid;

/// A wearable's body/clothing slot (LL's `LLWearableType::EType`). Carried by an
/// `AgentWearablesUpdate` (the simulator telling the agent what it is wearing,
/// surfaced as [`Event::AgentWearables`](crate::Event::AgentWearables)) and `AgentIsNowWearing` (the agent
/// telling the simulator to change its outfit, sent by
/// [`Session::set_wearing`](crate::Session::set_wearing)).
///
/// The first four slots ([`Shape`](Self::Shape), [`Skin`](Self::Skin),
/// [`Hair`](Self::Hair), [`Eyes`](Self::Eyes)) are *body parts* — an avatar
/// always wears exactly one of each; the rest are *clothing* layers that may be
/// absent or stacked. [`WearableType::is_body_part`] distinguishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WearableType {
    /// Body shape (`WT_SHAPE`).
    Shape,
    /// Skin (`WT_SKIN`).
    Skin,
    /// Hair (`WT_HAIR`).
    Hair,
    /// Eyes (`WT_EYES`).
    Eyes,
    /// Shirt (`WT_SHIRT`).
    Shirt,
    /// Pants (`WT_PANTS`).
    Pants,
    /// Shoes (`WT_SHOES`).
    Shoes,
    /// Socks (`WT_SOCKS`).
    Socks,
    /// Jacket (`WT_JACKET`).
    Jacket,
    /// Gloves (`WT_GLOVES`).
    Gloves,
    /// Undershirt (`WT_UNDERSHIRT`).
    Undershirt,
    /// Underpants (`WT_UNDERPANTS`).
    Underpants,
    /// Skirt (`WT_SKIRT`).
    Skirt,
    /// Alpha mask (`WT_ALPHA`).
    Alpha,
    /// Tattoo (`WT_TATTOO`).
    Tattoo,
    /// Physics (`WT_PHYSICS`).
    Physics,
    /// Universal (`WT_UNIVERSAL`).
    Universal,
    /// An unknown / future wearable slot, preserving the raw wire byte.
    Other(u8),
}

impl WearableType {
    /// The `LLWearableType::EType` wire byte for this slot.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Shape => 0,
            Self::Skin => 1,
            Self::Hair => 2,
            Self::Eyes => 3,
            Self::Shirt => 4,
            Self::Pants => 5,
            Self::Shoes => 6,
            Self::Socks => 7,
            Self::Jacket => 8,
            Self::Gloves => 9,
            Self::Undershirt => 10,
            Self::Underpants => 11,
            Self::Skirt => 12,
            Self::Alpha => 13,
            Self::Tattoo => 14,
            Self::Physics => 15,
            Self::Universal => 16,
            Self::Other(code) => code,
        }
    }

    /// Classifies an `LLWearableType::EType` wire byte; codes outside the known
    /// range become [`Other`](Self::Other).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Shape,
            1 => Self::Skin,
            2 => Self::Hair,
            3 => Self::Eyes,
            4 => Self::Shirt,
            5 => Self::Pants,
            6 => Self::Shoes,
            7 => Self::Socks,
            8 => Self::Jacket,
            9 => Self::Gloves,
            10 => Self::Undershirt,
            11 => Self::Underpants,
            12 => Self::Skirt,
            13 => Self::Alpha,
            14 => Self::Tattoo,
            15 => Self::Physics,
            16 => Self::Universal,
            other => Self::Other(other),
        }
    }

    /// Whether this is a *body part* (shape/skin/hair/eyes) — worn exactly once —
    /// rather than a clothing layer.
    #[must_use]
    pub const fn is_body_part(self) -> bool {
        matches!(self, Self::Shape | Self::Skin | Self::Hair | Self::Eyes)
    }
}

/// One wearable an avatar has on. From an `AgentWearablesUpdate` (the simulator's
/// view of the agent's outfit) the [`asset_id`](Self::asset_id) names the
/// wearable asset; when passed to
/// [`Session::set_wearing`](crate::Session::set_wearing) (which sends
/// `AgentIsNowWearing`) only the [`item_id`](Self::item_id) and
/// [`wearable_type`](Self::wearable_type) are sent, so the asset id may be nil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wearable {
    /// The inventory item id of the worn wearable.
    pub item_id: Uuid,
    /// The wearable's asset id (nil when not known, e.g. when sending).
    pub asset_id: Uuid,
    /// Which body/clothing slot this wearable occupies.
    pub wearable_type: WearableType,
}

/// Avatar texture-entry slot indices (LL's `ETextureIndex`): the faces of the
/// per-avatar `TextureEntry` carried by `AvatarAppearance`. The *baked* slots
/// (`*_BAKED`) hold the composited texture UUIDs other clients fetch and render
/// onto the avatar mesh (see [`Session::request_texture`](crate::Session::request_texture));
/// the remaining slots are the individual per-wearable layer textures used to
/// produce those bakes.
pub mod avatar_texture {
    /// The number of avatar texture slots (`TEX_NUM_INDICES`); the face count of
    /// an avatar `TextureEntry`.
    pub const COUNT: usize = 45;
    /// The baked head texture (`TEX_HEAD_BAKED`).
    pub const HEAD_BAKED: usize = 8;
    /// The baked upper-body texture (`TEX_UPPER_BAKED`).
    pub const UPPER_BAKED: usize = 9;
    /// The baked lower-body texture (`TEX_LOWER_BAKED`).
    pub const LOWER_BAKED: usize = 10;
    /// The baked eyes texture (`TEX_EYES_BAKED`).
    pub const EYES_BAKED: usize = 11;
    /// The baked skirt texture (`TEX_SKIRT_BAKED`).
    pub const SKIRT_BAKED: usize = 19;
    /// The baked hair texture (`TEX_HAIR_BAKED`).
    pub const HAIR_BAKED: usize = 20;
    /// The baked left-arm texture (`TEX_LEFT_ARM_BAKED`), a "universal" bake.
    pub const LEFT_ARM_BAKED: usize = 40;
    /// The baked left-leg texture (`TEX_LEFT_LEG_BAKED`), a "universal" bake.
    pub const LEFT_LEG_BAKED: usize = 41;
    /// The baked aux1 texture (`TEX_AUX1_BAKED`), a "universal" bake.
    pub const AUX1_BAKED: usize = 42;
    /// The baked aux2 texture (`TEX_AUX2_BAKED`), a "universal" bake.
    pub const AUX2_BAKED: usize = 43;
    /// The baked aux3 texture (`TEX_AUX3_BAKED`), a "universal" bake.
    pub const AUX3_BAKED: usize = 44;
    /// The baked-slot indices in order, each with a short human-readable name.
    pub const BAKED: [(usize, &str); 11] = [
        (HEAD_BAKED, "head"),
        (UPPER_BAKED, "upper"),
        (LOWER_BAKED, "lower"),
        (EYES_BAKED, "eyes"),
        (SKIRT_BAKED, "skirt"),
        (HAIR_BAKED, "hair"),
        (LEFT_ARM_BAKED, "left_arm"),
        (LEFT_LEG_BAKED, "left_leg"),
        (AUX1_BAKED, "aux1"),
        (AUX2_BAKED, "aux2"),
        (AUX3_BAKED, "aux3"),
    ];
}

/// One decoded face of a `TextureEntry`: its texture and surface parameters. A
/// `TextureEntry` packs per-face data (texture id, tint colour, repeats/offsets,
/// rotation, bump/shiny/fullbright, media, glow, material) into a run-length
/// encoded blob shared by objects (`ObjectUpdate`) and avatars
/// (`AvatarAppearance`); [`decode_texture_entry`](crate::decode_texture_entry)
/// unpacks it into one of these per face. Values are converted to natural units
/// (matching the reference viewer's `applyParsedTEMessage`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureFace {
    /// The face's texture asset id. For an avatar's baked slots (see
    /// [`avatar_texture`]) this is the composited bake to fetch and render.
    pub texture_id: Uuid,
    /// The tint colour applied to the texture, as RGBA bytes (un-inverted from
    /// the wire's `255 - value` encoding; `[255; 4]` is opaque white = no tint).
    pub color: [u8; 4],
    /// Horizontal texture repeats.
    pub scale_s: f32,
    /// Vertical texture repeats.
    pub scale_t: f32,
    /// Horizontal texture offset, in the range −1..1.
    pub offset_s: f32,
    /// Vertical texture offset, in the range −1..1.
    pub offset_t: f32,
    /// Texture rotation, in radians.
    pub rotation: f32,
    /// The packed bump/shiny/fullbright byte (LL's `getBumpShinyFullbright`).
    pub bump_shiny_fullbright: u8,
    /// The packed media/texture-generation flags byte (LL's `getMediaTexGen`).
    pub media_flags: u8,
    /// Glow amount, in the range 0..1.
    pub glow: f32,
    /// The material id (a legacy materials asset; nil if none).
    pub material_id: Uuid,
}

impl TextureFace {
    /// The bump-map (normal/emboss) code packed into
    /// [`bump_shiny_fullbright`](Self::bump_shiny_fullbright) — the low 5 bits
    /// (LL's `getBumpmap()`; `0` = none, otherwise a `BE_*` bump type).
    #[must_use]
    pub const fn bumpmap(self) -> u8 {
        self.bump_shiny_fullbright & 0x1f
    }

    /// Whether the face is full-bright (unlit), from bit 5 of
    /// [`bump_shiny_fullbright`](Self::bump_shiny_fullbright) (LL's
    /// `getFullbright()`).
    #[must_use]
    pub const fn fullbright(self) -> bool {
        (self.bump_shiny_fullbright >> 5) & 0x01 != 0
    }

    /// The shininess code, from the top 2 bits of
    /// [`bump_shiny_fullbright`](Self::bump_shiny_fullbright) (LL's `getShiny()`):
    /// `0` = none, `1` = low, `2` = medium, `3` = high.
    #[must_use]
    pub const fn shininess(self) -> u8 {
        (self.bump_shiny_fullbright >> 6) & 0x03
    }

    /// Whether **media** is enabled on the face, from bit 0 of
    /// [`media_flags`](Self::media_flags) (LL's `getMediaFlags()`).
    #[must_use]
    pub const fn media_enabled(self) -> bool {
        self.media_flags & 0x01 != 0
    }

    /// The texture-coordinate generation mode, from bits 1–2 of
    /// [`media_flags`](Self::media_flags) (LL's `getTexGen()`): `0` = default
    /// (per-face), `2` = planar, with the other values reserved.
    #[must_use]
    pub const fn tex_gen(self) -> u8 {
        self.media_flags & 0x06
    }
}

/// A decoded `TextureEntry`: one [`TextureFace`] per face. For an avatar (from
/// `AvatarAppearance`) the faces are indexed by the [`avatar_texture`] slot
/// constants; for an object they follow the prim's face numbering. Decode a raw
/// blob (e.g. [`Object::texture_entry`](crate::Object::texture_entry)) with
/// [`decode_texture_entry`](crate::decode_texture_entry).
#[derive(Debug, Clone, PartialEq)]
pub struct TextureEntry {
    /// The per-face data, in face-index order.
    pub faces: Vec<TextureFace>,
}

impl TextureEntry {
    /// The face at `index`, or `None` if the entry has fewer faces.
    #[must_use]
    pub fn face(&self, index: usize) -> Option<&TextureFace> {
        self.faces.get(index)
    }

    /// The texture id at slot `index`, or `None` if the entry has fewer faces.
    /// Combine with the [`avatar_texture`] baked-slot constants to read an
    /// avatar's baked textures.
    #[must_use]
    pub fn texture_id(&self, index: usize) -> Option<Uuid> {
        self.faces.get(index).map(|face| face.texture_id)
    }
}

/// A decoded `AvatarAppearance`: another avatar's baked textures and visual
/// parameters, pushed by the simulator when an avatar comes into range or
/// changes appearance. Surfaced as [`Event::AvatarAppearance`](crate::Event::AvatarAppearance). The baked
/// texture ids (read from [`texture_entry`](Self::texture_entry) via the
/// [`avatar_texture`] slot constants) can be fetched with
/// [`Session::request_texture`](crate::Session::request_texture) to render the
/// avatar.
#[derive(Debug, Clone, PartialEq)]
pub struct AvatarAppearance {
    /// The avatar this appearance describes.
    pub avatar_id: Uuid,
    /// Whether the avatar is on a trial account.
    pub is_trial: bool,
    /// The decoded per-face texture entry (the baked avatar textures live in the
    /// [`avatar_texture`] baked slots).
    pub texture_entry: TextureEntry,
    /// The visual parameters, one quantized byte (0..255) per parameter in the
    /// reference viewer's parameter order. Each maps a slider/morph to its range;
    /// the byte is `round((value - min) / (max - min) * 255)`.
    pub visual_params: Vec<u8>,
    /// The appearance message version (`AppearanceData.AppearanceVersion`), or
    /// `None` when the simulator sent no `AppearanceData` block (older path).
    pub appearance_version: Option<u8>,
    /// The Current Outfit Folder version this appearance was baked from
    /// (`AppearanceData.CofVersion`), or `None` when absent.
    pub cof_version: Option<i32>,
    /// The appearance flags (`AppearanceData.Flags`), or `None` when absent.
    pub appearance_flags: Option<u32>,
    /// The avatar's hover height offset, in metres, if the simulator sent an
    /// `AppearanceHover` block.
    pub hover_height: Option<Vector>,
    /// The avatar's HUD/attachment ids and their attachment points, if the
    /// simulator sent an `AttachmentBlock`.
    pub attachments: Vec<AvatarAttachment>,
}

/// One animation an avatar is currently playing, from an `AvatarAnimation`
/// update (surfaced inside [`Event::AvatarAnimation`](crate::Event::AvatarAnimation)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayingAnimation {
    /// The animation asset id (a built-in animation UUID or an uploaded
    /// animation asset; fetch custom ones with
    /// [`Session::request_asset`](crate::Session::request_asset)).
    pub anim_id: Uuid,
    /// The simulator's per-avatar animation sequence number. It increments each
    /// time an animation (re)starts, so a viewer can tell a fresh start from an
    /// animation that has merely been re-listed.
    pub sequence_id: i32,
    /// The object that triggered the animation, when the simulator names one
    /// (an `AnimationSourceList` entry — e.g. a scripted `llStartAnimation`).
    /// `None` for animations the agent or simulator started directly.
    pub source_id: Option<Uuid>,
}

/// The playback flags carried by an [`Event::AttachedSound`](crate::Event::AttachedSound) (`AttachedSound`'s
/// `Flags` byte). The values match the viewer's `LL_SOUND_FLAG_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SoundFlags(pub u8);

impl SoundFlags {
    /// The sound loops until stopped (`llLoopSound`) rather than playing once.
    pub const LOOP: u8 = 1 << 0;
    /// This sound is the timing master of a synchronised group.
    pub const SYNC_MASTER: u8 = 1 << 1;
    /// This sound is a slave that follows the group's sync master.
    pub const SYNC_SLAVE: u8 = 1 << 2;
    /// This sound is waiting to be synchronised to a master.
    pub const SYNC_PENDING: u8 = 1 << 3;
    /// Queue this sound behind the currently-playing one rather than interrupting.
    pub const QUEUE: u8 = 1 << 4;
    /// Stop the object's attached sound (rather than starting one).
    pub const STOP: u8 = 1 << 5;

    /// Whether all of the bits in `mask` are set.
    #[must_use]
    pub const fn contains(self, mask: u8) -> bool {
        self.0 & mask == mask
    }

    /// Whether the sound loops ([`Self::LOOP`]).
    #[must_use]
    pub const fn is_loop(self) -> bool {
        self.contains(Self::LOOP)
    }

    /// Whether this message stops the attached sound ([`Self::STOP`]).
    #[must_use]
    pub const fn is_stop(self) -> bool {
        self.contains(Self::STOP)
    }
}

/// One sound the simulator asks the viewer to pre-fetch, from a `PreloadSound`
/// update (surfaced inside [`Event::PreloadSound`](crate::Event::PreloadSound)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundPreload {
    /// The sound asset to pre-fetch.
    pub sound_id: Uuid,
    /// The object that will play the sound.
    pub object_id: Uuid,
    /// The object owner's id.
    pub owner_id: Uuid,
}

/// One entry of an [`AvatarAppearance`] attachment block: an attached object and
/// where it is worn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvatarAttachment {
    /// The attached object's id.
    pub id: Uuid,
    /// The attachment point byte (LL's attachment-point enumeration).
    pub attachment_point: u8,
}

/// An avatar attachment point: the body joint or HUD slot an attached object
/// hangs from (LL's attachment-point enumeration, mirroring the viewer's
/// `avatar_lad.xml`). Carried by the attachment commands
/// ([`Command::AttachObject`](crate::Command::AttachObject),
/// [`Command::RezAttachment`](crate::Command::RezAttachment),
/// [`Command::RemoveAttachment`](crate::Command::RemoveAttachment)) and the
/// matching server events.
///
/// On the wire the point shares a byte with an "add" flag (`ATTACHMENT_ADD`,
/// `0x80`): when set, the object is *added* to the point alongside anything
/// already there rather than *replacing* it. The flag is modelled separately as
/// an [`AttachmentMode`] (the `mode` field on the commands), so
/// [`to_code`](Self::to_code) / [`from_code`](Self::from_code) carry only the
/// point itself (the low 7 bits); use [`split_code`](Self::split_code) /
/// [`with_mode`](Self::with_mode) to combine or separate the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachmentPoint {
    /// The item's default attachment point (`0`); the simulator picks the slot
    /// the object was last attached to (or its scripted default).
    Default,
    /// Chest (`1`).
    Chest,
    /// Skull / head (`2`).
    Skull,
    /// Left shoulder (`3`).
    LeftShoulder,
    /// Right shoulder (`4`).
    RightShoulder,
    /// Left hand (`5`).
    LeftHand,
    /// Right hand (`6`).
    RightHand,
    /// Left foot (`7`).
    LeftFoot,
    /// Right foot (`8`).
    RightFoot,
    /// Spine / back (`9`).
    Spine,
    /// Pelvis (`10`).
    Pelvis,
    /// Mouth (`11`).
    Mouth,
    /// Chin (`12`).
    Chin,
    /// Left ear (`13`).
    LeftEar,
    /// Right ear (`14`).
    RightEar,
    /// Left eyeball (`15`).
    LeftEyeball,
    /// Right eyeball (`16`).
    RightEyeball,
    /// Nose (`17`).
    Nose,
    /// Right upper arm (`18`).
    RUpperArm,
    /// Right forearm (`19`).
    RForearm,
    /// Left upper arm (`20`).
    LUpperArm,
    /// Left forearm (`21`).
    LForearm,
    /// Right hip (`22`).
    RightHip,
    /// Right upper leg (`23`).
    RUpperLeg,
    /// Right lower leg (`24`).
    RLowerLeg,
    /// Left hip (`25`).
    LeftHip,
    /// Left upper leg (`26`).
    LUpperLeg,
    /// Left lower leg (`27`).
    LLowerLeg,
    /// Stomach / belly (`28`).
    Stomach,
    /// Left pectoral (`29`).
    LeftPec,
    /// Right pectoral (`30`).
    RightPec,
    /// HUD centre 2 (`31`).
    HudCenter2,
    /// HUD top right (`32`).
    HudTopRight,
    /// HUD top (`33`).
    HudTop,
    /// HUD top left (`34`).
    HudTopLeft,
    /// HUD centre (`35`).
    HudCenter,
    /// HUD bottom left (`36`).
    HudBottomLeft,
    /// HUD bottom (`37`).
    HudBottom,
    /// HUD bottom right (`38`).
    HudBottomRight,
    /// Neck (`39`).
    Neck,
    /// Avatar centre / root (`40`).
    AvatarCenter,
    /// Left ring finger (`41`).
    LeftRingFinger,
    /// Right ring finger (`42`).
    RightRingFinger,
    /// Tail base (`43`).
    TailBase,
    /// Tail tip (`44`).
    TailTip,
    /// Left wing (`45`).
    LeftWing,
    /// Right wing (`46`).
    RightWing,
    /// Jaw (`47`).
    Jaw,
    /// Alternate left ear (`48`).
    AltLeftEar,
    /// Alternate right ear (`49`).
    AltRightEar,
    /// Alternate left eye (`50`).
    AltLeftEye,
    /// Alternate right eye (`51`).
    AltRightEye,
    /// Tongue (`52`).
    Tongue,
    /// Groin (`53`).
    Groin,
    /// Left hind foot (`54`).
    LeftHindFoot,
    /// Right hind foot (`55`).
    RightHindFoot,
    /// An unknown / future attachment point, preserving the raw wire byte (the
    /// point only, with the `ATTACHMENT_ADD` flag already stripped).
    Other(u8),
}

impl AttachmentPoint {
    /// The `ATTACHMENT_ADD` wire flag (`0x80`): set in the attachment-point byte
    /// to add an attachment to the point rather than replace what is there.
    pub const ADD_FLAG: u8 = 0x80;

    /// The attachment-point wire byte for this slot (the point only, without the
    /// [`ADD_FLAG`](Self::ADD_FLAG); combine with [`with_mode`](Self::with_mode)).
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Default => 0,
            Self::Chest => 1,
            Self::Skull => 2,
            Self::LeftShoulder => 3,
            Self::RightShoulder => 4,
            Self::LeftHand => 5,
            Self::RightHand => 6,
            Self::LeftFoot => 7,
            Self::RightFoot => 8,
            Self::Spine => 9,
            Self::Pelvis => 10,
            Self::Mouth => 11,
            Self::Chin => 12,
            Self::LeftEar => 13,
            Self::RightEar => 14,
            Self::LeftEyeball => 15,
            Self::RightEyeball => 16,
            Self::Nose => 17,
            Self::RUpperArm => 18,
            Self::RForearm => 19,
            Self::LUpperArm => 20,
            Self::LForearm => 21,
            Self::RightHip => 22,
            Self::RUpperLeg => 23,
            Self::RLowerLeg => 24,
            Self::LeftHip => 25,
            Self::LUpperLeg => 26,
            Self::LLowerLeg => 27,
            Self::Stomach => 28,
            Self::LeftPec => 29,
            Self::RightPec => 30,
            Self::HudCenter2 => 31,
            Self::HudTopRight => 32,
            Self::HudTop => 33,
            Self::HudTopLeft => 34,
            Self::HudCenter => 35,
            Self::HudBottomLeft => 36,
            Self::HudBottom => 37,
            Self::HudBottomRight => 38,
            Self::Neck => 39,
            Self::AvatarCenter => 40,
            Self::LeftRingFinger => 41,
            Self::RightRingFinger => 42,
            Self::TailBase => 43,
            Self::TailTip => 44,
            Self::LeftWing => 45,
            Self::RightWing => 46,
            Self::Jaw => 47,
            Self::AltLeftEar => 48,
            Self::AltRightEar => 49,
            Self::AltLeftEye => 50,
            Self::AltRightEye => 51,
            Self::Tongue => 52,
            Self::Groin => 53,
            Self::LeftHindFoot => 54,
            Self::RightHindFoot => 55,
            Self::Other(code) => code,
        }
    }

    /// Classifies an attachment-point wire byte (the point only — strip the
    /// [`ADD_FLAG`](Self::ADD_FLAG) first, e.g. via [`split_code`](Self::split_code));
    /// codes outside the known range become [`Other`](Self::Other).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Default,
            1 => Self::Chest,
            2 => Self::Skull,
            3 => Self::LeftShoulder,
            4 => Self::RightShoulder,
            5 => Self::LeftHand,
            6 => Self::RightHand,
            7 => Self::LeftFoot,
            8 => Self::RightFoot,
            9 => Self::Spine,
            10 => Self::Pelvis,
            11 => Self::Mouth,
            12 => Self::Chin,
            13 => Self::LeftEar,
            14 => Self::RightEar,
            15 => Self::LeftEyeball,
            16 => Self::RightEyeball,
            17 => Self::Nose,
            18 => Self::RUpperArm,
            19 => Self::RForearm,
            20 => Self::LUpperArm,
            21 => Self::LForearm,
            22 => Self::RightHip,
            23 => Self::RUpperLeg,
            24 => Self::RLowerLeg,
            25 => Self::LeftHip,
            26 => Self::LUpperLeg,
            27 => Self::LLowerLeg,
            28 => Self::Stomach,
            29 => Self::LeftPec,
            30 => Self::RightPec,
            31 => Self::HudCenter2,
            32 => Self::HudTopRight,
            33 => Self::HudTop,
            34 => Self::HudTopLeft,
            35 => Self::HudCenter,
            36 => Self::HudBottomLeft,
            37 => Self::HudBottom,
            38 => Self::HudBottomRight,
            39 => Self::Neck,
            40 => Self::AvatarCenter,
            41 => Self::LeftRingFinger,
            42 => Self::RightRingFinger,
            43 => Self::TailBase,
            44 => Self::TailTip,
            45 => Self::LeftWing,
            46 => Self::RightWing,
            47 => Self::Jaw,
            48 => Self::AltLeftEar,
            49 => Self::AltRightEar,
            50 => Self::AltLeftEye,
            51 => Self::AltRightEye,
            52 => Self::Tongue,
            53 => Self::Groin,
            54 => Self::LeftHindFoot,
            55 => Self::RightHindFoot,
            other => Self::Other(other),
        }
    }

    /// The full wire byte combining this point with the attachment `mode`: sets
    /// the [`ADD_FLAG`](Self::ADD_FLAG) bit for [`AttachmentMode::Add`].
    #[must_use]
    pub const fn with_mode(self, mode: AttachmentMode) -> u8 {
        match mode {
            AttachmentMode::Add => self.to_code() | Self::ADD_FLAG,
            AttachmentMode::Replace => self.to_code(),
        }
    }

    /// Splits a raw attachment-point wire byte into its point and
    /// [`AttachmentMode`] (the inverse of [`with_mode`](Self::with_mode)).
    #[must_use]
    pub const fn split_code(byte: u8) -> (Self, AttachmentMode) {
        (
            Self::from_code(byte & !Self::ADD_FLAG),
            AttachmentMode::from_add_flag(byte & Self::ADD_FLAG != 0),
        )
    }

    /// Whether this is one of the HUD slots ([`HudCenter2`](Self::HudCenter2)
    /// through [`HudBottomRight`](Self::HudBottomRight), codes `31`–`38`), which
    /// attach to the agent's own screen rather than the avatar's body.
    #[must_use]
    pub const fn is_hud(self) -> bool {
        matches!(self.to_code(), 31..=38)
    }
}

/// Whether an attachment is *added* to its point alongside whatever is already
/// worn there, or *replaces* it. This is the `ATTACHMENT_ADD` wire flag
/// ([`AttachmentPoint::ADD_FLAG`], `0x80`) modelled as a named intent rather
/// than a bare `bool`, carried by the attachment commands
/// ([`Command::AttachObject`](crate::Command::AttachObject),
/// [`Command::RezAttachment`](crate::Command::RezAttachment)) and their matching
/// server events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentMode {
    /// Add the object to the point *alongside* anything already worn there (the
    /// `ATTACHMENT_ADD` flag is set).
    Add,
    /// Replace whatever is currently on the point (the `ATTACHMENT_ADD` flag is
    /// clear). The simulator's historical default for a single attachment.
    Replace,
}

impl AttachmentMode {
    /// Whether this mode sets the `ATTACHMENT_ADD` wire flag
    /// ([`AttachmentPoint::ADD_FLAG`]): `true` for [`Add`](Self::Add), `false`
    /// for [`Replace`](Self::Replace).
    #[must_use]
    pub const fn is_add(self) -> bool {
        matches!(self, Self::Add)
    }

    /// The mode for an `ATTACHMENT_ADD` flag bit: [`Add`](Self::Add) when set,
    /// [`Replace`](Self::Replace) when clear.
    #[must_use]
    pub const fn from_add_flag(add: bool) -> Self {
        if add { Self::Add } else { Self::Replace }
    }
}

/// Whether wearing a batch of attachments should first *detach everything
/// currently worn* — replacing the whole outfit — or *keep* what is already worn
/// and add the batch alongside it. This is the `FirstDetachAll` wire flag on
/// `RezMultipleAttachmentsFromInv`, modelled as a named intent rather than a
/// bare `bool`, carried by
/// [`Command::RezAttachments`](crate::Command::RezAttachments) and its matching
/// server event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetachOrder {
    /// Detach everything currently worn before wearing the batch — replacing the
    /// whole outfit (the `FirstDetachAll` flag is set).
    DetachAllFirst,
    /// Keep whatever is already worn and add the batch alongside it (the
    /// `FirstDetachAll` flag is clear).
    Keep,
}

impl DetachOrder {
    /// Whether this order sets the `FirstDetachAll` wire flag: `true` for
    /// [`DetachAllFirst`](Self::DetachAllFirst), `false` for [`Keep`](Self::Keep).
    #[must_use]
    pub const fn detaches_all_first(self) -> bool {
        matches!(self, Self::DetachAllFirst)
    }

    /// The order for a `FirstDetachAll` flag bit:
    /// [`DetachAllFirst`](Self::DetachAllFirst) when set, [`Keep`](Self::Keep)
    /// when clear.
    #[must_use]
    pub const fn from_first_detach_all(first_detach_all: bool) -> Self {
        if first_detach_all {
            Self::DetachAllFirst
        } else {
            Self::Keep
        }
    }
}

/// An inventory item to wear as an attachment, passed to
/// [`Command::RezAttachment`](crate::Command::RezAttachment) and
/// [`Command::RezAttachments`](crate::Command::RezAttachments) (the
/// `RezSingleAttachmentFromInv` / `RezMultipleAttachmentsFromInv` messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RezAttachment {
    /// The inventory item id to wear.
    pub item_id: Uuid,
    /// The item's owner id (the agent's own id for an item from its inventory).
    pub owner_id: Uuid,
    /// The attachment point to wear it on ([`AttachmentPoint::Default`] lets the
    /// simulator pick the item's saved/scripted slot).
    pub attachment_point: AttachmentPoint,
    /// Whether to *add* the attachment alongside anything already on the point
    /// or *replace* it; the `ATTACHMENT_ADD` flag.
    pub mode: AttachmentMode,
    /// The item's name (sent verbatim; the simulator ignores it).
    pub name: String,
    /// The item's description (sent verbatim; the simulator ignores it).
    pub description: String,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    /// A bare [`TextureFace`] with a given packed `bump_shiny_fullbright` and
    /// `media_flags`, all other fields irrelevant to the accessor under test.
    fn face(bump_shiny_fullbright: u8, media_flags: u8) -> super::TextureFace {
        super::TextureFace {
            texture_id: super::Uuid::nil(),
            color: [255; 4],
            scale_s: 1.0,
            scale_t: 1.0,
            offset_s: 0.0,
            offset_t: 0.0,
            rotation: 0.0,
            bump_shiny_fullbright,
            media_flags,
            glow: 0.0,
            material_id: super::Uuid::nil(),
        }
    }

    #[test]
    fn texture_face_unpacks_bump_shiny_fullbright() {
        // bump = 0x05 (low 5 bits), fullbright = bit 5, shiny = top 2 bits = 2.
        let f = face(0x05 | 0x20 | (0b10 << 6), 0);
        assert_eq!(f.bumpmap(), 0x05);
        assert!(f.fullbright());
        assert_eq!(f.shininess(), 0b10);
        // A plain face: nothing set.
        let plain = face(0, 0);
        assert_eq!(plain.bumpmap(), 0);
        assert!(!plain.fullbright());
        assert_eq!(plain.shininess(), 0);
    }

    #[test]
    fn texture_face_unpacks_media_flags() {
        // media bit set, tex-gen = planar (0b10 in bits 1-2 -> value 0x02).
        let f = face(0, 0x01 | 0x02);
        assert!(f.media_enabled());
        assert_eq!(f.tex_gen(), 0x02);
        let plain = face(0, 0);
        assert!(!plain.media_enabled());
        assert_eq!(plain.tex_gen(), 0);
    }

    #[test]
    fn attachment_mode_maps_to_add_flag() {
        use super::AttachmentMode;
        assert!(AttachmentMode::Add.is_add());
        assert!(!AttachmentMode::Replace.is_add());
        assert_eq!(AttachmentMode::from_add_flag(true), AttachmentMode::Add);
        assert_eq!(
            AttachmentMode::from_add_flag(false),
            AttachmentMode::Replace
        );
    }

    #[test]
    fn detach_order_maps_to_first_detach_all_flag() {
        use super::DetachOrder;
        assert!(DetachOrder::DetachAllFirst.detaches_all_first());
        assert!(!DetachOrder::Keep.detaches_all_first());
        assert_eq!(
            DetachOrder::from_first_detach_all(true),
            DetachOrder::DetachAllFirst
        );
        assert_eq!(DetachOrder::from_first_detach_all(false), DetachOrder::Keep);
        // The flag round-trips bit-identically to the historical `bool`.
        for order in [DetachOrder::DetachAllFirst, DetachOrder::Keep] {
            assert_eq!(
                DetachOrder::from_first_detach_all(order.detaches_all_first()),
                order
            );
        }
    }

    #[test]
    fn with_mode_and_split_code_round_trip_bit_identically() {
        use super::{AttachmentMode, AttachmentPoint};
        // `with_mode(Add)` must set bit 0x80, `Replace` must leave it clear —
        // byte-identical to the historical `with_add(true/false)` behaviour.
        assert_eq!(
            AttachmentPoint::RightHand.with_mode(AttachmentMode::Add),
            0x80 | 6
        );
        assert_eq!(
            AttachmentPoint::RightHand.with_mode(AttachmentMode::Replace),
            6
        );
        // `split_code` is the exact inverse for every point/mode pair.
        for point in [
            AttachmentPoint::Default,
            AttachmentPoint::Chest,
            AttachmentPoint::LeftHand,
            AttachmentPoint::RightHindFoot,
            AttachmentPoint::Other(99),
        ] {
            for mode in [AttachmentMode::Add, AttachmentMode::Replace] {
                let byte = point.with_mode(mode);
                assert_eq!(super::AttachmentPoint::split_code(byte), (point, mode));
            }
        }
    }
}
