//! Nearby-avatar presence and viewer effects: coarse (minimap) locations, the
//! `ViewerEffect` look-at / point-at / beam machinery, and agent tracking.

use sl_types::key::AgentKey;
use sl_wire::{Reader, Writer};
use uuid::Uuid;

/// One avatar's coarse position, as carried by a `CoarseLocationUpdate`
/// (surfaced as [`Event::CoarseLocationUpdate`](crate::Event::CoarseLocationUpdate)).
///
/// These are the low-resolution positions a viewer draws on its minimap: each
/// coordinate is a whole metre relative to the region's south-west corner, so
/// the precision is one metre and heights above `1020` m are clamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoarseLocation {
    /// The avatar (or in-world object) the position belongs to.
    pub agent_id: AgentKey,
    /// Metres east of the region's south-west corner (`0`–`255`).
    pub x: u8,
    /// Metres north of the region's south-west corner (`0`–`255`).
    pub y: u8,
    /// Height in metres. On the wire this is a single byte in units of four
    /// metres, so the value is a multiple of four up to `1020`.
    pub z: u16,
}

/// The kind of a [`ViewerEffect`]: the viewer's HUD-effect type codes
/// (`LLHUDObject`'s effect enumeration). Most effects a normal viewer emits are
/// [`LookAt`](Self::LookAt), [`PointAt`](Self::PointAt) and the beam family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ViewerEffectType {
    /// Floating text (`LL_HUD_TEXT`, `0`).
    Text,
    /// A HUD icon (`LL_HUD_ICON`, `1`).
    Icon,
    /// A connector line (`LL_HUD_CONNECTOR`, `2`).
    Connector,
    /// A flexible object (`LL_HUD_FLEXIBLE_OBJECT`, `3`).
    FlexibleObject,
    /// Animal controls (`LL_HUD_ANIMAL_CONTROLS`, `4`).
    AnimalControls,
    /// A local animation object (`LL_HUD_LOCAL_ANIMATION_OBJECT`, `5`).
    LocalAnimationObject,
    /// Cloth (`LL_HUD_CLOTH`, `6`).
    Cloth,
    /// A beam, e.g. the editing/touch beam (`LL_HUD_EFFECT_BEAM`, `7`).
    Beam,
    /// A glow effect (`LL_HUD_EFFECT_GLOW`, `8`).
    Glow,
    /// A point effect (`LL_HUD_EFFECT_POINT`, `9`).
    Point,
    /// A trail effect (`LL_HUD_EFFECT_TRAIL`, `10`).
    Trail,
    /// A sphere effect (`LL_HUD_EFFECT_SPHERE`, `11`).
    Sphere,
    /// A spiral effect, e.g. the "ping" beam (`LL_HUD_EFFECT_SPIRAL`, `12`).
    Spiral,
    /// The edit beam shown while editing an object (`LL_HUD_EFFECT_EDIT`, `13`).
    Edit,
    /// An avatar's gaze direction (`LL_HUD_EFFECT_LOOKAT`, `14`).
    LookAt,
    /// An avatar's pointing gesture (`LL_HUD_EFFECT_POINTAT`, `15`).
    PointAt,
    /// The voice visualiser (`LL_HUD_EFFECT_VOICE_VISUALIZER`, `16`).
    VoiceVisualizer,
    /// A name tag (`LL_HUD_NAME_TAG`, `17`).
    NameTag,
    /// A blob effect (`LL_HUD_EFFECT_BLOB`, `18`).
    Blob,
    /// A skeleton reset (`LL_HUD_EFFECT_RESET_SKELETON`, `19`).
    ResetSkeleton,
    /// An unknown / future effect type, preserving the raw wire byte.
    Other(u8),
}

impl ViewerEffectType {
    /// The wire byte for this effect type.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::Text => 0,
            Self::Icon => 1,
            Self::Connector => 2,
            Self::FlexibleObject => 3,
            Self::AnimalControls => 4,
            Self::LocalAnimationObject => 5,
            Self::Cloth => 6,
            Self::Beam => 7,
            Self::Glow => 8,
            Self::Point => 9,
            Self::Trail => 10,
            Self::Sphere => 11,
            Self::Spiral => 12,
            Self::Edit => 13,
            Self::LookAt => 14,
            Self::PointAt => 15,
            Self::VoiceVisualizer => 16,
            Self::NameTag => 17,
            Self::Blob => 18,
            Self::ResetSkeleton => 19,
            Self::Other(code) => code,
        }
    }

    /// Classifies an effect-type wire byte; codes outside the known range become
    /// [`Other`](Self::Other).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Text,
            1 => Self::Icon,
            2 => Self::Connector,
            3 => Self::FlexibleObject,
            4 => Self::AnimalControls,
            5 => Self::LocalAnimationObject,
            6 => Self::Cloth,
            7 => Self::Beam,
            8 => Self::Glow,
            9 => Self::Point,
            10 => Self::Trail,
            11 => Self::Sphere,
            12 => Self::Spiral,
            13 => Self::Edit,
            14 => Self::LookAt,
            15 => Self::PointAt,
            16 => Self::VoiceVisualizer,
            17 => Self::NameTag,
            18 => Self::Blob,
            19 => Self::ResetSkeleton,
            other => Self::Other(other),
        }
    }

    /// Whether this type uses the 56-byte `LLHUDEffectSpiral` `TypeData` layout
    /// (the beam/glow/point/sphere/spiral/edit family).
    #[must_use]
    const fn is_spiral_family(self) -> bool {
        matches!(
            self,
            Self::Beam | Self::Glow | Self::Point | Self::Sphere | Self::Spiral | Self::Edit
        )
    }
}

/// What an avatar's gaze (a [`ViewerEffectType::LookAt`] effect) is directed at
/// (`ELookAtType`). The numeric order doubles as a priority: higher targets win.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LookAtType {
    /// No look-at target (`0`).
    None,
    /// Idle gaze wander (`1`).
    Idle,
    /// Looking at whoever the avatar is auto-listening to (`2`).
    AutoListen,
    /// Free look (`3`).
    FreeLook,
    /// Responding to someone (`4`).
    Respond,
    /// Hover (`5`).
    Hover,
    /// In conversation (`6`).
    Conversation,
    /// Looking at a selection (`7`).
    Select,
    /// Looking at the focus point (`8`).
    Focus,
    /// Mouse-look (`9`).
    MouseLook,
    /// Clear the look-at (`10`).
    Clear,
    /// An unknown / future look-at type, preserving the raw wire byte.
    Other(u8),
}

impl LookAtType {
    /// The wire byte for this look-at type.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Idle => 1,
            Self::AutoListen => 2,
            Self::FreeLook => 3,
            Self::Respond => 4,
            Self::Hover => 5,
            Self::Conversation => 6,
            Self::Select => 7,
            Self::Focus => 8,
            Self::MouseLook => 9,
            Self::Clear => 10,
            Self::Other(code) => code,
        }
    }

    /// Classifies a look-at-type wire byte; codes outside the known range become
    /// [`Other`](Self::Other).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::None,
            1 => Self::Idle,
            2 => Self::AutoListen,
            3 => Self::FreeLook,
            4 => Self::Respond,
            5 => Self::Hover,
            6 => Self::Conversation,
            7 => Self::Select,
            8 => Self::Focus,
            9 => Self::MouseLook,
            10 => Self::Clear,
            other => Self::Other(other),
        }
    }
}

/// What an avatar's pointing gesture (a [`ViewerEffectType::PointAt`] effect) is
/// directed at (`EPointAtType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointAtType {
    /// No point-at target (`0`).
    None,
    /// Pointing at a selection (`1`).
    Select,
    /// Pointing at a grabbed object (`2`).
    Grab,
    /// Clear the point-at (`3`).
    Clear,
    /// An unknown / future point-at type, preserving the raw wire byte.
    Other(u8),
}

impl PointAtType {
    /// The wire byte for this point-at type.
    #[must_use]
    pub const fn to_code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Select => 1,
            Self::Grab => 2,
            Self::Clear => 3,
            Self::Other(code) => code,
        }
    }

    /// Classifies a point-at-type wire byte; codes outside the known range become
    /// [`Other`](Self::Other).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::None,
            1 => Self::Select,
            2 => Self::Grab,
            3 => Self::Clear,
            other => Self::Other(other),
        }
    }
}

/// The effect-specific `TypeData` payload of a [`ViewerEffect`].
///
/// The well-known layouts are decoded into typed variants; anything else (or a
/// payload whose length does not match its type) is kept verbatim as
/// [`Raw`](Self::Raw) so it still round-trips.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ViewerEffectData {
    /// An avatar's gaze target (the 57-byte `LLHUDEffectLookAt` layout): the
    /// source avatar, an optional target object, the target position (a global
    /// offset, or an offset from the target object when one is set), and the
    /// look-at kind.
    LookAt {
        /// The avatar the gaze belongs to (nil if absent).
        source: AgentKey,
        /// The object being looked at (nil for none).
        target: Uuid,
        /// The global target position, in metres.
        target_position: [f64; 3],
        /// What the gaze is directed at.
        look_at_type: LookAtType,
    },
    /// An avatar's pointing gesture (the 57-byte `LLHUDEffectPointAt` layout).
    PointAt {
        /// The avatar doing the pointing (nil if absent).
        source: AgentKey,
        /// The object being pointed at (nil for none).
        target: Uuid,
        /// The global target position, in metres.
        target_position: [f64; 3],
        /// What the gesture is directed at.
        point_at_type: PointAtType,
    },
    /// A beam / glow / point / sphere / spiral / edit effect (the 56-byte
    /// `LLHUDEffectSpiral` layout): a source object, an optional target object,
    /// and a global position.
    Spiral {
        /// The object the effect emanates from (nil if absent).
        source: Uuid,
        /// The object the effect points to (nil for none).
        target: Uuid,
        /// The global position, in metres (zero when unused).
        position: [f64; 3],
    },
    /// Any other or unrecognised `TypeData`, kept verbatim.
    Raw(Vec<u8>),
}

impl ViewerEffectData {
    /// Decodes the `TypeData` blob of an effect of type `effect_type`. Payloads
    /// that do not match a known layout (by type and length) are returned as
    /// [`Raw`](Self::Raw).
    #[must_use]
    pub fn from_wire(effect_type: ViewerEffectType, bytes: &[u8]) -> Self {
        match effect_type {
            ViewerEffectType::LookAt if bytes.len() == LOOKAT_SIZE => {
                Self::decode_lookat(bytes).unwrap_or_else(|| Self::Raw(bytes.to_vec()))
            }
            ViewerEffectType::PointAt if bytes.len() == LOOKAT_SIZE => {
                Self::decode_pointat(bytes).unwrap_or_else(|| Self::Raw(bytes.to_vec()))
            }
            other if other.is_spiral_family() && bytes.len() == SPIRAL_SIZE => {
                Self::decode_spiral(bytes).unwrap_or_else(|| Self::Raw(bytes.to_vec()))
            }
            _ => Self::Raw(bytes.to_vec()),
        }
    }

    /// Decodes a 57-byte look-at `TypeData` blob (`None` on a short read).
    fn decode_lookat(bytes: &[u8]) -> Option<Self> {
        let mut reader = Reader::new(bytes);
        let source = AgentKey::from(reader.uuid().ok()?);
        let target = reader.uuid().ok()?;
        let target_position = reader.vector3d().ok()?;
        let look_at_type = LookAtType::from_code(reader.u8().ok()?);
        Some(Self::LookAt {
            source,
            target,
            target_position,
            look_at_type,
        })
    }

    /// Decodes a 57-byte point-at `TypeData` blob (`None` on a short read).
    fn decode_pointat(bytes: &[u8]) -> Option<Self> {
        let mut reader = Reader::new(bytes);
        let source = AgentKey::from(reader.uuid().ok()?);
        let target = reader.uuid().ok()?;
        let target_position = reader.vector3d().ok()?;
        let point_at_type = PointAtType::from_code(reader.u8().ok()?);
        Some(Self::PointAt {
            source,
            target,
            target_position,
            point_at_type,
        })
    }

    /// Decodes a 56-byte spiral-family `TypeData` blob (`None` on a short read).
    fn decode_spiral(bytes: &[u8]) -> Option<Self> {
        let mut reader = Reader::new(bytes);
        let source = reader.uuid().ok()?;
        let target = reader.uuid().ok()?;
        let position = reader.vector3d().ok()?;
        Some(Self::Spiral {
            source,
            target,
            position,
        })
    }

    /// Encodes the `TypeData` blob for the wire (the inverse of
    /// [`from_wire`](Self::from_wire)).
    #[must_use]
    pub fn to_wire(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::LookAt {
                source,
                target,
                target_position,
                look_at_type,
            } => {
                writer.put_uuid(source.uuid());
                writer.put_uuid(*target);
                writer.put_vector3d(*target_position);
                writer.put_u8(look_at_type.to_code());
            }
            Self::PointAt {
                source,
                target,
                target_position,
                point_at_type,
            } => {
                writer.put_uuid(source.uuid());
                writer.put_uuid(*target);
                writer.put_vector3d(*target_position);
                writer.put_u8(point_at_type.to_code());
            }
            Self::Spiral {
                source,
                target,
                position,
            } => {
                writer.put_uuid(*source);
                writer.put_uuid(*target);
                writer.put_vector3d(*position);
            }
            Self::Raw(bytes) => return bytes.clone(),
        }
        writer.into_bytes()
    }
}

/// The 57-byte `TypeData` size of look-at / point-at effects.
const LOOKAT_SIZE: usize = 57;
/// The 56-byte `TypeData` size of the spiral-family effects.
const SPIRAL_SIZE: usize = 56;

/// A single viewer effect, as sent with [`Command::ViewerEffect`](crate::Command::ViewerEffect)
/// or received as [`Event::ViewerEffect`](crate::Event::ViewerEffect) (one entry
/// of a `ViewerEffect` message, which may batch several).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewerEffect {
    /// A unique id for the effect (a fresh UUID per effect).
    pub id: Uuid,
    /// The avatar the effect belongs to (the source agent).
    pub agent_id: AgentKey,
    /// The effect type.
    pub effect_type: ViewerEffectType,
    /// How long the effect lasts, in seconds.
    pub duration: f32,
    /// The effect colour, as `RGBA` bytes.
    pub color: [u8; 4],
    /// The effect-specific payload.
    pub data: ViewerEffectData,
}

#[cfg(test)]
mod tests {
    use sl_types::key::AgentKey;

    use super::{LookAtType, PointAtType, ViewerEffectData, ViewerEffectType};
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    /// Every effect-type code round-trips through `from_code`/`to_code`.
    #[test]
    fn effect_type_round_trips() {
        for code in 0_u8..=25 {
            assert_eq!(ViewerEffectType::from_code(code).to_code(), code);
        }
    }

    /// A look-at payload round-trips through `to_wire`/`from_wire`.
    #[test]
    fn lookat_round_trips() {
        let data = ViewerEffectData::LookAt {
            source: AgentKey::from(Uuid::from_u128(1)),
            target: Uuid::from_u128(2),
            target_position: [1.5, -2.5, 3.5],
            look_at_type: LookAtType::Focus,
        };
        let bytes = data.to_wire();
        assert_eq!(bytes.len(), 57);
        assert_eq!(
            ViewerEffectData::from_wire(ViewerEffectType::LookAt, &bytes),
            data
        );
    }

    /// A point-at payload round-trips through `to_wire`/`from_wire`.
    #[test]
    fn pointat_round_trips() {
        let data = ViewerEffectData::PointAt {
            source: AgentKey::from(Uuid::from_u128(3)),
            target: Uuid::from_u128(4),
            target_position: [0.0, 0.0, 0.0],
            point_at_type: PointAtType::Grab,
        };
        let bytes = data.to_wire();
        assert_eq!(bytes.len(), 57);
        assert_eq!(
            ViewerEffectData::from_wire(ViewerEffectType::PointAt, &bytes),
            data
        );
    }

    /// A beam (spiral-family) payload round-trips through `to_wire`/`from_wire`.
    #[test]
    fn spiral_round_trips() {
        let data = ViewerEffectData::Spiral {
            source: Uuid::from_u128(5),
            target: Uuid::from_u128(6),
            position: [10.0, 20.0, 30.0],
        };
        let bytes = data.to_wire();
        assert_eq!(bytes.len(), 56);
        assert_eq!(
            ViewerEffectData::from_wire(ViewerEffectType::Beam, &bytes),
            data
        );
    }

    /// A payload whose length does not match its type stays `Raw`.
    #[test]
    fn mismatched_length_stays_raw() {
        let raw = vec![1_u8, 2, 3];
        assert_eq!(
            ViewerEffectData::from_wire(ViewerEffectType::LookAt, &raw),
            ViewerEffectData::Raw(raw),
        );
    }
}
