//! One real asset body per inventory class a viewer can open or save.
//!
//! An asset id in an inventory item is a promise: the grid that handed it out
//! can serve bytes for it, and those bytes are of the class the item declares.
//! A fixture grid that mints ids without bodies breaks that promise silently —
//! the item looks fine in the inventory window and every attempt to *use* it
//! (open the notecard, wear the shirt, play the sound) fails at the fetch,
//! which is exactly the failure a test tier below a live grid should be able to
//! reproduce.
//!
//! So this module is the table: for each class, an id, the item metadata that
//! declares it, a body, and a **second** body of the same class for the save
//! half. Two bodies rather than one because a round trip that re-fetches the
//! id it was given proves nothing if the bytes never changed — a grid that
//! swallowed the save and a grid that stored it answer identically.
//!
//! The bodies are written by the crate that owns each format (`sl-notecard`,
//! `sl-avatar`, `sl-mesh`, `sl-sound`, `sl-wire`, `sl-proto`), and this
//! module's own tests read every one of them back through the matching
//! decoder — which is what lets a consumer assert "the bytes are of the
//! declared class" by comparing against [`SeededAsset::body`] alone,
//! without linking every decoder itself.
//!
//! Two classes are deliberately absent, and their absence is the finding:
//! [`AssetType::Object`] has no codec in this workspace at all (an object asset
//! is `LLViewerObject`'s nested-block text, unrelated to the `ObjectUpdate`
//! wire form a fixture builds today), and [`AssetType::Gesture`] has no
//! *decoder* — its body here is written from the reference's format by hand and
//! is the one entry whose round trip is byte-level only. See
//! [`unsupported_classes`] for the machine-readable form of both statements.

use std::collections::BTreeMap;

use sl_avatar::{SaleType, WearableAsset, WearablePermissions};
use sl_proto::{AssetType, InventoryType, UpdatableAssetType, WearableType};
use sl_types::map::RegionCoordinates;
use uuid::Uuid;

/// The base of every fixture asset id, so a body served by the fake grid can be
/// recognised in a log without a lookup.
const ASSET_ID_BASE: u128 = 0x5A55_E700_0000_0000_0000_0000_0000_0000;

/// How a viewer writes a new body onto an item of a class — which decides what
/// a round-trip test has to drive, and whether an in-place save exists at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePath {
    /// The two-stage `Update*AgentInventory` capability (and its
    /// `Update*TaskInventory` sibling inside a prim).
    UpdateCap(UpdatableAssetType),
    /// `UpdateScriptAgent` / `UpdateScriptTask` — the same two-stage shape, but
    /// the simulator compiles the source and the completion carries the result.
    ScriptCap,
    /// The legacy UDP transaction upload (`AssetUploadRequest` bound to an
    /// `UpdateInventoryItem`). Wearables have no update capability, so this is
    /// how the appearance editor saves one.
    UdpTransaction,
    /// `NewFileAgentInventory` only: the class is uploaded as a *new* item and
    /// never written in place, so there is no id to re-fetch after a save.
    NewFileOnly,
}

/// An encoder refused to write a fixture body.
///
/// Carries the class it was writing and what the encoder said, flattened to a
/// string because the three encoders behind it return three unrelated error
/// types and nothing downstream branches on which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureError {
    /// The fixture class being written ("texture", "sound", "mesh").
    pub fixture: &'static str,
    /// What the encoder said.
    pub reason: String,
}

impl FixtureError {
    /// Wraps an encoder error against the class it was writing.
    fn new(fixture: &'static str, source: &dyn core::fmt::Display) -> Self {
        Self {
            fixture,
            reason: source.to_string(),
        }
    }
}

impl core::fmt::Display for FixtureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "writing the fixture {} body failed: {}",
            self.fixture, self.reason
        )
    }
}

impl core::error::Error for FixtureError {}

/// A seeded inventory item and the bytes behind its asset id.
#[derive(Debug, Clone)]
pub struct SeededAsset {
    /// The item's name, which is also how a test finds it in a fetched folder.
    pub name: &'static str,
    /// The class the item declares (its `LLAssetType`).
    pub asset_type: AssetType,
    /// The item's `LLInventoryType`.
    pub inv_type: InventoryType,
    /// The asset id the item carries, and the key the body is stored under.
    pub asset_id: Uuid,
    /// The bytes the id resolves to.
    pub body: Vec<u8>,
    /// A different, equally valid body of the same class — what a save-and-
    /// re-fetch test writes so the answer cannot be the seeded bytes.
    pub edited_body: Vec<u8>,
    /// How a viewer saves a new body onto an item of this class.
    pub save_path: SavePath,
    /// The slot a wearable fixture fills, or `None` for a class that is not
    /// worn. An inventory item carries this in its `flags`, which is how a
    /// viewer knows which layer an item fills *before* it fetches the asset.
    pub wearable_type: Option<WearableType>,
}

/// The wearable slot a clothing fixture fills (the shirt layer).
const SHIRT_SLOT: WearableType = WearableType::Shirt;

/// The wearable-asset format version the fixtures are written at — the version
/// the reference viewer writes, matching [`crate::builtin::library_wearables`].
const WEARABLE_VERSION: i32 = 22;

/// `PERM_ALL`: every permission bit a fixture asset grants.
const FULL_PERMISSIONS: u32 = 0x7FFF_FFFF;

/// The permissions block a fixture wearable carries: owned by nobody, not for
/// sale, every right granted.
const FIXTURE_WEARABLE_PERMISSIONS: WearablePermissions = WearablePermissions {
    base_mask: FULL_PERMISSIONS,
    owner_mask: FULL_PERMISSIONS,
    group_mask: 0,
    everyone_mask: 0,
    next_owner_mask: FULL_PERMISSIONS,
    creator_id: Uuid::nil(),
    owner_id: Uuid::nil(),
    last_owner_id: Uuid::nil(),
    group_id: Uuid::nil(),
    sale_type: SaleType::Not,
    sale_price: 0,
};

/// The region a fixture landmark points into.
///
/// **Not nil**: `sl_wire::parse_landmark` refuses a nil `region_id`, as it
/// should — a landmark naming no region is a landmark that cannot be
/// teleported to, and the reference viewer treats one as corrupt. So the
/// fixture names a region id of its own rather than a placeholder.
const LANDMARK_REGION: Uuid = Uuid::from_u128(0x5A55_E700_0000_0000_0000_0000_1A11_DEEE);

/// The visual-param a fixture clothing wearable differs on between its two
/// bodies (`shirt_bottom`, a length morph every skeleton has).
const SHIRT_PARAM: i32 = 619;

/// The visual-param a fixture body part differs on between its two bodies
/// (`height`, which every shape carries).
const SHAPE_PARAM: i32 = 33;

/// The whole table: one entry per class this workspace can write a real body
/// for, in `LLAssetType` code order.
///
/// Rebuilt on each call rather than cached — the bodies are small, the encoders
/// are pure, and a `static` would need every one of them to be const.
///
/// # Errors
///
/// Returns a [`FixtureError`] if one of the three fallible encoders (JPEG2000,
/// Ogg Vorbis, the mesh writer) refuses its input. That cannot happen for the
/// inputs here — they are small, non-empty and well-formed — but a fixture
/// crate has no business taking a process down over it, so the failure is
/// returned rather than panicked. A caller with nowhere to put it (a scenario
/// hook, which cannot fail) logs and seeds nothing, and the class then goes
/// missing loudly at the first test that looks for it.
pub fn seeded_assets() -> Result<Vec<SeededAsset>, FixtureError> {
    Ok(vec![
        SeededAsset {
            name: "Fixture Texture",
            asset_type: AssetType::Texture,
            inv_type: InventoryType::Texture,
            asset_id: fixture_id(AssetType::Texture),
            body: texture_body(crate::markers::RED, crate::markers::GREEN)?,
            edited_body: texture_body(crate::markers::BLUE, crate::markers::YELLOW)?,
            save_path: SavePath::NewFileOnly,
            wearable_type: None,
        },
        SeededAsset {
            name: "Fixture Sound",
            asset_type: AssetType::Sound,
            inv_type: InventoryType::Sound,
            asset_id: fixture_id(AssetType::Sound),
            body: sound_body(crate::sound::tones::MID)?,
            edited_body: sound_body(crate::sound::tones::HIGH)?,
            save_path: SavePath::NewFileOnly,
            wearable_type: None,
        },
        SeededAsset {
            name: "Fixture Landmark",
            asset_type: AssetType::Landmark,
            inv_type: InventoryType::Landmark,
            asset_id: fixture_id(AssetType::Landmark),
            body: landmark_body(128.0, 128.0, 25.0),
            edited_body: landmark_body(64.0, 192.0, 30.0),
            save_path: SavePath::NewFileOnly,
            wearable_type: None,
        },
        SeededAsset {
            name: "Fixture Shirt",
            asset_type: AssetType::Clothing,
            inv_type: InventoryType::Wearable,
            asset_id: fixture_id(AssetType::Clothing),
            body: wearable_body("Fixture Shirt", SHIRT_SLOT, SHIRT_PARAM, 0.25),
            edited_body: wearable_body("Fixture Shirt", SHIRT_SLOT, SHIRT_PARAM, 0.75),
            save_path: SavePath::UdpTransaction,
            wearable_type: Some(SHIRT_SLOT),
        },
        SeededAsset {
            name: "Fixture Shape",
            asset_type: AssetType::Bodypart,
            inv_type: InventoryType::Wearable,
            asset_id: fixture_id(AssetType::Bodypart),
            body: wearable_body("Fixture Shape", WearableType::Shape, SHAPE_PARAM, 0.25),
            edited_body: wearable_body("Fixture Shape", WearableType::Shape, SHAPE_PARAM, 0.75),
            save_path: SavePath::UdpTransaction,
            wearable_type: Some(WearableType::Shape),
        },
        SeededAsset {
            name: "Fixture Notecard",
            asset_type: AssetType::Notecard,
            inv_type: InventoryType::Notecard,
            asset_id: fixture_id(AssetType::Notecard),
            body: notecard_body("The fake grid's seeded notecard.\n"),
            edited_body: notecard_body("Edited by a round-trip test.\n"),
            save_path: SavePath::UpdateCap(UpdatableAssetType::Notecard),
            wearable_type: None,
        },
        SeededAsset {
            name: "Fixture Script",
            asset_type: AssetType::ScriptText,
            inv_type: InventoryType::Script,
            asset_id: fixture_id(AssetType::ScriptText),
            body: script_body("seeded"),
            edited_body: script_body("edited"),
            save_path: SavePath::ScriptCap,
            wearable_type: None,
        },
        SeededAsset {
            name: "Fixture Animation",
            asset_type: AssetType::Animation,
            inv_type: InventoryType::Animation,
            asset_id: fixture_id(AssetType::Animation),
            body: crate::anim::chest_twist_animation_asset(),
            edited_body: crate::anim::chest_twist_animation_asset(),
            save_path: SavePath::NewFileOnly,
            wearable_type: None,
        },
        SeededAsset {
            name: "Fixture Gesture",
            asset_type: AssetType::Gesture,
            inv_type: InventoryType::Gesture,
            asset_id: fixture_id(AssetType::Gesture),
            body: gesture_body("/fixture"),
            edited_body: gesture_body("/edited"),
            save_path: SavePath::UpdateCap(UpdatableAssetType::Gesture),
            wearable_type: None,
        },
        SeededAsset {
            name: "Fixture Mesh",
            asset_type: AssetType::Mesh,
            inv_type: InventoryType::Object,
            asset_id: fixture_id(AssetType::Mesh),
            body: mesh_body()?,
            edited_body: mesh_body()?,
            save_path: SavePath::NewFileOnly,
            wearable_type: None,
        },
        SeededAsset {
            name: "Fixture Settings",
            asset_type: AssetType::Settings,
            inv_type: InventoryType::Settings,
            asset_id: fixture_id(AssetType::Settings),
            body: crate::environment::noon_sky_asset(),
            edited_body: crate::environment::night_sky_asset(),
            save_path: SavePath::UpdateCap(UpdatableAssetType::Settings),
            wearable_type: None,
        },
        SeededAsset {
            name: "Fixture Material",
            asset_type: AssetType::Material,
            inv_type: InventoryType::Material,
            asset_id: fixture_id(AssetType::Material),
            body: crate::gltf_material_asset([1.0, 0.0, 0.0, 1.0], None),
            edited_body: crate::gltf_material_asset([0.0, 0.0, 1.0, 1.0], None),
            save_path: SavePath::UpdateCap(UpdatableAssetType::Material),
            wearable_type: None,
        },
    ])
}

/// The fixture asset id for a class — the class's own `LLAssetType` code in the
/// low bits of a fixed high prefix, so an id in a log names its class without a
/// lookup.
#[must_use]
pub fn fixture_id(asset_type: AssetType) -> Uuid {
    let code = u128::try_from(asset_type.to_code()).unwrap_or(0);
    Uuid::from_u128(ASSET_ID_BASE.wrapping_add(code))
}

/// The classes a viewer can hold in inventory that this table deliberately does
/// **not** carry a body for, each with the reason — the recorded "no" the
/// round-trip acceptance asks for.
///
/// Not the same question as [`SavePath`]: these are classes with no fixture at
/// all, not classes with no in-place save.
#[must_use]
pub const fn unsupported_classes() -> &'static [(AssetType, &'static str)] {
    &[
        (
            AssetType::Object,
            "no codec: an object asset is LLViewerObject's nested-block text, \
             which nothing in this workspace reads or writes (test-assets-object-asset-codec)",
        ),
        (
            AssetType::CallingCard,
            "no consumer: nothing fetches a calling card's body, and whether the \
             class is worth a fixture at all is test-assets-remaining-class-audit's call",
        ),
        (
            AssetType::ScriptBytecode,
            "server-side only: a viewer never decodes compiled bytecode, it only \
             ever names the class in an inventory item",
        ),
        (
            AssetType::TextureTga,
            "legacy: every texture path is JPEG2000; the TGA classes survive as a \
             group-notice icon (test-assets-remaining-class-audit)",
        ),
        (AssetType::ImageTga, "legacy: see TextureTga"),
        (AssetType::ImageJpeg, "legacy: see TextureTga"),
        (
            AssetType::SoundWav,
            "upload-side only: a grid transcodes an uploaded WAV to Ogg Vorbis and \
             serves the result as Sound, so no item ever names this class",
        ),
        (
            AssetType::Gltf,
            "reserved: distinct from Material (57), which is the class a viewer \
             actually saves (test-assets-remaining-class-audit)",
        ),
        (AssetType::GltfBin, "reserved: see Gltf"),
        (
            AssetType::Folder,
            "not an asset: the class is an inventory-offer bucket marker",
        ),
    ]
}

/// The class whose fixture body has no decoder in this workspace, so its round
/// trip can only be compared byte for byte.
pub const UNDECODED_CLASS: AssetType = AssetType::Gesture;

/// A JPEG2000 texture body: a 32-pixel checker of two marker colours.
fn texture_body(a: [u8; 4], b: [u8; 4]) -> Result<Vec<u8>, FixtureError> {
    crate::RgbaImage::checker(32, 8, a, b)
        .j2c()
        .map_err(|error| FixtureError::new("texture", &error))
}

/// An Ogg Vorbis sound body at `frequency_hz`.
fn sound_body(frequency_hz: f32) -> Result<Vec<u8>, FixtureError> {
    crate::sound::marker_tone(frequency_hz).map_err(|error| FixtureError::new("sound", &error))
}

/// A whole mesh asset body (the unit cube).
fn mesh_body() -> Result<Vec<u8>, FixtureError> {
    crate::mesh::unit_cube_mesh_asset().map_err(|error| FixtureError::new("mesh", &error))
}

/// A landmark body pointing at a region position.
fn landmark_body(x: f32, y: f32, z: f32) -> Vec<u8> {
    sl_wire::landmark_to_wire(LANDMARK_REGION, RegionCoordinates::new(x, y, z)).into_bytes()
}

/// A notecard body carrying `text` and no embedded items.
fn notecard_body(text: &str) -> Vec<u8> {
    sl_notecard::Notecard {
        source_version: sl_notecard::NotecardVersion::V2,
        items: Vec::new(),
        text: text.to_owned(),
    }
    .encode()
}

/// An LSL source body whose `llOwnerSay` names `tag`, so two bodies of this
/// class differ in a way a reader can see.
fn script_body(tag: &str) -> Vec<u8> {
    format!("default\n{{\n    state_entry()\n    {{\n        llOwnerSay(\"{tag}\");\n    }}\n}}\n")
        .into_bytes()
}

/// A wearable body whose single visual param `param` sits at `weight`, so two
/// bodies of one slot differ in a way a reader can see.
fn wearable_body(name: &str, wearable_type: WearableType, param: i32, weight: f32) -> Vec<u8> {
    let asset = WearableAsset {
        version: WEARABLE_VERSION,
        name: name.to_owned(),
        wearable_type,
        params: BTreeMap::from([(param, weight)]),
        textures: BTreeMap::new(),
    };
    asset.to_text(&FIXTURE_WEARABLE_PERMISSIONS).into_bytes()
}

/// A gesture body in the reference's `LLGesture` text format: the version, the
/// key/mask the gesture is bound to, its trigger word, its replacement text,
/// and a single chat step.
///
/// Hand-written from `LLMultiGesture::serialize`, because nothing in this
/// workspace decodes a gesture — see [`UNDECODED_CLASS`].
fn gesture_body(trigger: &str) -> Vec<u8> {
    // `2` is the current gesture version; the two zeros are the key and mask
    // the gesture is bound to (none); the empty replacement line means the
    // trigger is not substituted in chat; `1` step follows, of type `0` (chat).
    format!("2\n0\n0\n{trigger}\n\n1\nChat\n0\nHello from the fixture gesture\n").into_bytes()
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_proto::AssetSource as _;

    use super::*;

    /// What a test in this module returns when a fixture, a decoder or an
    /// assertion could not be carried out.
    type TestError = Box<dyn core::error::Error>;

    /// Every fixture's declared class matches a body its own decoder accepts —
    /// which is what lets a consumer assert the class by comparing bytes.
    ///
    /// `Gesture` is the documented exception: it is checked for its version
    /// header only, because no decoder exists to check it with.
    #[test]
    fn every_body_decodes_as_its_declared_class() -> Result<(), TestError> {
        for fixture in seeded_assets()? {
            for body in [&fixture.body, &fixture.edited_body] {
                assert!(
                    !body.is_empty(),
                    "{}: the fixture body is empty",
                    fixture.name
                );
                decode_as(fixture.asset_type, body)
                    .map_err(|reason| format!("{}: {reason}", fixture.name))?;
            }
        }
        Ok(())
    }

    /// Reads `body` back with the decoder that owns `asset_type`, or explains
    /// why it could not.
    fn decode_as(asset_type: AssetType, body: &[u8]) -> Result<(), String> {
        /// Reads a text asset's bytes as UTF-8 first, since every text format
        /// below fails the same uninformative way on non-text bytes.
        fn text(body: &[u8]) -> Result<&str, String> {
            std::str::from_utf8(body).map_err(|error| format!("not UTF-8: {error}"))
        }
        match asset_type {
            AssetType::Texture => sl_texture::decode_j2c(body, sl_proto::j2c::DiscardLevel::FULL)
                .map(drop)
                .map_err(|error| format!("not a JPEG2000 codestream: {error}")),
            AssetType::Sound => {
                let probed = symphonium::probe_from_source(
                    Box::new(std::io::Cursor::new(body.to_vec())),
                    None,
                    None,
                )
                .map_err(|error| format!("not a probeable sound: {error}"))?;
                symphonium::decode_f32(
                    probed,
                    &symphonium::DecodeConfig::default(),
                    None,
                    None,
                    None,
                )
                .map(drop)
                .map_err(|error| format!("not an Ogg Vorbis stream: {error}"))
            }
            AssetType::Landmark => sl_wire::parse_landmark(text(body)?)
                .map(drop)
                .map_err(|error| format!("not a landmark: {error}")),
            AssetType::Clothing | AssetType::Bodypart => WearableAsset::parse(text(body)?)
                .map(drop)
                .map_err(|error| format!("not a wearable: {error}")),
            AssetType::Notecard => sl_notecard::Notecard::decode(body)
                .map(drop)
                .map_err(|error| format!("not a notecard: {error}")),
            AssetType::ScriptText => {
                let parsed = sl_lsl::parse(text(body)?);
                if parsed.errors.is_empty() {
                    Ok(())
                } else {
                    Err(format!("not valid LSL: {:?}", parsed.errors))
                }
            }
            AssetType::Animation => sl_anim::Motion::from_bytes(body)
                .map(drop)
                .map_err(|error| format!("not an animation: {error}")),
            AssetType::Mesh => {
                let (header, header_size) =
                    sl_mesh::parse_header(body).ok_or_else(|| "no mesh header".to_owned())?;
                let block = header
                    .lod(sl_mesh::MeshLod::High)
                    .ok_or_else(|| "no high LOD".to_owned())?;
                let (start, end) = block.range(header_size);
                let lod = body
                    .get(start..end)
                    .ok_or_else(|| "the high LOD runs past the asset".to_owned())?;
                sl_mesh::decode_lod(lod, sl_mesh::MeshLod::High)
                    .map(drop)
                    .map_err(|error| format!("not a mesh asset: {error}"))
            }
            AssetType::Settings => sl_proto::environment_asset_from_bytes("fixture", body)
                .map(drop)
                .ok_or_else(|| "not a settings asset".to_owned()),
            AssetType::Material => sl_material::parse_material_asset(body)
                .map(drop)
                .map_err(|error| format!("not a material asset: {error}")),
            UNDECODED_CLASS => {
                // No decoder exists; the version header is all there is to check.
                if body.starts_with(b"2\n") {
                    Ok(())
                } else {
                    Err("not a version-2 gesture body".to_owned())
                }
            }
            other => Err(format!("no decoder wired up for {other:?}")),
        }
    }

    /// The two bodies of a class differ wherever an in-place save exists — a
    /// round trip that re-fetches the id it was handed cannot tell a stored save
    /// from a swallowed one unless the bytes changed.
    #[test]
    fn a_savable_class_has_two_distinct_bodies() -> Result<(), TestError> {
        for fixture in seeded_assets()? {
            if matches!(fixture.save_path, SavePath::NewFileOnly) {
                continue;
            }
            assert_ne!(
                fixture.body, fixture.edited_body,
                "{}: the edited body is the seeded one, so a save cannot be observed",
                fixture.name
            );
        }
        Ok(())
    }

    /// No two classes share an id or a name, since both are how a consumer
    /// addresses a fixture.
    #[test]
    fn ids_and_names_are_unique() -> Result<(), TestError> {
        let fixtures = seeded_assets()?;
        let mut ids: Vec<Uuid> = fixtures.iter().map(|fixture| fixture.asset_id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two fixtures share an asset id");
        let mut names: Vec<&str> = fixtures.iter().map(|fixture| fixture.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two fixtures share a name");
        Ok(())
    }

    /// Every class is accounted for: it either carries a body or carries a
    /// recorded reason it does not. A new `AssetType` variant fails this until
    /// somebody decides which.
    #[test]
    fn every_asset_class_is_either_written_or_explained() -> Result<(), TestError> {
        let written: Vec<AssetType> = seeded_assets()?
            .iter()
            .map(|fixture| fixture.asset_type)
            .collect();
        for asset_type in ALL_CLASSES {
            let has_body = written.contains(&asset_type);
            let has_reason = unsupported_classes()
                .iter()
                .any(|&(class, _reason)| class == asset_type);
            assert!(
                has_body != has_reason,
                "{asset_type:?} has {} — it needs exactly one of a fixture body \
                 and a recorded reason it has none",
                if has_body {
                    "both a body and a recorded reason"
                } else {
                    "neither a body nor a recorded reason"
                }
            );
        }
        Ok(())
    }

    /// Every named [`AssetType`] variant (the open-ended `Other` aside), so the
    /// accounting test above cannot pass by only checking the classes it knows.
    const ALL_CLASSES: [AssetType; 22] = [
        AssetType::Texture,
        AssetType::Sound,
        AssetType::CallingCard,
        AssetType::Landmark,
        AssetType::Clothing,
        AssetType::Object,
        AssetType::Notecard,
        AssetType::ScriptText,
        AssetType::ScriptBytecode,
        AssetType::TextureTga,
        AssetType::Bodypart,
        AssetType::SoundWav,
        AssetType::ImageTga,
        AssetType::ImageJpeg,
        AssetType::Animation,
        AssetType::Gesture,
        AssetType::Mesh,
        AssetType::Settings,
        AssetType::Material,
        AssetType::Gltf,
        AssetType::GltfBin,
        AssetType::Folder,
    ];

    /// The library body parts are a *second* set of `Bodypart` bodies, under
    /// Linden's own library ids rather than fixture ids — the ones an account
    /// wears. This pins that they are still readable as wearables, since the
    /// table's own `Fixture Shape` covers the class and would otherwise be the
    /// only thing keeping the format honest.
    #[test]
    fn body_parts_come_from_the_library_stand_ins() -> Result<(), TestError> {
        let wearables = crate::builtin::library_wearables();
        assert_eq!(
            wearables.len(),
            crate::builtin::DEFAULT_BODY_PARTS.len(),
            "one stand-in per default body part"
        );
        for (_id, body) in &wearables {
            let _parsed = WearableAsset::parse(core::str::from_utf8(body)?)?;
        }
        Ok(())
    }

    /// The store a fake grid folds these into serves every fixture id.
    #[test]
    fn the_table_folds_into_an_asset_source() -> Result<(), TestError> {
        let mut store = sl_proto::InMemoryAssetSource::new();
        for fixture in seeded_assets()? {
            let _previous = store.insert(sl_proto::AssetKey::from(fixture.asset_id), fixture.body);
        }
        for fixture in seeded_assets()? {
            assert_eq!(
                store.get(sl_proto::AssetKey::from(fixture.asset_id)),
                Some(fixture.body.as_slice()),
                "{}: the seeded id does not resolve",
                fixture.name
            );
        }
        Ok(())
    }
}
