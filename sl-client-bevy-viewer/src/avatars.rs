//! Avatar placeholders: a ~2 m sphere and a floating name tag per nearby avatar.
//!
//! This is the Phase 10 slice — placeholder spheres, no rig / baked textures /
//! animation. Avatars are learned from two independent streams:
//!
//! - **full in-world objects** (`pcode` 47): the precise, per-frame position of
//!   every avatar the simulator streams as an [`Object`]
//!   (the reliable source for avatars within draw distance, including the agent's
//!   own). [`update_avatar_objects`] spawns / moves / despawns one sphere per such
//!   avatar keyed by its agent id;
//! - **coarse (minimap) locations** (`CoarseLocationUpdate`): the low-resolution
//!   (1 m) positions the simulator pushes for nearby avatars, some of which are
//!   beyond the object interest radius and so never arrive as a full object.
//!   [`update_coarse_avatars`] renders a sphere for every coarse-only avatar (one
//!   already tracked as a full object is skipped, and the agent's own `you` entry
//!   is left to the object path), and despawns a sphere the moment its avatar
//!   drops out of the coarse list.
//!
//! Each avatar also carries a floating **name tag** — a world-space billboard
//! mesh (see [`crate::name_tag_billboard`]) that follows the avatar anchor and
//! renders through the main world camera with occlusion, constant on-screen
//! size and distance fade. The legacy name is resolved once per agent via
//! a `UUIDNameRequest` ([`Command::RequestAvatarNames`](sl_client_bevy::Command))
//! and cached in [`AvatarState`], so a repeatedly-updated avatar is never
//! re-requested; until the reply arrives the tag shows a short id fragment so the
//! avatars are still distinguishable.
//!
//! Both sources share one placeholder sphere mesh and material, built lazily on
//! first use. The spheres are plain world-space entities positioned via the
//! Second Life → Bevy [coordinate map](crate::coords) — they are markers, not the
//! avatar's object root, so (unlike a linkset root in [`objects`](crate::objects))
//! they carry no attachment children and are not scaled by the avatar object's
//! bounding box.

use std::collections::{HashMap, HashSet};

use bevy::app::Propagate;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::mesh::morph::MeshMorphWeights;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bytes::Bytes;
use sl_client_bevy::{
    AgentKey, AnimationPose, AvatarName, BakeRegion, BaseMesh, BaseMeshSkin, BevySkeleton,
    BodyPhysics, BodySizeMetrics, CoarseLocation, Command, DecodedTexture, DiscardLevel,
    DisplayName, JointOverrides, Layer, MAX_FACES, MaskTexture, MeshSkin, MorphWeights, Object,
    PartMorphMask, RUNTIME_MORPH_PARAMS, RegionHandle, ResolvedParams, ScopedObjectId,
    SkeletalDeformations, SlCommand, SlEvent, SlIdentity, SlSessionEvent, TextureEntry, TextureKey,
    Uuid, VolumeDeformations, avatar_texture, composite_region, decode_texture_entry,
    joint_position_overrides, pcode, to_bevy_base_mesh, to_bevy_image, to_bevy_morphed_mesh,
    to_bevy_runtime_morph_targets,
};

use crate::avatar_assets::{AvatarAssetLibrary, BodyRegion, LoadedBinding};
use crate::bake_inputs::OwnBakeInputs;
use crate::coords::{
    metres_to_f32, origin_shift_bevy, region_offset_bevy, sl_euler_deg_to_quat,
    sl_rotation_to_quat, sl_to_bevy_object_rotation, sl_to_bevy_vec,
};
use crate::face_material::{FaceMaterial, inert_face_material};
use crate::name_tag_billboard::name_tag_render_bundle;
use crate::name_tag_content::TagContent;
use crate::objects::ObjectState;
use crate::physics::{AvatarInterp, AvatarMotion};
use crate::probe_layers::dynamic_render_layers;
use crate::textures::{TextureDecoded, TextureManager, tint_color};

/// The radius, in metres, of an avatar placeholder sphere (a ~2 m-diameter
/// UV-sphere, roughly avatar-sized).
pub(crate) const AVATAR_SPHERE_RADIUS: f32 = 1.0;

/// The number of longitudinal segments (sectors) of the placeholder UV-sphere.
const SPHERE_SECTORS: u32 = 32;

/// The number of latitudinal segments (stacks) of the placeholder UV-sphere.
const SPHERE_STACKS: u32 = 18;

/// The soft-blue base colour of the placeholder material, so avatars stand out
/// from prims and terrain.
const AVATAR_COLOR: Color = Color::srgb(0.40, 0.60, 0.90);

/// The gap, in metres, between the top of an avatar (sphere top or body head)
/// and its name tag.
const NAME_TAG_GAP: f32 = 0.3;

/// The height, in metres above the avatar object position, at which a rigged
/// body's name tag floats — roughly the head of an average-height avatar (the
/// object position sits near the pelvis).
const BODY_TAG_HEIGHT: f32 = 1.9;

/// Extra height, in metres, added **above** the highest joint when deriving
/// the head top for the name tag's float height (the head joint sits inside
/// the skull, not at its crown) — mirroring the reference's `√2` skull
/// correction in spirit.
const HEAD_TOP_MARGIN: f32 = 0.18;

/// A skin-toned base colour for the un-textured Phase-13.2 body, before the
/// baked-texture phases (P14) drape real textures over it.
const BODY_COLOR: Color = Color::srgb(0.85, 0.70, 0.62);

/// The neutral fallback colour a bake-on-mesh face shows while its wearer's bake
/// has not resolved (R22). The reference viewer falls back to the neutral
/// `IMG_DEFAULT` for a missing baked texture (`getBakedTextureForMagicId`), *not*
/// to skin tone — so an unresolved BoM slot must not borrow the reddish
/// [`BODY_COLOR`] skin placeholder, which made a not-yet-resolved hand read redder
/// than the resolved arm (R22f).
const BOM_FALLBACK_COLOR: Color = Color::srgb(0.75, 0.75, 0.75);

/// The channel count of a decoded RGBA8 texture — the pixel stride used when
/// sampling a bake's alpha for the clothing-morph masks (P14.5).
const RGBA_CHANNELS: usize = 4;

/// How many leading hex characters of the agent id to show as a provisional tag
/// before the real name resolves.
const PROVISIONAL_ID_CHARS: usize = 8;

/// A marker component tagging an entity as an avatar placeholder sphere.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarSphere;

/// A marker component on the transform-bearing *anchor* entity of an avatar —
/// its placeholder sphere or the root of its rigged body — whose world position
/// the name-tag placement ([`crate::name_tag_billboard::follow_tag_anchors`])
/// follows to float the tag.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarAnchor;

/// A component tagging an entity as **part of** a specific avatar, carrying that
/// avatar's [`AgentKey`] — the reusable "what avatar is this?" identity that
/// picking reads.
///
/// It sits on every pickable piece of an avatar: the placeholder sphere, each
/// rigged base-body part, each **worn rigged-mesh submesh** (on a modern
/// mesh-body avatar the base body is hidden, so the worn mesh *is* the
/// silhouette), and the floating name tag. That breadth is the point — a ray
/// that hits any body part, or a pointer over the name tag (resolved by the
/// [`crate::name_tag_billboard::NameTagHitTest`] rect test — tags are custom
/// billboard meshes no picking backend covers), resolves to the
/// same agent through one component, so a caller never has to know *which* piece
/// it hit. Kept separate from [`AvatarBodyPart`] (which also holds an agent) so
/// non-mesh pieces (the sphere, the name tag) can carry the identity too, and
/// so consumers — the GPU pick-tag assignment
/// ([`crate::gpu_pick::assign_avatar_pick_tags`]) is the main one — read a
/// single, purpose-named component rather than three different markers.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarPickTarget {
    /// The avatar this entity is part of.
    agent: AgentKey,
}

impl AvatarPickTarget {
    /// Tag a pickable piece of `agent` (used by the rigged-attachment spawn in
    /// [`crate::objects`], where the wearer is known only sometimes).
    pub(crate) const fn new(agent: AgentKey) -> Self {
        Self { agent }
    }

    /// The avatar this entity belongs to.
    pub(crate) const fn agent(&self) -> AgentKey {
        self.agent
    }
}

/// A marker on one rigged base-part render entity, tying it back to its avatar
/// and its index in [`AvatarBody::parts`] / [`AvatarAssetLibrary::parts`] so the
/// appearance system ([`apply_avatar_appearance`]) can rebuild just that part's mesh from
/// the avatar's resolved visual-param weights.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarBodyPart {
    /// The avatar this part belongs to.
    agent: AgentKey,
    /// The part's index into the shared part list (base-mesh and render data
    /// share the same order).
    part: usize,
    /// Which baked region this part belongs to, so the visibility system
    /// ([`apply_avatar_part_visibility`]) can hide it when a worn attachment
    /// replaces its region, or (for the skirt) show it only when a skirt is worn.
    region: BodyRegion,
}

impl AvatarBodyPart {
    /// The avatar this part belongs to (read by the animation driver, in another
    /// module, to pose a rigid part's own `GlobalTransform`).
    pub(crate) const fn agent(&self) -> AgentKey {
        self.agent
    }

    /// The part's index into the shared [`AvatarBody::parts`] list.
    pub(crate) const fn part(&self) -> usize {
        self.part
    }
}

/// A marker on one rigged-mesh submesh face whose `TextureEntry` slot carries a
/// bake-on-mesh sentinel (`IMG_USE_BAKED_*`), tying it back to its wearer avatar
/// and the baked slot it should show (P17.3). A "BoM" mesh body face is textured
/// not from a fetched texture but from the wearer's own baked avatar texture — the
/// same server / client bake the base body region wears — so
/// [`apply_bom_face_materials`] keeps the face pointing at that region's material,
/// falling back to the opaque skin placeholder until the bake resolves.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct BomFace {
    /// The wearer avatar whose bake textures this face samples.
    agent: AgentKey,
    /// The baked slot ([`avatar_texture`]) the sentinel named — the region whose
    /// bake this face samples.
    slot: usize,
    /// The face's `TextureEntry` tint colour (RGBA). The reference viewer
    /// multiplies the baked texture by this per-face colour (its vertex colour), so
    /// a fully-transparent tint (`[_, _, _, 0]`) hides the face — a mesh body's
    /// alpha-cut / "onion shell" layer — and a non-opaque tint blends it.
    tint: [u8; 4],
    /// The face's per-face UV placement (`scale_s`/`scale_t`/offset/rotation), as
    /// the reference viewer's `xform`. Identity for an un-repeated bake.
    uv: Affine2,
}

impl BomFace {
    /// Build a marker for a bake-on-mesh face on `agent` sampling baked `slot`,
    /// carrying the face's `TextureEntry` `tint` and `uv` placement so
    /// [`apply_bom_face_materials`] can reproduce the reference viewer's per-face
    /// tint / hide / blend on the sampled bake.
    pub(crate) const fn new(agent: AgentKey, slot: usize, tint: [u8; 4], uv: Affine2) -> Self {
        Self {
            agent,
            slot,
            tint,
            uv,
        }
    }

    /// The face's `TextureEntry` tint colour (RGBA).
    pub(crate) const fn tint(&self) -> [u8; 4] {
        self.tint
    }

    /// The face's per-face UV placement transform.
    pub(crate) const fn uv(&self) -> Affine2 {
        self.uv
    }

    /// The appearance-service name of this face's baked slot (`upper`, `leftarm`,
    /// …), for diagnostics; `"?"` if the slot is not a known bake slot.
    pub(crate) fn slot_name(&self) -> &'static str {
        avatar_texture::BAKED
            .iter()
            .find_map(|&(slot, name)| (slot == self.slot).then_some(name))
            .unwrap_or("?")
    }
}

/// Whether per-face avatar / bake diagnostic logging is enabled
/// (`SL_VIEWER_LOG_AVATAR_FACES=1`): logs each rigged-mesh face's `TextureEntry`
/// (bake sentinel / real texture, tint, UV) and each decoded bake's dimensions +
/// alpha classification, for diagnosing BoM mesh-body texturing (R22) against the
/// Firestorm reference. Off by default (the dump is verbose).
pub(crate) fn log_avatar_faces_enabled() -> bool {
    std::env::var("SL_VIEWER_LOG_AVATAR_FACES").as_deref() == Ok("1")
}

/// Whether the R22b "blue sphere" interest diagnostic is enabled
/// (`SL_VIEWER_LOG_AVATAR_INTEREST=1`): logs each full avatar object the session
/// surfaces (agent, region handle, position) and, on a 5 s cadence, a census of the
/// coarse-only sphere avatars that have not resolved — each flagged with whether a
/// full object was *ever* received for it and its coarse `z` (a `z` at the 1020 m
/// ceiling is the "off this region" sentinel). This tells apart the two R22b
/// failure modes: the simulator never streaming a distant/neighbour avatar's full
/// object, versus the viewer receiving it but failing to render it. Off by default.
pub(crate) fn log_avatar_interest() -> bool {
    std::env::var("SL_VIEWER_LOG_AVATAR_INTEREST").as_deref() == Ok("1")
}

/// Whether the bake-on-mesh diagnostic flat-skin mode is enabled
/// (`SL_VIEWER_DEBUG_AVATAR_FLAT=1`): renders every BoM face with a flat neutral
/// material instead of its baked texture, so a texture / UV-seam artifact (which
/// disappears) can be distinguished from a geometry / normals one (which remains,
/// still lit by the mesh normals). An A/B diagnostic for the R22 arm seams.
fn debug_avatar_flat() -> bool {
    std::env::var("SL_VIEWER_DEBUG_AVATAR_FLAT").as_deref() == Ok("1")
}

/// Whether the bake-on-mesh diagnostic UV-grid mode is enabled
/// (`SL_VIEWER_DEBUG_AVATAR_GRID=1`): renders every BoM face with a generated UV
/// grid ([`uv_grid_image`]) instead of its baked texture, sampled through the same
/// per-face UV transform the bake uses. The grid makes the mesh's UV mapping
/// visible — a continuous grid across the arm means its UV layout is fine and the
/// seams are baked *skin content*; a broken / offset grid means a UV-mapping
/// problem. Takes precedence over [`debug_avatar_flat`].
fn debug_avatar_grid() -> bool {
    std::env::var("SL_VIEWER_DEBUG_AVATAR_GRID").as_deref() == Ok("1")
}

/// The side length of the generated UV-grid diagnostic texture.
const UV_GRID_SIZE: usize = 512;
/// The UV-grid cell size in texels (fine grid lines).
const UV_GRID_CELL: usize = 16;
/// The UV-grid coarse-line spacing in texels (every eighth fine line).
const UV_GRID_COARSE: usize = 128;

/// A UV-diagnostic grid texture (R22): an `x → red`, `y → green` position gradient
/// (so any UV discontinuity shows as a colour jump) overlaid with black grid lines
/// every [`UV_GRID_CELL`] texels and white lines every [`UV_GRID_COARSE`]. Rendered
/// on a BoM face in [`debug_avatar_grid`] mode to reveal how the mesh UVs map a
/// texture. Sampled nearest + repeat so the cells stay crisp.
fn uv_grid_image() -> Image {
    let size = UV_GRID_SIZE;
    let mut pixels = vec![0_u8; size.saturating_mul(size).saturating_mul(4)];
    for y in 0..size {
        for x in 0..size {
            let coarse = x.checked_rem(UV_GRID_COARSE) == Some(0)
                || y.checked_rem(UV_GRID_COARSE) == Some(0);
            let fine =
                x.checked_rem(UV_GRID_CELL) == Some(0) || y.checked_rem(UV_GRID_CELL) == Some(0);
            let rgb = if coarse {
                [255, 255, 255]
            } else if fine {
                [0, 0, 0]
            } else {
                let r = u8::try_from(x.saturating_mul(255).checked_div(size).unwrap_or(0))
                    .unwrap_or(255);
                let g = u8::try_from(y.saturating_mul(255).checked_div(size).unwrap_or(0))
                    .unwrap_or(255);
                [r, g, 96]
            };
            let base = y.saturating_mul(size).saturating_add(x).saturating_mul(4);
            if let Some(slot) = pixels.get_mut(base..base.saturating_add(4)) {
                slot.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
    let width = u32::try_from(size).unwrap_or(0);
    let decoded = DecodedTexture::new(
        width,
        width,
        4,
        DiscardLevel::FULL,
        Bytes::from(pixels),
        None,
    );
    let mut image = to_bevy_image(&decoded);
    // Nearest + repeat: crisp grid lines, and tiling if a UV strays outside [0, 1].
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

/// A marker on one skeleton-instance joint entity, tying it back to its avatar
/// and its index in the shared [`BevySkeleton`] so
/// the appearance system ([`apply_avatar_appearance`]) can re-set that joint's
/// local transform from the avatar's resolved skeletal deformations (P13.4).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarJoint {
    /// The avatar this joint belongs to.
    agent: AgentKey,
    /// The joint's index into the shared skeleton (joint order).
    index: usize,
}

/// A marker on one avatar attachment-point node (P16.2) — the node parented to a
/// skeleton joint at the `avatar_lad.xml` offset, off which a worn **rigid**
/// attachment hangs. The pose driver overwrites each joint's `GlobalTransform`
/// directly in `PostUpdate` *after* transform propagation, so Bevy never
/// recomputes these nodes (nor the rigid attachments below them) from the animated
/// joint — [`pose_attachment_nodes`](crate::animations::pose_attachment_nodes)
/// re-propagates their subtrees from the posed joint so a worn rigid attachment
/// (an earring, a piercing) tracks the head instead of freezing at the rest pose.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AttachmentPointNode;

/// A world-space name-tag billboard ([`crate::name_tag_billboard`]), pointing
/// back at the avatar anchor it floats over so the placement system can
/// follow the anchor's world position each frame.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NameTag {
    /// The avatar anchor entity (sphere or body root) this tag labels.
    pub(crate) anchor: Entity,
    /// The height, in metres above the anchor's world position, at which to
    /// float the tag (a sphere's top or a body's head).
    pub(crate) tag_height: f32,
}

/// One agent's resolved names, merged from every source: the instant
/// `ObjectUpdate` NameValue seed, the legacy `UUIDNameReply`, and the
/// `GetDisplayNames` cap (SL only — OpenSim generally lacks the cap, so the
/// legacy fields must always work on their own).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NameRecord {
    /// The legacy `"First Last"` name (`"First"` alone for a single-name
    /// account), from whichever source arrived first.
    pub(crate) legacy: Option<String>,
    /// The immutable dotted SLID (`"first.last"`), display-name cap only.
    pub(crate) username: Option<String>,
    /// The chosen display name, display-name cap only (`None` on OpenSim).
    pub(crate) display_name: Option<String>,
    /// Whether the display name is just the legacy-derived default (a custom
    /// display name shows with the username line under it, the reference's
    /// `is_display_name_default` behaviour).
    pub(crate) is_display_name_default: bool,
}

impl NameRecord {
    /// The name the tag's main line shows: the display name when one
    /// resolved, else the legacy name.
    pub(crate) fn preferred_name(&self) -> Option<&str> {
        self.display_name.as_deref().or(self.legacy.as_deref())
    }
}

/// The master name-tag toggle (the preferences General tab's headline switch;
/// the reference `AvatarNameTagMode` off/on axis). Honoured by
/// [`crate::name_tag_billboard::follow_tag_anchors`]; the full reference toggle set is the separate
/// `viewer-name-tags-preferences` task.
pub(crate) const SETTING_SHOW_NAME_TAGS: &str = "ShowNameTags";

/// Whether the logged-in avatar's own tag is shown (the reference
/// `RenderNameShowSelf`). Honoured by
/// [`crate::name_tag_billboard::follow_tag_anchors`].
pub(crate) const SETTING_SHOW_OWN_NAME_TAG: &str = "ShowOwnNameTag";

/// The settings section the name-tag toggles live in.
const NAME_TAG_SECTION: &[&str] = &["nametags"];

/// Register the name-tag settings.
pub(crate) fn register_settings(settings: &mut crate::settings::ViewerSettings) {
    settings.register_in(
        NAME_TAG_SECTION,
        SETTING_SHOW_NAME_TAGS,
        sl_settings::SettingValue::Bool(true),
        "Show floating name tags over avatars",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        SETTING_SHOW_OWN_NAME_TAG,
        sl_settings::SettingValue::Bool(true),
        "Show the name tag over your own avatar too",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_content::SETTING_SHOW_DISPLAY_NAMES,
        sl_settings::SettingValue::Bool(true),
        "Show display names on name tags (off: legacy names only)",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_content::SETTING_SHOW_USERNAMES,
        sl_settings::SettingValue::Bool(true),
        "Show the username line under a custom display name",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_content::SETTING_SHOW_GROUP_TITLES,
        sl_settings::SettingValue::Bool(true),
        "Show the active group title line on name tags",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_content::SETTING_SHOW_FRIEND_COLOR,
        sl_settings::SettingValue::Bool(true),
        "Colour friends' name tags",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_content::SETTING_SHOW_DISTANCE,
        sl_settings::SettingValue::Bool(true),
        "Show the camera distance line on name tags",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_content::SETTING_SHOW_TYPING,
        sl_settings::SettingValue::Bool(true),
        "Show a Typing status line while an avatar is typing",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_content::SETTING_COLOR_BY_DISTANCE,
        sl_settings::SettingValue::Bool(false),
        "Tint whole name tags by chat range (whisper/say/shout)",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_billboard::SETTING_FADE_START,
        sl_settings::SettingValue::F32(crate::name_tag_billboard::DEFAULT_FADE_START_METRES),
        "Distance in metres at which name tags start to fade",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_billboard::SETTING_FADE_RANGE,
        sl_settings::SettingValue::F32(crate::name_tag_billboard::DEFAULT_FADE_RANGE_METRES),
        "Metres past the fade start at which name tags are fully hidden",
    );
    settings.register_in(
        NAME_TAG_SECTION,
        crate::name_tag_billboard::SETTING_BUBBLE_OPACITY,
        sl_settings::SettingValue::F32(0.5),
        "Opacity of the name-tag backdrop bubble",
    );
}

/// The shared placeholder sphere mesh and material, built once and reused by
/// every avatar sphere.
struct AvatarAssets {
    /// The shared UV-sphere mesh handle.
    mesh: Handle<Mesh>,
    /// The shared soft-blue material handle.
    material: Handle<FaceMaterial>,
}

/// One nearby avatar as the map surfaces (minimap, radar) consume it — see
/// [`AvatarState::map_avatars`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct MapAvatar {
    /// The avatar's agent id.
    pub(crate) agent: AgentKey,
    /// The world entity whose transform places the avatar.
    pub(crate) anchor: Entity,
    /// For a coarse-only avatar, its last coarse altitude in metres (`0` /
    /// `1020` are the "unknown" sentinels); `None` for a precisely-known
    /// full-object avatar.
    pub(crate) coarse_z: Option<f32>,
}

/// The pair of entities rendering one avatar: its world-space anchor (a
/// placeholder sphere or the root of a rigged body) and its screen-space
/// name-tag text node.
#[derive(Clone, Copy)]
struct AvatarEntities {
    /// The anchor entity — a placeholder sphere or a rigged-body root. Despawned
    /// recursively, so a body's whole joint / mesh sub-hierarchy goes with it.
    anchor: Entity,
    /// The floating name-tag UI text entity.
    label: Entity,
}

/// Viewer-side avatar bookkeeping: the placeholder entities for every nearby
/// avatar, split by which stream it came from, plus a legacy-name cache.
///
/// A full-object avatar's `ObjectRemoved` carries only its scoped local id (not
/// its agent id), so [`by_scoped`](Self::by_scoped) maps back to the agent id the
/// avatar is keyed by.
#[derive(Resource, Default)]
pub(crate) struct AvatarState {
    /// The region the Bevy scene is anchored at (origin `<0,0,0>`), so a **full
    /// object** avatar in a neighbour region is offset onto the right terrain
    /// (mirroring the coarse-dot and object offsets) and every avatar is re-based
    /// when this moves ([`recenter_avatars`]). `None` until the first region is
    /// known; kept in lockstep with the object/terrain origins (all follow
    /// [`SlIdentity`]'s root handle).
    origin: Option<RegionHandle>,
    /// Avatars known as a full in-world object (`pcode` 47), keyed by agent id;
    /// their sphere follows the object's precise position.
    objects: HashMap<AgentKey, AvatarEntities>,
    /// Avatars known only from coarse (minimap) locations — not (currently) a full
    /// object — keyed by agent id; their sphere sits at the 1 m coarse position.
    coarse: HashMap<AgentKey, AvatarEntities>,
    /// The source region of each coarse-only avatar (R24). `CoarseLocationUpdate`
    /// arrives per-region (root *and* each neighbour child circuit), so a coarse
    /// dot is reconciled only against its own region's update — a neighbour's
    /// update must not despawn the root region's dots. Also lets a region's dots be
    /// dropped when that region is disabled (an empty update for the region).
    coarse_region: HashMap<AgentKey, RegionHandle>,
    /// A reverse map from an object's scoped id to its agent id, so an
    /// `ObjectRemoved` can find the avatar to despawn.
    by_scoped: HashMap<ScopedObjectId, AgentKey>,
    /// The skeleton-instance joint entities of each rigged-body avatar, in joint
    /// order (parallel to [`AvatarBody`]'s joint tables), keyed by agent id — the
    /// entities a worn attachment is parented to so it follows the posed skeleton
    /// (P16.1). Absent for a sphere-only (no `--viewer-assets`) avatar.
    joints: HashMap<AgentKey, Vec<Entity>>,
    /// The per-avatar attachment-point node entities, keyed by agent id then by
    /// raw attachment-point id (P16.2). Each node is a child of its skeleton joint
    /// carrying the fixed `avatar_lad.xml` offset; a worn attachment parents to the
    /// node for its point so it seats at the stored local offset from the joint.
    /// Absent for a sphere-only (no `--viewer-assets`) avatar.
    attachment_nodes: HashMap<AgentKey, HashMap<u8, Entity>>,
    /// Resolved names, keyed by agent id — the "simple name cache" that keeps
    /// a repeatedly-seen avatar from being re-requested; merged from the
    /// NameValue seed, the legacy `UUIDNameReply` and the display-name cap.
    names: HashMap<AgentKey, NameRecord>,
    /// Group titles from each avatar object's NameValue `Title` — the classic
    /// mechanism the reference reads for other avatars' tags. (The own
    /// avatar's fresher title comes from `ActiveGroupChanged` via
    /// [`crate::groups::GroupsModel`].)
    titles: HashMap<AgentKey, String>,
    /// Agents whose name has already been requested (but has not necessarily
    /// arrived), so the same request is never sent twice.
    requested: HashSet<AgentKey>,
    /// Agents queued for this frame's batched name request
    /// ([`flush_name_requests`]): one `UUIDNameRequest` **and** one
    /// `GetDisplayNames` cap call per frame, however many avatars appeared
    /// (each cap call costs an HTTP request; cap absence — OpenSim — is a
    /// silent no-op, which is why the legacy request always goes out too).
    pending_name_requests: HashSet<AgentKey>,
    /// The latest `AvatarAppearance.visual_params` byte vector per avatar, kept so
    /// a body spawned after (or re-spawned) can be morphed from the last known
    /// appearance (P13.3).
    appearances: HashMap<AgentKey, Vec<u8>>,
    /// Avatars whose rigged body needs its appearance (re)applied — its morphs
    /// re-blended and its skeleton re-deformed — set on a fresh appearance and on
    /// a newly spawned body, drained by [`apply_avatar_appearance`].
    appearance_dirty: HashSet<AgentKey>,
    /// The debounce ledger behind [`appearance_dirty`](Self::appearance_dirty):
    /// per still-unserviced avatar, when (app elapsed seconds) it was first and
    /// last marked dirty. [`apply_avatar_appearance`] folds fresh marks in each
    /// frame and picks avatars from here under its per-frame budget — a
    /// never-shaped avatar immediately, a re-marked one only after a quiet
    /// window, so the appearance → body-spawn → bake-decode trigger cascade
    /// resolves once instead of once per trigger.
    appearance_pending: HashMap<AgentKey, AppearanceDirtyStamps>,
    /// A generation counter over every input the skeleton pose fold consumes from
    /// this state (deformations, volume deformations, joint overrides, body
    /// physics): bumped by [`bump_pose_inputs`](Self::bump_pose_inputs) whenever
    /// one is (re)applied. The pose gate re-evaluates **all** avatars for one
    /// frame on any bump — coarse but simple, and these are rare events.
    pose_inputs_generation: u64,
    /// The joint position overrides each avatar's worn rigged meshes impose (R1),
    /// keyed by agent id then by the contributing **mesh asset id**. Kept per-mesh
    /// (rather than pre-merged) so the set can be rebuilt as meshes come and go — the
    /// reference viewer's `clearAttachmentOverrides` + rebuild — and so a per-joint
    /// conflict resolves to the highest-mesh-id override (`findActiveOverride`), via
    /// [`effective_joint_overrides`](Self::effective_joint_overrides). Absent for an
    /// avatar wearing no position-carrying rig — its skeleton stays on the plain
    /// appearance shape. `apply_avatar_appearance` folds the effective set in.
    joint_overrides: HashMap<AgentKey, HashMap<Uuid, JointOverrides>>,
    /// Every worn **rigged mesh asset id** bound to each avatar's skeleton, kept so
    /// the avatar-state dump (viewer-avatar-state-dump-replay) can record which
    /// meshes make up an avatar — the heavy geometry itself already persists in the
    /// mesh cache, so only the id set is needed to reconstruct it offline.
    worn_rigged_meshes: HashMap<AgentKey, HashSet<Uuid>>,
    /// Whether each avatar's `TEX_SKIRT_BAKED` slot holds a visible bake, from its
    /// latest appearance — the reference viewer's skirt-worn test. Absent means
    /// not yet known, treated as no skirt (the base skirt mesh stays hidden).
    skirt_visible: HashMap<AgentKey, bool>,
    /// Each avatar's ingested body-physics (`WT_PHYSICS`) configuration (P34.1),
    /// resolved from its latest appearance: the six breast / belly / butt
    /// spring-damper motions, their settings, and the runtime morph params each
    /// one drives. The per-frame simulation (P34.2) reads it; an avatar whose
    /// appearance switches physics off keeps an entry whose motions are all
    /// inactive.
    body_physics: HashMap<AgentKey, BodyPhysics>,
    /// The visible baked-texture id in each base-body region slot per avatar,
    /// from its latest appearance (P14.1): the published baked UUIDs the viewer
    /// fetches through the shared [`TextureManager`] and (from P14.2) drapes over
    /// the system body. Keyed by baked slot ([`BODY_BAKE_SLOTS`]); a slot with no
    /// real bake is simply absent.
    baked_textures: HashMap<AgentKey, HashMap<usize, TextureKey>>,
    /// The base-body region slots each avatar has baked **invisible**
    /// (`IMG_INVISIBLE`) via a worn system alpha layer, from its latest appearance
    /// (R22). These regions are hidden outright ([`apply_avatar_part_visibility`]),
    /// matching the reference viewer's `isTextureVisible`, so the system body does
    /// not render and z-fight a non-BOM mesh body worn over it.
    invisible_regions: HashMap<AgentKey, HashSet<usize>>,
    /// The Current Outfit Folder version whose bakes were last fetched per avatar
    /// (P14.4), so a later `AvatarAppearance` with a strictly-older `cof_version`
    /// (an out-of-order / duplicate resend) is skipped and cannot clobber a newer
    /// bake. Absent means none seen yet; an appearance with no `cof_version`
    /// (OpenSim / the older path) is always ingested.
    baked_cof_version: HashMap<AgentKey, i32>,
    /// Avatars whose body-region bake materials need (re)assigning — set on a
    /// fresh appearance and on a newly spawned body, drained by
    /// [`assign_avatar_bake_materials`] (P14.2).
    bake_dirty: HashSet<AgentKey>,
    /// The parent scoped id of every tracked non-root object (linkset children and
    /// attachments), so an attachment's chain can be chased up to its avatar root
    /// (P13.5 `IMG_USE_BAKED_*` region hide).
    object_parents: HashMap<ScopedObjectId, ScopedObjectId>,
    /// For every tracked non-root object whose texture entry carries
    /// `IMG_USE_BAKED_*` sentinels, the baked slots it replaces — aggregated up the
    /// attachment chain to hide the matching base-avatar mesh regions.
    baked_hides: HashMap<ScopedObjectId, Vec<usize>>,
    /// Non-root objects whose texture entry has already been scanned for
    /// `IMG_USE_BAKED_*` sentinels, so a motion-only update never re-decodes it.
    scanned_objects: HashSet<ScopedObjectId>,
    /// Each rigged avatar's resolved skeletal deformations, the shape
    /// [`apply_avatar_appearance`] last applied — kept so the animation driver
    /// (P18.3) can re-run the Second Life skeletal recurrence with the playing
    /// motion folded in and write each joint's world matrix straight to its
    /// `GlobalTransform` (avoiding the limb-shear a rotation overlaid onto the
    /// baked-scale rest transform would cause). Absent for a sphere-only
    /// (no `--viewer-assets`) avatar, or before its first appearance.
    deformations: HashMap<AgentKey, SkeletalDeformations>,
    /// Each rigged avatar's resolved **collision-volume** displacements (P34.3):
    /// the shape morphs' `<volume_morph>` children, which move the volumes a worn
    /// rigged-mesh body is rigged to. Resolved and folded into the skeletal
    /// recurrence alongside [`deformations`](Self::deformations).
    volume_deformations: HashMap<AgentKey, VolumeDeformations>,
    /// Each avatar's resolved **root drop** (R23): how far below the reported
    /// wire Z its body-root entity is planted, in Second Life Z-up metres —
    /// [`root_drop_from_metrics`] of the shape's `computeBodySize` quantities
    /// (the wire Z is the physics-capsule *centre*, so the drop is half the
    /// shape-scaled body height, corrected for the pelvis sitting above the
    /// root and any hover). Shoe heel / platform offsets (R17) fold in through
    /// the foot term of those metrics, as in the reference. Absent (the rest
    /// shape's [`AvatarBody::rest_root_drop`] applies) until an appearance
    /// resolves, or for a sphere-only avatar.
    root_drops: HashMap<AgentKey, f32>,
    /// Each avatar's resolved **seat drop** (R23 counterpart): the pelvis's
    /// shape-scaled local height above the body root (`pelvis_local_z`), keyed by
    /// agent. A sit offset targets the avatar **root** (hips), so a seated avatar's
    /// anchor is dropped by this so the hips land on the sit target
    /// ([`place_seated_avatars`]) — unlike the standing [`root_drops`](Self::root_drops),
    /// which also folds in the capsule-centre correction that does not apply while
    /// seated. Absent (the rest [`AvatarBody::rest_seat_drop`] applies, seeded on
    /// body spawn) until an appearance resolves, or for a sphere-only avatar.
    seat_drops: HashMap<AgentKey, f32>,
    /// R22b diagnostic: every agent the session has *ever* surfaced a full avatar
    /// object (`pcode` 47) for, so the [`log_avatar_interest`]-gated census can
    /// tell a "the simulator never streamed this avatar" case (agent absent here)
    /// from a "we received it but failed to render it" case (agent present here yet
    /// still a coarse sphere). Never pruned — it is a cumulative diagnostic marker.
    ever_full_object: HashSet<AgentKey>,
    /// The last coarse (minimap) position `(x, y, z)` seen per coarse-only
    /// agent — `x`/`y` region-local metres (0..255), `z` already in metres
    /// (0..1020, the `u8 × 4` coarse scale). A `z` at the 1020 ceiling is the
    /// simulator's "height unknown / off this region" sentinel; a `0` from some
    /// simulators means the same. Read by the R22b census diagnostic and by the
    /// minimap's dot layer (the unknown-altitude glyph).
    coarse_pos: HashMap<AgentKey, (u8, u8, u16)>,
    /// Avatars currently **seated on an object** (their full-object `ObjectUpdate`
    /// carries a non-zero `ParentID`), keyed by agent id — self and others alike
    /// (several avatars share one boat). The value is the seat and the avatar's
    /// pose **in the seat's frame** (the parent-relative wire transform, the
    /// `llSitTarget` offset): [`place_seated_avatars`] composes it onto the seat's
    /// live world transform each frame so the avatar rides the moving seat, and
    /// [`drive_avatar_motion`](crate::physics::drive_avatar_motion) leaves a
    /// [`Seated`] anchor alone (its motion is the seat's, not region dead-reckoned).
    /// Entries clear the instant an update arrives with `ParentID` zero (a stand).
    seated: HashMap<AgentKey, SeatedTarget>,
    /// The shared placeholder sphere mesh + material, built lazily on first use.
    assets: Option<AvatarAssets>,
}

/// Where a seated avatar sits: the seat object and the avatar's pose **relative to
/// the seat**, both taken from the seated avatar's `ObjectUpdate` (whose
/// `ParentID` is the seat and whose `motion` is parent-relative — the reference's
/// `sitOnObject` `rel_pos` / `rel_rot`). Kept in pure Second Life space (no axis
/// swap): the seat entity carries the single SL→Bevy basis change, so composing
/// this onto the seat's world transform places the avatar exactly as a linkset
/// child prim at the same offset would sit. **No root drop** is applied — the
/// reference skips the pelvis/capsule correction entirely while sitting on an
/// object (`LLVOAvatar::updateRootPositionAndRotation` takes the parent transform
/// directly).
#[derive(Debug, Clone, Copy)]
struct SeatedTarget {
    /// The seat object's scoped id — resolved to its scene entity through
    /// [`ObjectState::entity_by_scoped`](crate::objects::ObjectState::entity_by_scoped)
    /// each frame (the seat may stream in after, or independently of, the avatar).
    seat: ScopedObjectId,
    /// The avatar's pose in the seat's local frame, as a pure-SL [`Transform`].
    offset: Transform,
}

/// Marker on a seated avatar's anchor: its world pose is driven by
/// [`place_seated_avatars`] from its seat, so the region-space dead-reckoner
/// ([`drive_avatar_motion`](crate::physics::drive_avatar_motion)) must leave it be.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct Seated;

/// The maximum attachment/linkset depth chased when attributing an object's
/// `IMG_USE_BAKED_*` hide to its avatar, a guard against a malformed parent cycle.
const MAX_ATTACHMENT_DEPTH: usize = 32;

/// The shared, per-avatar-invariant render assets for the rigged base body,
/// built once from [`AvatarAssetLibrary`] and reused by every avatar body: one
/// mesh / material / inverse-bindposes set, plus the joint rest data a fresh
/// skeleton instance is spawned from.
///
/// Present as a resource only when the `--viewer-assets` directory loaded; its
/// absence is the signal to fall back to the placeholder sphere.
#[derive(Resource, Debug)]
pub(crate) struct AvatarBody {
    /// The shared skin material for the un-textured body.
    material: Handle<FaceMaterial>,
    /// One render entry per resolved base part.
    parts: Vec<BodyPart>,
    /// Each joint's local rest transform (Second Life Z-up), parallel to
    /// [`joint_parents`](Self::joint_parents); a fresh joint entity is spawned
    /// per avatar from these.
    joint_locals: Vec<Transform>,
    /// Each joint's parent index (`None` for a root), parallel to
    /// [`joint_locals`](Self::joint_locals).
    joint_parents: Vec<Option<usize>>,
    /// The rest shape's root drop (Second Life Z, metres): how far below the
    /// reported wire Z (the physics-capsule centre) the body root is planted
    /// until the avatar's own appearance resolves (R23). From
    /// [`root_drop_from_metrics`] of the rest-skeleton `computeBodySize`
    /// quantities, with no hover.
    rest_root_drop: f32,
    /// The rest shape's **seat drop** (Second Life Z, metres): how far below the
    /// avatar root (hips / `mPelvis`) the body-root entity sits — the pelvis's
    /// local rest height above the root (`pelvis_local_z`), until the avatar's own
    /// appearance resolves. A sit offset targets the avatar **root** (hips), not
    /// the feet, so a seated avatar's anchor is dropped by this so the hips land on
    /// the sit target ([`place_seated_avatars`]) — the seated counterpart of
    /// [`rest_root_drop`](Self::rest_root_drop) (which also corrects for the
    /// standing capsule centre, irrelevant when seated).
    rest_seat_drop: f32,
    /// Each attachment point's raw numeric id mapped to the joint it hangs from
    /// and its fixed local offset node (P16.1/P16.2). Built from the
    /// `avatar_lad.xml` `<attachment_point>` table; a HUD point (whose `mScreen`
    /// pseudo-joint is not a body joint) is absent, so only body attachments
    /// resolve to a joint.
    attachment_points: HashMap<u8, BodyAttachmentPoint>,
    /// The skeleton's joint canonical-name / alias → joint index lookup (P17.2),
    /// so a worn rigged mesh's own `joint_names` table can be resolved against a
    /// spawned avatar's skeleton-instance joint entities.
    joint_lookup: HashMap<String, usize>,
    /// Whether each joint is a collision volume rather than a bone, parallel to
    /// [`joint_locals`](Self::joint_locals) — so the rigged-mesh bind can report
    /// whether a rig is *fitted* (binds the volumes the shape displaces, P34.3).
    joint_is_volume: Vec<bool>,
}

impl AvatarBody {
    /// The skeleton joint index a rigged mesh's joint name binds to, resolving a
    /// canonical name or an alias like the base body does (P17.2). `None` for a
    /// name the standard skeleton does not carry.
    pub(crate) fn joint_index(&self, name: &str) -> Option<usize> {
        self.joint_lookup.get(name).copied()
    }

    /// Whether the joint a rigged mesh's `name` binds to is a **collision volume**
    /// (`LEFT_PEC`, `BELLY`, …) rather than a bone — i.e. whether binding it makes
    /// the rig *fitted*, following the shape's volume morphs (P34.3). `false` for a
    /// name the standard skeleton does not carry.
    pub(crate) fn is_collision_volume(&self, name: &str) -> bool {
        self.joint_index(name)
            .is_some_and(|index| self.joint_is_volume.get(index).copied().unwrap_or(false))
    }

    /// The skeleton joint index a **rigid** base part (the eyeballs) is pinned to,
    /// or `None` for a skinned part or an out-of-range index. The animation driver
    /// (P18.3) uses it to write a rigid part's `GlobalTransform` from its joint's
    /// posed world matrix, since Bevy's transform propagation ran before the driver
    /// overwrote the joint globals.
    pub(crate) fn rigid_joint_index(&self, part: usize) -> Option<usize> {
        match self.parts.get(part)?.binding {
            BodyPartBinding::Rigid(index) => Some(index),
            BodyPartBinding::Skinned { .. } => None,
        }
    }

    /// The joint position overrides a worn rigged mesh `skin` imposes on this
    /// skeleton (R1): its rig-supplied per-joint rest positions, resolved against
    /// the shared skeleton's name lookup and default local transforms. Empty when
    /// the rig ships no joint positions (an unfitted rig). The result is applied by
    /// [`apply_avatar_appearance`] so the mesh deforms undistorted (the reference
    /// viewer's `addAttachmentOverridesForObject`).
    pub(crate) fn joint_overrides(&self, skin: &MeshSkin) -> JointOverrides {
        joint_position_overrides(skin, &self.joint_lookup, &self.joint_locals)
    }

    /// Spawn a **bare** skeleton instance — one joint entity per skeleton joint,
    /// in joint order, parented into the hierarchy under `root` — with no base-body
    /// parts, attachment nodes, or name tag. Used by the animesh control avatar
    /// (P29), which drives the standard skeleton for a scripted linkset that has no
    /// wearer, so it needs the joints but none of the avatar body chrome.
    ///
    /// The joints carry no [`AvatarJoint`] marker (a control avatar is not an
    /// agent-keyed avatar and is not touched by the appearance pass); the caller
    /// owns them via the returned list and despawns them with the `root`
    /// sub-hierarchy. Mirrors the joint-spawning half of [`AvatarState::spawn_body`].
    pub(crate) fn spawn_bare_skeleton(&self, root: Entity, commands: &mut Commands) -> Vec<Entity> {
        let joints: Vec<Entity> = self
            .joint_locals
            .iter()
            .map(|local| commands.spawn((*local, Visibility::default())).id())
            .collect();
        for (entity, parent) in joints.iter().zip(self.joint_parents.iter().copied()) {
            let target = parent
                .and_then(|index| joints.get(index).copied())
                .unwrap_or(root);
            commands.entity(*entity).insert(ChildOf(target));
        }
        joints
    }
}

/// A resolved attachment point on the shared body (P16.2): the joint index it
/// hangs from and its fixed local offset [`Transform`] from that joint (Second
/// Life Z-up space, so it composes with a linkset child's local transform).
///
/// The reference viewer models each point as a node parented to its skeleton
/// joint at this offset (`avatar_lad.xml`'s `position` / `rotation`); a worn
/// object's own local transform is then relative to that node. The viewer spawns
/// one such node per avatar so a rigid attachment seats where it does there.
#[derive(Debug, Clone, Copy)]
struct BodyAttachmentPoint {
    /// The skeleton joint index this point hangs from.
    joint_index: usize,
    /// The point's fixed local offset from that joint (Second Life Z-up space).
    offset: Transform,
}

/// One base part's shared render data.
#[derive(Debug)]
struct BodyPart {
    /// The Bevy mesh, shared across avatars (identical un-morphed geometry).
    mesh: Handle<Mesh>,
    /// How the part binds to a skeleton instance's joint entities.
    binding: BodyPartBinding,
    /// Which baked region this part belongs to (for P13.5 visibility).
    region: BodyRegion,
}

/// A base part's skeleton binding, resolved to Bevy render data.
#[derive(Debug)]
enum BodyPartBinding {
    /// A skinned part: shared inverse bindposes plus the skeleton joint indices
    /// its `JOINT_INDEX` attribute maps to (in the part's own table order).
    Skinned {
        /// The shared inverse-bindposes asset, parallel to
        /// [`joint_map`](Self::Skinned::joint_map).
        inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
        /// The skeleton joint index each `JOINT_INDEX` slot refers to; mapped to
        /// this avatar's joint entities to fill `SkinnedMesh.joints`.
        joint_map: Vec<usize>,
    },
    /// A rigid (un-skinned) part parented to the skeleton joint at this index.
    Rigid(usize),
}

/// Startup system: if the avatar asset library loaded, build the shared body
/// render assets and insert them as [`AvatarBody`], so every rigged avatar reuses
/// one mesh / material / inverse-bindposes set. A no-op (leaving avatars as
/// spheres) when no `--viewer-assets` directory was given or it failed to load.
pub(crate) fn setup_avatar_body(
    library: Option<Res<AvatarAssetLibrary>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let Some(library) = library else {
        return;
    };
    let material = materials.add(inert_face_material(StandardMaterial {
        base_color: BODY_COLOR,
        ..default()
    }));
    let mut parts = Vec::with_capacity(library.parts().len());
    for part in library.parts() {
        let mesh = meshes.add(to_bevy_base_mesh(&part.mesh));
        let binding = match &part.binding {
            LoadedBinding::Skinned(skin) => BodyPartBinding::Skinned {
                inverse_bindposes: bindposes.add(SkinnedMeshInverseBindposes::from(
                    skin.inverse_bindposes.clone(),
                )),
                joint_map: skin.joints.clone(),
            },
            LoadedBinding::Rigid(index) => BodyPartBinding::Rigid(*index),
        };
        parts.push(BodyPart {
            mesh,
            binding,
            region: part.region,
        });
    }
    let skeleton = library.skeleton();
    let part_count = parts.len();
    commands.insert_resource(AvatarBody {
        material,
        parts,
        joint_locals: skeleton.local_transforms().to_vec(),
        joint_parents: skeleton.parents().to_vec(),
        joint_lookup: skeleton.lookup().clone(),
        joint_is_volume: (0..skeleton.len())
            .map(|index| skeleton.is_collision_volume(index))
            .collect(),
        // The rest-shape drop; a malformed skeleton (missing chain joints)
        // falls back to the old pelvis-rest-height plant rather than none.
        rest_root_drop: skeleton
            .body_size_metrics(&SkeletalDeformations::default(), &JointOverrides::default())
            .map_or_else(
                || library.pelvis_height(),
                |metrics| root_drop_from_metrics(&metrics, 0.0),
            ),
        // The rest-shape seat drop: the pelvis's rest height above the body root
        // (the sit offset targets the hips, so a seated body drops by this). The
        // pelvis rest height is the same fallback the root drop uses.
        rest_seat_drop: skeleton
            .body_size_metrics(&SkeletalDeformations::default(), &JointOverrides::default())
            .map_or_else(|| library.pelvis_height(), |metrics| metrics.pelvis_local_z),
        attachment_points: library
            .attachment_points()
            .into_iter()
            .map(|(id, info)| {
                (
                    id,
                    BodyAttachmentPoint {
                        joint_index: info.joint_index,
                        // The `avatar_lad.xml` offset lives in the joint's Second
                        // Life Z-up frame — the same frame a linkset child's local
                        // transform uses — so it needs no basis change here (P16.2).
                        offset: Transform {
                            translation: Vec3::new(
                                info.position[0],
                                info.position[1],
                                info.position[2],
                            ),
                            rotation: sl_euler_deg_to_quat(info.rotation_euler_deg),
                            scale: Vec3::ONE,
                        },
                    },
                )
            })
            .collect(),
    });
    info!("built rigged avatar body ({part_count} parts)");
}

/// A placeholder sphere avatar's world translation: its region-local position in
/// Bevy space, plus the neighbour-region offset (zero for the root region) so it
/// lands on the right terrain — the sphere counterpart of [`body_root_transform`].
fn sphere_translation(object: &Object, region_offset: Vec3) -> Vec3 {
    let local = sl_to_bevy_vec(&object.motion.position);
    Vec3::new(
        local.x + region_offset.x,
        local.y + region_offset.y,
        local.z + region_offset.z,
    )
}

/// The world [`Transform`] of a rigged avatar body root: the object's position
/// and orientation carried into Bevy's Y-up world by the Second Life → Bevy
/// basis change, lowered by `root_drop` (R23).
///
/// The wire position of an avatar is **not** its pelvis: OpenSim reports the
/// physics-capsule *centre* (`ScenePresence`'s `ground + 0.5 · AvatarHeight`),
/// and the reference viewer assumes the same
/// (`LLVOAvatar::updateCharacter`'s `root_pos.z -= 0.5·mBodySize.z −
/// mPelvisToFoot`, "correct for the fact that the pelvis is not necessarily
/// the center of the agent's physical representation"). `root_drop` is the
/// per-avatar [`root_drop_from_metrics`] of the shape's `computeBodySize`
/// quantities — half the shape-scaled body height, corrected for the pelvis
/// joint sitting `pelvis_local_z` above this root and lifted by any hover.
fn body_root_transform(object: &Object, root_drop: f32, region_offset: Vec3) -> Transform {
    let translation = sl_to_bevy_vec(&object.motion.position);
    Transform {
        // Per-component add/subtract to avoid the `arithmetic_side_effects` lint on
        // the glam `Vec3` operator. `region_offset` places a neighbour region's
        // avatar onto the right terrain (zero for one in the root region).
        translation: Vec3::new(
            translation.x + region_offset.x,
            translation.y - root_drop + region_offset.y,
            translation.z + region_offset.z,
        ),
        rotation: sl_to_bevy_object_rotation(&object.motion.rotation),
        scale: Vec3::ONE,
    }
}

/// A seated avatar's pose **in its seat's local frame**, as a pure Second Life
/// [`Transform`] (no axis swap, no root drop) — the parent-relative wire position
/// and rotation of an avatar `ObjectUpdate` whose `ParentID` is the seat. The seat
/// entity carries the single SL→Bevy basis change for the whole subtree, so
/// [`place_seated_avatars`] composes this onto the seat's world transform exactly
/// as a linkset child prim at the same offset would compose (`objects.rs`'s child
/// branch). The reference (`LLVOAvatar::sitOnObject`) parents the avatar root at
/// this same seat-relative `rel_pos` / `rel_rot` and applies **no** pelvis / root
/// correction while seated.
///
/// Because no root drop is applied, the shoe / heel-platform offset (R17) — which
/// rides the `pelvis_to_foot` term of the drop ([`root_drop_from_metrics`]) — is
/// **excluded** while seated, matching the reference: its
/// `updateRootPositionAndRotation` takes the `!(isSitting() && getParent())` branch
/// for an object sit and never folds the foot correction in.
fn seated_offset(object: &Object) -> Transform {
    Transform {
        translation: Vec3::new(
            object.motion.position.x,
            object.motion.position.y,
            object.motion.position.z,
        ),
        rotation: sl_rotation_to_quat(&object.motion.rotation),
        scale: Vec3::ONE,
    }
}

/// Drop a seated avatar's seat-relative pose so the **hips** (avatar root) land on
/// the sit target rather than the body root.
///
/// A sit offset targets the avatar root (the `mPelvis` hips), but our anchor is the
/// body root, which sits `seat_drop` (the pelvis's local rest height) below the
/// pelvis. Lower the anchor by `seat_drop` along the **avatar's** up (the sit
/// rotation's local up, so a reclined / tilted seat drops correctly), all in the
/// seat's pure-Second-Life frame — `place_seated_avatars` then composes it onto the
/// seat's world transform. The reference reaches the same placement differently
/// (its `mRoot` *is* the pelvis, so it needs no such drop —
/// `LLVOAvatar::updateRootPositionAndRotation` skips the standing correction while
/// seated).
fn drop_to_hips(offset: Transform, seat_drop: f32) -> Transform {
    // The pelvis-above-root offset, rotated into the sit orientation (pure SL).
    let drop = offset.rotation.mul_vec3(Vec3::new(0.0, 0.0, seat_drop));
    Transform {
        // Per-component subtract to avoid the `arithmetic_side_effects` lint on the
        // glam `Vec3` operator.
        translation: Vec3::new(
            offset.translation.x - drop.x,
            offset.translation.y - drop.y,
            offset.translation.z - drop.z,
        ),
        rotation: offset.rotation,
        scale: offset.scale,
    }
}

/// How far below an avatar's reported wire Z (the physics-capsule centre) its
/// body-root entity is planted (R23), in Second Life Z-up metres.
///
/// The reference (`LLVOAvatar::updateCharacter`) places its `mRoot` — whose
/// pelvis is zeroed under it, so `mRoot` *is* the pelvis — at
/// `reported_z − (0.5·mBodySize.z − mPelvisToFoot) + hover`. Our skeleton
/// instance keeps `mPelvis` at its local rest offset above the body root, so
/// the root sits a further `pelvis_local_z` below the pelvis:
///
/// `drop = 0.5·body_size_z − pelvis_to_foot + pelvis_local_z − hover`
///
/// Net effect matches the reference exactly: the soles land at
/// `reported_z − 0.5·body_size_z + hover` (the `pelvis_to_foot` term cancels
/// out of the sole height). `hover` is the shape's `Hover` visual param
/// ([`AVATAR_HOVER_PARAM`]); the region-side hover preference
/// (`getHoverOffset()`, the `AgentPreferences` capability) is not ingested
/// yet and is omitted.
fn root_drop_from_metrics(metrics: &BodySizeMetrics, hover: f32) -> f32 {
    0.5 * metrics.body_size_z - metrics.pelvis_to_foot + metrics.pelvis_local_z - hover
}

/// The `Hover` visual param id (`avatar_lad.xml` id 11001, the reference's
/// `AVATAR_HOVER`): a transmitted shape slider that raises / lowers the whole
/// avatar relative to the ground, added to the root plant (R23).
const AVATAR_HOVER_PARAM: i32 = 11001;

/// A debug affordance (env `SL_VIEWER_VOLUME_FOCUS`): aim the fly-camera at the
/// avatar whose shape displaces its **collision volumes** the most (P34.3 / P34.4),
/// from a few metres back — the counterpart of
/// [`focus_camera_on_particles`](crate::particles::focus_camera_on_particles).
///
/// The displacement moves only a worn *fitted* rigged mesh, and only as far as the
/// wearer's own shape sliders take them. That was a real obstacle for the P34.3
/// morph pass, which a slim, near-default shape barely engages: it displaces its
/// volumes by millimetres and shows nothing however hard the effect is amplified, so
/// a live check needed the most extreme shape *present in the region*, rarely the
/// agent's own — this finds it. The P34.4 skeletal pass is less picky (every avatar
/// has a height and a thickness, and the inherited delta is a *fraction* of the
/// volume's own size), but the same ranking still picks the clearest subject.
/// `SL_VIEWER_CAMERA_DISTANCE` sets how far back to stand (default 3 m).
///
/// Runs after the fly-camera (so it overrides the login snap and the input pose).
pub(crate) fn focus_camera_on_volume_shape(
    state: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
    roots: Query<&GlobalTransform>,
    mut mode: ResMut<crate::camera::CameraMode>,
    mut camera: Query<
        (&mut Transform, &mut crate::camera::CameraRig),
        With<crate::camera::ViewerCamera>,
    >,
    mut setting: Local<Option<String>>,
    mut framed: Local<bool>,
) {
    // Frame the subject **once**, then leave the camera alone: re-aiming every frame
    // would pin the fly-camera (no flying around the avatar to inspect it) and, worse,
    // would jump to a different avatar the moment the `V` toggle zeroes every
    // displacement and the "most displaced" pick changes.
    if *framed {
        return;
    }
    // `SL_VIEWER_VOLUME_FOCUS=1` frames whichever avatar is displaced most; set it to
    // an **agent id** instead to pin the subject. An empty setting means the switch is
    // off (the env is absent), which is the common case.
    let setting = setting.get_or_insert_with(|| {
        std::env::var("SL_VIEWER_VOLUME_FOCUS").unwrap_or_else(|_error| String::new())
    });
    if setting.is_empty() {
        return;
    }
    let pinned = (setting.as_str() != "1").then_some(setting.as_str());
    // The pinned avatar, else the one with the largest total displacement (summed
    // over every volume, the scale deltas and the position deltas in metres alike —
    // both move a mesh body).
    let Some((agent, score)) = state
        .volume_deformations
        .iter()
        .filter(|(agent, _)| pinned.is_none_or(|id| agent.uuid().to_string() == id))
        .map(|(&agent, volumes)| (agent, volume_displacement_score(volumes)))
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
    else {
        return;
    };
    // Aim at the avatar's **chest joint**, not its body-root anchor: the joint is
    // where the posed skeleton (and therefore the rendered mesh body) actually is,
    // and its world rotation gives the direction the avatar faces. The anchor's
    // rotation is not a reliable basis for that.
    let Some(chest) = body.as_ref().and_then(|body| {
        let index = body.joint_index("mChest")?;
        state.joint_entities_of(agent)?.get(index).copied()
    }) else {
        return;
    };
    let Ok(global) = roots.get(chest) else {
        return;
    };
    let Ok((mut transform, mut rig)) = camera.single_mut() else {
        return;
    };
    let distance = std::env::var("SL_VIEWER_CAMERA_DISTANCE")
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
        .unwrap_or(3.0);
    // Stand off the avatar's own forward (its Second Life +X, carried into Bevy by
    // the joint's world rotation) at chest height, looking back at the chest. A
    // degenerate forward (a joint whose facing flattens to nothing) falls back to
    // world +X, so the camera never ends up *inside* the avatar seeing nothing.
    let target = global.translation();
    let forward = global.rotation().mul_vec3(Vec3::X);
    let flat = Vec3::new(forward.x, 0.0, forward.z)
        .try_normalize()
        .unwrap_or(Vec3::X);
    let eye = Vec3::new(
        target.x + flat.x * distance,
        target.y + 0.2,
        target.z + flat.z * distance,
    );
    info!(
        "volume focus: framing {agent} (displacement score {score:.4}) \
         at chest {target:?} from {eye:?}; the camera is yours from here — press V to \
         toggle the collision-volume displacement (P34.3 / P34.4)"
    );
    // Switch to flycam (the only mode whose pose a system may write; the others
    // recompute it) and seed the rig aim so the flycam driver reproduces the look.
    *mode = crate::camera::CameraMode::Flycam;
    let look = Vec3::new(target.x - eye.x, target.y - eye.y, target.z - eye.z);
    rig.aim_along(look);
    *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    *framed = true;
}

/// How much a resolved shape displaces an avatar's collision volumes in total — the
/// ranking [`focus_camera_on_volume_shape`] picks its subject by. Sums the magnitude
/// of every volume's scale and position delta.
fn volume_displacement_score(volumes: &VolumeDeformations) -> f32 {
    volumes
        .iter()
        .map(|(_name, deform)| {
            Vec3::from_array(deform.scale).length() + Vec3::from_array(deform.position).length()
        })
        .sum()
}

/// The live A/B state of the shape's collision-volume displacement (P34.3 morph
/// pass + P34.4 skeletal inheritance): the gain every resolved
/// [`VolumeDeformations`] is scaled by.
///
/// A resource rather than a plain env read so the effect can be toggled **during a
/// session** ([`toggle_volume_morphs`], the `V` key). Two logins can never be
/// compared honestly — the sun has moved, the scene has streamed differently, and
/// (on a live grid) the other avatars in the region are different people — so the
/// only sound A/B of an avatar-shaping effect is one taken on the same avatar, in
/// the same frame sequence, with a switch flipped.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct VolumeMorphGain {
    /// The multiplier on every volume displacement: `1` is faithful, `0` reproduces
    /// the rest volumes an avatar had before either pass existed, and a large value
    /// exaggerates a shape whose real displacements are only centimetres.
    pub(crate) gain: f32,
}

impl Default for VolumeMorphGain {
    fn default() -> Self {
        Self {
            gain: volume_morph_gain(),
        }
    }
}

/// The `V` key: toggle the shape's collision-volume displacement between faithful
/// (`1`) and off (`0`) and re-resolve every avatar's appearance, so the effect can
/// be seen appearing and disappearing on one avatar, live (P34.3 / P34.4).
///
/// The volume displacement moves only a worn *fitted* mesh body, so this is the one
/// way to watch the effect land on a real avatar without trusting a cross-login
/// screenshot pair.
pub(crate) fn toggle_volume_morphs(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut gain: ResMut<VolumeMorphGain>,
    mut state: ResMut<AvatarState>,
) {
    if !keyboard.just_pressed(KeyCode::KeyV) {
        return;
    }
    gain.gain = if gain.gain > 0.5 { 0.0 } else { 1.0 };
    info!(
        "collision-volume displacement {} (gain {}) — the shape's volume morphs \
         (P34.3) and the skeletal params' inherited bone scale (P34.4)",
        if gain.gain > 0.5 { "ON" } else { "OFF" },
        gain.gain
    );
    // Re-resolve every avatar's shape with the new gain.
    let agents: Vec<AgentKey> = state.appearances.keys().copied().collect();
    state.appearance_dirty.extend(agents);
}

/// The debug A/B gain (env `SL_VIEWER_VOLUME_MORPH_GAIN`, default `1`) applied to
/// the shape's collision-volume displacements (P34.3 / P34.4) — `0` leaves every
/// volume at its `avatar_skeleton.xml` rest transform, reproducing the rigged-mesh
/// body that ignores the shape sliders entirely; a large value exaggerates them.
///
/// The displacements are centimetres, and they move *only* a worn rigged mesh (the
/// system body is not skinned to the collision volumes), so an exaggerated gain is
/// the only way to *see* on a live avatar that the accumulation reaches a mesh body
/// at all — the counterpart of the reference viewer's `physics_test` switch, ported
/// as `SL_VIEWER_PHYSICS_TEST` for the same reason. A malformed value is ignored.
fn volume_morph_gain() -> f32 {
    std::env::var("SL_VIEWER_VOLUME_MORPH_GAIN")
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
        .filter(|gain| gain.is_finite())
        .unwrap_or(1.0)
}

/// Spawn one base part's render entity into a skeleton instance: a `SkinnedMesh`
/// under the body root for a skinned part, or a plain mesh parented to a single
/// joint for a rigid part. A part whose joints cannot be resolved is skipped.
///
/// Each spawned entity carries an [`AvatarBodyPart`] marker (its `agent` and part
/// `index`) so [`apply_avatar_appearance`] can later swap in a morphed mesh.
fn spawn_body_part(
    part: &BodyPart,
    index: usize,
    agent: AgentKey,
    joints: &[Entity],
    root: Entity,
    material: &Handle<FaceMaterial>,
    commands: &mut Commands,
) {
    let marker = AvatarBodyPart {
        agent,
        part: index,
        region: part.region,
    };
    // The skirt is hidden until an appearance says a skirt is worn; every other
    // region shows by default, hidden only if a worn attachment replaces it. The
    // per-frame [`apply_avatar_part_visibility`] keeps this current; the initial
    // value only avoids a one-frame flash of an un-worn skirt.
    let initial = match part.region {
        BodyRegion::Skirt => Visibility::Hidden,
        _other => Visibility::Inherited,
    };
    match &part.binding {
        BodyPartBinding::Skinned {
            inverse_bindposes,
            joint_map,
        } => {
            let Some(part_joints) = joint_map
                .iter()
                .map(|&index| joints.get(index).copied())
                .collect::<Option<Vec<Entity>>>()
            else {
                return;
            };
            commands.spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                initial,
                SkinnedMesh {
                    inverse_bindposes: inverse_bindposes.clone(),
                    joints: part_joints,
                },
                // A skinned mesh's frustum bounds are computed once from its bind
                // pose, which does not track the posed/animated vertices; without
                // this the whole avatar is wrongly culled when the camera zooms in
                // close (the narrow near frustum misses the stale bounds).
                NoFrustumCulling,
                ChildOf(root),
                marker,
                // Reusable avatar identity: a ray hitting this part resolves to
                // its wearer. See [`AvatarPickTarget`].
                AvatarPickTarget { agent },
            ));
        }
        BodyPartBinding::Rigid(joint_index) => {
            let Some(joint) = joints.get(*joint_index).copied() else {
                return;
            };
            commands.spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                initial,
                // Match the skinned parts: never frustum-cull an avatar part, so a
                // close camera can pass through the body the way it does in Second
                // Life instead of the part popping out of view.
                NoFrustumCulling,
                ChildOf(joint),
                marker,
                // Reusable avatar identity: a ray hitting this part resolves to
                // its wearer. See [`AvatarPickTarget`].
                AvatarPickTarget { agent },
            ));
        }
    }
}

/// The placeholder UV-sphere mesh (radius [`AVATAR_SPHERE_RADIUS`]).
fn placeholder_sphere_mesh() -> Mesh {
    Sphere::new(AVATAR_SPHERE_RADIUS)
        .mesh()
        .uv(SPHERE_SECTORS, SPHERE_STACKS)
}

/// Float each rigged avatar's name tag just above its skeleton's head top,
/// every frame (the spawn-time `BODY_TAG_HEIGHT` guess sits *in* the head of
/// a tall mesh body — the reference anchors the tag above the actual head).
///
/// **Frame:** the body root carries the whole Second Life → Bevy basis change
/// ([`crate::coords::sl_to_bevy_object_rotation`], a `-90°` turn about `X`),
/// so its child subtree — the skeleton — is in **Second Life space**: `+Z`
/// up. The head top is the joints' `z` span top, grown past the head joint to
/// the crown ([`HEAD_TOP_MARGIN`]) and quantised to a 1 cm grid so idle
/// breathe/sway cannot churn the tag target every frame.
///
/// (Until the Phase 4 joint-entity removal, the joints this reads are the
/// CPU-written — in the GPU in-place path, frozen rest-pose — globals; the
/// tag height only needs the avatar's standing extent, which those carry.)
pub(crate) fn fit_avatar_tag_heights(
    avatars: Res<AvatarState>,
    globals: Query<&GlobalTransform>,
    mut tags: Query<&mut NameTag>,
) {
    for (agent, _anchor, _tag) in avatars.labelled_avatars() {
        let (Some(root), Some(joints)) = (
            avatars.body_root_of(agent),
            avatars.joint_entities_of(agent),
        ) else {
            continue;
        };
        let Ok(root_global) = globals.get(root) else {
            continue;
        };
        // Joint heights in the root's own (Second Life, Z-up) frame, via its
        // inverse — the `z` component is the vertical one here.
        let to_local = root_global.affine().inverse();
        let mut max_z = f32::NEG_INFINITY;
        for joint in joints {
            let Ok(joint_global) = globals.get(*joint) else {
                continue;
            };
            let z = to_local.transform_point3(joint_global.translation()).z;
            max_z = max_z.max(z);
        }
        if !max_z.is_finite() {
            continue;
        }
        let quantise = |value: f32| (value * 100.0).round() / 100.0;
        let top = quantise(max_z + HEAD_TOP_MARGIN);
        if let Some(label) = avatars.label_of(agent)
            && let Ok(mut tag) = tags.get_mut(label)
        {
            // `top` already carries the crown margin; a small extra clearance
            // approximates the reference's 0.17×height ellipsoid offset
            // (the +25 px screen lift rides in the tag mesh itself).
            let desired = top + 0.1;
            if (tag.tag_height - desired).abs() > 0.05 {
                tag.tag_height = desired;
            }
        }
    }
}

/// The placeholder material (opaque soft blue).
fn placeholder_material() -> FaceMaterial {
    inert_face_material(StandardMaterial {
        base_color: AVATAR_COLOR,
        ..default()
    })
}

/// The coarse (minimap) position of an avatar as a Bevy translation.
///
/// A [`CoarseLocation`] is a whole-metre position relative to the region's
/// south-west corner (`x`/`y` in `0`–`255`, `z` already in metres), carried into
/// Bevy's Y-up world by the Second Life → Bevy [axis map](crate::coords). It sits
/// in the root region's frame like the objects in [`objects`](crate::objects) —
/// no multi-region origin offset yet.
fn coarse_translation(location: &CoarseLocation, offset_east: f32, offset_north: f32) -> Vec3 {
    let position = sl_client_bevy::Vector {
        x: offset_east + f32::from(location.x),
        y: offset_north + f32::from(location.y),
        z: f32::from(location.z),
    };
    sl_to_bevy_vec(&position)
}

/// The provisional tag text for an agent before its real name resolves: a short
/// leading fragment of its id, so the avatars are distinguishable immediately.
fn provisional_label(agent: AgentKey) -> String {
    agent
        .uuid()
        .simple()
        .to_string()
        .chars()
        .take(PROVISIONAL_ID_CHARS)
        .collect()
}

impl AvatarState {
    /// The shared placeholder mesh and material handles, building them on first
    /// use. Borrows only [`assets`](Self::assets), so a caller can hold a
    /// disjoint borrow of the other maps.
    fn asset_handles(
        assets: &mut Option<AvatarAssets>,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<FaceMaterial>,
    ) -> (Handle<Mesh>, Handle<FaceMaterial>) {
        let built = assets.get_or_insert_with(|| AvatarAssets {
            mesh: meshes.add(placeholder_sphere_mesh()),
            material: materials.add(placeholder_material()),
        });
        (built.mesh.clone(), built.material.clone())
    }

    /// The tag text for an agent: its display name when resolved, else its
    /// legacy name, else a provisional id fragment until either arrives.
    pub(crate) fn label_text(&self, agent: AgentKey) -> String {
        self.names
            .get(&agent)
            .and_then(NameRecord::preferred_name)
            .map_or_else(|| provisional_label(agent), str::to_owned)
    }

    /// Every labelled avatar: `(agent, anchor entity, label entity)` — full
    /// objects first, then the coarse-only spheres (the object path despawns
    /// a coarse twin, but the filter keeps a mid-frame overlap harmless).
    /// The tag-content composer iterates this.
    pub(crate) fn labelled_avatars(&self) -> impl Iterator<Item = (AgentKey, Entity, Entity)> + '_ {
        self.objects
            .iter()
            .map(|(agent, entities)| (*agent, entities.anchor, entities.label))
            .chain(
                self.coarse
                    .iter()
                    .filter(|(agent, _)| !self.objects.contains_key(agent))
                    .map(|(agent, entities)| (*agent, entities.anchor, entities.label)),
            )
    }

    /// This agent's resolved legacy name, if one has arrived yet.
    ///
    /// The avatar context menu reads it for actions that carry a name on the wire
    /// (a mute entry names the muted avatar); a `None` means the name has not
    /// resolved, and the caller falls back to a provisional label.
    pub(crate) fn name_of(&self, agent: AgentKey) -> Option<&str> {
        self.names
            .get(&agent)
            .and_then(|record| record.legacy.as_deref())
    }

    /// This agent's full name record, if any of its sources answered yet —
    /// the tag-content composer reads the display name / username / default
    /// flag from it.
    pub(crate) fn name_record(&self, agent: AgentKey) -> Option<&NameRecord> {
        self.names.get(&agent)
    }

    /// This agent's group title (from its avatar object's NameValue `Title`),
    /// if it has one.
    pub(crate) fn title_of(&self, agent: AgentKey) -> Option<&str> {
        self.titles.get(&agent).map(String::as_str)
    }

    /// Every avatar this viewer currently knows in-world, with the anchor
    /// entity whose transform places it — full objects first, then the
    /// coarse-only dots. The avatar picker's Near Me tab reads this.
    pub(crate) fn known_agents(&self) -> Vec<(AgentKey, Entity)> {
        let mut agents: Vec<(AgentKey, Entity)> = self
            .objects
            .iter()
            .map(|(agent, entities)| (*agent, entities.anchor))
            .collect();
        for (agent, entities) in &self.coarse {
            if !self.objects.contains_key(agent) {
                agents.push((*agent, entities.anchor));
            }
        }
        agents
    }

    /// Every nearby avatar as the map surfaces (minimap, radar) consume it:
    /// full-object avatars first (precise positions from their anchor
    /// transforms), then the coarse-only dots, deduplicated by agent — the
    /// reference's `LLWorld::getAvatars` merge. A coarse-only entry carries its
    /// last coarse altitude so the consumer can detect the "altitude unknown"
    /// sentinel ([`crate::minimap_math::coarse_altitude_unknown`]).
    pub(crate) fn map_avatars(&self) -> Vec<MapAvatar> {
        let mut avatars: Vec<MapAvatar> = self
            .objects
            .iter()
            .map(|(agent, entities)| MapAvatar {
                agent: *agent,
                anchor: entities.anchor,
                coarse_z: None,
            })
            .collect();
        for (agent, entities) in &self.coarse {
            if !self.objects.contains_key(agent) {
                avatars.push(MapAvatar {
                    agent: *agent,
                    anchor: entities.anchor,
                    coarse_z: Some(f32::from(
                        self.coarse_pos.get(agent).map_or(0, |&(_, _, z)| z),
                    )),
                });
            }
        }
        avatars
    }

    /// The anchor entity of an agent's in-world presence (a full object
    /// preferred over a coarse dot), if any.
    pub(crate) fn root_entity_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects
            .get(&agent)
            .or_else(|| self.coarse.get(&agent))
            .map(|entities| entities.anchor)
    }

    /// Spawn the floating world-space name-tag billboard for `agent`, anchored
    /// to `anchor`, floating `tag_height` metres above it, and pulled toward
    /// the camera by `pull_radius` metres so the avatar's own body cannot
    /// occlude it ([`crate::name_tag_billboard`]).
    fn spawn_label(
        &self,
        agent: AgentKey,
        anchor: Entity,
        tag_height: f32,
        pull_radius: f32,
        commands: &mut Commands,
    ) -> Entity {
        commands
            .spawn((
                name_tag_render_bundle(pull_radius),
                // The initial content: the resolved (or provisional) name as a
                // plain white line; the content composer refines it.
                TagContent::plain_name(self.label_text(agent)),
                NameTag { anchor, tag_height },
                // The name tag is a valid avatar pick target (a right-click on it
                // opens the avatar menu, matching the reference) — resolved by the
                // `NameTagHitTest` rect test, since no picking backend covers the
                // custom tag meshes.
                AvatarPickTarget { agent },
            ))
            .id()
    }

    /// Spawn a placeholder sphere and its floating name tag for `agent` at
    /// `translation`, returning both entities.
    fn spawn_sphere(
        &mut self,
        agent: AgentKey,
        translation: Vec3,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<FaceMaterial>,
    ) -> AvatarEntities {
        let (mesh, material) = Self::asset_handles(&mut self.assets, meshes, materials);
        let sphere = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(translation),
                AvatarSphere,
                AvatarAnchor,
                // Avatars are dynamic content for reflection probes: this whole
                // subtree (here just the placeholder sphere) rides the dynamic
                // probe layer, so local probes capture it only when the setting
                // includes dynamic content (the object entity's own `Propagate`
                // does not reach here — the avatar body is a separate root).
                Propagate(dynamic_render_layers()),
                // The whole sphere *is* the avatar here, so a ray that hits it
                // resolves straight to this agent.
                AvatarPickTarget { agent },
            ))
            .id();
        let label = self.spawn_label(
            agent,
            sphere,
            AVATAR_SPHERE_RADIUS + NAME_TAG_GAP,
            AVATAR_SPHERE_RADIUS,
            commands,
        );
        AvatarEntities {
            anchor: sphere,
            label,
        }
    }

    /// Spawn a rigged base body for `agent` from the shared [`AvatarBody`]
    /// assets: a fresh joint-entity skeleton instance under a body-root anchor,
    /// with each base part skinned or pinned to it, plus the floating name tag.
    ///
    /// Returns the pair of avatar entities, the fresh joint-entity list (in joint
    /// order), and the attachment-point node entities (keyed by raw point id),
    /// which the caller records so a worn attachment can be parented to the right
    /// joint at its stored offset (P16.1/P16.2).
    fn spawn_body(
        &self,
        agent: AgentKey,
        object: &Object,
        body: &AvatarBody,
        region_offset: Vec3,
        commands: &mut Commands,
    ) -> (AvatarEntities, Vec<Entity>, HashMap<u8, Entity>) {
        let root_drop = self
            .root_drops
            .get(&agent)
            .copied()
            .unwrap_or(body.rest_root_drop);
        let root = commands
            .spawn((
                body_root_transform(object, root_drop, region_offset),
                Visibility::default(),
                AvatarAnchor,
                // Avatars are dynamic content for reflection probes: propagate the
                // dynamic probe layer to the whole skeleton / body-part / worn-
                // attachment subtree hanging off this body root (a separate root,
                // so the object entity's own `Propagate` does not reach it). A worn
                // attachment carries its own `Propagate` and so overrides this with
                // the static-geometry layer — acceptable, and only visible when the
                // dynamic-content setting is off.
                Propagate(dynamic_render_layers()),
            ))
            .id();
        // A fresh joint entity per skeleton joint, parented in a second pass once
        // all entities exist (a parent always precedes its children, but building
        // first keeps the parenting simple). Each carries an [`AvatarJoint`]
        // marker so the appearance system can re-deform it (P13.4).
        let joints: Vec<Entity> = body
            .joint_locals
            .iter()
            .enumerate()
            .map(|(index, local)| {
                commands
                    .spawn((*local, Visibility::default(), AvatarJoint { agent, index }))
                    .id()
            })
            .collect();
        for (entity, parent) in joints.iter().zip(body.joint_parents.iter().copied()) {
            let target = parent
                .and_then(|index| joints.get(index).copied())
                .unwrap_or(root);
            commands.entity(*entity).insert(ChildOf(target));
        }
        for (index, part) in body.parts.iter().enumerate() {
            spawn_body_part(part, index, agent, &joints, root, &body.material, commands);
        }
        // One attachment-point node per point, parented to its joint at the fixed
        // `avatar_lad.xml` offset (P16.2). A worn attachment then parents to the
        // node for its point and carries only its own local transform, matching
        // the reference viewer's joint → attachment-point → object chain.
        let attachment_nodes: HashMap<u8, Entity> = body
            .attachment_points
            .iter()
            .filter_map(|(&point_id, point)| {
                let joint = joints.get(point.joint_index).copied()?;
                let node = commands
                    .spawn((
                        point.offset,
                        Visibility::default(),
                        AttachmentPointNode,
                        ChildOf(joint),
                    ))
                    .id();
                Some((point_id, node))
            })
            .collect();
        // (The old invisible pick-collider box is gone: the GPU ID-buffer
        // pick — `crate::gpu_pick` — picks the drawn, GPU-posed pixels
        // directly, so no rigid stand-in volume is needed.)
        // A rigged body is roughly half a metre across at head height — the
        // camera pull that keeps the avatar's own head from occluding its tag.
        let label = self.spawn_label(agent, root, BODY_TAG_HEIGHT, 0.5, commands);
        (
            AvatarEntities {
                anchor: root,
                label,
            },
            joints,
            attachment_nodes,
        )
    }

    /// Seed the name cache and title map from an avatar object's NameValue
    /// pairs (`FirstName` / `LastName` / `Title` — the classic mechanism; the
    /// simulator sends them with every avatar `ObjectUpdate`, so the legacy
    /// name and group title arrive *with the object*, zero round trips).
    /// Never clobbers a legacy name another source already resolved, and only
    /// touches the title when a `Title` pair is actually present (a present
    /// but empty title means "title taken off").
    fn seed_from_name_values(&mut self, agent: AgentKey, object: &Object) {
        self.seed_name_fields(
            agent,
            object.name_value_data("FirstName"),
            object.name_value_data("LastName"),
            object.name_value_data("Title"),
        );
    }

    /// The merge rules of [`Self::seed_from_name_values`], on the extracted
    /// NameValue fields (split out so they are unit-testable without
    /// constructing a full [`Object`]).
    fn seed_name_fields(
        &mut self,
        agent: AgentKey,
        first: Option<String>,
        last: Option<String>,
        title: Option<String>,
    ) {
        if let Some(first) = first {
            let legacy = match last {
                Some(last) if !last.is_empty() && !last.eq_ignore_ascii_case("Resident") => {
                    format!("{first} {last}")
                }
                _ => first,
            };
            if !legacy.is_empty() {
                let record = self.names.entry(agent).or_default();
                if record.legacy.is_none() {
                    record.legacy = Some(legacy);
                }
            }
        }
        if let Some(title) = title {
            // The reference strips control characters from titles.
            let cleaned: String = title.chars().filter(|c| !c.is_control()).collect();
            if cleaned.is_empty() {
                self.titles.remove(&agent);
            } else {
                self.titles.insert(agent, cleaned);
            }
        }
    }

    /// Fold one display-name record from the `GetDisplayNames` cap (or a
    /// pushed `DisplayNameUpdate`) into the cache. A `missing` placeholder
    /// (the grid could not resolve the id) changes nothing — the legacy
    /// fallback stays. (The tag refreshes via the content composer.)
    fn set_display_name(&mut self, resolved: &DisplayName) {
        if !self.merge_display_name_record(resolved) {
            return;
        }
        debug!(
            "resolved display name {} = {:?} (@{})",
            resolved.id, resolved.display_name, resolved.username
        );
    }

    /// Fold one non-`missing` display-name record into the name cache;
    /// returns whether anything was (potentially) updated. Split from
    /// [`Self::set_display_name`] so the merge rules are unit-testable
    /// without an ECS world.
    fn merge_display_name_record(&mut self, resolved: &DisplayName) -> bool {
        if resolved.missing {
            return false;
        }
        let record = self.names.entry(resolved.id).or_default();
        record.legacy = Some(resolved.legacy_name());
        record.username = Some(resolved.username.clone());
        record.display_name = Some(resolved.display_name.clone());
        record.is_display_name_default = resolved.is_display_name_default;
        true
    }

    /// Queue a name request for `agent` once — a no-op if it is already in
    /// flight or answered. The actual wire traffic goes out batched, once per
    /// frame, in [`flush_name_requests`]. `pub(crate)` for the build
    /// floater's General tab, which resolves a selected object's creator /
    /// owner through the same cache.
    pub(crate) fn request_name(&mut self, agent: AgentKey) {
        if !self.requested.insert(agent) {
            return;
        }
        self.pending_name_requests.insert(agent);
    }

    /// Spawn or move a full-object avatar (`pcode` 47): its rigged base body when
    /// the [`AvatarBody`] assets are loaded, else the placeholder sphere.
    ///
    /// A full object supersedes any coarse placeholder for the same agent (the
    /// object position is precise), so an existing coarse sphere is despawned.
    fn apply_object(
        &mut self,
        object: &Object,
        body: Option<&AvatarBody>,
        own: Option<AgentKey>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<FaceMaterial>,
    ) {
        let agent = AgentKey::from(object.full_id.uuid());
        let scoped = object.scoped_id();
        // The **own** avatar is authoritative in the root region (the scene
        // origin) only. A non-root circuit that streams it — most notably the
        // destination during a deferred-teardown teleport handshake, which begins
        // streaming our full object as soon as we send `CompleteAgentMovement`,
        // *before* the commit shifts the origin — reports it in that region's
        // frame. Applying that update would offset the body by a whole inter-
        // region delta (`region_offset` below) and the camera, following it, would
        // leave the visible world: the "own avatar vanishes on a slow / failed
        // teleport, and snaps back on the next move" bug. Ignore an own-avatar
        // update whose region is not the current origin; the root region's stream
        // (and, on commit, the shifted origin) place it correctly.
        if own == Some(agent)
            && self
                .origin
                .is_some_and(|origin| origin != object.region_handle)
        {
            return;
        }
        // A neighbour region's avatar is offset onto the right terrain, exactly
        // like the coarse dots and world objects (zero for the root region).
        let region_offset = region_offset_bevy(object.region_handle, self.origin);
        self.seed_from_name_values(agent, object);
        // The authoritative motion the P31.4 dead-reckoner (`drive_avatar_motion`)
        // extrapolates between updates; re-inserted on every update so its change
        // detection reseeds the prediction. A rigged body root carries the object
        // rotation, a placeholder sphere does not.
        let avatar_motion = AvatarMotion::from_object(object, body.is_some());
        // Seated on an object? A non-zero `ParentID` means this avatar's wire
        // position / rotation are **relative to the seat**, not region-local, and
        // it rides the seat. Record the seat + seat-relative pose so
        // [`place_seated_avatars`] drives the anchor from the seat's live world
        // transform; a `ParentID` of zero (a stand) clears it, restoring region
        // placement. Self and others alike — several avatars can share one seat.
        let seated = object.parent_id.get() != 0;
        if seated {
            self.seated.insert(
                agent,
                SeatedTarget {
                    seat: object.scoped_parent_id(),
                    offset: seated_offset(object),
                },
            );
        } else {
            self.seated.remove(&agent);
        }
        // A precise full object takes over from any coarse dot for this agent.
        if let Some(entities) = self.coarse.remove(&agent) {
            despawn_avatar(entities, commands);
        }
        self.coarse_region.remove(&agent);
        if let Some(existing) = self.objects.get(&agent) {
            let mut anchor = commands.entity(existing.anchor);
            anchor.insert(avatar_motion);
            if seated {
                // The seat owns the anchor's world pose; just tag it so
                // `place_seated_avatars` drives it and the dead-reckoner leaves it
                // be. The transform is written each frame from the seat, so none is
                // set here.
                anchor.insert(Seated);
            } else {
                // Move the existing anchor: a body root gets the full position +
                // orientation transform, a sphere just its translation. Standing up
                // drops the seated tag so the dead-reckoner resumes.
                let transform = match body {
                    Some(body) => {
                        let root_drop = self
                            .root_drops
                            .get(&agent)
                            .copied()
                            .unwrap_or(body.rest_root_drop);
                        body_root_transform(object, root_drop, region_offset)
                    }
                    None => Transform::from_translation(sphere_translation(object, region_offset)),
                };
                // `set_if_neq` via `entry`, NOT a plain `insert`: the simulator
                // streams updates for the own avatar continuously even when it
                // stands still, and re-inserting an identical Transform marks it
                // changed — which re-propagated the whole avatar subtree and
                // woke the pose gate every frame (the "anchor wake, delta 0"
                // signature in the gate meter).
                anchor
                    .entry::<Transform>()
                    .and_modify(move |mut existing| {
                        existing.set_if_neq(transform);
                    })
                    .or_insert(transform);
                anchor.remove::<Seated>();
            }
            return;
        }
        self.request_name(agent);
        let entities = match body {
            Some(body) => {
                let (entities, joints, attachment_nodes) =
                    self.spawn_body(agent, object, body, region_offset, commands);
                // Record the joint entities and per-point attachment nodes so a
                // worn attachment can be parented at the right joint offset once it
                // arrives (P16.1/P16.2).
                self.joints.insert(agent, joints);
                self.attachment_nodes.insert(agent, attachment_nodes);
                entities
            }
            None => self.spawn_sphere(
                agent,
                sphere_translation(object, region_offset),
                commands,
                meshes,
                materials,
            ),
        };
        commands.entity(entities.anchor).insert(avatar_motion);
        if seated {
            // A freshly-streamed avatar that is already seated: tag it so
            // `place_seated_avatars` drives its world pose from the seat.
            commands.entity(entities.anchor).insert(Seated);
        }
        // Seed the seat drop with the rest pelvis height for a rigged body, so a
        // seated body lands hips-on-target before its own appearance resolves (a
        // sphere has no skeleton, so no drop — its centre sits on the target).
        if let Some(body) = body {
            self.seat_drops.entry(agent).or_insert(body.rest_seat_drop);
        }
        self.by_scoped.insert(scoped, agent);
        self.objects.insert(agent, entities);
        debug!(
            "spawned avatar for {agent} ({} tracked)",
            self.objects.len()
        );
    }

    /// Despawn the placeholder of the full-object avatar that left the scene under
    /// `scoped`, if one is tracked.
    fn remove_object(&mut self, scoped: ScopedObjectId, commands: &mut Commands) {
        let Some(agent) = self.by_scoped.remove(&scoped) else {
            return;
        };
        if let Some(entities) = self.objects.remove(&agent) {
            despawn_avatar(entities, commands);
        }
        // The body's joint entities and attachment-point nodes are despawned with
        // its anchor; drop the stores so a later attachment can no longer resolve
        // them (P16.1/P16.2). The recorded joint overrides go too, so a re-spawn
        // rebuilds them from the meshes that re-bind (R1).
        let _dropped = self.joints.remove(&agent);
        let _dropped_nodes = self.attachment_nodes.remove(&agent);
        let _dropped_deform = self.deformations.remove(&agent);
        let _dropped_volumes = self.volume_deformations.remove(&agent);
        let _dropped_physics = self.body_physics.remove(&agent);
        let _dropped_seat = self.seated.remove(&agent);
        let _dropped_seat_drop = self.seat_drops.remove(&agent);
        self.clear_joint_overrides(agent);
    }

    /// Whether `agent`'s avatar is currently seated on an object — its latest
    /// full-object update carried a non-zero `ParentID`. The camera reads this to
    /// take a seated own avatar's world pose from its (seat-driven) global
    /// transform rather than its region-space motion.
    pub(crate) fn is_seated(&self, agent: AgentKey) -> bool {
        self.seated.contains_key(&agent)
    }

    /// Unseat any avatars seated on the object `seat` that was just removed
    /// (`ObjectRemoved`) — drop their seated state and the [`Seated`] tag so the
    /// dead-reckoner resumes owning their anchor. Their anchor stays at its last
    /// seat-driven world pose until the simulator's own stand / motion update lands.
    ///
    /// The simulator normally unseats a rider before (or as) it kills the seat, so
    /// the avatar's own `ObjectUpdate` (with `ParentID` zero) already cleared the
    /// seat; this covers the seat vanishing — deleted, or culled from the interest
    /// list — *without* or *before* that update, so an avatar is never left frozen,
    /// invisibly parented, to a seat that no longer exists. Returns the agents it
    /// unseated (empty when the removed object was not anyone's seat).
    fn unseat_from_seat(&mut self, seat: ScopedObjectId, commands: &mut Commands) {
        let riders: Vec<AgentKey> = self
            .seated
            .iter()
            .filter(|(_agent, target)| target.seat == seat)
            .map(|(agent, _target)| *agent)
            .collect();
        for agent in riders {
            let _unseated = self.seated.remove(&agent);
            if let Some(entities) = self.objects.get(&agent) {
                commands.entity(entities.anchor).remove::<Seated>();
            }
        }
    }

    /// Each seated avatar's `(anchor entity, seat scoped id, seat-relative pose,
    /// seat drop)`, for [`place_seated_avatars`] to drive the anchor from its seat's
    /// live world transform. The seat drop is the pelvis's height above the body
    /// root (zero for a sphere), applied so the hips land on the sit target. Skips
    /// any avatar whose anchor is not (yet) a tracked full object.
    fn seated_placements(
        &self,
    ) -> impl Iterator<Item = (Entity, ScopedObjectId, Transform, f32)> + '_ {
        self.seated.iter().filter_map(|(agent, target)| {
            let anchor = self.objects.get(agent)?.anchor;
            let seat_drop = self.seat_drops.get(agent).copied().unwrap_or(0.0);
            Some((anchor, target.seat, target.offset, seat_drop))
        })
    }

    /// The agent whose avatar is tracked under the scoped object id `avatar_scoped`
    /// — the wearer of an attachment whose parent is that object. `None` if no
    /// avatar object with that scoped id is tracked (yet).
    ///
    /// The HUD routing (P35.1) needs it to tell the agent's **own** HUD attachments
    /// (which go to the screen-space HUD layer) from another avatar's (which are
    /// hidden: the reference viewer gives a non-self avatar no HUD joints at all).
    pub(crate) fn agent_of(&self, avatar_scoped: ScopedObjectId) -> Option<AgentKey> {
        self.by_scoped.get(&avatar_scoped).copied()
    }

    /// The attachment-point node entity a worn attachment parents to (P16.2): the
    /// node for raw attachment-point `point_id` on the rigged body of the avatar
    /// tracked under `avatar_scoped`, carrying the fixed `avatar_lad.xml` offset
    /// from its skeleton joint. `None` if that avatar is not a tracked full-object
    /// rigged body yet, or the point has no body joint (a HUD point) — in which
    /// case the caller holds the attachment pending and retries.
    pub(crate) fn attachment_point_entity(
        &self,
        avatar_scoped: ScopedObjectId,
        point_id: u8,
    ) -> Option<Entity> {
        let agent = self.by_scoped.get(&avatar_scoped)?;
        self.attachment_nodes.get(agent)?.get(&point_id).copied()
    }

    /// The rigged-body root (anchor) entity of `agent`'s avatar (P17.2): the entity
    /// a worn rigged mesh's skinned submeshes are parented to so they despawn with
    /// the avatar and inherit its visibility. `None` if that avatar is not a tracked
    /// full-object avatar yet.
    /// The name-tag (label) entity of `agent`, if it is currently rendered.
    pub(crate) fn label_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects
            .get(&agent)
            .or_else(|| self.coarse.get(&agent))
            .map(|entities| entities.label)
    }

    /// The rigged-body root (anchor) entity of `agent`'s full-object avatar,
    /// if one is rendered.
    pub(crate) fn body_root_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects.get(&agent).map(|entities| entities.anchor)
    }

    /// The skeleton-instance joint entities (in joint order) of `agent`'s avatar
    /// (P17.2): the entities a worn rigged mesh's `SkinnedMesh` binds to, indexed by
    /// skeleton joint index. `None` if that avatar has no rigged body (a sphere-only,
    /// no-`--viewer-assets` avatar, or simply not spawned yet).
    pub(crate) fn joint_entities_of(&self, agent: AgentKey) -> Option<&Vec<Entity>> {
        self.joints.get(&agent)
    }

    /// The attachment-point node entities of `agent`'s rigged avatar (P16.2),
    /// in no particular order — the GPU-avatar real path's socket scan walks
    /// them to find which attachment-point joints carry a worn subtree and
    /// must therefore stay CPU-written while the skinning joints are frozen.
    /// Empty for an avatar with no rigged body.
    pub(crate) fn attachment_node_entities(
        &self,
        agent: AgentKey,
    ) -> impl Iterator<Item = Entity> + '_ {
        self.attachment_nodes
            .get(&agent)
            .into_iter()
            .flat_map(|nodes| nodes.values().copied())
    }

    /// The resolved skeletal deformations the animation driver (P18.3) folds a
    /// playing motion into when recomputing each joint's world matrix, as last
    /// shaped by [`apply_avatar_appearance`]. `None` for an avatar with no rigged
    /// body, or before its first appearance.
    pub(crate) fn deformations(&self, agent: AgentKey) -> Option<&SkeletalDeformations> {
        self.deformations.get(&agent)
    }

    /// The resolved collision-volume displacements (P34.3) the animation driver
    /// folds into the same recurrence, as last shaped by
    /// [`apply_avatar_appearance`]. An avatar whose shape displaces no volume has
    /// no entry, which is the same as the (empty) default.
    pub(crate) fn volume_deformations(&self, agent: AgentKey) -> Option<&VolumeDeformations> {
        self.volume_deformations.get(&agent)
    }

    /// Every avatar with a spawned rigged-body skeleton instance (P18.3): the
    /// driver writes each one's joint world matrices every frame — its animated
    /// pose or its plain deformed rest — so an avatar returns to rest when its
    /// animations stop and overlapping animations compose without a per-animation
    /// reset (Bevy's dirty-bit propagation cannot un-freeze a joint whose global
    /// the driver overwrote).
    pub(crate) fn rigged_agents(&self) -> Vec<AgentKey> {
        self.joints.keys().copied().collect()
    }

    /// Note that `agent` wears the rigged mesh asset `mesh` (for the avatar-state
    /// dump). Idempotent; forgotten with the avatar on despawn.
    pub(crate) fn record_worn_rigged_mesh(&mut self, agent: AgentKey, mesh: Uuid) {
        let _new = self
            .worn_rigged_meshes
            .entry(agent)
            .or_default()
            .insert(mesh);
    }

    /// The anchor entity (rigged-body root, or placeholder sphere) of `agent`'s
    /// full-object avatar, if one is tracked — the world pose the replay test rig
    /// (an orbiting light, a reflection probe) centres itself on.
    pub(crate) fn anchor_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects.get(&agent).map(|entities| entities.anchor)
    }

    /// Record the joint position overrides that worn rigged `mesh` imposes on
    /// `agent`'s skeleton (R1), replacing any previous contribution from that mesh
    /// (a rebind is idempotent). Flags the avatar for a skeleton re-deform **only
    /// when the contribution actually changed**, so re-binding identical rig parts
    /// (a mesh body's many same-rigged pieces) does not thrash the appearance pass.
    pub(crate) fn record_joint_overrides(
        &mut self,
        agent: AgentKey,
        mesh: Uuid,
        overrides: JointOverrides,
    ) {
        let per_mesh = self.joint_overrides.entry(agent).or_default();
        if per_mesh.get(&mesh) == Some(&overrides) {
            return;
        }
        if overrides.is_empty() {
            // A mesh that used to override but no longer does: drop its entry so the
            // rebuilt effective set no longer carries it.
            if per_mesh.remove(&mesh).is_none() {
                return;
            }
        } else {
            let _prev = per_mesh.insert(mesh, overrides);
        }
        self.appearance_dirty.insert(agent);
    }

    /// The body-physics configuration ingested from `agent`'s latest appearance
    /// (P34.1), or `None` before one arrived. Every motion it holds is ready to
    /// simulate: a motion whose `Max_Effect` is zero is present but
    /// [inactive](sl_client_bevy::PhysicsSettings::is_active).
    pub(crate) fn body_physics(&self, agent: AgentKey) -> Option<&BodyPhysics> {
        self.body_physics.get(&agent)
    }

    /// The current pose-inputs generation (see the field doc): the pose gate
    /// stores it per avatar and re-evaluates when it moved.
    pub(crate) const fn pose_inputs_generation(&self) -> u64 {
        self.pose_inputs_generation
    }

    /// Record that an input the skeleton pose fold consumes (deformations, volume
    /// deformations, joint overrides, body physics) was (re)applied. Over-bumping
    /// is harmless — an extra bump costs one frame of full re-evaluation.
    pub(crate) const fn bump_pose_inputs(&mut self) {
        self.pose_inputs_generation = self.pose_inputs_generation.wrapping_add(1);
    }

    /// The effective joint position overrides for `agent` (R1): the per-joint winner
    /// across every worn rigged mesh, resolved to the **highest mesh id** on a
    /// conflict (the reference viewer's `findActiveOverride`) with the scale lock
    /// sticky. `None` when the avatar wears no position-carrying rig.
    pub(crate) fn effective_joint_overrides(&self, agent: AgentKey) -> Option<JointOverrides> {
        let per_mesh = self.joint_overrides.get(&agent)?;
        if per_mesh.is_empty() {
            return None;
        }
        // Merge in ascending mesh-id order so the highest mesh id wins each joint.
        let mut meshes: Vec<(&Uuid, &JointOverrides)> = per_mesh.iter().collect();
        meshes.sort_by_key(|(mesh, _)| **mesh);
        let mut effective = JointOverrides::default();
        for (_mesh, overrides) in meshes {
            effective.merge(overrides);
        }
        Some(effective)
    }

    /// Forget every joint position override recorded for `agent` (R1) — e.g. when
    /// the avatar despawns, so a re-spawn rebuilds them from scratch.
    pub(crate) fn clear_joint_overrides(&mut self, agent: AgentKey) {
        let _prev = self.joint_overrides.remove(&agent);
        let _worn = self.worn_rigged_meshes.remove(&agent);
        self.bump_pose_inputs();
    }

    /// The agent whose avatar a worn object `scoped` hangs off — chasing parent
    /// links up to the tracked avatar root, so a rigged mesh that is a *child link*
    /// of a multi-prim attachment linkset (a mesh body, whose parts parent to the
    /// linkset root prim, not the avatar) still resolves to its wearer (P17.2).
    /// `None` if the chain does not reach an avatar.
    pub(crate) fn wearer_of(&self, scoped: ScopedObjectId) -> Option<AgentKey> {
        self.avatar_root_of(scoped)
    }

    /// Reconcile the coarse-only avatar placeholders with one region's
    /// `CoarseLocationUpdate`: spawn/move a sphere for every coarse avatar that is
    /// not already a full object (and is not the agent's own `you` entry), and
    /// despawn any coarse placeholder **from this region** that has dropped out of
    /// its list.
    ///
    /// `region` is the region these locations belong to and `origin` the scene
    /// origin (the agent's own region); a neighbour region's coarse `x`/`y` are
    /// relative to *its* south-west corner, so its dots are offset by
    /// `region − origin` (mirroring the terrain placement) to land on the right
    /// neighbour terrain (R24). The reconcile is scoped to `region`, so a
    /// neighbour's update never despawns another region's dots — and an empty
    /// update for a region (emitted when it is disabled) drops exactly its dots.
    #[expect(
        clippy::too_many_arguments,
        reason = "reconciling one region's coarse dots needs the region + scene \
                  origin (to offset), the update's locations + you index, and the \
                  Commands / mesh / material sinks to spawn spheres"
    )]
    fn apply_coarse(
        &mut self,
        region: RegionHandle,
        origin: Option<RegionHandle>,
        own: Option<AgentKey>,
        locations: &[CoarseLocation],
        you: Option<usize>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<FaceMaterial>,
    ) {
        // The neighbour region's south-west corner relative to the scene origin, in
        // Second Life east/north metres (0 for the root region itself).
        let (region_x, region_y) = region.global_coordinates();
        let (origin_x, origin_y) = origin.unwrap_or(region).global_coordinates();
        let offset_east = metres_to_f32(region_x) - metres_to_f32(origin_x);
        let offset_north = metres_to_f32(region_y) - metres_to_f32(origin_y);
        let mut present: HashSet<AgentKey> = HashSet::new();
        for (index, location) in locations.iter().enumerate() {
            let agent = location.agent_id;
            // The agent's own coarse dot is left to the (precise) self-marker
            // path. Skip it by the update's `you` index *and* by the agent id: a
            // region we became a **child** of after a crossing (the region we left)
            // still lists us in its coarse update — sometimes without setting
            // `you` — and without the id check that stale entry would spawn a ghost
            // self-dot back in the old region (viewer-crossing-stale-minimap-self-dot).
            if Some(index) == you || own == Some(agent) {
                continue;
            }
            // A full-object avatar renders from its precise object position.
            if self.objects.contains_key(&agent) {
                continue;
            }
            present.insert(agent);
            self.coarse_pos
                .insert(agent, (location.x, location.y, location.z));
            let translation = coarse_translation(location, offset_east, offset_north);
            if let Some(existing) = self.coarse.get(&agent) {
                commands
                    .entity(existing.anchor)
                    .insert(Transform::from_translation(translation));
            } else {
                self.request_name(agent);
                let entities = self.spawn_sphere(agent, translation, commands, meshes, materials);
                self.coarse.insert(agent, entities);
            }
            self.coarse_region.insert(agent, region);
        }
        // Despawn coarse placeholders from THIS region that dropped out of its
        // list; leave other regions' dots untouched.
        let stale: Vec<AgentKey> = self
            .coarse
            .keys()
            .copied()
            .filter(|agent| {
                self.coarse_region.get(agent) == Some(&region) && !present.contains(agent)
            })
            .collect();
        for agent in stale {
            if let Some(entities) = self.coarse.remove(&agent) {
                despawn_avatar(entities, commands);
            }
            self.coarse_region.remove(&agent);
            self.coarse_pos.remove(&agent);
        }
    }

    /// Despawn every **other** avatar (full objects and coarse dots) and forget
    /// their per-agent state — the scene-mirror purge a **distant** teleport
    /// needs, since the session cleared its object cache with no per-object
    /// `KillObject` to drive the incremental removal path
    /// ([`Event::RegionChanged`](sl_client_bevy::SlSessionEvent)'s `world_reset`).
    ///
    /// The agent's **own** avatar (`own`) is kept — its body, skeleton, appearance
    /// and worn state all cross with the agent on a teleport, so despawning it
    /// would flash the self view and force an appearance / bake refetch. Its
    /// visible body simply re-anchors when the destination re-streams its
    /// (agent-keyed) full object. The scoped-id-keyed bookkeeping is dropped
    /// wholesale (the source region's local-id space is gone) and rebuilt as the
    /// destination streams. Also drops the origin anchor so [`recenter_avatars`]
    /// re-anchors on the destination without a spurious re-base shift.
    pub(crate) fn purge(&mut self, own: Option<AgentKey>, commands: &mut Commands) {
        let keep = |agent: &AgentKey| own == Some(*agent);
        // Despawn every non-own avatar's entities (full objects + coarse dots).
        let others: Vec<AgentKey> = self
            .objects
            .keys()
            .chain(self.coarse.keys())
            .copied()
            .filter(|agent| !keep(agent))
            .collect();
        for agent in others {
            if let Some(entities) = self.objects.remove(&agent) {
                despawn_avatar(entities, commands);
            }
            if let Some(entities) = self.coarse.remove(&agent) {
                despawn_avatar(entities, commands);
            }
        }
        // Retain only the own agent on the per-agent bookkeeping.
        self.coarse_region.retain(|agent, _| keep(agent));
        self.coarse_pos.retain(|agent, _| keep(agent));
        self.joints.retain(|agent, _| keep(agent));
        self.attachment_nodes.retain(|agent, _| keep(agent));
        self.names.retain(|agent, _| keep(agent));
        self.titles.retain(|agent, _| keep(agent));
        self.requested.retain(keep);
        self.pending_name_requests.retain(keep);
        self.appearances.retain(|agent, _| keep(agent));
        self.appearance_dirty.retain(keep);
        self.appearance_pending.retain(|agent, _| keep(agent));
        self.joint_overrides.retain(|agent, _| keep(agent));
        self.worn_rigged_meshes.retain(|agent, _| keep(agent));
        self.skirt_visible.retain(|agent, _| keep(agent));
        self.body_physics.retain(|agent, _| keep(agent));
        self.baked_textures.retain(|agent, _| keep(agent));
        self.invisible_regions.retain(|agent, _| keep(agent));
        self.baked_cof_version.retain(|agent, _| keep(agent));
        self.bake_dirty.retain(keep);
        self.deformations.retain(|agent, _| keep(agent));
        self.volume_deformations.retain(|agent, _| keep(agent));
        self.root_drops.retain(|agent, _| keep(agent));
        self.seat_drops.retain(|agent, _| keep(agent));
        self.ever_full_object.retain(keep);
        self.seated.retain(|agent, _| keep(agent));
        // The source region's local-id space is gone; drop every scoped-id-keyed
        // entry (own included — its ids are reassigned when the destination
        // re-streams it). `by_scoped` is repopulated by `apply_object`, the parent
        // / hide maps by `track_object`.
        self.by_scoped.clear();
        self.object_parents.clear();
        self.baked_hides.clear();
        self.scanned_objects.clear();
        self.origin = None;
    }

    /// Re-issue the baked-texture fetches for `agent` — the avatar pies' manual
    /// **Tex Refresh**. Each recorded bake slot is re-requested through the
    /// [`TextureManager`], which clears any retry-exhausted state and spawns a
    /// fresh fetch ([`request_from`](TextureManager::request_from) drops the id
    /// from its `retry` map and starts a new task), so an avatar left grey by a
    /// transient bake-service failure gets another set of tries without waiting
    /// for a fresh `AvatarAppearance`. The recorded COF version is also forgotten
    /// so the next appearance resend re-fetches too. A no-op for an agent with no
    /// bakes recorded yet. Slots are re-requested by the same rule the initial
    /// ingest uses ([`bake_service_slot_name`]): the server bake service when the
    /// grid central-bakes and the slot has a service name, else a by-UUID fetch.
    pub(crate) fn refetch_bakes(
        &mut self,
        agent: AgentKey,
        manager: &mut TextureManager,
        appearance_service: Option<&url::Url>,
    ) {
        let Some(bakes) = self.baked_textures.get(&agent) else {
            debug!("tex refresh: no baked textures recorded for {agent} yet");
            return;
        };
        let count = bakes.len();
        // Snapshot the ids: `forget` + the re-request borrow `manager` mutably, so
        // the immutable borrow of `self.baked_textures` must end first.
        let slots: Vec<(usize, TextureKey)> = bakes.iter().map(|(&slot, &id)| (slot, id)).collect();
        for (slot, id) in slots {
            // Evict first so a cached (or retry-exhausted) bake actually re-fetches
            // rather than the request short-circuiting on the cache — a true refresh.
            manager.forget(id);
            match appearance_service.zip(bake_service_slot_name(slot)) {
                Some((service, name)) => {
                    let url = format!("{service}texture/{}/{name}/{id}", agent.uuid());
                    manager.request_server_bake(id, url);
                }
                None => {
                    manager.request_boosted(id, crate::render_priority::AVATAR_BOOST_PRIORITY);
                }
            }
        }
        let _forgotten = self.baked_cof_version.remove(&agent);
        info!("tex refresh: re-requested {count} baked texture(s) for {agent}");
    }

    /// Record a resolved legacy name. (The tag itself refreshes via the
    /// content composer, which recomposes whenever this state changes.)
    fn set_name(&mut self, name: &AvatarName) {
        let agent = name.id;
        let resolved = name.legacy_name();
        self.names.entry(agent).or_default().legacy = Some(resolved.clone());
        debug!("resolved avatar name {agent} = {resolved:?}");
    }

    /// Record the parenting of an in-world object and, once, scan its texture
    /// entry for the `IMG_USE_BAKED_*` sentinels a worn attachment uses to hide a
    /// base-avatar region. Called for every object; a *root* object (no parent)
    /// can never be an attachment, so it is ignored.
    fn track_object(&mut self, object: &Object) {
        if object.parent_id.get() == 0 {
            return;
        }
        let scoped = object.scoped_id();
        self.object_parents
            .insert(scoped, object.scoped_parent_id());
        // Decode + scan a given object's texture entry only once (attachments do
        // not change their baked-body sentinels under normal wear).
        if self.scanned_objects.insert(scoped) {
            let slots = used_baked_slots(&object.texture_entry);
            if !slots.is_empty() {
                self.baked_hides.insert(scoped, slots);
            }
        }
    }

    /// Forget a departed object's attachment bookkeeping.
    fn forget_object(&mut self, scoped: ScopedObjectId) {
        self.object_parents.remove(&scoped);
        self.baked_hides.remove(&scoped);
        self.scanned_objects.remove(&scoped);
    }

    /// The agent whose avatar `scoped` hangs off, by chasing parent links up to a
    /// tracked avatar root; `None` if the chain does not reach an avatar (an
    /// ordinary in-world linkset) or is malformed.
    fn avatar_root_of(&self, scoped: ScopedObjectId) -> Option<AgentKey> {
        let mut current = scoped;
        for _ in 0..MAX_ATTACHMENT_DEPTH {
            if let Some(&agent) = self.by_scoped.get(&current) {
                return Some(agent);
            }
            match self.object_parents.get(&current) {
                Some(&parent) => current = parent,
                None => return None,
            }
        }
        None
    }

    /// Diagnostic form of [`avatar_root_of`]: `Ok(agent)` when the parent chain
    /// reaches a recognised avatar, else `Err((terminus, hops))` — the object the
    /// walk stopped at (a root with no recorded parent, or the last hop when the
    /// depth cap is hit) and how many hops it took. Lets a stuck rigged
    /// attachment's `wearer not resolved` failure be classified against the object
    /// state: a *tracked in-world* terminus means it is genuinely not worn (an
    /// in-world rigged mesh), while an *untracked* terminus means the wearer /
    /// linkset-root object never arrived (a parenting / ordering gap).
    pub(crate) fn avatar_root_walk(
        &self,
        scoped: ScopedObjectId,
    ) -> Result<AgentKey, (ScopedObjectId, usize)> {
        let mut current = scoped;
        for hops in 0..MAX_ATTACHMENT_DEPTH {
            if let Some(&agent) = self.by_scoped.get(&current) {
                return Ok(agent);
            }
            match self.object_parents.get(&current) {
                Some(&parent) => current = parent,
                None => return Err((current, hops)),
            }
        }
        Err((current, MAX_ATTACHMENT_DEPTH))
    }

    /// The set of baked slots to hide for each avatar: every tracked attachment
    /// whose texture entry carries `IMG_USE_BAKED_*` sentinels is attributed to
    /// its avatar (by chasing its chain), and its replaced slots unioned in.
    fn hidden_slots_per_agent(&self) -> HashMap<AgentKey, HashSet<usize>> {
        let mut hidden: HashMap<AgentKey, HashSet<usize>> = HashMap::new();
        for (&scoped, slots) in &self.baked_hides {
            if let Some(agent) = self.avatar_root_of(scoped) {
                hidden
                    .entry(agent)
                    .or_default()
                    .extend(slots.iter().copied());
            }
        }
        hidden
    }
}

/// The base-body baked-texture slots draped over the **system** body (P14): the
/// six region bakes — head, upper body, lower body, eyes, hair, and skirt — each
/// with a matching base-mesh region part.
const BODY_BAKE_SLOTS: [usize; 6] = [
    avatar_texture::HEAD_BAKED,
    avatar_texture::UPPER_BAKED,
    avatar_texture::LOWER_BAKED,
    avatar_texture::EYES_BAKED,
    avatar_texture::HAIR_BAKED,
    avatar_texture::SKIRT_BAKED,
];

/// The **universal** baked-texture slots a modern mesh body samples via
/// bake-on-mesh for its arms / legs / detached parts (R22). The system base mesh
/// has no matching region — these bakes are fetched only so a worn mesh body's BoM
/// faces on those slots ([`apply_bom_face_materials`]) can show the real baked skin
/// instead of the flat skin placeholder; they are never draped on a system part.
const UNIVERSAL_BAKE_SLOTS: [usize; 5] = [
    avatar_texture::LEFT_ARM_BAKED,
    avatar_texture::LEFT_LEG_BAKED,
    avatar_texture::AUX1_BAKED,
    avatar_texture::AUX2_BAKED,
    avatar_texture::AUX3_BAKED,
];

/// The appearance-service URL path name for a baked slot — the reference viewer's
/// per-slot `mDefaultImageName`, the `<slot>` segment of a server bake's URL
/// (`<service>texture/<avatar>/<slot>/<uuid>`). `None` for a slot with no service
/// name (the "universal" bakes, which the base body does not fetch).
pub(crate) const fn bake_service_slot_name(slot: usize) -> Option<&'static str> {
    match slot {
        avatar_texture::HEAD_BAKED => Some("head"),
        avatar_texture::UPPER_BAKED => Some("upper"),
        avatar_texture::LOWER_BAKED => Some("lower"),
        avatar_texture::EYES_BAKED => Some("eyes"),
        avatar_texture::HAIR_BAKED => Some("hair"),
        avatar_texture::SKIRT_BAKED => Some("skirt"),
        // The "universal" bakes a modern mesh body samples via bake-on-mesh for its
        // arms / legs / detached parts (R22), fetched from the appearance service by
        // the same `<slot>` URL names the reference viewer uses (`llavatarappearance
        // defines.cpp` `BakedEntry`).
        avatar_texture::LEFT_ARM_BAKED => Some("leftarm"),
        avatar_texture::LEFT_LEG_BAKED => Some("leftleg"),
        avatar_texture::AUX1_BAKED => Some("aux1"),
        avatar_texture::AUX2_BAKED => Some("aux2"),
        avatar_texture::AUX3_BAKED => Some("aux3"),
        _other => None,
    }
}

/// The base-body region slots whose baked texture is the `IMG_INVISIBLE` sentinel
/// (R22) — a worn system alpha layer carved the region away. The reference viewer's
/// `isTextureVisible` treats these as not visible and hides the region; only the
/// system-body [`BODY_BAKE_SLOTS`] are checked (a universal slot has no base part).
fn invisible_body_slots(texture_entry: &TextureEntry) -> HashSet<usize> {
    BODY_BAKE_SLOTS
        .into_iter()
        .filter(|&slot| {
            texture_entry
                .texture_id(slot)
                .is_some_and(|id| id.uuid() == avatar_texture::IMG_INVISIBLE)
        })
        .collect()
}

/// The visible baked texture id in each baked slot of an avatar's texture entry —
/// every [`BODY_BAKE_SLOTS`] (system-body region) and [`UNIVERSAL_BAKE_SLOTS`]
/// (mesh-body bake-on-mesh) slot whose id names a real, renderable bake
/// ([`is_bake_visible`](avatar_texture::is_bake_visible)), keyed by baked slot. A
/// slot that is empty, defaulted, or invisible is omitted, so a region with no
/// published bake has nothing to fetch. The universal slots have no system-body
/// part, so they are draped only onto a worn mesh body's BoM faces (R22).
fn visible_body_bakes(texture_entry: &TextureEntry) -> HashMap<usize, TextureKey> {
    let mut bakes = HashMap::new();
    for slot in BODY_BAKE_SLOTS.into_iter().chain(UNIVERSAL_BAKE_SLOTS) {
        if let Some(id) = texture_entry.texture_id(slot)
            && avatar_texture::is_bake_visible(id)
        {
            let _replaced = bakes.insert(slot, id);
        }
    }
    bakes
}

/// Scan a raw texture-entry blob for the `IMG_USE_BAKED_*` sentinels and return
/// the (sorted, de-duplicated) baked slots it signals should be replaced — empty
/// for an ordinary object.
fn used_baked_slots(texture_entry: &[u8]) -> Vec<usize> {
    let entry = decode_texture_entry(texture_entry, MAX_FACES);
    let mut slots: Vec<usize> = entry
        .faces
        .iter()
        .filter_map(|face| avatar_texture::use_baked_slot(face.texture_id))
        .collect();
    slots.sort_unstable();
    slots.dedup();
    slots
}

/// Despawn both entities of an avatar (its anchor — sphere or body root, whose
/// sub-hierarchy goes with it — and its name tag).
fn despawn_avatar(entities: AvatarEntities, commands: &mut Commands) {
    commands.entity(entities.anchor).try_despawn();
    commands.entity(entities.label).try_despawn();
}

/// Keep the scene origin on the root region for **avatars**: when the root region
/// changes — a border crossing, or a teleport to an already-connected region —
/// shift every (non-seated) avatar anchor, and its dead-reckoning interpolation,
/// by the same `-shift` the camera, terrain and objects re-base by, and record
/// the new origin so a freshly-streamed avatar is placed against it
/// ([`AvatarState::apply_object`]).
///
/// Like [`recenter_objects`](crate::objects::recenter_objects) this is
/// belt-and-braces with the per-update placement: a moving avatar re-snaps itself
/// on its next update (with the new offset), so the shift here keeps a *stationary*
/// neighbour avatar — one receiving no update across the handover — in place. A
/// **seated** avatar is skipped: its anchor is driven from its seat object (which
/// [`recenter_objects`](crate::objects::recenter_objects) already re-based), so it
/// follows for free — shifting it here too would double-move it.
///
/// Runs before [`update_avatar_objects`] and the dead-reckoner
/// ([`drive_avatar_motion`](crate::physics::drive_avatar_motion)) so a same-frame
/// update / interpolation step sees the re-based pose.
#[expect(
    clippy::type_complexity,
    reason = "the anchor query pairs each avatar's transform with its optional \
              dead-reckoning interp, filtered to non-seated anchors"
)]
pub(crate) fn recenter_avatars(
    identity: Res<SlIdentity>,
    mut state: ResMut<AvatarState>,
    mut anchors: Query<
        (&mut Transform, Option<&mut AvatarInterp>),
        (With<AvatarAnchor>, Without<Seated>),
    >,
) {
    let Some(root) = identity.region_handle else {
        return;
    };
    match state.origin {
        // Unchanged origin: nothing to re-base.
        Some(current) if current == root => {}
        Some(previous) => {
            let shift = origin_shift_bevy(previous, root);
            for (mut transform, interp) in &mut anchors {
                // Per-component (not the glam vector operator) for the
                // `arithmetic_side_effects` lint, matching `recenter_terrain`.
                transform.translation.x -= shift.x;
                transform.translation.y -= shift.y;
                transform.translation.z -= shift.z;
                if let Some(mut interp) = interp {
                    interp.rebase(Vec3::new(-shift.x, -shift.y, -shift.z));
                }
            }
            state.origin = Some(root);
        }
        // First region learned (login): anchor the origin without shifting.
        None => state.origin = Some(root),
    }
}

/// Spawn / move / despawn the placeholder of every avatar the simulator streams
/// as a full in-world object (`pcode` 47), requesting each avatar's legacy name
/// once.
pub(crate) fn update_avatar_objects(
    mut events: MessageReader<SlEvent>,
    identity: Res<SlIdentity>,
    mut state: ResMut<AvatarState>,
    body: Option<Res<AvatarBody>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let body = body.as_deref();
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                // Track every object's attachment linkage (an avatar's worn mesh
                // hides base-body regions via `IMG_USE_BAKED_*` faces), then render
                // the avatars themselves.
                state.track_object(object);
                if object.pcode == pcode::AVATAR {
                    // R22b diagnostic: record that the simulator streamed a full
                    // object for this agent, and log its arrival, so a live census
                    // can tell "never streamed" apart from "streamed but unrendered".
                    let agent = AgentKey::from(object.full_id.uuid());
                    if log_avatar_interest() {
                        let first = state.ever_full_object.insert(agent);
                        info!(
                            "R22b full avatar object {}agent={agent} region={:?} pos={:?}",
                            if first { "(first) " } else { "" },
                            object.region_handle,
                            object.motion.position,
                        );
                    } else {
                        state.ever_full_object.insert(agent);
                    }
                    state.apply_object(
                        object,
                        body,
                        identity.agent_id,
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                    );
                }
            }
            SlSessionEvent::ObjectRemoved { local_id, .. } => {
                state.forget_object(*local_id);
                // A removed object might be someone's seat: unseat its riders so
                // they are not left frozen to a seat that no longer exists.
                state.unseat_from_seat(*local_id, &mut commands);
                state.remove_object(*local_id, &mut commands);
            }
            _other => {}
        }
    }
}

/// The seat-chain query [`place_seated_avatars`] walks to compose a seat's
/// current-frame world transform: every non-anchor entity's local [`Transform`]
/// and optional [`ChildOf`] parent. Aliased so the system signature and the
/// [`seat_world_transform`] helper stay readable and clear of
/// `clippy::type_complexity`. `Without<AvatarAnchor>` keeps it disjoint from the
/// mutable anchor query (a seat object is never an avatar anchor), and
/// `Without<ViewerCamera>` from the camera's mutable transform query — the same
/// helper composes the scripted sit camera's seat pose in
/// [`sit_camera_pose`](crate::camera), whose system mutably borrows the camera
/// transform (a seat object is never the camera either).
pub(crate) type SeatChainQuery<'world, 'state> = Query<
    'world,
    'state,
    (&'static Transform, Option<&'static ChildOf>),
    (Without<AvatarAnchor>, Without<crate::camera::ViewerCamera>),
>;

/// Drive every **seated** avatar's world pose from its seat each frame — self and
/// others alike, so a boat full of avatars rides together.
///
/// A seated avatar's wire transform is relative to its seat, not the region, so
/// its anchor is left out of the region-space dead-reckoner ([`Seated`], skipped by
/// [`drive_avatar_motion`](crate::physics::drive_avatar_motion)) and placed here
/// instead: compose the avatar's seat-relative pose ([`SeatedTarget::offset`]) onto
/// the seat object's live world transform, exactly as a linkset child prim at the
/// same offset composes. The seat is resolved from the object scene
/// ([`ObjectState`]) — the seat may stream in after, or independently of, the
/// avatar, so a not-yet-tracked seat simply defers a frame. Mirrors the reference's
/// `LLVOAvatar::sitOnObject` parenting (the avatar root rides the seat's transform,
/// with no pelvis / root correction while seated).
///
/// The seat's world transform is composed **here** from the chain of local
/// [`Transform`]s up its `ChildOf` parents ([`seat_world_transform`]), each of
/// which is this frame's value once its mover has run — deliberately *not* the
/// seat's [`GlobalTransform`], which Bevy only recomputes in `PostUpdate` and so
/// is a frame stale. That one-frame lag is exactly the visible rubber-band on a
/// moving vehicle (`viewer-seated-avatar-vehicle-rubberband`): the seat mesh
/// renders at this frame's pose while a rider read from the stale
/// `GlobalTransform` trails at last frame's, lurching on each of the vehicle's
/// dead-reckon / snap corrections. Composing from the current-frame locals locks
/// the rider to the seat rigidly, with zero lag.
///
/// Ordered after the movers that write the seat's local transform
/// ([`update_objects`](crate::objects::update_objects) for the authoritative snap,
/// [`drive_physical_objects`](crate::physics::drive_physical_objects) for the
/// between-update dead-reckon) and after
/// [`drive_avatar_motion`](crate::physics::drive_avatar_motion) (whose write it
/// overrides for a seated anchor), and before the camera follow — so both the
/// seat's locals and the seated own avatar's world pose are current when read.
pub(crate) fn place_seated_avatars(
    state: Res<AvatarState>,
    objects: Res<ObjectState>,
    // The seat and its linkset ancestors, read to compose the seat's
    // current-frame world pose from local transforms ([`seat_world_transform`]).
    chain: SeatChainQuery,
    // Narrowed to avatar anchors (not every entity with a `Transform`) so this
    // mutable query conflicts with as few other systems as the scheduler allows.
    mut anchors: Query<&mut Transform, With<AvatarAnchor>>,
) {
    for (anchor, seat_scoped, offset, seat_drop) in state.seated_placements() {
        let Some(seat_entity) = objects.entity_by_scoped(&seat_scoped) else {
            continue;
        };
        let Some(seat_world) = seat_world_transform(seat_entity, &chain) else {
            continue;
        };
        // Drop the anchor by the pelvis height so the hips land on the sit target.
        let seated = drop_to_hips(offset, seat_drop);
        let world = seat_world.mul_transform(seated);
        if let Ok(mut transform) = anchors.get_mut(anchor)
            && *transform != world
        {
            *transform = world;
        }
    }
}

/// Compose an object entity's **current-frame** world [`Transform`] from the chain
/// of local transforms up its `ChildOf` parents — the manual equivalent of the
/// `GlobalTransform` Bevy propagates in `PostUpdate`, but read from this frame's
/// local values so a seat driven this frame is not a frame stale (the point of
/// [`place_seated_avatars`]). SL linksets are flat (a root plus its children), so
/// the chain is at most two deep; the walk is general regardless. Returns `None`
/// if any entity in the chain has lost its [`Transform`] (left the scene
/// mid-frame). Object entities carry no scale (it rides their geometry holder, a
/// separate child), so the composed world transform matches what propagation would
/// produce.
pub(crate) fn seat_world_transform(seat: Entity, chain: &SeatChainQuery) -> Option<Transform> {
    // Collect the chain leaf-first, then compose root-first so each parent's
    // transform pre-multiplies its child's (`world = root · … · seat`).
    let mut locals: Vec<Transform> = Vec::new();
    let mut current = seat;
    loop {
        let (transform, parent) = chain.get(current).ok()?;
        locals.push(*transform);
        match parent {
            Some(child_of) => current = child_of.parent(),
            None => break,
        }
    }
    let mut world = Transform::IDENTITY;
    for local in locals.iter().rev() {
        world = world.mul_transform(*local);
    }
    Some(world)
}

/// Render a placeholder for every coarse-only avatar, keeping the set current with
/// each `CoarseLocationUpdate`.
///
/// Runs after [`update_avatar_objects`] so the full-object set it dedupes against
/// is current within the frame.
pub(crate) fn update_coarse_avatars(
    mut events: MessageReader<SlEvent>,
    identity: Res<SlIdentity>,
    mut state: ResMut<AvatarState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let origin = identity.region_handle;
    let own = identity.agent_id;
    for event in events.read() {
        if let SlSessionEvent::CoarseLocationUpdate {
            locations,
            you,
            region_handle,
            ..
        } = &event.0
        {
            state.apply_coarse(
                *region_handle,
                origin,
                own,
                locations,
                *you,
                &mut commands,
                &mut meshes,
                &mut materials,
            );
        }
    }
}

/// R22b diagnostic: on a 5 s cadence (when `SL_VIEWER_LOG_AVATAR_INTEREST=1`), log a
/// census of the coarse-only "blue sphere" avatars that have not resolved to a full
/// object — each flagged with whether the simulator *ever* streamed a full object for
/// it and its coarse `z` (a `z` at the 1020 m ceiling is the "off this region"
/// sentinel). Read against the per-arrival `R22b full avatar object` lines, this
/// pinpoints whether an unresolved sphere is a "never streamed" (interest-list /
/// cross-region) case or a "streamed but unrendered" (viewer) case. A no-op unless the
/// env flag is set.
pub(crate) fn log_avatar_interest_census(
    time: Res<Time>,
    state: Res<AvatarState>,
    mut next_at: Local<f32>,
) {
    if !log_avatar_interest() {
        return;
    }
    let now = time.elapsed_secs();
    if now < *next_at {
        return;
    }
    *next_at = now + 5.0;
    info!(
        "R22b census: {} full-object avatars, {} coarse-only spheres",
        state.objects.len(),
        state.coarse.len()
    );
    for agent in state.coarse.keys() {
        let name = state
            .names
            .get(agent)
            .and_then(NameRecord::preferred_name)
            .unwrap_or("<unresolved>");
        let ever_object = state.ever_full_object.contains(agent);
        let pos = state.coarse_pos.get(agent);
        info!(
            "  sphere agent={agent} name={name:?} ever_full_object={ever_object} coarse_pos={pos:?}"
        );
    }
}

/// Fold resolved legacy and display names into the name cache (the tags
/// themselves refresh via the content composer, which watches this state).
pub(crate) fn apply_avatar_names(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarState>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::AvatarNames(names) => {
                for name in names {
                    state.set_name(name);
                }
            }
            SlSessionEvent::DisplayNames(names) => {
                for name in names {
                    state.set_display_name(name);
                }
            }
            SlSessionEvent::DisplayNameUpdate(update) => {
                state.set_display_name(&update.name);
            }
            _ => {}
        }
    }
}

/// Send this frame's queued name requests as **one** batched legacy
/// `UUIDNameRequest` and **one** batched `GetDisplayNames` cap call (the cap
/// spawns an HTTP request per command, so batching matters; on grids without
/// the cap — OpenSim — the cap command is a silent no-op and the legacy reply
/// carries the day).
pub(crate) fn flush_name_requests(
    mut state: ResMut<AvatarState>,
    mut commands: MessageWriter<SlCommand>,
) {
    if state.pending_name_requests.is_empty() {
        return;
    }
    let batch: Vec<AgentKey> = state.pending_name_requests.drain().collect();
    commands.write(SlCommand(Command::RequestAvatarNames(batch.clone())));
    commands.write(SlCommand(Command::RequestDisplayNames(batch)));
}

/// A request to re-fetch an avatar's baked textures — the avatar pies' manual
/// **Tex Refresh** action, for kicking another set of tries at a bake left grey
/// by a transient bake-service failure.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct RefetchAvatarTextures {
    /// The avatar whose bakes to re-request.
    pub(crate) agent: AgentKey,
}

/// Handle a [`RefetchAvatarTextures`] request by re-issuing the agent's baked-
/// texture fetches ([`AvatarState::refetch_bakes`]), using the same server-bake /
/// by-UUID rule the initial ingest does.
pub(crate) fn handle_refetch_avatar_textures(
    mut requests: MessageReader<RefetchAvatarTextures>,
    mut state: ResMut<AvatarState>,
    mut manager: ResMut<TextureManager>,
    identity: Res<SlIdentity>,
) {
    let service = identity.agent_appearance_service.clone();
    for request in requests.read() {
        state.refetch_bakes(request.agent, &mut manager, service.as_ref());
    }
}

/// Ingest each avatar's server-published baked textures (P14.1): on an
/// `AvatarAppearance`, read the baked-slot UUIDs from its texture entry
/// ([`visible_body_bakes`]), fetch each visible bake through the shared
/// [`TextureManager`] (the Phase-6 fetch / off-thread-decode / disk-cache
/// pipeline — deduped, so a bake shared by many avatars is fetched once), and
/// record them per avatar for the region materials (P14.2) to drape over the
/// system body.
///
/// These baked UUIDs are the composited avatar textures other clients render: on
/// Second Life they come from the server "Sunshine" bake, on OpenSim from other
/// avatars' viewers' client-side bakes — either way they are published ids the
/// viewer simply fetches. A slot with no real bake (empty / default / invisible)
/// is skipped, so a region with no published texture keeps its flat skin tint.
pub(crate) fn ingest_avatar_bakes(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarState>,
    mut manager: ResMut<TextureManager>,
    identity: Res<SlIdentity>,
) {
    // The server-bake ("Sunshine") appearance service, if the grid central-bakes.
    // Present -> baked textures are fetched from it (`FTT_SERVER_BAKE`); absent
    // (OpenSim) -> the published baked ids are ordinary assets fetched by UUID.
    let appearance_service = identity.agent_appearance_service.clone();
    for event in events.read() {
        if let SlSessionEvent::AvatarAppearance(appearance) = &event.0 {
            // Skip an out-of-order / duplicate resend so a stale appearance cannot
            // clobber a newer bake (P14.4); a newer or equal COF version, or one
            // with no COF version at all, is (re)fetched.
            let seen = state.baked_cof_version.get(&appearance.avatar_id).copied();
            if !should_refetch_bakes(seen, appearance.cof_version) {
                continue;
            }
            let bakes = visible_body_bakes(&appearance.texture_entry);
            // The base regions this avatar has baked **invisible** (`IMG_INVISIBLE`)
            // — a worn system alpha layer that carves the system body away so a
            // (non-BOM) mesh body shows through cleanly. The reference viewer's
            // `isTextureVisible` returns false for these, hiding the region; we do
            // the same in `apply_avatar_part_visibility` (R22). Without it the
            // untextured system body renders and z-fights the mesh body (blotches).
            state.invisible_regions.insert(
                appearance.avatar_id,
                invisible_body_slots(&appearance.texture_entry),
            );
            for (&slot, &id) in &bakes {
                // On a central-baking grid a baked id is fetched from the appearance
                // service (`<svc>texture/<avatar>/<slot>/<uuid>`), not by UUID from
                // the CDN which rejects it. Fall back to a plain fetch when the grid
                // has no such service or the slot has no service name.
                let slot_name = bake_service_slot_name(slot).unwrap_or("?");
                match appearance_service
                    .as_ref()
                    .zip(bake_service_slot_name(slot))
                {
                    Some((service, name)) => {
                        let url = format!("{service}texture/{}/{name}/{id}", appearance.avatar_id);
                        // Per-slot request log (R22h): correlate a later
                        // `texture <id> fetch/decode failed` warning to the region it
                        // came from — the upper bake specifically fails to resolve on
                        // some avatars while head / lower succeed.
                        debug!("requesting server bake slot {slot} ({slot_name}) = {id}");
                        manager.request_server_bake(id, url);
                    }
                    None => {
                        debug!("requesting bake slot {slot} ({slot_name}) = {id} (by-UUID)");
                        manager.request_boosted(id, crate::render_priority::AVATAR_BOOST_PRIORITY);
                    }
                }
            }
            debug!(
                "requested {} baked texture(s) for {} (server-bake service: {})",
                bakes.len(),
                appearance.avatar_id,
                appearance_service.is_some()
            );
            if let Some(cof_version) = appearance.cof_version {
                state
                    .baked_cof_version
                    .insert(appearance.avatar_id, cof_version);
            }
            state.baked_textures.insert(appearance.avatar_id, bakes);
            // Flag the avatar so its body-region materials are (re)assigned to the
            // new bakes (P14.2); the actual draping is deferred until the textures
            // decode.
            state.bake_dirty.insert(appearance.avatar_id);
        }
    }
}

/// Whether a newly arrived `AvatarAppearance` should have its baked textures
/// (re)fetched (P14.4), given the COF version whose bakes were last fetched for
/// that avatar (`seen`) and the new appearance's COF version (`cof`).
///
/// A later appearance whose COF version is *strictly older* than the one already
/// fetched is an out-of-order / duplicate resend and is skipped, so a stale
/// appearance cannot clobber a newer bake. An *equal* version is still ingested —
/// a same-outfit rebake (e.g. after a `RebakeAvatarTextures`) can republish new
/// baked ids at the same version — and an appearance with *no* COF version
/// (OpenSim / the older path, where there is nothing to compare) always ingests.
const fn should_refetch_bakes(seen: Option<i32>, cof: Option<i32>) -> bool {
    match (seen, cof) {
        (Some(seen), Some(cof)) => cof >= seen,
        _ => true,
    }
}

/// The per-region baked-texture materials draped over the system body (P14.2):
/// one [`StandardMaterial`] per `(avatar, baked slot)`, plus the uploaded baked
/// images (deduped across avatars) and the materials parked on a bake that has
/// not decoded yet.
#[derive(Resource, Default)]
pub(crate) struct AvatarBakeMaterials {
    /// Uploaded baked Bevy images by texture id, so a bake shared by several
    /// avatars (or regions) is turned into a Bevy [`Image`] once.
    images: HashMap<TextureKey, Handle<Image>>,
    /// The material draped on each avatar body region, keyed by
    /// `(avatar, baked slot)`; its `base_color_texture` is filled once the bake
    /// decodes.
    materials: HashMap<(AgentKey, usize), Handle<FaceMaterial>>,
    /// Region materials parked on a not-yet-decoded baked texture id, filled by
    /// [`apply_avatar_bake_textures`] once it decodes.
    pending: HashMap<TextureKey, Vec<Handle<FaceMaterial>>>,
    /// The composited-alpha classification of each decoded baked texture (P14.3),
    /// computed once per id: whether it is opaque, alpha-masked, or wholly carved
    /// away (a worn mesh body's alpha layer). Drives each region material's
    /// [`AlphaMode`] and, when [`Transparent`](BakeAlpha::Transparent), hides the
    /// base region outright ([`apply_avatar_part_visibility`]).
    alpha: HashMap<TextureKey, BakeAlpha>,
    /// The [`uv_grid_image`] handle, built once on first use of the
    /// [`debug_avatar_grid`] diagnostic mode.
    debug_grid: Option<Handle<Image>>,
}

impl AvatarBakeMaterials {
    /// The diagnostic UV-grid image handle ([`uv_grid_image`]), built and uploaded
    /// once on first use (the [`debug_avatar_grid`] mode).
    fn debug_grid(&mut self, images: &mut Assets<Image>) -> Handle<Image> {
        self.debug_grid
            .get_or_insert_with(|| images.add(uv_grid_image()))
            .clone()
    }

    /// The uploaded Bevy [`Image`] for a baked texture `id` together with its
    /// composited-alpha classification (P14.3), uploading and classifying it from
    /// the manager's decoded pixels on first use (both cached), or `None` if the
    /// bake is not decoded yet (still in flight or the fetch failed).
    fn ensure_bake(
        &mut self,
        id: TextureKey,
        manager: &TextureManager,
        images: &mut Assets<Image>,
    ) -> Option<(Handle<Image>, BakeAlpha)> {
        if let Some(handle) = self.images.get(&id) {
            let alpha = self.alpha.get(&id).copied().unwrap_or(BakeAlpha::Opaque);
            return Some((handle.clone(), alpha));
        }
        let decoded = manager.decoded(id)?;
        let alpha = classify_bake_alpha(decoded);
        if log_avatar_faces_enabled() {
            info!(
                "bake {id}: {}x{} {}c discard={:?} -> {alpha:?}",
                decoded.width, decoded.height, decoded.components, decoded.discard_level
            );
        }
        let handle = images.add(to_bevy_image(decoded));
        let _inserted = self.images.insert(id, handle.clone());
        let _classified = self.alpha.insert(id, alpha);
        Some((handle, alpha))
    }

    /// Whether the decoded bake `id` is wholly transparent — an alpha wearable
    /// carved the entire region away (typically a worn mesh body) — so the base
    /// region mesh it drapes should be hidden (P14.3). `false` for a bake that is
    /// opaque, partly masked, or not yet decoded.
    fn region_transparent(&self, id: TextureKey) -> bool {
        self.alpha
            .get(&id)
            .is_some_and(|alpha| alpha.hides_region())
    }

    /// The material for one avatar body region, keyed by `(agent, slot)`: reused
    /// across the region's parts (and re-pointed on a fresh appearance), with its
    /// baked texture filled immediately when already decoded, else parked on the
    /// bake id so [`apply_avatar_bake_textures`] fills it when it decodes.
    fn region_material(
        &mut self,
        agent: AgentKey,
        slot: usize,
        id: TextureKey,
        manager: &TextureManager,
        images: &mut Assets<Image>,
        materials: &mut Assets<FaceMaterial>,
    ) -> Handle<FaceMaterial> {
        let handle = self
            .materials
            .entry((agent, slot))
            .or_insert_with(|| materials.add(baked_region_material()))
            .clone();
        match self.ensure_bake(id, manager, images) {
            Some((image, alpha)) => {
                if let Some(mut material) = materials.get_mut(&handle) {
                    apply_bake_image(&mut material.base, image, alpha.alpha_mode());
                }
            }
            None => self.pending.entry(id).or_default().push(handle.clone()),
        }
        handle
    }

    /// The material for one avatar body region draped with a **locally composited**
    /// client-side bake (P15.3) rather than a fetched server bake: reuse (or
    /// create) the `(agent, slot)` region material and set the already-uploaded
    /// composited `image` + its composited-alpha `alpha` mode directly, bypassing
    /// the fetched-UUID [`ensure_bake`](Self::ensure_bake) path. Shares the same
    /// per-region material slot as [`region_material`](Self::region_material), so a
    /// server bake arriving later cleanly replaces the local one.
    fn local_region_material(
        &mut self,
        agent: AgentKey,
        slot: usize,
        image: Handle<Image>,
        alpha: BakeAlpha,
        materials: &mut Assets<FaceMaterial>,
    ) -> Handle<FaceMaterial> {
        let handle = self
            .materials
            .entry((agent, slot))
            .or_insert_with(|| materials.add(baked_region_material()))
            .clone();
        if let Some(mut material) = materials.get_mut(&handle) {
            apply_bake_image(&mut material.base, image, alpha.alpha_mode());
        }
        handle
    }
}

/// The un-textured base material for a body region: the skin tint as a fallback
/// until the baked texture decodes and is draped over it (P14.2). Opaque until a
/// bake with alpha overrides it; once the bake fills `base_color_texture`,
/// [`apply_bake_image`] resets the tint to white and sets the region's
/// [`AlphaMode`] from the bake's composited alpha (P14.3).
fn baked_region_material() -> FaceMaterial {
    inert_face_material(StandardMaterial {
        base_color: BODY_COLOR,
        perceptual_roughness: 0.9,
        // Single-sided, matching the prim / base-body surfaces: Second Life
        // renders a face only from its front.
        ..default()
    })
}

/// The initial material for a bake-on-mesh face (R22): each BoM face owns its
/// material (rather than sharing the region's) so [`apply_bom_face_materials`] can
/// give it the reference viewer's per-face tint / blend / hide on the sampled
/// bake. Until the wearer's bake resolves it shows the neutral
/// [`BOM_FALLBACK_COLOR`] (matching `IMG_DEFAULT`), multiplied by the face `tint`
/// alpha and placed by its `uv` transform. A fully-transparent tint is hidden by
/// visibility, not material, so its base colour is left neutral.
pub(crate) fn bom_face_material(tint: [u8; 4], uv: Affine2) -> FaceMaterial {
    inert_face_material(StandardMaterial {
        base_color: BOM_FALLBACK_COLOR,
        perceptual_roughness: 0.9,
        uv_transform: uv,
        // A rigged face never alpha-masks (reference: `LLFace::canRenderAsMask`
        // returns false for rigged faces); a non-opaque tint blends, else opaque.
        alpha_mode: if tint[3] < 255 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        ..default()
    })
}

/// The [`AlphaMode`] for a mesh-body BoM face, from the face's TE tint alpha and
/// whether the wearer's region bake carries alpha (a worn alpha layer carved it
/// transparent — `BakeAlpha::Masked` / `Transparent`).
///
/// A translucent tint blends (the reference's `color_alpha` → alpha pool).
/// Otherwise a **carved** bake masks at the reference cutoff so the hidden region
/// (e.g. the feet under mesh boots) does not render, while bare skin — an
/// opaque bake — stays opaque so it never goes see-through and no UV-seam ring
/// appears on an un-alpha'd arm. That is the fix for the earlier blanket-opaque
/// behaviour: R22d broke because it masked an *un-carved* bake; gating on
/// `bake_has_alpha` masks only where a layer actually carved transparency.
const fn bom_face_alpha_mode(tint_alpha: u8, bake_has_alpha: bool) -> AlphaMode {
    if tint_alpha < 255 {
        AlphaMode::Blend
    } else if bake_has_alpha {
        AlphaMode::Mask(BAKE_ALPHA_MASK_THRESHOLD)
    } else {
        AlphaMode::Opaque
    }
}

/// Drape a decoded baked texture over a region material: set its diffuse image,
/// reset `base_color` to white so the composited bake (which already carries the
/// skin / clothing colour) is shown unmodified rather than tinted by the fallback
/// skin colour, and set its [`AlphaMode`] from the bake's composited alpha (P14.3)
/// so an alpha wearable carved into the bake turns that part of the region
/// invisible.
fn apply_bake_image(material: &mut StandardMaterial, image: Handle<Image>, alpha_mode: AlphaMode) {
    material.base_color = Color::WHITE;
    material.base_color_texture = Some(image);
    material.alpha_mode = alpha_mode;
}

/// The alpha threshold, as a fraction, below which a baked-texture fragment is
/// discarded — an alpha wearable carved it away. This is the reference viewer's
/// avatar alpha-mask cutoff `LLDrawPoolAvatar::sMinimumAlpha` (`0.2`), the
/// `minimum_alpha` uniform the rigged / avatar alpha-mask shader discards below;
/// a body bake's alpha *at or above* it renders fully opaque (which is why bare
/// mesh-body skin is not see-through — R22d). Matches [`BAKE_ALPHA_CUTOFF`].
const BAKE_ALPHA_MASK_THRESHOLD: f32 = 0.2;

/// The 8-bit alpha value below which a baked-texture pixel counts as carved away
/// when classifying a bake ([`classify_bake_alpha`]) — `0.2 * 255`, rounded, to
/// match the reference viewer's `sMinimumAlpha` and [`BAKE_ALPHA_MASK_THRESHOLD`].
const BAKE_ALPHA_CUTOFF: u8 = 51;

/// How a decoded baked texture's composited alpha channel renders its body
/// region (P14.3): the alpha wearables the grid composited into the bake carve
/// pixels away, and the region is drawn accordingly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BakeAlpha {
    /// Fully opaque — no alpha channel, or every pixel at or above the cutoff.
    /// Rendered as [`AlphaMode::Opaque`] (cheapest, and correct for plain skin).
    Opaque,
    /// A mix of kept and carved pixels — an alpha wearable cut part of the region
    /// away. Rendered as [`AlphaMode::Mask`] so the carved pixels vanish.
    Masked,
    /// Every pixel carved away — the whole region is invisible (typically a worn
    /// mesh body's alpha layer). The base region mesh is hidden outright.
    Transparent,
}

impl BakeAlpha {
    /// The Bevy [`AlphaMode`] to render a region bake with: opaque skin stays in
    /// the cheap opaque pass, anything carved uses masking (a wholly transparent
    /// region also masks, though it is normally hidden by
    /// [`hides_region`](Self::hides_region) before it draws).
    const fn alpha_mode(self) -> AlphaMode {
        match self {
            Self::Opaque => AlphaMode::Opaque,
            Self::Masked | Self::Transparent => AlphaMode::Mask(BAKE_ALPHA_MASK_THRESHOLD),
        }
    }

    /// Whether the region this bake drapes should be hidden entirely — true only
    /// when the whole bake is carved away ([`Transparent`](Self::Transparent)).
    const fn hides_region(self) -> bool {
        matches!(self, Self::Transparent)
    }
}

/// Classify a decoded baked texture's composited alpha (P14.3) from its source
/// component count and RGBA8 pixels: a source with no alpha channel
/// (`components < 4`) is always [`Opaque`](BakeAlpha::Opaque); otherwise the
/// alpha bytes are scanned once — all at or above the cutoff is `Opaque`, all
/// below is [`Transparent`](BakeAlpha::Transparent), and any mix is
/// [`Masked`](BakeAlpha::Masked).
const fn classify_bake_alpha(decoded: &DecodedTexture) -> BakeAlpha {
    // No alpha channel: the decoder filled alpha to fully opaque.
    if decoded.components < 4 {
        return BakeAlpha::Opaque;
    }
    // O(1) off the precomputed alpha range (the pixel scan happened once in
    // the decode / composite task, never on the frame thread). Checked
    // min-first so an empty image (range `(255, 0)`) classifies opaque.
    if decoded.min_alpha >= BAKE_ALPHA_CUTOFF {
        // Nothing carved (or no pixels at all) → opaque.
        BakeAlpha::Opaque
    } else if decoded.max_alpha < BAKE_ALPHA_CUTOFF {
        // Every pixel carved → wholly transparent.
        BakeAlpha::Transparent
    } else {
        // A mix of kept and carved pixels → masked.
        BakeAlpha::Masked
    }
}

/// Drape each avatar's server-published baked textures over its system body
/// (P14.2): give every base part a per-`(avatar, region)` material carrying that
/// region's baked texture (head → head bake, upper → upper-body bake, …), so the
/// avatar renders skin- and clothing-textured instead of flat skin tone. A region
/// with no published bake keeps the shared un-textured skin material.
///
/// Deferred and idempotent, mirroring [`apply_avatar_appearance`]: a fresh
/// appearance (or a body part that just spawned, matched by [`Added`]) flags the
/// avatar, and its region materials are (re)assigned from the tracked bakes — so a
/// bake ingested before the body still lands once the body exists. The baked
/// image itself is filled in when it decodes ([`apply_avatar_bake_textures`]). A
/// no-op when no avatar asset library / body loaded (avatars stay spheres).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system reading the tracked bakes and the ECS resources the region materials need"
)]
pub(crate) fn assign_avatar_bake_materials(
    mut state: ResMut<AvatarState>,
    body: Option<Res<AvatarBody>>,
    mut bake_mats: ResMut<AvatarBakeMaterials>,
    manager: Res<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    added: Query<&AvatarBodyPart, Added<AvatarBodyPart>>,
    mut parts: Query<(&AvatarBodyPart, &mut MeshMaterial3d<FaceMaterial>)>,
) {
    // A newly spawned part needs its region material assigned (the bakes can
    // arrive before the body object does).
    for part in &added {
        if state.baked_textures.contains_key(&part.agent) {
            state.bake_dirty.insert(part.agent);
        }
    }
    if state.bake_dirty.is_empty() {
        return;
    }
    let Some(body) = body else {
        state.bake_dirty.clear();
        return;
    };
    let mut draped = 0_usize;
    for (part, mut material) in &mut parts {
        if !state.bake_dirty.contains(&part.agent) {
            continue;
        }
        let slot = part.region.baked_slot();
        let desired = match state
            .baked_textures
            .get(&part.agent)
            .and_then(|bakes| bakes.get(&slot))
        {
            // A published bake for this region: its per-avatar region material.
            Some(&id) => bake_mats.region_material(
                part.agent,
                slot,
                id,
                &manager,
                &mut images,
                &mut materials,
            ),
            // No bake for this region: the shared un-textured skin material.
            None => body.material.clone(),
        };
        if material.0 != desired {
            *material = MeshMaterial3d(desired);
            draped = draped.saturating_add(1);
        }
    }
    if draped > 0 {
        debug!("assigned bake material to {draped} avatar body part(s)");
    }
    state.bake_dirty.clear();
}

/// Fill each newly decoded avatar bake into the region materials parked on it
/// (P14.2): upload (and cache) the baked [`Image`], then drop it into every parked
/// material's `base_color_texture`. Mirrors [`apply_prim_textures`](crate::textures::apply_prim_textures);
/// a decode that failed leaves the parked materials on their fallback skin tint.
pub(crate) fn apply_avatar_bake_textures(
    mut decoded: MessageReader<TextureDecoded>,
    manager: Res<TextureManager>,
    mut bake_mats: ResMut<AvatarBakeMaterials>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let mut filled = 0_usize;
    for &TextureDecoded(id) in decoded.read() {
        let Some(parked) = bake_mats.pending.remove(&id) else {
            // Not a bake any avatar region is waiting on (e.g. a prim texture).
            continue;
        };
        let Some((image, alpha)) = bake_mats.ensure_bake(id, &manager, &mut images) else {
            // The fetch failed: the parked regions keep their flat skin tint.
            continue;
        };
        for material_handle in parked {
            if let Some(mut material) = materials.get_mut(&material_handle) {
                apply_bake_image(&mut material.base, image.clone(), alpha.alpha_mode());
                filled = filled.saturating_add(1);
            }
        }
    }
    if filled > 0 {
        debug!("draped {filled} decoded bake(s) onto avatar body region material(s)");
    }
}

/// The side length, in pixels, of a locally composited client-side bake region
/// (P15.3). The reference viewer bakes body regions at 512×512; each source
/// wearable layer is bilinearly resampled to this by [`composite_region`].
const LOCAL_BAKE_SIZE: u32 = 512;

/// Our own avatar's **client-side** composited bake (P15.3): one uploaded
/// [`Image`] plus its composited-alpha classification per baked slot, built once
/// the client-side bake inputs ([`OwnBakeInputs`]) are assembled.
///
/// On a grid that publishes no server "Sunshine" bake for our own avatar
/// (OpenSim, and any grid without central baking) our own avatar would otherwise
/// stay an untextured cloud: the P14 [`ingest_avatar_bakes`] path finds no baked
/// UUIDs in our own appearance, so [`assign_avatar_bake_materials`] leaves our
/// body on the flat skin material. This resource instead holds the bake the
/// *viewer* composited from the worn wearable layers (P15.1/P15.2), which
/// [`apply_own_local_bake`] drapes over our own body regions — the client-bake
/// counterpart of the server bake other avatars (and our own on Second Life)
/// carry.
#[derive(Resource, Default)]
pub(crate) struct OwnLocalBake {
    /// The composited region image + its alpha classification, keyed by baked
    /// slot ([`BakeRegion::slot`]); a region with no worn layers is absent (so its
    /// body part keeps the flat skin material rather than a transparent bake).
    regions: HashMap<usize, (Handle<Image>, BakeAlpha)>,
    /// The [`OwnBakeInputs`] generation the composite was last built from, or `None`
    /// before the first build. When the inputs re-assemble (a runtime outfit change,
    /// or an appearance-editor live edit) their generation advances past this and
    /// [`apply_own_local_bake`] re-composites.
    built_generation: Option<u64>,
    /// The background composite task and the [`OwnBakeInputs`] generation it is
    /// compositing, while it runs — the composite is done off the frame thread
    /// (never blocked on) and its images installed on completion, a later frame.
    /// `None` when no composite is in flight; a newer generation supersedes an
    /// older in-flight one.
    task: Option<(u64, Task<CompositedRegions>)>,
}

/// The per-slot composited region images a background bake job produces: each
/// baked slot ([`BakeRegion::slot`]) with its not-yet-uploaded Bevy [`Image`] and
/// the composited-alpha classification for that region.
type CompositedRegions = Vec<(usize, Image, BakeAlpha)>;

impl OwnLocalBake {
    /// Force the client-side bake to re-composite on the next
    /// [`apply_own_local_bake`] — the appearance editor calls this after a live
    /// texture / tint edit changes the worn bake inputs.
    pub(crate) const fn invalidate(&mut self) {
        self.built_generation = None;
    }
}

/// Flip an RGBA8 image's rows in place — mirror it about its horizontal axis —
/// mapping between the top-down decoded-image row order and the OpenGL bottom-up
/// convention Second Life avatar UVs are authored in (P15.3). A zero dimension,
/// or a pixel buffer too short for `width`×`height` RGBA, is left untouched.
const fn flip_rows_vertically(pixels: &mut [u8], width: usize, height: usize) {
    let stride = width.saturating_mul(RGBA_CHANNELS);
    // Guard the swaps: every index touched must be within the buffer.
    if stride == 0 || height == 0 || pixels.len() < stride.saturating_mul(height) {
        return;
    }
    let mut row = 0_usize;
    while row < height / 2 {
        let opposite = height.saturating_sub(1).saturating_sub(row);
        let top = row.saturating_mul(stride);
        let bottom = opposite.saturating_mul(stride);
        let mut offset = 0_usize;
        while offset < stride {
            pixels.swap(top.saturating_add(offset), bottom.saturating_add(offset));
            offset = offset.saturating_add(1);
        }
        row = row.saturating_add(1);
    }
}

/// Force every pixel of an RGBA8 image fully opaque (alpha byte → 255), so a
/// bake draped on a solid surface (the eyeball) is not carved by stray
/// source-texture transparency (P15.3).
fn force_alpha_opaque(pixels: &mut [u8]) {
    let mut index = RGBA_CHANNELS.saturating_sub(1);
    while index < pixels.len() {
        // The alpha byte of each RGBA texel.
        if let Some(alpha) = pixels.get_mut(index) {
            *alpha = u8::MAX;
        }
        index = index.saturating_add(RGBA_CHANNELS);
    }
}

/// Composite one bake region of our own avatar from its ready client-side bake
/// inputs (P15.2) into the canonical baked RGBA image for that region, or `None`
/// when the region has no worn layers (an empty composite is wholly transparent
/// and would wrongly carve the region away).
///
/// The result is the orientation a Second Life baked texture is stored and
/// consumed in — the same bytes are both draped onto our own body (P15.3) and,
/// when published, J2C-encoded and uploaded (P15.4):
///
/// - **Vertical flip.** SL avatar `.llm` UVs are authored bottom-up (V = 0 at the
///   bottom), so the body samples a baked texture upside down relative to a
///   top-down decoded image. The compositor works top-down (like a fetched J2C),
///   which would land the head bake's chin/teeth on the forehead, so its rows are
///   flipped — matching how a server-published bake is stored (the reference
///   viewer bakes into a bottom-up GL surface), which is why the P14 fetched-bake
///   drape path renders straight without a flip.
/// - **Opaque eyes.** The eyeball is an opaque surface, but our simplified eye
///   composite carries only the iris layer (not the opaque sclera base the
///   reference eye layer-set builds), whose transparent surround would classify
///   the bake as masked and carve the eyeballs into empty sockets — so the eye
///   region is forced fully opaque.
pub(crate) fn composite_own_region(
    inputs: &OwnBakeInputs,
    region: BakeRegion,
) -> Option<DecodedTexture> {
    composite_region_from_layers(region, inputs.region_layers(region))
}

/// Composite one bake region from an owned layer list — the layer-list form of
/// [`composite_own_region`], borrowing no ECS state so it runs on the background
/// composite task ([`run_local_bake_job`]). `None` for an empty region (no worn
/// layers). See [`composite_own_region`] for the flip / opaque-eye rationale.
fn composite_region_from_layers(region: BakeRegion, layers: &[Layer]) -> Option<DecodedTexture> {
    if layers.is_empty() {
        return None;
    }
    let mut baked = composite_region(region, LOCAL_BAKE_SIZE, layers);
    let side = usize::try_from(LOCAL_BAKE_SIZE).unwrap_or(0);
    flip_rows_vertically(&mut baked.pixels, side, side);
    if region == BakeRegion::Eyes {
        force_alpha_opaque(&mut baked.pixels);
    }
    Some(baked.to_decoded_image())
}

/// Composite our own avatar's ready client-side bake inputs (P15.2) into one
/// image + alpha classification per baked slot: composite each bake region
/// ([`composite_region_from_layers`]), classify the composited alpha (so an alpha
/// wearable carved into the bake renders masked, P14.3), and build (not yet upload)
/// a Bevy [`Image`]. Runs on a background [`AsyncComputeTaskPool`] task off the
/// frame thread ([`apply_own_local_bake`] uploads the images on completion) — the
/// per-region composite (layer blend + resample + V-flip + alpha classification)
/// is the ~55 ms hitch this avoids stalling on. A region with no worn layers is
/// skipped.
fn run_local_bake_job(job: &LocalBakeJob) -> CompositedRegions {
    let mut regions: CompositedRegions = Vec::new();
    let mut summary: Vec<String> = Vec::new();
    for (region, layers) in &job.regions {
        let Some(decoded) = composite_region_from_layers(*region, layers) else {
            continue;
        };
        let alpha = classify_bake_alpha(&decoded);
        regions.push((region.slot(), to_bevy_image(&decoded), alpha));
        summary.push(format!(
            "{}={} layer(s)/{alpha:?}",
            region.name(),
            layers.len()
        ));
    }
    info!(
        "composited client-side bake for own avatar: {}",
        summary.join(" ")
    );
    regions
}

/// The owned inputs a background client-side bake composite needs: each non-empty
/// bake region's layer list, cloned out of [`OwnBakeInputs`] so the composite runs
/// on an [`AsyncComputeTaskPool`] task without borrowing ECS state.
struct LocalBakeJob {
    /// Each bake region paired with its (owned) composite layer list.
    regions: Vec<(BakeRegion, Vec<Layer>)>,
}

/// Drape our own avatar's locally composited client-side bake (P15.3) over its
/// body regions when the grid publishes no server bake for us (OpenSim).
///
/// Once the bake inputs are assembled ([`OwnBakeInputs::is_ready`]) the composite
/// is built on a background task ([`run_local_bake_job`]) and, for each of our own
/// body parts whose
/// region the grid did **not** bake for us, the composited region image is set as
/// that region's material — reusing the same per-`(agent, slot)` material slot the
/// P14 server-bake path uses, so a server bake (Second Life) cleanly wins over the
/// local one. Runs every frame but idempotent: it only re-assigns a body part
/// whose material actually differs, so it self-heals after
/// [`assign_avatar_bake_materials`] resets a part on a fresh appearance, and lands
/// on parts that spawn after the composite is ready.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system compositing our own bake and draping it over the body-region materials"
)]
pub(crate) fn apply_own_local_bake(
    identity: Res<SlIdentity>,
    inputs: Res<OwnBakeInputs>,
    state: Res<AvatarState>,
    body: Option<Res<AvatarBody>>,
    mut local: ResMut<OwnLocalBake>,
    mut bake_mats: ResMut<AvatarBakeMaterials>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut parts: Query<(&AvatarBodyPart, &mut MeshMaterial3d<FaceMaterial>)>,
) {
    // Nothing to drape until the body assets loaded, the bake inputs are ready,
    // and we know which agent is our own avatar.
    if body.is_none() || !inputs.is_ready() {
        return;
    }
    let Some(agent) = identity.agent_id else {
        return;
    };
    // Spawn a background composite when the current inputs generation is not built
    // and none is already compositing it (a fresh outfit → new generation
    // supersedes an in-flight older one). The composite runs off the frame thread;
    // a region with no worn layers (a server-bake grid, where the layers are gated
    // off, or an empty outfit) composites nothing, so skip the task and mark built.
    let generation = inputs.generation();
    if local.built_generation != Some(generation)
        && local
            .task
            .as_ref()
            .is_none_or(|(task_gen, _task)| *task_gen != generation)
    {
        let regions: Vec<(BakeRegion, Vec<Layer>)> = BakeRegion::ALL
            .into_iter()
            .filter_map(|region| {
                let layers = inputs.region_layers(region);
                (!layers.is_empty()).then(|| (region, layers.to_vec()))
            })
            .collect();
        if regions.is_empty() {
            local.regions.clear();
            local.built_generation = Some(generation);
        } else {
            let job = LocalBakeJob { regions };
            local.task = Some((
                generation,
                AsyncComputeTaskPool::get().spawn(async move { run_local_bake_job(&job) }),
            ));
        }
    }
    // Install a finished composite (never blocking on it — `poll_once` yields
    // `None` while it is still running, so its images land a later frame).
    let finished = local
        .task
        .as_mut()
        .and_then(|(task_gen, task)| block_on(poll_once(task)).map(|regions| (*task_gen, regions)));
    if let Some((task_generation, composited)) = finished {
        let mut regions = HashMap::new();
        for (slot, image, alpha) in composited {
            let handle = images.add(image);
            let _prev = regions.insert(slot, (handle, alpha));
        }
        local.regions = regions;
        local.built_generation = Some(task_generation);
        local.task = None;
    }
    if local.regions.is_empty() {
        return;
    }
    let mut draped = 0_usize;
    for (part, mut material) in &mut parts {
        if part.agent != agent {
            continue;
        }
        let slot = part.region.baked_slot();
        // A server-published bake for this region wins (P14 / Second Life); the
        // local composite only fills regions the grid did not bake for us.
        if state
            .baked_textures
            .get(&agent)
            .is_some_and(|bakes| bakes.contains_key(&slot))
        {
            continue;
        }
        let Some((image, alpha)) = local.regions.get(&slot) else {
            continue;
        };
        let desired =
            bake_mats.local_region_material(agent, slot, image.clone(), *alpha, &mut materials);
        if material.0 != desired {
            *material = MeshMaterial3d(desired);
            draped = draped.saturating_add(1);
        }
    }
    if draped > 0 {
        debug!("draped client-side bake onto {draped} own avatar body part(s)");
    }
}

/// Render our own avatar from its worn shape rather than the server's echoed
/// appearance (R12).
///
/// On a legacy-bake grid the `AvatarAppearance.visual_params` the sim broadcasts
/// for our own avatar is only ever what *we* last published, so a placeholder
/// there deforms our own body (an all-`128` set half-applies every asymmetric
/// body morph → a bloated, spiking avatar). Resolve the real transmitted vector
/// from the worn wearables ([`OwnBakeInputs::visual_params`] — the same bytes
/// [`drive_bake_publish`](crate::bake_publish::drive_bake_publish) advertises) and
/// install it as our own avatar's cached appearance whenever it differs, flagging
/// the avatar for re-shaping. Self-healing: it re-asserts the worn shape if a
/// later server appearance overwrites it, and picks up a re-outfit; a param no
/// worn wearable sets falls back to its table default (the neutral Ruth shape).
pub(crate) fn apply_own_shape_from_wearables(
    identity: Res<SlIdentity>,
    inputs: Res<OwnBakeInputs>,
    library: Option<Res<AvatarAssetLibrary>>,
    mut state: ResMut<AvatarState>,
) {
    if !inputs.is_ready() {
        return;
    }
    let (Some(library), Some(agent)) = (library, identity.agent_id) else {
        return;
    };
    let bytes = inputs.visual_params(library.params());
    if state.appearances.get(&agent) == Some(&bytes) {
        return;
    }
    let _prev = state.appearances.insert(agent, bytes);
    state.appearance_dirty.insert(agent);
    debug!("resolved own avatar shape from worn wearables");
}

/// When an avatar was first and last marked appearance-dirty, in
/// [`Time::elapsed_secs_f64`] seconds (see
/// [`AvatarState::appearance_pending`]).
#[derive(Debug, Clone, Copy)]
struct AppearanceDirtyStamps {
    /// When the avatar entered the pending set (unchanged by re-marks).
    first: f64,
    /// When the avatar was most recently marked (each re-mark refreshes it).
    last: f64,
}

/// How long a re-marked avatar's dirty state must stay quiet (no further
/// marks) before its appearance is re-applied — coalesces the appearance →
/// body-spawn → bake-decode trigger cascade, whose triggers land frames
/// apart, into one rebuild instead of one per trigger.
const APPEARANCE_QUIET_SECS: f64 = 0.3;

/// The longest a re-marked avatar may be deferred by the quiet window — a
/// steady re-mark stream (e.g. a live appearance edit) still resolves at
/// least this often.
const APPEARANCE_MAX_WAIT_SECS: f64 = 1.0;

/// The default for [`AppearanceApplyBudget`]: resolving an avatar's shape
/// re-morphs every base body part and re-uploads their meshes (multi-ms for a
/// heavy avatar), so a crowd streaming in is spread across frames rather than
/// resolved in one.
const DEFAULT_APPEARANCE_APPLY_BUDGET: usize = 2;

/// The per-frame cap on avatars whose appearance
/// [`apply_avatar_appearance`] resolves and re-meshes (see
/// [`DEFAULT_APPEARANCE_APPLY_BUDGET`]).
#[derive(Resource)]
pub(crate) struct AppearanceApplyBudget {
    /// How many avatars may be resolved + re-meshed each frame.
    per_frame: usize,
}

impl Default for AppearanceApplyBudget {
    fn default() -> Self {
        let per_frame = std::env::var("SL_VIEWER_APPEARANCE_APPLY_BUDGET")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_APPEARANCE_APPLY_BUDGET);
        Self { per_frame }
    }
}

/// Apply each rigged avatar's appearance (P13.3 morphs + P13.4 skeletal shape):
/// resolve an `AvatarAppearance.visual_params` vector once into its
/// driver-propagated, sex-gated weights, then (a) rebuild every affected base
/// part's mesh from the morph-target deltas so the body takes its real shape and
/// (b) re-deform the skeleton instance's joint transforms so the avatar's
/// proportions (height, limb / head scale, hips) match. Re-applied whenever a
/// newer appearance arrives.
///
/// The work is deferred and idempotent: a fresh appearance (or a body part that
/// just spawned, matched by [`Added`]) marks the avatar dirty, and the
/// appearance is (re)built from the cached vector — so an appearance that arrives
/// before the body still lands once the body exists. A no-op when no avatar asset
/// library loaded (avatars stay as un-shaped bodies or spheres).
///
/// The rebuild is budgeted and debounced: at most [`AppearanceApplyBudget`]
/// avatars resolve per frame (own avatar first), a never-shaped avatar
/// resolves immediately, and a re-marked one waits [`APPEARANCE_QUIET_SECS`]
/// of quiet (capped at [`APPEARANCE_MAX_WAIT_SECS`]) so the appearance →
/// body-spawn → bake-decode cascade coalesces instead of re-meshing the whole
/// body once per trigger. Deferral is safe: a later pass re-reads the newest
/// cached appearance vector.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system folding appearances and bakes into the morphed body meshes"
)]
#[expect(
    clippy::type_complexity,
    reason = "a Bevy query whose disjointness filters spell out the exact anchor archetype"
)]
pub(crate) fn apply_avatar_appearance(
    mut events: MessageReader<SlEvent>,
    mut decoded: MessageReader<TextureDecoded>,
    library: Option<Res<AvatarAssetLibrary>>,
    manager: Res<TextureManager>,
    mut state: ResMut<AvatarState>,
    volume_gain: Res<VolumeMorphGain>,
    time: Res<Time>,
    budget: Res<AppearanceApplyBudget>,
    identity: Res<SlIdentity>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    added: Query<&AvatarBodyPart, Added<AvatarBodyPart>>,
    mut parts: Query<(Entity, &AvatarBodyPart, &mut Mesh3d)>,
    mut joints: Query<(&AvatarJoint, &mut Transform)>,
    // The rigged body roots, re-planted when their shoe lift changes (R17);
    // disjoint from `joints` (never an `AvatarJoint`) and from the sphere anchors.
    mut anchors: Query<
        &mut Transform,
        (
            With<AvatarAnchor>,
            Without<AvatarSphere>,
            Without<AvatarJoint>,
            Without<AvatarBodyPart>,
        ),
    >,
) {
    // A decoded baked texture of a masked body region (head / upper / lower)
    // supplies the clothing-morph mask, so re-shape any avatar wearing it: its
    // flared morphs were applied unmasked until the bake decoded (P14.5).
    for &TextureDecoded(id) in decoded.read() {
        let wearers: Vec<AgentKey> = state
            .baked_textures
            .iter()
            .filter(|(_, bakes)| {
                bakes
                    .iter()
                    .any(|(&slot, &bake)| bake == id && is_masked_body_slot(slot))
            })
            .map(|(&agent, _)| agent)
            .collect();
        for agent in wearers {
            state.appearance_dirty.insert(agent);
        }
    }
    // Fold any fresh appearance vectors into the cache and flag those avatars.
    for event in events.read() {
        if let SlSessionEvent::AvatarAppearance(appearance) = &event.0 {
            state
                .appearances
                .insert(appearance.avatar_id, appearance.visual_params.clone());
            // The base skirt mesh renders only when the skirt bake is visible (the
            // reference viewer's `isWearingWearableType(WT_SKIRT) &&
            // isTextureVisible(TEX_SKIRT_BAKED)`, which for another avatar reduces
            // to the baked slot holding a real, non-invisible texture).
            let skirt_visible = appearance
                .texture_entry
                .texture_id(avatar_texture::SKIRT_BAKED)
                .is_some_and(avatar_texture::is_bake_visible);
            state
                .skirt_visible
                .insert(appearance.avatar_id, skirt_visible);
            debug!(
                "appearance for {}: skirt {}",
                appearance.avatar_id,
                if skirt_visible { "worn" } else { "not worn" }
            );
            state.appearance_dirty.insert(appearance.avatar_id);
        }
    }
    // A body part that just spawned needs its cached appearance applied (the
    // appearance can arrive before the body object does). The joints spawn with
    // the same body, so this one signal covers both morphs and skeleton.
    for part in &added {
        if state.appearances.contains_key(&part.agent) {
            state.appearance_dirty.insert(part.agent);
        }
    }
    // Fold this frame's fresh marks into the debounce ledger. The marking
    // sites stay cheap set-inserts; draining the set here means a re-mark of
    // a still-pending avatar refreshes its `last` stamp and restarts the
    // quiet window.
    let now = time.elapsed_secs_f64();
    let marks = std::mem::take(&mut state.appearance_dirty);
    for agent in marks {
        state
            .appearance_pending
            .entry(agent)
            .and_modify(|stamps| stamps.last = now)
            .or_insert(AppearanceDirtyStamps {
                first: now,
                last: now,
            });
    }
    if state.appearance_pending.is_empty() {
        return;
    }
    let Some(library) = library else {
        state.appearance_pending.clear();
        return;
    };
    // Pick this frame's avatars: a never-shaped avatar (no recorded
    // deformations) resolves immediately — first visibility wins — while a
    // re-marked one waits for its trigger cascade to go quiet (bounded by the
    // max wait). The own avatar goes first so our own body never queues
    // behind a crowd, and the per-frame budget spreads a crowd across frames.
    let mut eligible: Vec<AgentKey> = state
        .appearance_pending
        .iter()
        .filter(|(agent, stamps)| {
            !state.deformations.contains_key(agent)
                || now - stamps.last >= APPEARANCE_QUIET_SECS
                || now - stamps.first >= APPEARANCE_MAX_WAIT_SECS
        })
        .map(|(&agent, _)| agent)
        .collect();
    if eligible.is_empty() {
        return;
    }
    if let Some(own) = identity.agent_id
        && let Some(position) = eligible.iter().position(|&agent| agent == own)
    {
        eligible.swap(0, position);
    }
    eligible.truncate(budget.per_frame);
    // The chosen avatars' deformations / volumes / physics are about to be
    // re-resolved below: wake the pose gate so it re-poses them next frame.
    state.bump_pose_inputs();
    // Piggybacks on the pose-gate diagnostics: a steady stream here means some
    // caller re-marks appearances dirty (each re-apply rebuilds part meshes and
    // re-resolves the shape — real per-event cost worth tracing).
    if std::env::var_os("SL_VIEWER_LOG_POSE_GATE").is_some() {
        info!(
            "appearance re-apply: {} of {} pending avatar(s)",
            eligible.len(),
            state.appearance_pending.len()
        );
    }
    // Resolve each dirty avatar's appearance once into its morph weights and the
    // deformed joint transforms (both share one `ResolvedParams`).
    let log_geometry = std::env::var_os("SL_VIEWER_LOG_AVATAR_GEOMETRY").is_some();
    // The reference viewer's `physics_test` switch (P34.2): every `Max_Effect` is
    // zero unless a tuned physics wearable turns it on, so this is what makes the
    // bounce visible on an avatar that wears none.
    let force_physics = crate::body_physics::force_enabled();
    // The debug A/B knob for the collision-volume displacement (P34.3), live-toggled
    // by the `V` key.
    let volume_gain = volume_gain.gain;
    let mut morph_weights: HashMap<AgentKey, MorphWeights> = HashMap::new();
    // The rest weights of the per-frame runtime morph params (P31.12a), kept
    // apart from the baked shape so a part's render-time morph targets start at
    // the avatar's own resolved values rather than zero.
    let mut runtime_weights: HashMap<AgentKey, MorphWeights> = HashMap::new();
    let mut joint_transforms: HashMap<AgentKey, Vec<Transform>> = HashMap::new();
    let mut deformations: HashMap<AgentKey, SkeletalDeformations> = HashMap::new();
    // The per-avatar root drop resolved from the shape (R23).
    let mut root_drops: HashMap<AgentKey, f32> = HashMap::new();
    // The per-avatar seat drop (pelvis rest height above the root) resolved from
    // the shape — the seated placement drops the anchor by this (hips on target).
    let mut seat_drops: HashMap<AgentKey, f32> = HashMap::new();
    // The shape's collision-volume displacements per avatar (P34.3).
    let mut volumes: HashMap<AgentKey, VolumeDeformations> = HashMap::new();
    // The ingested body-physics configuration per avatar (P34.1).
    let mut physics: HashMap<AgentKey, BodyPhysics> = HashMap::new();
    // The rest deformed joint **world** matrices per avatar, kept only for the
    // geometry diagnostic (R13) so it can reproduce the GPU skinning on the CPU.
    let mut world_matrices: HashMap<AgentKey, Vec<Mat4>> = HashMap::new();
    for &agent in &eligible {
        if let Some(bytes) = state.appearances.get(&agent) {
            let resolved = ResolvedParams::from_appearance(library.params(), bytes);
            // Bake every shape morph except the per-frame runtime params, which
            // the render-time morph pipeline (P31.12a) drives instead.
            morph_weights.insert(
                agent,
                MorphWeights::from_resolved_static(library.params(), &resolved),
            );
            runtime_weights.insert(
                agent,
                MorphWeights::from_resolved_runtime(library.params(), &resolved),
            );
            // Ingest the physics wearable off the same resolved appearance (P34.1):
            // the spring-damper settings and driven morph params the body-physics
            // motions need. The morph targets they drive are among the runtime
            // params above, so the bounce needs no re-bake.
            let mut body = BodyPhysics::from_resolved(library.params(), &resolved);
            if force_physics {
                body.force_max_effect(crate::body_physics::FORCED_MAX_EFFECT);
            }
            physics.insert(agent, body);
            let deform = SkeletalDeformations::from_resolved(library.params(), &resolved);
            // The shape's collision-volume displacements: the volumes are bindable
            // joints, so this is what makes a worn rigged-mesh body follow the shape
            // sliders — the system body's morph targets cannot reach it. Both the
            // morph params' `<volume_morph>` children (P34.3, the chest / belly /
            // butt / head sliders) and the skeletal params' inherited bone scale
            // (P34.4, height / thickness / limb length) land in the one accumulation.
            let mut volume = VolumeDeformations::from_resolved_with_skeleton(
                library.params(),
                &resolved,
                library.character_skeleton(),
            );
            if (volume_gain - 1.0).abs() > f32::EPSILON {
                volume.amplify(volume_gain);
            }
            // Fold in the worn rigged meshes' joint position overrides (R1) so a
            // fitted mesh body/head poses the skeleton to the positions its
            // inverse-bind matrices were baked against, rather than the plain shape.
            let overrides = state.effective_joint_overrides(agent).unwrap_or_default();
            joint_transforms.insert(
                agent,
                library
                    .skeleton()
                    .deformed_local_transforms_with(&deform, &volume, &overrides),
            );
            // The shape's root plant (R23): the `computeBodySize` quantities of
            // the deformed (and override-posed) chain, lifted by the shape's
            // `Hover` param, decide how far below the wire Z (the capsule
            // centre) the body root sits. Worn-shoe foot offsets (R17) fold in
            // through the metrics' foot term.
            if let Some(metrics) = library.skeleton().body_size_metrics(&deform, &overrides) {
                let hover = resolved.weight(AVATAR_HOVER_PARAM).unwrap_or(0.0);
                root_drops.insert(agent, root_drop_from_metrics(&metrics, hover));
                seat_drops.insert(agent, metrics.pelvis_local_z);
            }
            if log_geometry {
                world_matrices.insert(
                    agent,
                    library.skeleton().deformed_world_matrices(
                        &deform,
                        &volume,
                        &overrides,
                        &AnimationPose::default(),
                    ),
                );
            }
            deformations.insert(agent, deform);
            volumes.insert(agent, volume);
        }
    }
    // Record each avatar's resolved deformations so the animation driver (P18.3)
    // can re-run the skeletal recurrence with the playing motion folded in.
    for (agent, deform) in deformations {
        let _prev = state.deformations.insert(agent, deform);
    }
    // Re-plant each avatar whose resolved root drop changed (R23): the shape
    // (height sliders, hover, worn shoes, a mesh body's joint overrides) moves
    // the plant, so an already-spawned (possibly stationary) body is re-planted
    // straight away rather than waiting for its next position update. The rest
    // drop is the previous value for an avatar whose appearance resolves for
    // the first time — the drop its body was spawned with.
    let rest_drop = library
        .skeleton()
        .body_size_metrics(&SkeletalDeformations::default(), &JointOverrides::default())
        .map_or_else(
            || library.pelvis_height(),
            |metrics| root_drop_from_metrics(&metrics, 0.0),
        );
    for (agent, drop) in root_drops {
        let previous = state.root_drops.insert(agent, drop).unwrap_or(rest_drop);
        if (drop - previous).abs() > f32::EPSILON
            && let Some(entities) = state.objects.get(&agent)
            && let Ok(mut transform) = anchors.get_mut(entities.anchor)
        {
            transform.translation.y -= drop - previous;
        }
    }
    // The seat drop needs no re-plant: a seated avatar's anchor is rewritten every
    // frame by `place_seated_avatars`, which reads this straight back.
    for (agent, drop) in seat_drops {
        let _previous = state.seat_drops.insert(agent, drop);
    }
    // …and its resolved collision-volume displacements, which the same per-frame
    // recurrence folds into the volume joints a rigged mesh body rides (P34.3).
    for (agent, volume) in volumes {
        if !volume.is_empty() {
            debug!(
                "shape displaces {} collision volume(s) for {agent}",
                volume.len()
            );
            if log_geometry {
                for (name, deform) in volume.iter() {
                    let [sx, sy, sz] = deform.scale;
                    let [px, py, pz] = deform.position;
                    debug!(
                        "  volume {name}: scale ({sx:+.4},{sy:+.4},{sz:+.4}) \
                         pos ({px:+.4},{py:+.4},{pz:+.4})"
                    );
                }
            }
        }
        let _prev = state.volume_deformations.insert(agent, volume);
    }
    // Record each avatar's ingested body physics (P34.1) for the per-frame
    // simulation to drive (P34.2).
    for (agent, body) in physics {
        if !body.motions().is_empty() {
            let active = body
                .motions()
                .iter()
                .filter(|config| config.settings.is_active())
                .count();
            debug!(
                "body physics for {agent}: {active} of {} motion(s) active",
                body.motions().len()
            );
        }
        let _prev = state.body_physics.insert(agent, body);
    }
    // Rebuild the mesh of every part belonging to a resolved avatar, masking its
    // clothing morphs by the region's decoded bake where one is available (P14.5).
    let mut morphed_parts = 0_usize;
    for (entity, part, mut mesh) in &mut parts {
        if let Some(weights) = morph_weights.get(&part.agent)
            && let Some(loaded) = library.parts().get(part.part)
        {
            let morphed = match part_clothing_mask(
                &library,
                &manager,
                state.baked_textures.get(&part.agent),
                part.region,
                &loaded.mesh,
            ) {
                Some(mask) => weights.apply_masked(&loaded.mesh, &mask),
                None => weights.apply(&loaded.mesh),
            };
            if log_geometry {
                let skin = match &loaded.binding {
                    LoadedBinding::Skinned(skin) => Some(skin),
                    LoadedBinding::Rigid(_) => None,
                };
                log_geometry_outliers(
                    part.region,
                    &loaded.mesh,
                    morphed.positions(),
                    skin,
                    world_matrices.get(&part.agent).map(Vec::as_slice),
                    library.skeleton(),
                );
            }
            let mut bevy_mesh = to_bevy_morphed_mesh(&loaded.mesh, &morphed);
            // Layer the per-frame runtime morphs this part carries onto the baked
            // geometry as Bevy native morph targets (P31.12a), and give the part
            // a `MeshMorphWeights` seeded at each param's resolved rest weight so
            // an un-driven avatar renders identically while a driver can still
            // animate blink / physics without re-baking the body.
            attach_runtime_morphs(
                &mut commands,
                entity,
                &mut bevy_mesh,
                &loaded.mesh,
                runtime_weights.get(&part.agent),
            );
            *mesh = Mesh3d(meshes.add(bevy_mesh));
            morphed_parts = morphed_parts.saturating_add(1);
        }
    }
    // Re-set every joint transform of a resolved avatar's skeleton instance.
    // Write-on-change: a re-apply with an unchanged shape re-derives identical
    // transforms, and an unguarded write would dirty the whole avatar transform
    // tree — which makes Bevy propagation revert the pose driver's joint
    // globals to rest and forces the pose gate to re-evaluate (see
    // `log_pose_gate_churn`).
    let mut deformed_joints = 0_usize;
    for (joint, mut transform) in &mut joints {
        if let Some(transforms) = joint_transforms.get(&joint.agent)
            && let Some(deformed) = transforms.get(joint.index)
            && *transform != *deformed
        {
            *transform = *deformed;
            deformed_joints = deformed_joints.saturating_add(1);
        }
    }
    if morphed_parts > 0 || deformed_joints > 0 {
        debug!(
            "shaped {morphed_parts} body part(s) + {deformed_joints} joint(s) across {} avatar(s)",
            morph_weights.len()
        );
    }
    for agent in &eligible {
        let _stamps = state.appearance_pending.remove(agent);
    }
}

/// Per-frame overrides for avatar runtime morph params (P31.12a): the eye-blink
/// ([[viewer-p31-12b]]) and body-physics ([[viewer-p34-1]]) drivers write a named
/// param's target weight here, keyed by avatar, and [`apply_avatar_runtime_morphs`]
/// folds each into the affected parts' `MeshMorphWeights` every frame. A param
/// with no entry stays at its appearance-resolved rest weight.
#[derive(Resource, Debug, Default)]
pub(crate) struct AvatarRuntimeMorphs {
    /// Per avatar, the current override weight of each runtime param it drives.
    by_agent: HashMap<AgentKey, HashMap<String, f32>>,
}

impl AvatarRuntimeMorphs {
    /// Set the per-frame weight of one runtime morph `param` on `agent` (a
    /// driver calls this every frame it wants the param off its rest value).
    ///
    /// An already-known param is updated in place: the drivers push every param they
    /// own each frame (the hand-pose cross-fade alone drives thirteen), so the naive
    /// `insert(param.to_owned(), …)` would allocate a `String` per param per avatar
    /// per frame just to overwrite the key it already has.
    pub(crate) fn set(&mut self, agent: AgentKey, param: &str, weight: f32) {
        let params = self.by_agent.entry(agent).or_default();
        if let Some(slot) = params.get_mut(param) {
            *slot = weight;
        } else {
            let _absent = params.insert(param.to_owned(), weight);
        }
    }

    /// Drop the override of one runtime morph `param` on `agent`, letting it fall
    /// back to its appearance-resolved rest weight.
    ///
    /// The body-physics driver (P34.2) releases the params of a motion that is
    /// switched off this way, rather than leaving them where the last bounce put
    /// them.
    pub(crate) fn clear(&mut self, agent: AgentKey, param: &str) {
        if let Some(params) = self.by_agent.get_mut(&agent) {
            let _dropped = params.remove(param);
        }
    }

    /// Drop every override of every avatar not in `live` (they have despawned),
    /// so a long session does not accumulate the drivers' state for avatars that
    /// left.
    pub(crate) fn retain(&mut self, live: &impl Fn(AgentKey) -> bool) {
        self.by_agent.retain(|&agent, _params| live(agent));
    }

    /// The current override weight of `param` on `agent`, if a driver set one.
    fn weight(&self, agent: AgentKey, param: &str) -> Option<f32> {
        self.by_agent.get(&agent)?.get(param).copied()
    }
}

/// The per-frame runtime morph params attached to one avatar base part
/// (P31.12a), parallel to the part mesh's Bevy morph targets and its
/// `MeshMorphWeights` weight slots.
///
/// Recorded when [`apply_avatar_appearance`] rebuilds a part that carries any
/// runtime morph, so [`apply_avatar_runtime_morphs`] can map an avatar's named
/// override weights onto the right weight index without touching the mesh asset.
#[derive(Component, Debug, Clone)]
pub(crate) struct RuntimeMorphParams {
    /// Runtime morph-target names, in the mesh's morph-target (weight) order.
    names: Vec<String>,
    /// Each param's rest (appearance-resolved) weight — the value the per-frame
    /// driver falls back to when it is not overriding that param.
    rest: Vec<f32>,
}

/// Attach the per-frame runtime morph targets a base part carries to its rebuilt
/// `bevy_mesh`, and (re)seed the part entity's `MeshMorphWeights` +
/// [`RuntimeMorphParams`] at the avatar's resolved rest weights (P31.12a).
///
/// A part with none of the [`RUNTIME_MORPH_PARAMS`] gets no morph machinery. The
/// rest weight of a target comes from the avatar's runtime [`MorphWeights`]
/// (`0.0` for an un-driven param such as an open-eye blink), so the seeded mesh
/// renders identically to the fully-baked body until a driver moves a weight.
fn attach_runtime_morphs(
    commands: &mut Commands,
    entity: Entity,
    bevy_mesh: &mut Mesh,
    base: &BaseMesh,
    runtime_weights: Option<&MorphWeights>,
) {
    let Some(targets) = to_bevy_runtime_morph_targets(base, RUNTIME_MORPH_PARAMS) else {
        return;
    };
    let rest: Vec<f32> = targets
        .names()
        .iter()
        .map(|name| runtime_weights.map_or(0.0, |weights| weights.weight(name)))
        .collect();
    let names = targets.names().to_vec();
    targets.attach_to(bevy_mesh);
    commands.entity(entity).insert((
        MeshMorphWeights::Value {
            weights: rest.clone(),
        },
        RuntimeMorphParams { names, rest },
    ));
}

/// Fold each avatar's per-frame runtime morph overrides ([`AvatarRuntimeMorphs`])
/// into its parts' `MeshMorphWeights` every frame (P31.12a).
///
/// For each part with runtime morphs, every weight slot is set to its driver
/// override if one is present, else to its appearance-resolved rest weight — so
/// a driver need only push the params it is currently animating and everything
/// else holds the avatar's own shape.
///
/// The weights are compared before any mutable deref: a `Mut` deref alone marks
/// `MeshMorphWeights` changed and re-uploads the part's morph weights, so an
/// idle avatar (no blink mid-cycle, no body-physics displacement) must take the
/// read-only path and upload nothing.
pub(crate) fn apply_avatar_runtime_morphs(
    morphs: Res<AvatarRuntimeMorphs>,
    mut parts: Query<(&AvatarBodyPart, &RuntimeMorphParams, &mut MeshMorphWeights)>,
) {
    for (part, params, mut weights) in &mut parts {
        let MeshMorphWeights::Value { weights: current } = weights.as_ref() else {
            continue;
        };
        let desired = |index: usize, name: &String| {
            let rest = params.rest.get(index).copied().unwrap_or(0.0);
            morphs.weight(part.agent, name).unwrap_or(rest)
        };
        // A slot missing from the weight vector cannot be written below either,
        // so it never counts as a difference (else it would force the mutable
        // pass every frame without effect). Bit-equality is deliberate: the
        // question is "would the write change the stored value", not a numeric
        // tolerance.
        let unchanged = params.names.iter().enumerate().all(|(index, name)| {
            current
                .get(index)
                .is_none_or(|slot| slot.to_bits() == desired(index, name).to_bits())
        });
        if unchanged {
            continue;
        }
        let MeshMorphWeights::Value { weights } = &mut *weights else {
            continue;
        };
        for (index, name) in params.names.iter().enumerate() {
            let value = desired(index, name);
            if let Some(slot) = weights.get_mut(index) {
                *slot = value;
            }
        }
    }
}

/// Env-gated (`SL_VIEWER_LOG_AVATAR_GEOMETRY`) diagnostic for localising a
/// rest-pose base-body geometry artifact (R13): reproduce the GPU matrix-palette
/// skinning on the CPU for each vertex of a skinned part and log the vertices the
/// skinning displaces furthest from their (morphed) rest position.
///
/// At a true bind pose every skin matrix is identity, so this displacement is
/// ~0; but the skeletal-deformation visual params move the joints off the
/// bindpose the base part's inverse-binds were baked against, so a vertex bound to
/// the *wrong* joint (the reference viewer's joint-render-data list is per-side)
/// is dragged away and spikes even at rest. Each logged vertex carries the
/// render-list index its weight selects and the skeleton joint that index
/// resolves to, so the offending part / vertex / joint is named directly.
fn log_geometry_outliers(
    region: BodyRegion,
    base: &BaseMesh,
    morphed_positions: &[[f32; 3]],
    skin: Option<&BaseMeshSkin>,
    world_matrices: Option<&[Mat4]>,
    skeleton: &BevySkeleton,
) {
    let weights = base.weights();
    let (Some(skin), Some(world)) = (skin, world_matrices) else {
        return;
    };
    // One-shot dump of the reconstructed joint-render-data list (raw weight index
    // -> skeleton joint), the table the per-vertex weight's integer part indexes.
    let render_list: Vec<(usize, Option<&str>)> = skin
        .joints
        .iter()
        .map(|&joint| (joint, skeleton.joint_name(joint)))
        .collect();
    info!("geom[{region:?}] render-data list: {render_list:?}");
    let count = weights.len().min(morphed_positions.len());
    let mut displacements: Vec<(f32, usize, usize)> = Vec::with_capacity(count);
    for index in 0..count {
        let (Some(weight), Some(rest)) = (weights.get(index), morphed_positions.get(index)) else {
            continue;
        };
        let rest = Vec3::new(rest[0], rest[1], rest[2]);
        // The two adjacent render-list palette slots this vertex blends between.
        let slot0 = weight.joint;
        let slot1 = slot0
            .saturating_add(1)
            .min(skin.joints.len().saturating_sub(1));
        let contrib = |slot: usize| -> Option<Vec3> {
            let joint = *skin.joints.get(slot)?;
            let inverse_bind = skin.inverse_bindposes.get(slot)?;
            let joint_world = world.get(joint)?;
            // palette = joint_world · inverse_bind, applied to the rest point.
            Some(joint_world.transform_point3(inverse_bind.transform_point3(rest)))
        };
        let (Some(p0), Some(p1)) = (contrib(slot0), contrib(slot1)) else {
            continue;
        };
        let blend = weight.blend;
        // mix(M0,M1,t)·p == (1-t)·M0·p + t·M1·p (matrix-vector is linear).
        let skinned = Vec3::new(
            p0.x + (p1.x - p0.x) * blend,
            p0.y + (p1.y - p0.y) * blend,
            p0.z + (p1.z - p0.z) * blend,
        );
        // `distance` is glam's own subtraction/length, so it stays clear of the
        // workspace `arithmetic_side_effects` lint the `Vec3` `-` operator trips.
        displacements.push((skinned.distance(rest), index, slot0));
    }
    displacements.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for &(distance, index, slot) in displacements.iter().take(10) {
        let joint = skin.joints.get(slot).copied();
        let name = joint.and_then(|joint| skeleton.joint_name(joint));
        let rest = morphed_positions.get(index).copied().unwrap_or_default();
        info!(
            "geom[{region:?}] v{index} skin-disp {distance:.3} \
             at rest ({:.3},{:.3},{:.3}) render-slot {slot} -> joint {joint:?} {name:?}",
            rest[0], rest[1], rest[2]
        );
    }
}

/// Whether a baked-texture `slot` supplies a clothing-morph mask (P14.5) — the
/// head, upper-body and lower-body region bakes, whose alpha channel masks the
/// flared clothing morphs. A decode of one of these re-shapes the wearing avatar.
const fn is_masked_body_slot(slot: usize) -> bool {
    slot == avatar_texture::HEAD_BAKED
        || slot == avatar_texture::UPPER_BAKED
        || slot == avatar_texture::LOWER_BAKED
}

/// The per-vertex clothing-morph mask (P14.5) for one base part, sampled from its
/// region's decoded baked texture, or `None` when the part has no masked morphs,
/// no published bake for its region, or the bake has not decoded yet (its morphs
/// then apply unmasked — the full flare — until the bake arrives and re-shapes it).
fn part_clothing_mask(
    library: &AvatarAssetLibrary,
    manager: &TextureManager,
    baked: Option<&HashMap<usize, TextureKey>>,
    region: BodyRegion,
    mesh: &BaseMesh,
) -> Option<PartMorphMask> {
    let region_name = region.morph_mask_region()?;
    if !library.masks().has_region(region_name) {
        return None;
    }
    let id = *baked?.get(&region.baked_slot())?;
    let decoded = manager.decoded(id)?;
    // The decoded pixels are always expanded to RGBA8 (stride 4, alpha at offset
    // 3) regardless of the source component count; a source with no alpha channel
    // decodes to opaque alpha (255), which masks nothing — the correct fallback
    // when a bake carries no clothing-coverage mask (Firestorm's null-aux path).
    let texture = MaskTexture {
        pixels: &decoded.pixels,
        width: usize::try_from(decoded.width).unwrap_or(0),
        height: usize::try_from(decoded.height).unwrap_or(0),
        components: RGBA_CHANNELS,
    };
    let mask = library.masks().sample_part(mesh, region_name, &texture);
    if mask.is_empty() { None } else { Some(mask) }
}

/// Show or hide each rigged base-part mesh from the avatar's worn items (P13.5
/// whole-mesh show/hide): hide a whole base region (head / hair / eyes / upper /
/// lower / skirt) when a worn attachment face carries the matching
/// `IMG_USE_BAKED_*` sentinel (a mesh body replacing it), and render the skirt
/// part only when the avatar's `TEX_SKIRT_BAKED` slot holds a visible bake.
///
/// A region is also hidden when its whole baked texture is transparent (P14.3):
/// an alpha wearable carved the entire region away (typically a worn mesh body),
/// which the `IMG_USE_BAKED_*` sentinel path may not signal on its own.
///
/// Runs every frame — cheap (a handful of parts per avatar, and only the rare
/// `IMG_USE_BAKED_*`-bearing attachment is chased) and idempotent: it only writes
/// a [`Visibility`] that actually changed, so it never churns change-detection.
/// The clothing-morph alpha masks (P14.5) — the per-vertex flared-cuff carving —
/// are a *geometry* mask applied in [`apply_avatar_appearance`], not a visibility
/// toggle, so they are not handled here.
pub(crate) fn apply_avatar_part_visibility(
    state: Res<AvatarState>,
    bake_mats: Res<AvatarBakeMaterials>,
    mut parts: Query<(&AvatarBodyPart, &mut Visibility)>,
) {
    let hidden = state.hidden_slots_per_agent();
    let mut changed = 0_usize;
    for (part, mut visibility) in &mut parts {
        let slot = part.region.baked_slot();
        // Hidden either by a worn mesh's `IMG_USE_BAKED_*` sentinel (P13.5) or by
        // the region's own bake being wholly carved away by alpha (P14.3).
        let alpha_hidden = state
            .baked_textures
            .get(&part.agent)
            .and_then(|bakes| bakes.get(&slot))
            .is_some_and(|&id| bake_mats.region_transparent(id));
        // A region baked `IMG_INVISIBLE` by a worn system alpha layer is hidden
        // outright (R22), matching the reference viewer's `isTextureVisible`.
        let invisible = state
            .invisible_regions
            .get(&part.agent)
            .is_some_and(|slots| slots.contains(&slot));
        let region_hidden = alpha_hidden
            || invisible
            || hidden
                .get(&part.agent)
                .is_some_and(|slots| slots.contains(&slot));
        let visible = match part.region {
            // A skirt shows only when worn (and not itself replaced by a mesh).
            BodyRegion::Skirt => {
                !region_hidden
                    && state
                        .skirt_visible
                        .get(&part.agent)
                        .copied()
                        .unwrap_or(false)
            }
            _other => !region_hidden,
        };
        let desired = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != desired {
            *visibility = desired;
            changed = changed.saturating_add(1);
        }
    }
    if changed > 0 {
        debug!("updated visibility of {changed} avatar body part(s)");
    }
}

/// Texture each bake-on-mesh (BoM) rigged-mesh face from its wearer's own baked
/// texture (P17.3 / R22): a modern mesh body's faces carry an `IMG_USE_BAKED_*`
/// sentinel meaning "show the avatar's own baked skin here". Each such [`BomFace`]
/// owns its material (built by [`bom_face_material`]); this system fills it every
/// frame to reproduce the reference viewer's per-face handling of the sampled bake:
///
/// - **Per-face tint.** The reference multiplies the baked texture by the face's
///   `TextureEntry` colour (its vertex colour, `llface.cpp`). A fully-transparent
///   tint (alpha `0`) makes the face invisible — the mechanism a mesh body uses to
///   hide its unused alpha-cut / "onion shell" layers — so such a face is hidden by
///   visibility rather than drawn as opaque skin (R22d/R22e). A non-white tint
///   multiplies the bake.
/// - **Opaque — the bake alpha is ignored.** A 5-channel server bake never
///   satisfies `getPoolTypeFromTE`'s `getComponents()==4` alpha test, so a
///   BoM face with an opaque tint and no material is batched into `sSimpleFaces`
///   (`llvovolume.cpp`) — the opaque simple pass, which does *not* alpha-test. The
///   bake's composited alpha carves the *system* avatar body (and drives region
///   hiding), not this mesh-body attachment; applying it here made bare skin
///   see-through and cut UV-seam rings into the arm (R22d). Only a non-opaque
///   *tint* blends; a fully-transparent tint hides the face (above).
/// - **Neutral fallback.** Until the wearer's bake resolves the face shows the
///   neutral [`BOM_FALLBACK_COLOR`] (matching the reference `IMG_DEFAULT`), not the
///   reddish skin placeholder (R22f).
/// - **UV placement.** The face's `TextureEntry` UV transform is applied, as the
///   reference applies `xform` to a baked face like any other.
///
/// The sampled bake comes from the wearer's fetched server / universal bake
/// ([`AvatarBakeMaterials::ensure_bake`], covering both the classic
/// [`BODY_BAKE_SLOTS`] and the [`UNIVERSAL_BAKE_SLOTS`] a mesh body's arms / legs
/// use), falling back to the material draped on the wearer's matching base-body
/// region by the client-side composite (OpenSim own avatar,
/// [`apply_own_local_bake`]). Runs every frame and is idempotent.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: the ECS resources / queries it needs plus a diagnostic Local"
)]
pub(crate) fn apply_bom_face_materials(
    state: Res<AvatarState>,
    mut bake_mats: ResMut<AvatarBakeMaterials>,
    manager: Res<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    parts: Query<(&AvatarBodyPart, &MeshMaterial3d<FaceMaterial>), Without<BomFace>>,
    mut faces: Query<
        (&BomFace, &MeshMaterial3d<FaceMaterial>, &mut Visibility),
        Without<AvatarBodyPart>,
    >,
    // Diagnostic-only (R22h): the last per-(agent, slot) resolution tally logged,
    // so the summary is emitted only when it changes (see the loop below).
    mut last_tally: Local<String>,
) {
    if faces.is_empty() {
        return;
    }
    // The material each avatar base-body region currently wears, keyed by (agent,
    // baked slot) — used only as the fallback bake source for a classic-slot face on
    // a grid whose bake reached the body region but not `baked_textures` (the
    // OpenSim own-avatar client-side composite).
    let mut part_materials: HashMap<(AgentKey, usize), Handle<FaceMaterial>> = HashMap::new();
    for (part, material) in &parts {
        let _prev =
            part_materials.insert((part.agent, part.region.baked_slot()), material.0.clone());
    }
    // Phase 1: resolve the bake each needed (agent, slot) samples to its decoded
    // image + whether it carries alpha (so the face blends). Prefer the wearer's
    // fetched bake — this covers both classic and universal slots — else read the
    // image already draped on the base-body region material.
    let mut needed: HashSet<(AgentKey, usize)> = HashSet::new();
    for (face, _, _) in &faces {
        let _new = needed.insert((face.agent, face.slot));
    }
    let mut region_bake: HashMap<(AgentKey, usize), (Handle<Image>, bool)> = HashMap::new();
    for &(agent, slot) in &needed {
        if let Some(&id) = state
            .baked_textures
            .get(&agent)
            .and_then(|bakes| bakes.get(&slot))
            && let Some((image, alpha)) = bake_mats.ensure_bake(id, &manager, &mut images)
        {
            let _prev = region_bake.insert((agent, slot), (image, alpha != BakeAlpha::Opaque));
            continue;
        }
        if let Some(handle) = part_materials.get(&(agent, slot))
            && let Some(material) = materials.get(handle)
            && let Some(image) = material.base.base_color_texture.clone()
        {
            let has_alpha = !matches!(material.base.alpha_mode, AlphaMode::Opaque);
            let _prev = region_bake.insert((agent, slot), (image, has_alpha));
        }
    }
    // Phase 2: fill each face's own material + drive its visibility.
    let mut retextured = 0_usize;
    // Diagnostic tally (R22h): per (agent, slot), how many visible BoM faces the
    // wearer's bake resolved vs fell back to the neutral placeholder — the direct
    // signal for "this avatar's `upper` never textures" (gated by
    // `SL_VIEWER_LOG_AVATAR_FACES`; logged after the loop only when it changes).
    let mut tally: HashMap<(AgentKey, usize), (usize, usize)> = HashMap::new();
    for (face, material, mut visibility) in &mut faces {
        // Alpha-cut / onion-shell hiding: a face the wearer set fully transparent
        // (TE tint alpha 0) is invisible in the reference — hide it rather than
        // drawing opaque skin over the layer it was meant to reveal.
        if face.tint[3] == 0 {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        }
        if *visibility == Visibility::Hidden {
            *visibility = Visibility::Inherited;
        }
        // A mesh-body BoM face is opaque for bare skin, but alpha-**masks** when a
        // worn alpha layer has carved the bake transparent (`BakeAlpha::Masked` /
        // `Transparent`), so a region the wearer hid — e.g. the feet under mesh
        // boots — does not render. Only the *carved* case masks: bare skin
        // classifies as `BakeAlpha::Opaque` and stays opaque, so it never goes
        // see-through and no UV-seam ring appears on an un-alpha'd arm (the R22d
        // regression came from masking an *un-carved* bake). The per-face TE tint
        // still wins: a non-opaque tint blends (reference `color_alpha` → alpha
        // pool); a fully-transparent tint is hidden by visibility above.
        let bake = region_bake.get(&(face.agent, face.slot));
        let bake_has_alpha = bake.is_some_and(|&(_, has_alpha)| has_alpha);
        let alpha_mode = bom_face_alpha_mode(face.tint[3], bake_has_alpha);
        let (texture, base_color) = if debug_avatar_grid() {
            // Diagnostic (R22): render the mesh's UV mapping as a grid, so a broken
            // grid (UV-mapping problem) can be told apart from a continuous one
            // (seams are baked skin content). Same per-face UV transform as the bake.
            (Some(bake_mats.debug_grid(&mut images)), Color::WHITE)
        } else if debug_avatar_flat() {
            // Diagnostic (R22): drop the bake and render a flat neutral skin so a
            // texture/UV-seam artifact (vanishes) can be told apart from a
            // geometry/normals one (persists — still lit by the mesh normals).
            (None, Color::srgb(0.6, 0.6, 0.6))
        } else {
            match bake {
                Some((image, _)) => (Some(image.clone()), tint_color(face.tint)),
                // No bake resolved yet: neutral fallback (reference IMG_DEFAULT), not
                // the reddish skin placeholder.
                None => (None, BOM_FALLBACK_COLOR),
            }
        };
        if log_avatar_faces_enabled() {
            let resolved = region_bake.contains_key(&(face.agent, face.slot));
            let counts = tally.entry((face.agent, face.slot)).or_insert((0, 0));
            counts.0 = counts.0.saturating_add(1);
            if resolved {
                counts.1 = counts.1.saturating_add(1);
            }
        }
        // Only touch the material when something actually changed — `get_mut` marks
        // the asset modified (rebuilding its bind group), so an unconditional write
        // every frame would needlessly re-upload every BoM face.
        let up_to_date = materials.get(&material.0).is_some_and(|current| {
            current.base.base_color_texture == texture
                && current.base.base_color == base_color
                && current.base.alpha_mode == alpha_mode
                && current.base.uv_transform == face.uv
        });
        if up_to_date {
            continue;
        }
        let Some(mut material) = materials.get_mut(&material.0) else {
            continue;
        };
        material.base.base_color_texture = texture;
        material.base.base_color = base_color;
        material.base.alpha_mode = alpha_mode;
        material.base.uv_transform = face.uv;
        retextured = retextured.saturating_add(1);
    }
    if retextured > 0 {
        debug!("retextured {retextured} bake-on-mesh face(s) from their wearer's bake");
    }
    if log_avatar_faces_enabled() && !tally.is_empty() {
        let mut lines: Vec<String> = tally
            .iter()
            .map(|(&(agent, slot), &(total, resolved))| {
                let name = bake_service_slot_name(slot).unwrap_or("?");
                format!("{agent} {slot}({name}) {resolved}/{total}")
            })
            .collect();
        lines.sort();
        let summary = lines.join("; ");
        if *last_tally != summary {
            info!("BoM face bake resolution [agent slot(name) textured/total]: {summary}");
            *last_tally = summary;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AvatarEntities, AvatarState, BAKE_ALPHA_MASK_THRESHOLD, BakeAlpha, BodySizeMetrics,
        PROVISIONAL_ID_CHARS, SeatChainQuery, Seated, SeatedTarget, body_root_transform,
        bom_face_alpha_mode, classify_bake_alpha, coarse_translation, drop_to_hips,
        invisible_body_slots, provisional_label, root_drop_from_metrics, seat_world_transform,
        seated_offset, should_refetch_bakes, used_baked_slots, visible_body_bakes,
    };
    use crate::avatar_assets::BodyRegion;
    use crate::coords::{sl_rotation_to_quat, sl_to_bevy_rotation};
    use bevy::math::{Quat, Vec3};
    use bevy::prelude::{AlphaMode, Transform};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, BakeRegion, CircuitId, CoarseLocation, Object, ObjectMotion, RegionHandle,
        RegionLocalObjectId, Rotation, ScopedObjectId, TextureEntry, TextureFace, TextureKey, Uuid,
        Vector, avatar_texture, encode_texture_entry,
    };

    /// The zero vector (`Vector` does not derive `Default`).
    const fn zero() -> Vector {
        Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    /// A minimal avatar object at `position` with an identity (facing) rotation.
    fn avatar_object_at(position: Vector) -> Object {
        Object {
            region_handle: RegionHandle(0),
            local_id: RegionLocalObjectId(1),
            circuit: CircuitId::new(1),
            full_id: Uuid::from_u128(1).into(),
            parent_id: RegionLocalObjectId(0),
            pcode: sl_client_bevy::pcode::AVATAR,
            state: 0,
            crc: 0,
            material: 0,
            click_action: 0,
            update_flags: 0,
            scale: Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            motion: ObjectMotion {
                position,
                velocity: zero(),
                acceleration: zero(),
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    s: 1.0,
                },
                angular_velocity: zero(),
                collision_plane: None,
            },
            owner_id: Uuid::from_u128(0),
            sound: Uuid::from_u128(0),
            gain: 0.0,
            sound_flags: 0,
            sound_radius: 0.0,
            text: String::new(),
            text_color: [0; 4],
            name_value: String::new(),
            media_url: None,
            texture_entry: Vec::new(),
            texture_anim: Vec::new(),
            texture_animation: None,
            shape: sl_client_bevy::PrimShapeParams::default(),
            particle_system: Vec::new(),
            particles: None,
            data: Vec::new(),
            extra_params: Vec::new(),
            extra: sl_client_bevy::ObjectExtraParams::default(),
            properties: None,
            joint_type: 0,
            joint_pivot: zero(),
            joint_axis_or_anchor: zero(),
        }
    }

    /// A seated avatar's offset is its wire pose in **pure Second Life space** — the
    /// parent-relative position with no axis swap and no root drop, plus the SL
    /// rotation — so composing it under the seat entity's basis change places it
    /// exactly like a linkset child prim at the same offset (the reference's
    /// `sitOnObject` `rel_pos` / `rel_rot`). A regression that applied the SL→Bevy
    /// axis swap here (double-swapping under the seat) or subtracted a root drop (the
    /// pelvis correction the reference skips while seated) would trip this.
    #[test]
    fn seated_offset_is_pure_sl_with_no_root_drop() {
        let mut object = avatar_object_at(Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        });
        // A quarter turn about Second Life Z, to check the rotation is the SL
        // quaternion, not the axis-swapped object rotation.
        let rotation = Rotation {
            x: 0.0,
            y: 0.0,
            z: core::f32::consts::FRAC_1_SQRT_2,
            s: core::f32::consts::FRAC_1_SQRT_2,
        };
        object.motion.rotation = rotation.clone();
        let offset = seated_offset(&object);
        assert_eq!(
            offset.translation,
            Vec3::new(1.0, 2.0, 3.0),
            "pure Second Life translation — no axis swap, no root drop",
        );
        assert_eq!(
            offset.rotation,
            sl_rotation_to_quat(&rotation),
            "the Second Life rotation, not the axis-swapped object rotation",
        );
        assert_eq!(offset.scale, Vec3::ONE);
    }

    /// Removing a seat object unseats exactly its riders: their seated state clears
    /// and the [`Seated`] tag is stripped (so the dead-reckoner resumes), while an
    /// avatar seated on a *different* object is untouched. This is the guard against
    /// an avatar left frozen, invisibly parented, to a seat that was deleted or
    /// culled without the simulator's own stand update.
    #[test]
    fn removing_a_seat_unseats_only_its_riders() -> Result<(), Box<dyn core::error::Error>> {
        use bevy::ecs::world::CommandQueue;
        use bevy::prelude::{App, Commands, Transform};

        let mut app = App::new();
        // A dummy anchor tagged `Seated`, standing in for the rider's body root.
        let rider_anchor = app.world_mut().spawn(Seated).id();
        let rider_label = app.world_mut().spawn_empty().id();
        let other_anchor = app.world_mut().spawn(Seated).id();
        let other_label = app.world_mut().spawn_empty().id();

        let seat = ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId(42));
        let other_seat = ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId(99));
        let rider = AgentKey::from(Uuid::from_u128(7));
        let other = AgentKey::from(Uuid::from_u128(8));

        let mut state = AvatarState::default();
        state.seated.insert(
            rider,
            SeatedTarget {
                seat,
                offset: Transform::IDENTITY,
            },
        );
        state.objects.insert(
            rider,
            AvatarEntities {
                anchor: rider_anchor,
                label: rider_label,
            },
        );
        state.seated.insert(
            other,
            SeatedTarget {
                seat: other_seat,
                offset: Transform::IDENTITY,
            },
        );
        state.objects.insert(
            other,
            AvatarEntities {
                anchor: other_anchor,
                label: other_label,
            },
        );

        // Remove the seat and apply the deferred `Seated`-tag removal.
        let mut queue = CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, app.world());
            state.unseat_from_seat(seat, &mut commands);
        }
        queue.apply(app.world_mut());

        assert!(!state.is_seated(rider), "the seat's rider was unseated");
        assert!(
            app.world().get::<Seated>(rider_anchor).is_none(),
            "the rider's Seated tag was stripped",
        );
        assert!(
            state.is_seated(other),
            "an avatar on a different seat is untouched",
        );
        assert!(
            app.world().get::<Seated>(other_anchor).is_some(),
            "the untouched rider keeps its Seated tag",
        );
        Ok(())
    }

    /// The seated pose drops by the pelvis height along the **sit orientation's
    /// up**, so the hips land on the sit target. Upright (identity sit rotation),
    /// the drop is straight down Second Life Z; the sit rotation is preserved. A
    /// regression that dropped along world up (ignoring a reclined seat) or by the
    /// wrong amount — the ~1 m "avatar floats above the seat" bug — would trip here.
    #[test]
    fn drop_to_hips_lowers_by_the_pelvis_height_along_the_sit_up() {
        let seat_drop = 1.067;
        // Upright sit at (1, 2, 3) in the seat frame.
        let upright = Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        };
        let dropped = drop_to_hips(upright, seat_drop);
        // Pure-SL space: up is +Z, so the drop subtracts from Z only.
        assert_eq!(dropped.translation, Vec3::new(1.0, 2.0, 3.0 - seat_drop));
        assert_eq!(dropped.rotation, Quat::IDENTITY, "the sit rotation is kept");

        // A quarter-turn about SL X tips the avatar's up (+Z) onto +Y, so the drop
        // moves along Y instead — the reclined-seat case.
        let tipped = Transform {
            rotation: Quat::from_rotation_x(-core::f32::consts::FRAC_PI_2),
            ..upright
        };
        let dropped = drop_to_hips(tipped, seat_drop);
        assert!(
            (dropped.translation.y - (2.0 - seat_drop)).abs() < 1.0e-5
                && (dropped.translation.z - 3.0).abs() < 1.0e-5,
            "a tilted seat drops along the avatar's up, got {:?}",
            dropped.translation,
        );
    }

    /// [`seat_world_transform`] composes the seat's world pose from the chain of
    /// **local** transforms, matching exactly what Bevy's `PostUpdate` propagation
    /// produces for the same hierarchy — but read from this frame's locals rather
    /// than the frame-late `GlobalTransform`. That is what locks a seated rider
    /// rigidly to a moving vehicle (`viewer-seated-avatar-vehicle-rubberband`). A
    /// child-prim seat (the avatar sat on a non-root prim of a linkset) must compose
    /// `root · child`, so this checks a root seat and a child seat alike against the
    /// propagated ground truth.
    #[test]
    fn seat_world_transform_matches_propagation() -> Result<(), Box<dyn core::error::Error>> {
        use bevy::ecs::system::SystemState;
        use bevy::prelude::{App, ChildOf, GlobalTransform, TransformPlugin};

        let mut app = App::new();
        app.add_plugins(TransformPlugin);

        // A linkset: a moved-and-rotated root prim, plus a child prim at a local
        // offset — the two seat topologies a rider can sit on.
        let root_local = Transform::from_translation(Vec3::new(10.0, 0.0, 5.0))
            .with_rotation(Quat::from_rotation_y(core::f32::consts::FRAC_PI_2));
        let child_local = Transform::from_translation(Vec3::new(0.0, 0.0, 2.0));
        let root = app.world_mut().spawn(root_local).id();
        let child = app.world_mut().spawn((child_local, ChildOf(root))).id();

        // Let propagation compute the ground-truth `GlobalTransform`s.
        app.update();

        let mut system_state: SystemState<SeatChainQuery> = SystemState::new(app.world_mut());
        // `SystemState::get` is fallible and `expect` is denied workspace-wide.
        let chain = system_state
            .get(app.world())
            .ok()
            .ok_or("the seat-chain query must build")?;

        for entity in [root, child] {
            let composed =
                seat_world_transform(entity, &chain).ok_or("entity should be in the chain")?;
            let propagated = app
                .world()
                .get::<GlobalTransform>(entity)
                .ok_or("entity should have a propagated global transform")?
                .compute_transform();
            assert!(
                composed
                    .translation
                    .abs_diff_eq(propagated.translation, 1.0e-5),
                "composed translation {:?} matches propagation {:?}",
                composed.translation,
                propagated.translation,
            );
            assert!(
                composed.rotation.abs_diff_eq(propagated.rotation, 1.0e-5),
                "composed rotation {:?} matches propagation {:?}",
                composed.rotation,
                propagated.rotation,
            );
        }
        Ok(())
    }

    /// A coarse location maps its whole-metre region-relative position through the
    /// Second Life → Bevy axis map (Second Life `(x, y, z)` → Bevy `(x, z, -y)`),
    /// with no region offset for a same-region (root) dot.
    #[test]
    fn coarse_translation_maps_through_axis_swap() {
        let location = CoarseLocation {
            agent_id: AgentKey::from(Uuid::from_u128(1)),
            x: 10,
            y: 20,
            z: 24,
        };
        assert_eq!(
            coarse_translation(&location, 0.0, 0.0),
            Vec3::new(10.0, 24.0, -20.0)
        );
    }

    /// A neighbour region's coarse dot is offset by the region's east/north metres
    /// from the scene origin before the axis swap, so it lands on that neighbour's
    /// terrain (R24): a dot one region (256 m) east and 256 m north maps its local
    /// `(10, 20)` to Bevy `(266, 24, -276)`.
    #[test]
    fn coarse_translation_offsets_a_neighbour_region() {
        let location = CoarseLocation {
            agent_id: AgentKey::from(Uuid::from_u128(1)),
            x: 10,
            y: 20,
            z: 24,
        };
        assert_eq!(
            coarse_translation(&location, 256.0, 256.0),
            Vec3::new(266.0, 24.0, -276.0)
        );
    }

    /// A NameValue seed fills the legacy name instantly, never clobbers a
    /// record the display-name cap already resolved, and only touches the
    /// title when a `Title` pair is present.
    #[test]
    fn name_value_seed_merges_without_clobbering() {
        use sl_client_bevy::DisplayName;

        let agent: super::AgentKey = super::Uuid::from_u128(0x51).into();
        let mut state = AvatarState::default();

        state.seed_name_fields(
            agent,
            Some("Avatar".to_owned()),
            Some("Tester".to_owned()),
            Some("Crew Chief".to_owned()),
        );
        assert_eq!(state.name_of(agent), Some("Avatar Tester"));
        assert_eq!(state.title_of(agent), Some("Crew Chief"));
        assert_eq!(state.label_text(agent), "Avatar Tester");

        // The cap answers: display name + username take over, legacy refreshed.
        let record = DisplayName {
            id: agent,
            username: "avatar.tester".to_owned(),
            display_name: "Shiny Name".to_owned(),
            legacy_first_name: "Avatar".to_owned(),
            legacy_last_name: "Tester".to_owned(),
            is_display_name_default: false,
            ..DisplayName::default()
        };
        assert!(state.merge_display_name_record(&record));
        assert_eq!(state.label_text(agent), "Shiny Name");
        // `name_of` (the wire-facing accessor) still answers the LEGACY name.
        assert_eq!(state.name_of(agent), Some("Avatar Tester"));

        // A later NameValue seed must not clobber the cap record.
        state.seed_name_fields(
            agent,
            Some("Avatar".to_owned()),
            Some("Tester".to_owned()),
            Some("Crew Chief".to_owned()),
        );
        assert_eq!(state.label_text(agent), "Shiny Name");

        // A `Title`-less update leaves the title alone; an empty one clears it.
        state.seed_name_fields(
            agent,
            Some("Avatar".to_owned()),
            Some("Tester".to_owned()),
            None,
        );
        assert_eq!(state.title_of(agent), Some("Crew Chief"));
        state.seed_name_fields(agent, None, None, Some(String::new()));
        assert_eq!(state.title_of(agent), None);
    }

    /// A `missing` display-name placeholder changes nothing — the legacy
    /// fallback (OpenSim, unresolvable ids) stays authoritative.
    #[test]
    fn missing_display_name_keeps_legacy_fallback() {
        use sl_client_bevy::DisplayName;

        let agent: super::AgentKey = super::Uuid::from_u128(0x52).into();
        let mut state = AvatarState::default();
        state.names.entry(agent).or_default().legacy = Some("Old Timer".to_owned());
        let record = DisplayName {
            id: agent,
            missing: true,
            ..DisplayName::default()
        };
        assert!(!state.merge_display_name_record(&record));
        assert_eq!(state.label_text(agent), "Old Timer");
    }

    /// A single-name ("Resident") account seeds as just the first name,
    /// mirroring `legacy_name()`.
    #[test]
    fn resident_last_name_collapses_in_seed() {
        let agent: super::AgentKey = super::Uuid::from_u128(0x53).into();
        let mut state = AvatarState::default();
        state.seed_name_fields(
            agent,
            Some("bobsmith123".to_owned()),
            Some("Resident".to_owned()),
            None,
        );
        assert_eq!(state.name_of(agent), Some("bobsmith123"));
    }

    /// The provisional tag is the agent id's leading hex fragment, so two distinct
    /// avatars read differently before their names resolve.
    #[test]
    fn provisional_label_is_a_short_id_fragment() {
        let agent = AgentKey::from(Uuid::from_u128(0x1234_5678_9abc));
        let label = provisional_label(agent);
        assert_eq!(label.chars().count(), PROVISIONAL_ID_CHARS);
        assert!(agent.uuid().simple().to_string().starts_with(&label));
    }

    /// A body root maps the object position through the Second Life → Bevy axis
    /// swap and lowers it by the resolved root drop (R23) — the wire Z is the
    /// physics-capsule centre, not the pelvis; with an identity facing
    /// rotation, the root carries just the basis change.
    #[test]
    fn body_root_plants_the_capsule_centre_at_the_object_position() {
        // The rest-shape drop: 0.5·1.707 − 0.979 + 1.067 ≈ 0.94.
        let root_drop = 0.9415;
        let object = avatar_object_at(Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        });
        // No region offset (root region) → the raw region-local placement.
        let transform = body_root_transform(&object, root_drop, Vec3::ZERO);
        // Second Life (10, 20, 30) → Bevy (10, 30, -20), then lowered in Y by the
        // root drop.
        assert_eq!(
            transform.translation,
            Vec3::new(10.0, 30.0 - root_drop, -20.0)
        );
        // An identity object rotation leaves only the basis change at the root.
        assert!(
            transform
                .rotation
                .abs_diff_eq(sl_to_bevy_rotation(), 1.0e-6)
        );
    }

    /// The root drop places the soles at `reported_z − 0.5·body_size_z + hover`
    /// (R23): with the pelvis `pelvis_local_z` above the root and the soles
    /// `pelvis_to_foot` below the pelvis, the sole height under the drop is
    /// independent of `pelvis_to_foot` — the reference's cancellation — and a
    /// positive hover raises the plant.
    #[test]
    fn root_drop_matches_the_reference_sole_height() {
        let metrics = BodySizeMetrics {
            pelvis_to_foot: 0.979,
            body_size_z: 1.707,
            pelvis_local_z: 1.067,
        };
        let drop = root_drop_from_metrics(&metrics, 0.0);
        // Sole (SL Z) for a report at z: `z − drop` is the root; the pelvis sits
        // `pelvis_local_z` above it and the sole `pelvis_to_foot` below that.
        let sole = -drop + metrics.pelvis_local_z - metrics.pelvis_to_foot;
        assert!((sole - (-0.5 * metrics.body_size_z)).abs() < 1.0e-6);
        // A worn shoe grows both `pelvis_to_foot` and `body_size_z` by its lift
        // (the reference folds the foot offset into both), which *raises* the
        // root by half the lift.
        let shod = BodySizeMetrics {
            pelvis_to_foot: 0.979 + 0.08,
            body_size_z: 1.707 + 0.08,
            pelvis_local_z: 1.067,
        };
        let shod_drop = root_drop_from_metrics(&shod, 0.0);
        assert!((drop - shod_drop - 0.04).abs() < 1.0e-6);
        // Hover lifts the whole body directly.
        let hovered = root_drop_from_metrics(&metrics, 0.25);
        assert!((drop - hovered - 0.25).abs() < 1.0e-6);
    }

    /// Each body region keys its visibility off its own baked slot — the head
    /// (and eyelashes) off the head bake, the eyes off the eyes bake, and so on.
    #[test]
    fn body_region_maps_to_its_baked_slot() {
        assert_eq!(BodyRegion::Head.baked_slot(), avatar_texture::HEAD_BAKED);
        assert_eq!(BodyRegion::Hair.baked_slot(), avatar_texture::HAIR_BAKED);
        assert_eq!(BodyRegion::Eyes.baked_slot(), avatar_texture::EYES_BAKED);
        assert_eq!(BodyRegion::Upper.baked_slot(), avatar_texture::UPPER_BAKED);
        assert_eq!(BodyRegion::Lower.baked_slot(), avatar_texture::LOWER_BAKED);
        assert_eq!(BodyRegion::Skirt.baked_slot(), avatar_texture::SKIRT_BAKED);
    }

    /// The client-side bake (P15.3) keys its composited regions by
    /// [`BakeRegion::slot`], and looks them up per body part by
    /// [`BodyRegion::baked_slot`]; the two slot mappings must agree for every
    /// region, or a composited bake would never be found for its body part.
    #[test]
    fn body_region_baked_slots_round_trip_through_bake_region() {
        for region in [
            BodyRegion::Head,
            BodyRegion::Hair,
            BodyRegion::Eyes,
            BodyRegion::Upper,
            BodyRegion::Lower,
            BodyRegion::Skirt,
        ] {
            let slot = region.baked_slot();
            // Every body region's baked slot names a bake region, and that bake
            // region reports the same slot the local composite is keyed by.
            assert_eq!(
                BakeRegion::from_slot(slot).map(BakeRegion::slot),
                Some(slot)
            );
        }
    }

    /// The client-side bake flip (P15.3) mirrors the image about its horizontal
    /// axis: the top row and bottom row swap, and a degenerate buffer is left
    /// untouched. A 1×2 RGBA image (one red row over one blue row) inverts.
    #[test]
    fn flip_rows_vertically_mirrors_top_and_bottom() {
        // Row 0 red, row 1 blue (1 px wide, RGBA).
        let mut pixels = vec![255, 0, 0, 255, 0, 0, 255, 255];
        super::flip_rows_vertically(&mut pixels, 1, 2);
        assert_eq!(pixels, vec![0, 0, 255, 255, 255, 0, 0, 255]);
        // A buffer too short for its declared geometry is left as-is.
        let mut short = vec![1, 2, 3];
        super::flip_rows_vertically(&mut short, 1, 2);
        assert_eq!(short, vec![1, 2, 3]);
    }

    /// The eye-bake opacity forcing (P15.3) sets every texel's alpha byte to 255
    /// while leaving the colour channels untouched, so a transparent-surround iris
    /// no longer carves the opaque eyeball away.
    #[test]
    fn force_alpha_opaque_fills_only_the_alpha_channel() {
        // Two RGBA texels with varied colour and alpha 0 / 128.
        let mut pixels = vec![10, 20, 30, 0, 40, 50, 60, 128];
        super::force_alpha_opaque(&mut pixels);
        assert_eq!(pixels, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }

    /// Only the head, upper-body and lower-body regions carry masked clothing
    /// morphs (P14.5); their bakes are the ones whose decode re-shapes the body,
    /// and they map to the `<morph_masks>` `body_region` names.
    #[test]
    fn masked_body_regions_map_to_morph_mask_names() {
        assert_eq!(BodyRegion::Head.morph_mask_region(), Some("head"));
        assert_eq!(BodyRegion::Upper.morph_mask_region(), Some("upper_body"));
        assert_eq!(BodyRegion::Lower.morph_mask_region(), Some("lower_body"));
        assert_eq!(BodyRegion::Hair.morph_mask_region(), None);
        assert_eq!(BodyRegion::Eyes.morph_mask_region(), None);
        assert_eq!(BodyRegion::Skirt.morph_mask_region(), None);

        // The masked slots are exactly the head / upper / lower bakes.
        assert!(super::is_masked_body_slot(avatar_texture::HEAD_BAKED));
        assert!(super::is_masked_body_slot(avatar_texture::UPPER_BAKED));
        assert!(super::is_masked_body_slot(avatar_texture::LOWER_BAKED));
        assert!(!super::is_masked_body_slot(avatar_texture::HAIR_BAKED));
        assert!(!super::is_masked_body_slot(avatar_texture::EYES_BAKED));
        assert!(!super::is_masked_body_slot(avatar_texture::SKIRT_BAKED));
    }

    /// A texture entry carrying an `IMG_USE_BAKED_*` sentinel yields that region's
    /// baked slot; an ordinary entry yields none.
    #[test]
    fn used_baked_slots_reads_the_sentinels() {
        let with_sentinel = TextureEntry {
            faces: vec![
                TextureFace::new(TextureKey::from(Uuid::from_u128(0x1234))),
                TextureFace::new(TextureKey::from(avatar_texture::IMG_USE_BAKED_UPPER)),
            ],
        };
        assert_eq!(
            used_baked_slots(&encode_texture_entry(&with_sentinel)),
            vec![avatar_texture::UPPER_BAKED]
        );

        let ordinary = TextureEntry {
            faces: vec![TextureFace::new(TextureKey::from(Uuid::from_u128(0x99)))],
        };
        assert!(used_baked_slots(&encode_texture_entry(&ordinary)).is_empty());
        // An empty blob decodes to no faces, so no slots.
        assert!(used_baked_slots(&[]).is_empty());
    }

    /// `visible_body_bakes` picks out the visible baked texture in each base-body
    /// region slot (keyed by slot) and skips a slot left empty or set to the
    /// invisible / default sentinel.
    #[test]
    fn visible_body_bakes_reads_the_region_slots() {
        let head = TextureKey::from(Uuid::from_u128(0xabc));
        let upper = TextureKey::from(Uuid::from_u128(0xdef));
        // Build a full-length face table so every baked slot index exists, with a
        // real bake in head/upper, the invisible sentinel in lower, and the null
        // id everywhere else (built by index to avoid slice indexing).
        let faces = (0..avatar_texture::COUNT)
            .map(|slot| {
                let id = if slot == avatar_texture::HEAD_BAKED {
                    head
                } else if slot == avatar_texture::UPPER_BAKED {
                    upper
                } else if slot == avatar_texture::LOWER_BAKED {
                    TextureKey::from(avatar_texture::IMG_INVISIBLE)
                } else {
                    TextureKey::from(Uuid::nil())
                };
                TextureFace::new(id)
            })
            .collect();
        let bakes = visible_body_bakes(&TextureEntry { faces });
        assert_eq!(bakes.get(&avatar_texture::HEAD_BAKED), Some(&head));
        assert_eq!(bakes.get(&avatar_texture::UPPER_BAKED), Some(&upper));
        // The invisible-sentinel lower slot and the empty eyes/hair/skirt slots
        // are not visible bakes.
        assert!(!bakes.contains_key(&avatar_texture::LOWER_BAKED));
        assert!(!bakes.contains_key(&avatar_texture::EYES_BAKED));
        assert_eq!(bakes.len(), 2, "only the two real bakes are picked up");
    }

    /// A region whose baked slot is the `IMG_INVISIBLE` sentinel (a worn system
    /// alpha layer) is reported as invisible so the system body is hidden (R22);
    /// a real bake, the null id, and a non-body (universal) slot are not.
    #[test]
    fn invisible_body_slots_flags_only_the_invisible_regions() {
        let faces = (0..avatar_texture::COUNT)
            .map(|slot| {
                let id = if slot == avatar_texture::LOWER_BAKED {
                    TextureKey::from(avatar_texture::IMG_INVISIBLE)
                } else if slot == avatar_texture::HEAD_BAKED {
                    TextureKey::from(Uuid::from_u128(0xabc))
                } else if slot == avatar_texture::LEFT_ARM_BAKED {
                    // A universal slot baked invisible must NOT flag a base region.
                    TextureKey::from(avatar_texture::IMG_INVISIBLE)
                } else {
                    TextureKey::from(Uuid::nil())
                };
                TextureFace::new(id)
            })
            .collect();
        let invisible = invisible_body_slots(&TextureEntry { faces });
        assert!(invisible.contains(&avatar_texture::LOWER_BAKED));
        assert!(!invisible.contains(&avatar_texture::HEAD_BAKED));
        assert!(!invisible.contains(&avatar_texture::LEFT_ARM_BAKED));
        assert_eq!(invisible.len(), 1);
    }

    /// A baked texture's composited alpha (P14.3) is classified from its source
    /// component count and RGBA8 pixels: no alpha channel is opaque, an all-carved
    /// alpha is wholly transparent, an all-kept alpha is opaque, and any mix is
    /// masked.
    #[test]
    fn classify_bake_alpha_reads_the_alpha_channel() {
        /// A one-row bake stand-in whose alpha range is computed from `pixels`
        /// exactly as the decode task does ([`DecodedTexture::new`]).
        fn bake(components: u16, pixels: &[u8]) -> sl_client_bevy::DecodedTexture {
            sl_client_bevy::DecodedTexture::new(
                u32::try_from(pixels.len() / 4).unwrap_or(0),
                1,
                components,
                sl_client_bevy::DiscardLevel::FULL,
                bytes::Bytes::copy_from_slice(pixels),
                None,
            )
        }
        // No alpha channel (RGB source): opaque regardless of the filled byte.
        assert_eq!(
            classify_bake_alpha(&bake(3, &[10, 20, 30, 0])),
            BakeAlpha::Opaque
        );
        // Every alpha at/above the cutoff → opaque.
        assert_eq!(
            classify_bake_alpha(&bake(4, &[0, 0, 0, 255, 1, 1, 1, 200])),
            BakeAlpha::Opaque
        );
        // Every alpha below the cutoff → wholly transparent (hide the region).
        assert_eq!(
            classify_bake_alpha(&bake(4, &[9, 9, 9, 0, 9, 9, 9, 10])),
            BakeAlpha::Transparent
        );
        // A mix of kept and carved pixels → masked.
        assert_eq!(
            classify_bake_alpha(&bake(4, &[9, 9, 9, 255, 9, 9, 9, 0])),
            BakeAlpha::Masked
        );
        // The cutoff is the reference `sMinimumAlpha` (0.2 → 51): a pixel at alpha
        // 60 is *kept* (opaque), where the old 0.5 cutoff (128) would have carved it
        // — which is what stopped bare mesh-body skin rendering see-through (R22d).
        assert_eq!(
            classify_bake_alpha(&bake(4, &[0, 0, 0, 60, 1, 1, 1, 255])),
            BakeAlpha::Opaque
        );
        // A pixel just below the cutoff (40 < 51) still carves, so it masks.
        assert_eq!(
            classify_bake_alpha(&bake(4, &[0, 0, 0, 40, 1, 1, 1, 255])),
            BakeAlpha::Masked
        );
        // No pixels at all → opaque (nothing is carved away).
        assert_eq!(classify_bake_alpha(&bake(4, &[])), BakeAlpha::Opaque);
    }

    /// Each classification maps to the right render behaviour: opaque skin stays
    /// opaque, a carved bake masks, and only a wholly transparent bake hides its
    /// region.
    #[test]
    fn bake_alpha_drives_render_mode_and_hiding() {
        assert_eq!(BakeAlpha::Opaque.alpha_mode(), AlphaMode::Opaque);
        assert!(matches!(BakeAlpha::Masked.alpha_mode(), AlphaMode::Mask(_)));
        assert!(matches!(
            BakeAlpha::Transparent.alpha_mode(),
            AlphaMode::Mask(_)
        ));
        assert!(!BakeAlpha::Opaque.hides_region());
        assert!(!BakeAlpha::Masked.hides_region());
        assert!(BakeAlpha::Transparent.hides_region());
    }

    /// A BoM mesh-body face masks only when a worn alpha layer carved the bake:
    /// bare skin (opaque bake, opaque tint) stays opaque so it is never
    /// see-through (the R22d failure), a carved bake masks so the hidden region
    /// (feet under boots) vanishes, and a translucent tint always blends.
    #[test]
    fn bom_face_alpha_mode_masks_only_carved_bakes() {
        // Bare skin: opaque tint, no carved alpha in the bake -> stays opaque.
        assert_eq!(bom_face_alpha_mode(255, false), AlphaMode::Opaque);
        // A worn alpha layer carved the bake -> mask the carved (feet) region.
        assert_eq!(
            bom_face_alpha_mode(255, true),
            AlphaMode::Mask(BAKE_ALPHA_MASK_THRESHOLD)
        );
        // A translucent TE tint blends regardless of the bake's alpha.
        assert_eq!(bom_face_alpha_mode(128, false), AlphaMode::Blend);
        assert_eq!(bom_face_alpha_mode(128, true), AlphaMode::Blend);
    }

    /// Bake re-fetch is gated on the COF version (P14.4): a first appearance and
    /// any appearance without a COF version always ingest, a newer or equal COF
    /// version re-fetches, and only a strictly-older (out-of-order / duplicate)
    /// appearance is skipped.
    #[test]
    fn should_refetch_bakes_gates_on_cof_version() {
        // No COF version seen yet, or none on the appearance: always ingest.
        assert!(should_refetch_bakes(None, Some(15)));
        assert!(should_refetch_bakes(None, None));
        assert!(should_refetch_bakes(Some(15), None));
        // Newer and equal COF versions re-fetch (equal covers a same-outfit
        // rebake republishing new baked ids).
        assert!(should_refetch_bakes(Some(15), Some(16)));
        assert!(should_refetch_bakes(Some(15), Some(15)));
        // A strictly-older appearance is a stale resend and is skipped.
        assert!(!should_refetch_bakes(Some(15), Some(14)));
    }

    /// An attachment's `IMG_USE_BAKED_*` hide is attributed to the avatar it hangs
    /// off, by chasing the parent chain up (through nested linkset prims) to the
    /// avatar root; an object whose chain does not reach an avatar is ignored.
    #[test]
    fn hidden_slots_chase_the_attachment_chain_to_the_avatar() {
        let mut state = AvatarState::default();
        let agent = AgentKey::from(Uuid::from_u128(0xa5));
        let circuit = CircuitId::new(1);
        let avatar = ScopedObjectId::new(circuit, RegionLocalObjectId(100));
        let attachment = ScopedObjectId::new(circuit, RegionLocalObjectId(200));
        let child_prim = ScopedObjectId::new(circuit, RegionLocalObjectId(300));
        let orphan = ScopedObjectId::new(circuit, RegionLocalObjectId(400));

        state.by_scoped.insert(avatar, agent);
        // child prim -> attachment root -> avatar root.
        state.object_parents.insert(attachment, avatar);
        state.object_parents.insert(child_prim, attachment);
        // A deep child prim of the worn mesh replaces the upper region.
        state
            .baked_hides
            .insert(child_prim, vec![avatar_texture::UPPER_BAKED]);
        // An object whose chain does not reach any avatar is not attributed.
        state
            .baked_hides
            .insert(orphan, vec![avatar_texture::HEAD_BAKED]);

        let hidden = state.hidden_slots_per_agent();
        assert_eq!(hidden.len(), 1, "only the one avatar gets a hide set");
        let slots = hidden.get(&agent).cloned().unwrap_or_default();
        assert!(slots.contains(&avatar_texture::UPPER_BAKED));
        assert!(
            !slots.contains(&avatar_texture::HEAD_BAKED),
            "the orphan's hide must not leak onto the avatar"
        );
    }
}
