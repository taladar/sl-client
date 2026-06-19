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
}
