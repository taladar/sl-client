//! The small enumerations and the permission-mask newtype that describe an
//! embedded inventory item, kept string-faithful so an unrecognised value
//! round-trips verbatim rather than being dropped.

/// The asset class of an embedded item, identified by the short type name the
/// simulator writes into the Linden-text stream (`LLAssetType`'s `mTypeName`,
/// e.g. `"notecard"`, `"landmark"`, `"lsltext"`).
///
/// Any name this crate does not recognise is preserved as
/// [`Other`](AssetType::Other) so it round-trips unchanged — the on-wire format
/// is defined by what the simulator emits, not by this list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetType {
    /// A JPEG2000 texture (`texture`).
    Texture,
    /// A streamed sound (`sound`).
    Sound,
    /// A calling card (`callcard`).
    CallingCard,
    /// A landmark (`landmark`).
    Landmark,
    /// The deprecated legacy script asset (`script`, `AT_SCRIPT`).
    Script,
    /// A clothing wearable (`clothing`).
    Clothing,
    /// An object (`object`).
    Object,
    /// A notecard (`notecard`).
    Notecard,
    /// A folder / category (`category`).
    Category,
    /// LSL source text (`lsltext`).
    LslText,
    /// Compiled LSL bytecode (`lslbyte`).
    LslBytecode,
    /// An uncompressed TGA texture (`txtr_tga`).
    TextureTga,
    /// A body-part wearable (`bodypart`).
    Bodypart,
    /// A WAV sound (`snd_wav`).
    SoundWav,
    /// A TGA image (`img_tga`).
    ImageTga,
    /// A JPEG image (`jpeg`).
    ImageJpeg,
    /// An animation (`animatn`).
    Animation,
    /// A gesture (`gesture`).
    Gesture,
    /// A region simstate (`simstate`).
    Simstate,
    /// An inventory link (`link`).
    Link,
    /// An inventory folder link (`link_f`).
    LinkFolder,
    /// A rigged / static mesh (`mesh`).
    Mesh,
    /// A UI widget (`widget`).
    Widget,
    /// A person record (`person`).
    Person,
    /// An environment settings blob (`settings`).
    Settings,
    /// A render material (`material`).
    Material,
    /// A glTF asset (`gltf`).
    Gltf,
    /// A glTF binary buffer (`glbin`).
    GltfBin,
    /// An unrecognised type name, preserved verbatim.
    Other(String),
}

impl AssetType {
    /// Classify the short asset-type name written into a Linden-text stream.
    #[must_use]
    pub fn from_type_name(name: &str) -> Self {
        match name {
            "texture" => Self::Texture,
            "sound" => Self::Sound,
            "callcard" => Self::CallingCard,
            "landmark" => Self::Landmark,
            "script" => Self::Script,
            "clothing" => Self::Clothing,
            "object" => Self::Object,
            "notecard" => Self::Notecard,
            "category" => Self::Category,
            "lsltext" => Self::LslText,
            "lslbyte" => Self::LslBytecode,
            "txtr_tga" => Self::TextureTga,
            "bodypart" => Self::Bodypart,
            "snd_wav" => Self::SoundWav,
            "img_tga" => Self::ImageTga,
            "jpeg" => Self::ImageJpeg,
            "animatn" => Self::Animation,
            "gesture" => Self::Gesture,
            "simstate" => Self::Simstate,
            "link" => Self::Link,
            "link_f" => Self::LinkFolder,
            "mesh" => Self::Mesh,
            "widget" => Self::Widget,
            "person" => Self::Person,
            "settings" => Self::Settings,
            "material" => Self::Material,
            "gltf" => Self::Gltf,
            "glbin" => Self::GltfBin,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The short asset-type name to write back into a Linden-text stream, the
    /// inverse of [`from_type_name`](Self::from_type_name).
    #[must_use]
    pub const fn type_name(&self) -> &str {
        match self {
            Self::Texture => "texture",
            Self::Sound => "sound",
            Self::CallingCard => "callcard",
            Self::Landmark => "landmark",
            Self::Script => "script",
            Self::Clothing => "clothing",
            Self::Object => "object",
            Self::Notecard => "notecard",
            Self::Category => "category",
            Self::LslText => "lsltext",
            Self::LslBytecode => "lslbyte",
            Self::TextureTga => "txtr_tga",
            Self::Bodypart => "bodypart",
            Self::SoundWav => "snd_wav",
            Self::ImageTga => "img_tga",
            Self::ImageJpeg => "jpeg",
            Self::Animation => "animatn",
            Self::Gesture => "gesture",
            Self::Simstate => "simstate",
            Self::Link => "link",
            Self::LinkFolder => "link_f",
            Self::Mesh => "mesh",
            Self::Widget => "widget",
            Self::Person => "person",
            Self::Settings => "settings",
            Self::Material => "material",
            Self::Gltf => "gltf",
            Self::GltfBin => "glbin",
            Self::Other(name) => name.as_str(),
        }
    }
}

/// The inventory classification of an embedded item (`LLInventoryType`'s short
/// name, e.g. `"notecard"`, `"landmark"`, `"wearable"`).
///
/// [`None`](InventoryType::None) means the `inv_type` field was absent (the
/// simulator omits it when the inventory type is unset), so an absent field is
/// re-emitted as absent. Unrecognised names are preserved as
/// [`Other`](InventoryType::Other).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryType {
    /// The `inv_type` field was absent.
    None,
    /// A texture (`texture`).
    Texture,
    /// A sound (`sound`).
    Sound,
    /// A calling card (`callcard`).
    CallingCard,
    /// A landmark (`landmark`).
    Landmark,
    /// An object (`object`).
    Object,
    /// A notecard (`notecard`).
    Notecard,
    /// A folder / category (`category`).
    Category,
    /// The root inventory category (`root`).
    Root,
    /// A script (`script`).
    Script,
    /// A snapshot texture (`snapshot`).
    Snapshot,
    /// A worn attachment (`attach`).
    Attachment,
    /// A wearable (`wearable`).
    Wearable,
    /// An animation (`animation`).
    Animation,
    /// A gesture (`gesture`).
    Gesture,
    /// A mesh (`mesh`).
    Mesh,
    /// A UI widget (`widget`).
    Widget,
    /// A person record (`person`).
    Person,
    /// An environment settings blob (`settings`).
    Settings,
    /// A render material (`material`).
    Material,
    /// A glTF asset (`gltf`).
    Gltf,
    /// A glTF binary buffer (`glbin`).
    GltfBin,
    /// An unrecognised inventory-type name, preserved verbatim.
    Other(String),
}

impl InventoryType {
    /// Classify the short inventory-type name written into a Linden-text stream.
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
            "root" => Self::Root,
            "script" => Self::Script,
            "snapshot" => Self::Snapshot,
            "attach" => Self::Attachment,
            "wearable" => Self::Wearable,
            "animation" => Self::Animation,
            "gesture" => Self::Gesture,
            "mesh" => Self::Mesh,
            "widget" => Self::Widget,
            "person" => Self::Person,
            "settings" => Self::Settings,
            "material" => Self::Material,
            "gltf" => Self::Gltf,
            "glbin" => Self::GltfBin,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The short inventory-type name to write back, or `None` when the field
    /// should be omitted entirely ([`None`](InventoryType::None)).
    #[must_use]
    pub const fn type_name(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::Texture => Some("texture"),
            Self::Sound => Some("sound"),
            Self::CallingCard => Some("callcard"),
            Self::Landmark => Some("landmark"),
            Self::Object => Some("object"),
            Self::Notecard => Some("notecard"),
            Self::Category => Some("category"),
            Self::Root => Some("root"),
            Self::Script => Some("script"),
            Self::Snapshot => Some("snapshot"),
            Self::Attachment => Some("attach"),
            Self::Wearable => Some("wearable"),
            Self::Animation => Some("animation"),
            Self::Gesture => Some("gesture"),
            Self::Mesh => Some("mesh"),
            Self::Widget => Some("widget"),
            Self::Person => Some("person"),
            Self::Settings => Some("settings"),
            Self::Material => Some("material"),
            Self::Gltf => Some("gltf"),
            Self::GltfBin => Some("glbin"),
            Self::Other(name) => Some(name.as_str()),
        }
    }
}

/// How an embedded item may be sold (`LLSaleInfo`'s sale type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaleType {
    /// Not for sale (`not`).
    NotForSale,
    /// The original is sold (`orig`).
    Original,
    /// A copy is sold (`copy`).
    Copy,
    /// The contents are sold (`cntn`).
    Contents,
    /// An unrecognised sale-type name, preserved verbatim.
    Other(String),
}

impl SaleType {
    /// Classify the short sale-type name written into a Linden-text stream.
    #[must_use]
    pub fn from_type_name(name: &str) -> Self {
        match name {
            "not" => Self::NotForSale,
            "orig" => Self::Original,
            "copy" => Self::Copy,
            "cntn" => Self::Contents,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The short sale-type name to write back.
    #[must_use]
    pub const fn type_name(&self) -> &str {
        match self {
            Self::NotForSale => "not",
            Self::Original => "orig",
            Self::Copy => "copy",
            Self::Contents => "cntn",
            Self::Other(name) => name.as_str(),
        }
    }
}

/// A Second Life permission bit-mask (the `U32` of `PERM_*` flags stored for
/// each of the base / owner / group / everyone / next-owner scopes).
///
/// The raw value is kept intact for faithful round-tripping; the named
/// accessors interpret the well-known bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PermissionMask(pub u32);

impl PermissionMask {
    /// The `PERM_TRANSFER` bit (`1 << 13`).
    pub const TRANSFER: u32 = 1 << 13;
    /// The `PERM_MODIFY` bit (`1 << 14`).
    pub const MODIFY: u32 = 1 << 14;
    /// The `PERM_COPY` bit (`1 << 15`).
    pub const COPY: u32 = 1 << 15;
    /// The `PERM_MOVE` bit (`1 << 19`).
    pub const MOVE: u32 = 1 << 19;

    /// Whether this mask grants transfer.
    #[must_use]
    pub const fn can_transfer(self) -> bool {
        self.0 & Self::TRANSFER != 0
    }

    /// Whether this mask grants modify.
    #[must_use]
    pub const fn can_modify(self) -> bool {
        self.0 & Self::MODIFY != 0
    }

    /// Whether this mask grants copy.
    #[must_use]
    pub const fn can_copy(self) -> bool {
        self.0 & Self::COPY != 0
    }

    /// Whether this mask grants move.
    #[must_use]
    pub const fn can_move(self) -> bool {
        self.0 & Self::MOVE != 0
    }
}
