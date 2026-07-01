//! Assets, textures, and transfer value types.

use sl_types::key::TextureKey;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Asset & texture pipeline (#19): asset/texture fetch value types.
// ---------------------------------------------------------------------------

/// The Second Life asset class (`LLAssetType` / `AT_*`), identifying what kind
/// of asset a UUID names. Used to build a generic asset
/// [transfer](crate::Session::request_asset) and to pick the
/// [`GetAsset`](crate::CAP_GET_ASSET) HTTP query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AssetType {
    /// A texture (`AT_TEXTURE`, a JPEG-2000 / `.j2c` image).
    Texture,
    /// A sound clip (`AT_SOUND`).
    Sound,
    /// A calling card (`AT_CALLINGCARD`).
    CallingCard,
    /// A landmark (`AT_LANDMARK`).
    Landmark,
    /// A wearable clothing layer (`AT_CLOTHING`).
    Clothing,
    /// An object / coalesced object (`AT_OBJECT`).
    Object,
    /// A notecard (`AT_NOTECARD`).
    Notecard,
    /// Script source text — LSL or Lua/SLua (`AT_LSL_TEXT`; both languages share
    /// this asset type, distinguished by the inventory item's language subtype).
    ScriptText,
    /// Compiled script bytecode (`AT_LSL_BYTECODE`).
    ScriptBytecode,
    /// A TGA texture (`AT_TEXTURE_TGA`).
    TextureTga,
    /// A wearable body part (`AT_BODYPART`).
    Bodypart,
    /// A WAV sound (`AT_SOUND_WAV`).
    SoundWav,
    /// A TGA image (`AT_IMAGE_TGA`).
    ImageTga,
    /// A JPEG image (`AT_IMAGE_JPEG`).
    ImageJpeg,
    /// An animation (`AT_ANIMATION`).
    Animation,
    /// A gesture (`AT_GESTURE`).
    Gesture,
    /// A mesh (`AT_MESH`).
    Mesh,
    /// A settings asset (`AT_SETTINGS`).
    Settings,
    /// A render material (`AT_MATERIAL`), an LLSD-wrapped GLTF 2.0 material
    /// document.
    Material,
    /// A glTF document (`AT_GLTF`).
    Gltf,
    /// A glTF binary buffer (`AT_GLTF_BIN`).
    GltfBin,
    /// An inventory folder / category (`AT_CATEGORY`), used as the leading
    /// byte of an inventory-offer binary bucket when a whole folder is offered.
    Folder,
    /// Any other / unrecognised asset class, carrying the raw `AT_*` code.
    Other(i32),
}

impl AssetType {
    /// The numeric `LLAssetType` code for this asset class, as sent in a
    /// `TransferRequest` `Params` block.
    #[must_use]
    pub const fn to_code(self) -> i32 {
        match self {
            Self::Texture => 0,
            Self::Sound => 1,
            Self::CallingCard => 2,
            Self::Landmark => 3,
            Self::Clothing => 5,
            Self::Object => 6,
            Self::Notecard => 7,
            Self::ScriptText => 10,
            Self::ScriptBytecode => 11,
            Self::TextureTga => 12,
            Self::Bodypart => 13,
            Self::SoundWav => 17,
            Self::ImageTga => 18,
            Self::ImageJpeg => 19,
            Self::Animation => 20,
            Self::Gesture => 21,
            Self::Mesh => 49,
            Self::Settings => 56,
            Self::Material => 57,
            Self::Gltf => 58,
            Self::GltfBin => 59,
            Self::Folder => 8,
            Self::Other(code) => code,
        }
    }

    /// Classifies an `LLAssetType` code (unknown codes become
    /// [`Other`](Self::Other)).
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Texture,
            1 => Self::Sound,
            2 => Self::CallingCard,
            3 => Self::Landmark,
            5 => Self::Clothing,
            6 => Self::Object,
            7 => Self::Notecard,
            10 => Self::ScriptText,
            11 => Self::ScriptBytecode,
            12 => Self::TextureTga,
            13 => Self::Bodypart,
            17 => Self::SoundWav,
            18 => Self::ImageTga,
            19 => Self::ImageJpeg,
            20 => Self::Animation,
            21 => Self::Gesture,
            49 => Self::Mesh,
            56 => Self::Settings,
            57 => Self::Material,
            58 => Self::Gltf,
            59 => Self::GltfBin,
            8 => Self::Folder,
            other => Self::Other(other),
        }
    }

    /// The query-parameter name the OpenSim/Second Life `GetAsset` capability
    /// expects for this asset class (e.g. `"texture_id"`, `"sound_id"`), or
    /// `None` for classes the cap does not serve by UUID.
    #[must_use]
    pub const fn get_asset_query_key(self) -> Option<&'static str> {
        match self {
            Self::Texture => Some("texture_id"),
            Self::Sound => Some("sound_id"),
            Self::CallingCard => Some("callcard_id"),
            Self::Landmark => Some("landmark_id"),
            Self::Clothing => Some("clothing_id"),
            Self::Object => Some("object_id"),
            Self::Notecard => Some("notecard_id"),
            Self::ScriptText => Some("lsltext_id"),
            Self::ScriptBytecode => Some("lslbyte_id"),
            Self::TextureTga => Some("txtr_tga_id"),
            Self::Bodypart => Some("bodypart_id"),
            Self::SoundWav => Some("snd_wav_id"),
            Self::ImageTga => Some("img_tga_id"),
            Self::ImageJpeg => Some("jpeg_id"),
            Self::Animation => Some("animatn_id"),
            Self::Gesture => Some("gesture_id"),
            Self::Mesh => Some("mesh_id"),
            Self::Settings => Some("settings_id"),
            // Second Life serves materials over the `ViewerAsset` cap by
            // `material_id`; the legacy `RenderMaterials` cap is the OpenSim path.
            Self::Material => Some("material_id"),
            Self::Gltf | Self::GltfBin | Self::Folder | Self::Other(_) => None,
        }
    }

    /// The short asset-type name the CAPS upload (`NewFileAgentInventory`)
    /// expects for this asset class (LL's `LLAssetType` `mTypeName`, e.g.
    /// `"texture"`, `"animatn"`, `"lsltext"`), or `None` for classes that are not
    /// uploaded by this capability.
    #[must_use]
    pub const fn caps_asset_name(self) -> Option<&'static str> {
        match self {
            Self::Texture => Some("texture"),
            Self::Sound => Some("sound"),
            Self::CallingCard => Some("callcard"),
            Self::Landmark => Some("landmark"),
            Self::Clothing => Some("clothing"),
            Self::Object => Some("object"),
            Self::Notecard => Some("notecard"),
            Self::ScriptText => Some("lsltext"),
            Self::ScriptBytecode => Some("lslbyte"),
            Self::TextureTga => Some("txtr_tga"),
            Self::Bodypart => Some("bodypart"),
            Self::SoundWav => Some("snd_wav"),
            Self::ImageTga => Some("img_tga"),
            Self::ImageJpeg => Some("jpeg"),
            Self::Animation => Some("animatn"),
            Self::Gesture => Some("gesture"),
            Self::Mesh => Some("mesh"),
            Self::Settings => Some("settings"),
            Self::Material => Some("material"),
            Self::Gltf => Some("gltf"),
            Self::GltfBin => Some("glbin"),
            Self::Folder | Self::Other(_) => None,
        }
    }

    /// Classifies a short asset-type name (LL's `LLAssetType` `mTypeName`, the
    /// inverse of [`caps_asset_name`](Self::caps_asset_name)) as written into a
    /// task-inventory `Xfer` listing by `Utils.AssetTypeToString`. The legacy
    /// `"script"` (the deprecated `AT_SCRIPT` bytecode, code 4) has no enum
    /// variant, so it maps to [`Other(4)`](Self::Other); any unrecognised name
    /// maps to the [`Other(-1)`](Self::Other) sentinel.
    #[must_use]
    pub fn from_type_name(name: &str) -> Self {
        match name {
            "texture" => Self::Texture,
            "sound" => Self::Sound,
            "callcard" => Self::CallingCard,
            "landmark" => Self::Landmark,
            "clothing" => Self::Clothing,
            "object" => Self::Object,
            "notecard" => Self::Notecard,
            "lsltext" => Self::ScriptText,
            "lslbyte" => Self::ScriptBytecode,
            "txtr_tga" => Self::TextureTga,
            "bodypart" => Self::Bodypart,
            "snd_wav" => Self::SoundWav,
            "img_tga" => Self::ImageTga,
            "jpeg" => Self::ImageJpeg,
            "animatn" => Self::Animation,
            "gesture" => Self::Gesture,
            "mesh" => Self::Mesh,
            "settings" => Self::Settings,
            "material" => Self::Material,
            "gltf" => Self::Gltf,
            "glbin" => Self::GltfBin,
            "category" | "root" => Self::Folder,
            "script" => Self::Other(4),
            _ => Self::Other(-1),
        }
    }

    /// The name of the capability that updates an *existing* inventory item's
    /// asset for this asset class (the modern in-place edit path:
    /// `UpdateGestureAgentInventory`, `UpdateNotecardAgentInventory`,
    /// `UpdateScriptAgent`, `UpdateSettingsAgentInventory`), or `None` for classes
    /// with no such capability (use the
    /// [`new-asset upload`](Self::caps_asset_name) path instead).
    #[must_use]
    pub const fn update_item_cap(self) -> Option<&'static str> {
        match self {
            Self::Gesture => Some("UpdateGestureAgentInventory"),
            Self::Notecard => Some("UpdateNotecardAgentInventory"),
            Self::ScriptText => Some("UpdateScriptAgent"),
            Self::Settings => Some("UpdateSettingsAgentInventory"),
            Self::Material => Some("UpdateMaterialAgentInventory"),
            _ => None,
        }
    }

    /// Whether this is a script asset class ([`ScriptText`](Self::ScriptText) or
    /// [`ScriptBytecode`](Self::ScriptBytecode)). Scripts do not travel the
    /// generic asset-upload commands — they go through
    /// [`Command::UploadScript`](crate::Command::UploadScript) so compile results
    /// are surfaced — so the generic upload dispatch rejects a script class.
    #[must_use]
    pub const fn is_script(self) -> bool {
        matches!(self, Self::ScriptText | Self::ScriptBytecode)
    }
}

/// The asset classes whose *existing* inventory item can have its asset replaced
/// through the generic `Update*AgentInventory` capability path
/// ([`Command::UpdateInventoryAsset`](crate::Command::UpdateInventoryAsset)) — a
/// **script-excluding** narrowing of [`AssetType`].
///
/// Scripts are deliberately absent: an LSL/Lua script must be uploaded via
/// [`Command::UploadScript`](crate::Command::UploadScript) so the simulator's
/// compile result (success and any compiler errors) is surfaced rather than
/// silently discarded as a plain asset write. Because there is no script variant
/// here, updating a script through the generic path is unrepresentable — a
/// compile error, not a runtime check. Widen to an [`AssetType`] with
/// [`From`]/[`Into`]; narrow a raw [`AssetType`] with [`TryFrom`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UpdatableAssetType {
    /// A gesture (`UpdateGestureAgentInventory`).
    Gesture,
    /// A notecard (`UpdateNotecardAgentInventory`).
    Notecard,
    /// A settings asset (`UpdateSettingsAgentInventory`).
    Settings,
    /// A render material (`UpdateMaterialAgentInventory`).
    Material,
}

impl UpdatableAssetType {
    /// The capability that updates an existing inventory item's asset for this
    /// class. Unlike [`AssetType::update_item_cap`] this is **total** — every
    /// [`UpdatableAssetType`] has an update capability.
    #[must_use]
    pub const fn cap(self) -> &'static str {
        match self {
            Self::Gesture => "UpdateGestureAgentInventory",
            Self::Notecard => "UpdateNotecardAgentInventory",
            Self::Settings => "UpdateSettingsAgentInventory",
            Self::Material => "UpdateMaterialAgentInventory",
        }
    }
}

impl From<UpdatableAssetType> for AssetType {
    /// Infallible widening — every [`UpdatableAssetType`] is an [`AssetType`].
    fn from(value: UpdatableAssetType) -> Self {
        match value {
            UpdatableAssetType::Gesture => Self::Gesture,
            UpdatableAssetType::Notecard => Self::Notecard,
            UpdatableAssetType::Settings => Self::Settings,
            UpdatableAssetType::Material => Self::Material,
        }
    }
}

/// The error from narrowing an [`AssetType`] that has no generic in-place update
/// capability (a script — use
/// [`Command::UploadScript`](crate::Command::UploadScript) — or any class the
/// `Update*AgentInventory` path does not serve) into an [`UpdatableAssetType`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("asset type {0:?} has no generic inventory-update capability")]
pub struct NotUpdatableAssetType(pub AssetType);

impl TryFrom<AssetType> for UpdatableAssetType {
    type Error = NotUpdatableAssetType;

    /// Fallible narrowing — a script (or any non-updatable class) is rejected.
    fn try_from(value: AssetType) -> Result<Self, Self::Error> {
        match value {
            AssetType::Gesture => Ok(Self::Gesture),
            AssetType::Notecard => Ok(Self::Notecard),
            AssetType::Settings => Ok(Self::Settings),
            AssetType::Material => Ok(Self::Material),
            other => Err(NotUpdatableAssetType(other)),
        }
    }
}

/// The Second Life inventory-item class (`LLInventoryType` / `IT_*`), describing
/// how an inventory item behaves (as opposed to [`AssetType`], which describes
/// the underlying asset bytes). One asset class can map to several inventory
/// types — a `Texture` asset can be an ordinary [`Texture`](Self::Texture) or a
/// [`Snapshot`](Self::Snapshot); a `Clothing`/`Bodypart` asset is a
/// [`Wearable`](Self::Wearable). Used to build the CAPS upload
/// (`NewFileAgentInventory`) request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InventoryType {
    /// A texture (`IT_TEXTURE`).
    Texture,
    /// A sound clip (`IT_SOUND`).
    Sound,
    /// A calling card (`IT_CALLINGCARD`).
    CallingCard,
    /// A landmark (`IT_LANDMARK`).
    Landmark,
    /// An object / attachment (`IT_OBJECT`).
    Object,
    /// A notecard (`IT_NOTECARD`).
    Notecard,
    /// A folder / category (`IT_CATEGORY`).
    Category,
    /// An LSL script (`IT_LSL`).
    Script,
    /// A snapshot photo (`IT_SNAPSHOT`).
    Snapshot,
    /// A worn attachment (`IT_ATTACHMENT`).
    Attachment,
    /// A wearable (clothing or body part) (`IT_WEARABLE`).
    Wearable,
    /// An animation (`IT_ANIMATION`).
    Animation,
    /// A gesture (`IT_GESTURE`).
    Gesture,
    /// A mesh (`IT_MESH`).
    Mesh,
    /// A settings asset (`IT_SETTINGS`).
    Settings,
    /// A render material (`IT_MATERIAL`).
    Material,
    /// Any other / unrecognised inventory type, carrying the raw `IT_*` code.
    Other(i32),
}

impl InventoryType {
    /// The numeric `LLInventoryType` code for this inventory class.
    #[must_use]
    pub const fn to_code(self) -> i32 {
        match self {
            Self::Texture => 0,
            Self::Sound => 1,
            Self::CallingCard => 2,
            Self::Landmark => 3,
            Self::Object => 6,
            Self::Notecard => 7,
            Self::Category => 8,
            Self::Script => 10,
            Self::Snapshot => 15,
            Self::Attachment => 17,
            Self::Wearable => 18,
            Self::Animation => 19,
            Self::Gesture => 20,
            Self::Mesh => 22,
            Self::Settings => 25,
            Self::Material => 57,
            Self::Other(code) => code,
        }
    }

    /// Classifies an `LLInventoryType` code (unknown codes become
    /// [`Other`](Self::Other)).
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Texture,
            1 => Self::Sound,
            2 => Self::CallingCard,
            3 => Self::Landmark,
            6 => Self::Object,
            7 => Self::Notecard,
            8 => Self::Category,
            10 => Self::Script,
            15 => Self::Snapshot,
            17 => Self::Attachment,
            18 => Self::Wearable,
            19 => Self::Animation,
            20 => Self::Gesture,
            22 => Self::Mesh,
            25 => Self::Settings,
            57 => Self::Material,
            other => Self::Other(other),
        }
    }

    /// The short inventory-type name the CAPS upload (`NewFileAgentInventory`)
    /// expects (LL's `LLInventoryType` `mName`, e.g. `"texture"`, `"wearable"`,
    /// `"script"`), or `None` for [`Other`](Self::Other).
    #[must_use]
    pub const fn caps_name(self) -> Option<&'static str> {
        match self {
            Self::Texture => Some("texture"),
            Self::Sound => Some("sound"),
            Self::CallingCard => Some("callcard"),
            Self::Landmark => Some("landmark"),
            Self::Object => Some("object"),
            Self::Notecard => Some("notecard"),
            Self::Category => Some("category"),
            Self::Script => Some("script"),
            Self::Snapshot => Some("snapshot"),
            Self::Attachment => Some("attach"),
            Self::Wearable => Some("wearable"),
            Self::Animation => Some("animation"),
            Self::Gesture => Some("gesture"),
            Self::Mesh => Some("mesh"),
            Self::Settings => Some("settings"),
            Self::Material => Some("material"),
            Self::Other(_) => None,
        }
    }

    /// Classifies a short inventory-type name (LL's `LLInventoryType` `mName`,
    /// the inverse of [`caps_name`](Self::caps_name)) as written into a
    /// task-inventory `Xfer` listing by `Utils.InventoryTypeToString`. Accepts
    /// `"lsl"` as an alias for the LSL [`Script`](Self::Script) name; any
    /// unrecognised name maps to the [`Other(-1)`](Self::Other) sentinel.
    #[must_use]
    pub fn from_type_name(name: &str) -> Self {
        match name {
            "texture" => Self::Texture,
            "sound" => Self::Sound,
            "callcard" => Self::CallingCard,
            "landmark" => Self::Landmark,
            "object" => Self::Object,
            "notecard" => Self::Notecard,
            "category" => Self::Category,
            "script" | "lsl" => Self::Script,
            "snapshot" => Self::Snapshot,
            "attach" => Self::Attachment,
            "wearable" => Self::Wearable,
            "animation" => Self::Animation,
            "gesture" => Self::Gesture,
            "mesh" => Self::Mesh,
            "settings" => Self::Settings,
            "material" => Self::Material,
            _ => Self::Other(-1),
        }
    }
}

/// The image codec of a texture delivered over the legacy UDP image path
/// (`ImageData`'s `Codec` field / `EImageCodec`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageCodec {
    /// JPEG 2000 codestream (`IMG_CODEC_J2C`) — the normal Second Life texture
    /// format.
    J2c,
    /// Raw RGB (`IMG_CODEC_RGB`).
    Rgb,
    /// Windows bitmap (`IMG_CODEC_BMP`).
    Bmp,
    /// Targa (`IMG_CODEC_TGA`).
    Tga,
    /// JPEG (`IMG_CODEC_JPEG`).
    Jpeg,
    /// S3TC/DXT compressed (`IMG_CODEC_DXT`).
    Dxt,
    /// PNG (`IMG_CODEC_PNG`).
    Png,
    /// An invalid or unrecognised codec, carrying the raw byte.
    Other(u8),
}

impl ImageCodec {
    /// Classifies an `ImageData` `Codec` byte.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            2 => Self::J2c,
            1 => Self::Rgb,
            3 => Self::Bmp,
            4 => Self::Tga,
            5 => Self::Jpeg,
            6 => Self::Dxt,
            7 => Self::Png,
            other => Self::Other(other),
        }
    }
}

/// The status of a generic asset [transfer](crate::Session::request_asset)
/// (`LLTSCode`), reported in a `TransferInfo`/`TransferPacket`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransferStatus {
    /// In progress (`LLTS_OK`).
    Ok,
    /// The transfer completed successfully (`LLTS_DONE`).
    Done,
    /// The source asked to skip (`LLTS_SKIP`).
    Skip,
    /// The transfer was aborted (`LLTS_ABORT`).
    Abort,
    /// A generic error (`LLTS_ERROR`).
    Error,
    /// The asset does not exist — the transfer equivalent of a 404
    /// (`LLTS_UNKNOWN_SOURCE`).
    UnknownSource,
    /// The agent lacks permission to fetch the asset
    /// (`LLTS_INSUFFICIENT_PERMISSIONS`).
    InsufficientPermissions,
    /// Any other / unrecognised status code.
    Other(i32),
}

impl TransferStatus {
    /// Classifies an `LLTSCode` status integer.
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Ok,
            1 => Self::Done,
            2 => Self::Skip,
            3 => Self::Abort,
            -1 => Self::Error,
            -2 => Self::UnknownSource,
            -3 => Self::InsufficientPermissions,
            other => Self::Other(other),
        }
    }

    /// Whether this status indicates the transfer succeeded (`LLTS_DONE`).
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Done)
    }
}

/// A fetched texture: its asset id, the codec the simulator reported (UDP path)
/// and the raw encoded image bytes (a JPEG-2000 codestream for the usual
/// [`J2c`](ImageCodec::J2c) codec). The bytes are **not** decoded into pixels —
/// see [`crate::j2c`] for header parsing / LOD truncation helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    /// The texture's asset UUID.
    pub id: TextureKey,
    /// The codec of [`data`](Self::data). For the HTTP `GetTexture` path this is
    /// always [`J2c`](ImageCodec::J2c) (the cap serves a `.j2c` codestream).
    pub codec: ImageCodec,
    /// The raw encoded image bytes.
    pub data: Vec<u8>,
}

/// A fetched generic asset: its UUID, asset class and raw encoded bytes (a sound
/// clip, animation, notecard, landmark, mesh, …). Delivered over the UDP
/// transfer path or the HTTP `GetAsset`/`GetMesh` capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// The asset's UUID.
    pub id: Uuid,
    /// The asset class.
    pub asset_type: AssetType,
    /// The raw encoded asset bytes.
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::{ImageCodec, Texture, TextureKey};
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    /// A [`TextureKey`] is a transparent wrapper over the wire `Uuid`: wrapping a
    /// raw id and reading it back is the identity, so a [`Texture`] keyed by a
    /// `TextureKey` carries the exact same 16 bytes the wire did (the typed key
    /// changes nothing the simulator sees).
    #[test]
    fn texture_key_round_trips_raw_uuid() {
        let raw = Uuid::from_u128(0x89556747_24cb_43ed_920b_47caed15465f);
        assert_eq!(TextureKey::from(raw).uuid(), raw);
        let texture = Texture {
            id: TextureKey::from(raw),
            codec: ImageCodec::J2c,
            data: vec![1, 2, 3],
        };
        assert_eq!(texture.id.uuid(), raw);
        // The default (nil) texture id round-trips too.
        assert_eq!(TextureKey::from(Uuid::nil()).uuid(), Uuid::nil());
    }
}
