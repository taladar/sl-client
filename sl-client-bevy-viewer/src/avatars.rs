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
//! Each avatar also carries a floating **name tag** — a `bevy_ui` text node
//! positioned each frame over the sphere by projecting its world position to the
//! screen ([`position_name_tags`]). The legacy name is resolved once per agent via
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

use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use bytes::Bytes;
use sl_client_bevy::{
    AgentKey, AnimationPose, AvatarName, BakeRegion, BaseMesh, BaseMeshSkin, BevySkeleton,
    CoarseLocation, Command, DecodedTexture, DiscardLevel, JointOverrides, MAX_FACES, MaskTexture,
    MeshSkin, MorphWeights, Object, PartMorphMask, RegionHandle, ResolvedParams, ScopedObjectId,
    SkeletalDeformations, SlCommand, SlEvent, SlIdentity, SlSessionEvent, TextureEntry, TextureKey,
    Uuid, avatar_texture, composite_region, decode_texture_entry, joint_position_overrides, pcode,
    to_bevy_base_mesh, to_bevy_image, to_bevy_morphed_mesh,
};

use crate::avatar_assets::{AvatarAssetLibrary, BodyRegion, LoadedBinding};
use crate::bake_inputs::OwnBakeInputs;
use crate::coords::{
    metres_to_f32, sl_euler_deg_to_quat, sl_to_bevy_object_rotation, sl_to_bevy_vec,
};
use crate::physics::AvatarMotion;
use crate::textures::{TextureDecoded, TextureManager, tint_color};

/// The radius, in metres, of an avatar placeholder sphere (a ~2 m-diameter
/// UV-sphere, roughly avatar-sized).
const AVATAR_SPHERE_RADIUS: f32 = 1.0;

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

/// The name-tag font size, in logical pixels.
const NAME_TAG_FONT_SIZE: f32 = 16.0;

/// How many leading hex characters of the agent id to show as a provisional tag
/// before the real name resolves.
const PROVISIONAL_ID_CHARS: usize = 8;

/// A marker component tagging an entity as an avatar placeholder sphere.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarSphere;

/// A marker component on the transform-bearing *anchor* entity of an avatar —
/// its placeholder sphere or the root of its rigged body — whose world position
/// [`position_name_tags`] projects to place the floating name tag.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct AvatarAnchor;

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
    let decoded = DecodedTexture {
        width,
        height: width,
        components: 4,
        discard_level: DiscardLevel::FULL,
        pixels: Bytes::from(pixels),
        aux: None,
    };
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

/// A `bevy_ui` name-tag text node, pointing back at the avatar anchor it floats
/// over so [`position_name_tags`] can project the anchor's world position to the
/// screen each frame.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NameTag {
    /// The avatar anchor entity (sphere or body root) this tag labels.
    anchor: Entity,
    /// The height, in metres above the anchor's world position, at which to
    /// float the tag (a sphere's top or a body's head).
    tag_height: f32,
}

/// The shared placeholder sphere mesh and material, built once and reused by
/// every avatar sphere.
struct AvatarAssets {
    /// The shared UV-sphere mesh handle.
    mesh: Handle<Mesh>,
    /// The shared soft-blue material handle.
    material: Handle<StandardMaterial>,
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
    /// Resolved legacy names, keyed by agent id — the "simple name cache" that
    /// keeps a repeatedly-seen avatar from being re-requested.
    names: HashMap<AgentKey, String>,
    /// Agents whose legacy name has already been requested (but has not
    /// necessarily arrived), so the same `UUIDNameRequest` is not sent twice.
    requested: HashSet<AgentKey>,
    /// The latest `AvatarAppearance.visual_params` byte vector per avatar, kept so
    /// a body spawned after (or re-spawned) can be morphed from the last known
    /// appearance (P13.3).
    appearances: HashMap<AgentKey, Vec<u8>>,
    /// Avatars whose rigged body needs its appearance (re)applied — its morphs
    /// re-blended and its skeleton re-deformed — set on a fresh appearance and on
    /// a newly spawned body, drained by [`apply_avatar_appearance`].
    appearance_dirty: HashSet<AgentKey>,
    /// The joint position overrides each avatar's worn rigged meshes impose (R1),
    /// keyed by agent id then by the contributing **mesh asset id**. Kept per-mesh
    /// (rather than pre-merged) so the set can be rebuilt as meshes come and go — the
    /// reference viewer's `clearAttachmentOverrides` + rebuild — and so a per-joint
    /// conflict resolves to the highest-mesh-id override (`findActiveOverride`), via
    /// [`effective_joint_overrides`](Self::effective_joint_overrides). Absent for an
    /// avatar wearing no position-carrying rig — its skeleton stays on the plain
    /// appearance shape. `apply_avatar_appearance` folds the effective set in.
    joint_overrides: HashMap<AgentKey, HashMap<Uuid, JointOverrides>>,
    /// Whether each avatar's `TEX_SKIRT_BAKED` slot holds a visible bake, from its
    /// latest appearance — the reference viewer's skirt-worn test. Absent means
    /// not yet known, treated as no skirt (the base skirt mesh stays hidden).
    skirt_visible: HashMap<AgentKey, bool>,
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
    /// The extra vertical plant height each avatar gains from its worn shoes'
    /// heel / platform height (R17), in Second Life Z-up metres: the reference
    /// viewer's `computeBodySize` folds the shoe's downward foot-bone offset into
    /// `mPelvisToFoot`, raising the avatar so its shod feet rest on the ground.
    /// Added to the fixed pelvis rest height when planting the body root
    /// ([`body_root_transform`]). Absent (treated as `0`) until an appearance
    /// resolves the shoe params, or for a sphere-only avatar.
    pelvis_lift: HashMap<AgentKey, f32>,
    /// R22b diagnostic: every agent the session has *ever* surfaced a full avatar
    /// object (`pcode` 47) for, so the [`log_avatar_interest`]-gated census can
    /// tell a "the simulator never streamed this avatar" case (agent absent here)
    /// from a "we received it but failed to render it" case (agent present here yet
    /// still a coarse sphere). Never pruned — it is a cumulative diagnostic marker.
    ever_full_object: HashSet<AgentKey>,
    /// R22b diagnostic: the last coarse (minimap) position `(x, y, z)` seen per
    /// coarse-only agent — `x`/`y` region-local metres (0..255), `z` already in
    /// metres (0..1020, the `u8 × 4` coarse scale). A `z` at the 1020 ceiling is the
    /// simulator's "height unknown / off this region" sentinel, so the census can
    /// flag a sphere that is really a neighbour-region avatar. Populated only when
    /// [`log_avatar_interest`] is set.
    coarse_pos: HashMap<AgentKey, (u8, u8, u16)>,
    /// The shared placeholder sphere mesh + material, built lazily on first use.
    assets: Option<AvatarAssets>,
}

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
    material: Handle<StandardMaterial>,
    /// One render entry per resolved base part.
    parts: Vec<BodyPart>,
    /// Each joint's local rest transform (Second Life Z-up), parallel to
    /// [`joint_parents`](Self::joint_parents); a fresh joint entity is spawned
    /// per avatar from these.
    joint_locals: Vec<Transform>,
    /// Each joint's parent index (`None` for a root), parallel to
    /// [`joint_locals`](Self::joint_locals).
    joint_parents: Vec<Option<usize>>,
    /// The pelvis rest height (Second Life Z, metres) used to plant the body
    /// vertically so its pelvis sits at the reported object position.
    pelvis_height: f32,
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
}

impl AvatarBody {
    /// The skeleton joint index a rigged mesh's joint name binds to, resolving a
    /// canonical name or an alias like the base body does (P17.2). `None` for a
    /// name the standard skeleton does not carry.
    pub(crate) fn joint_index(&self, name: &str) -> Option<usize> {
        self.joint_lookup.get(name).copied()
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let Some(library) = library else {
        return;
    };
    let material = materials.add(StandardMaterial {
        base_color: BODY_COLOR,
        ..default()
    });
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
        pelvis_height: library.pelvis_height(),
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

/// The world [`Transform`] of a rigged avatar body root: the object's position
/// and orientation carried into Bevy's Y-up world by the Second Life → Bevy
/// basis change, lowered by the pelvis rest height so the pelvis sits at the
/// reported object position (Second Life reports an avatar near its pelvis).
///
/// `shoe_lift` (R17) is the extra height the worn shoes' heel / platform add to
/// the pelvis-to-foot distance: subtracting it too raises the whole body so its
/// shod feet still rest on the ground rather than sinking in.
fn body_root_transform(object: &Object, pelvis_height: f32, shoe_lift: f32) -> Transform {
    let translation = sl_to_bevy_vec(&object.motion.position);
    Transform {
        // Per-component subtract to avoid the `arithmetic_side_effects` lint on
        // the glam `Vec3` operator.
        translation: Vec3::new(
            translation.x,
            translation.y - pelvis_height - shoe_lift,
            translation.z,
        ),
        rotation: sl_to_bevy_object_rotation(&object.motion.rotation),
        scale: Vec3::ONE,
    }
}

/// The extra vertical plant height (R17), in Second Life Z-up metres, a set of
/// resolved skeletal deformations gives an avatar from its worn shoes: the
/// reference viewer's `computeBodySize` folds the shoe params' downward foot-bone
/// offset into `mPelvisToFoot` (the `Shoe_Heels` id 197 / `Shoe_Platform` id 502
/// `param_skeleton`s offset `mFootLeft` / `mFootRight` by a negative Z, scaled by
/// the ankle), raising the avatar so its shod feet rest on the ground.
///
/// Taken from the left foot (the sides are symmetric); a shoe only ever raises the
/// avatar, so a non-negative result is used and any spurious lowering is ignored.
fn shoe_lift(deform: &SkeletalDeformations) -> f32 {
    let foot_offset_z = deform.offset(FOOT_JOINT)[2];
    let ankle_scale_z = 1.0 + deform.scale(ANKLE_JOINT)[2];
    (-foot_offset_z * ankle_scale_z).max(0.0)
}

/// The skeleton bone the shoe heel / platform params offset downward (R17).
const FOOT_JOINT: &str = "mFootLeft";

/// The skeleton bone whose scale the shoe foot offset is measured through (R17),
/// matching the reference viewer's `mPelvisToFoot` `- foot.z * ankle_scale.z`.
const ANKLE_JOINT: &str = "mAnkleLeft";

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
    material: &Handle<StandardMaterial>,
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

/// The placeholder material (opaque soft blue).
fn placeholder_material() -> StandardMaterial {
    StandardMaterial {
        base_color: AVATAR_COLOR,
        ..default()
    }
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
        materials: &mut Assets<StandardMaterial>,
    ) -> (Handle<Mesh>, Handle<StandardMaterial>) {
        let built = assets.get_or_insert_with(|| AvatarAssets {
            mesh: meshes.add(placeholder_sphere_mesh()),
            material: materials.add(placeholder_material()),
        });
        (built.mesh.clone(), built.material.clone())
    }

    /// The tag text for an agent: its resolved legacy name, or a provisional id
    /// fragment until the name arrives.
    fn label_text(&self, agent: AgentKey) -> String {
        self.names
            .get(&agent)
            .cloned()
            .unwrap_or_else(|| provisional_label(agent))
    }

    /// Spawn the floating name-tag text node for `agent`, anchored to `anchor`
    /// and floating `tag_height` metres above it.
    fn spawn_label(
        &self,
        agent: AgentKey,
        anchor: Entity,
        tag_height: f32,
        commands: &mut Commands,
    ) -> Entity {
        commands
            .spawn((
                Text::new(self.label_text(agent)),
                TextFont {
                    font_size: FontSize::Px(NAME_TAG_FONT_SIZE),
                    ..default()
                },
                TextColor(Color::WHITE),
                // Positioned each frame by `position_name_tags`; hidden until the
                // first projection so it never flashes at the origin.
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Visibility::Hidden,
                NameTag { anchor, tag_height },
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
        materials: &mut Assets<StandardMaterial>,
    ) -> AvatarEntities {
        let (mesh, material) = Self::asset_handles(&mut self.assets, meshes, materials);
        let sphere = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(translation),
                AvatarSphere,
                AvatarAnchor,
            ))
            .id();
        let label = self.spawn_label(agent, sphere, AVATAR_SPHERE_RADIUS + NAME_TAG_GAP, commands);
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
        commands: &mut Commands,
    ) -> (AvatarEntities, Vec<Entity>, HashMap<u8, Entity>) {
        let shoe_lift = self.pelvis_lift.get(&agent).copied().unwrap_or(0.0);
        let root = commands
            .spawn((
                body_root_transform(object, body.pelvis_height, shoe_lift),
                Visibility::default(),
                AvatarAnchor,
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
                    .spawn((point.offset, Visibility::default(), ChildOf(joint)))
                    .id();
                Some((point_id, node))
            })
            .collect();
        let label = self.spawn_label(agent, root, BODY_TAG_HEIGHT, commands);
        (
            AvatarEntities {
                anchor: root,
                label,
            },
            joints,
            attachment_nodes,
        )
    }

    /// Request the legacy name of `agent` once — skipped if it is already cached
    /// or already in flight.
    fn request_name(&mut self, agent: AgentKey, commands: &mut MessageWriter<SlCommand>) {
        if self.names.contains_key(&agent) || !self.requested.insert(agent) {
            return;
        }
        commands.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
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
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        writer: &mut MessageWriter<SlCommand>,
    ) {
        let agent = AgentKey::from(object.full_id.uuid());
        let scoped = object.scoped_id();
        // The authoritative motion the P31.4 dead-reckoner (`drive_avatar_motion`)
        // extrapolates between updates; re-inserted on every update so its change
        // detection reseeds the prediction. A rigged body root carries the object
        // rotation, a placeholder sphere does not.
        let avatar_motion = AvatarMotion::from_object(object, body.is_some());
        // A precise full object takes over from any coarse dot for this agent.
        if let Some(entities) = self.coarse.remove(&agent) {
            despawn_avatar(entities, commands);
        }
        self.coarse_region.remove(&agent);
        if let Some(existing) = self.objects.get(&agent) {
            // Move the existing anchor: a body root gets the full position +
            // orientation transform, a sphere just its translation.
            let transform = match body {
                Some(body) => {
                    let shoe_lift = self.pelvis_lift.get(&agent).copied().unwrap_or(0.0);
                    body_root_transform(object, body.pelvis_height, shoe_lift)
                }
                None => Transform::from_translation(sl_to_bevy_vec(&object.motion.position)),
            };
            commands
                .entity(existing.anchor)
                .insert((transform, avatar_motion));
            return;
        }
        self.request_name(agent, writer);
        let entities = match body {
            Some(body) => {
                let (entities, joints, attachment_nodes) =
                    self.spawn_body(agent, object, body, commands);
                // Record the joint entities and per-point attachment nodes so a
                // worn attachment can be parented at the right joint offset once it
                // arrives (P16.1/P16.2).
                self.joints.insert(agent, joints);
                self.attachment_nodes.insert(agent, attachment_nodes);
                entities
            }
            None => self.spawn_sphere(
                agent,
                sl_to_bevy_vec(&object.motion.position),
                commands,
                meshes,
                materials,
            ),
        };
        commands.entity(entities.anchor).insert(avatar_motion);
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
        self.clear_joint_overrides(agent);
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

    /// The resolved skeletal deformations the animation driver (P18.3) folds a
    /// playing motion into when recomputing each joint's world matrix, as last
    /// shaped by [`apply_avatar_appearance`]. `None` for an avatar with no rigged
    /// body, or before its first appearance.
    pub(crate) fn deformations(&self, agent: AgentKey) -> Option<&SkeletalDeformations> {
        self.deformations.get(&agent)
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
                  Commands / mesh / material / command-writer sinks to spawn spheres"
    )]
    fn apply_coarse(
        &mut self,
        region: RegionHandle,
        origin: Option<RegionHandle>,
        locations: &[CoarseLocation],
        you: Option<usize>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        writer: &mut MessageWriter<SlCommand>,
    ) {
        // The neighbour region's south-west corner relative to the scene origin, in
        // Second Life east/north metres (0 for the root region itself).
        let (region_x, region_y) = region.global_coordinates();
        let (origin_x, origin_y) = origin.unwrap_or(region).global_coordinates();
        let offset_east = metres_to_f32(region_x) - metres_to_f32(origin_x);
        let offset_north = metres_to_f32(region_y) - metres_to_f32(origin_y);
        let mut present: HashSet<AgentKey> = HashSet::new();
        for (index, location) in locations.iter().enumerate() {
            // The agent's own coarse dot is left to the (precise) object path.
            if Some(index) == you {
                continue;
            }
            let agent = location.agent_id;
            // A full-object avatar renders from its precise object position.
            if self.objects.contains_key(&agent) {
                continue;
            }
            present.insert(agent);
            if log_avatar_interest() {
                self.coarse_pos
                    .insert(agent, (location.x, location.y, location.z));
            }
            let translation = coarse_translation(location, offset_east, offset_north);
            if let Some(existing) = self.coarse.get(&agent) {
                commands
                    .entity(existing.anchor)
                    .insert(Transform::from_translation(translation));
            } else {
                self.request_name(agent, writer);
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
        }
    }

    /// Record a resolved legacy name and refresh the tag text of any avatar
    /// currently rendered for that agent.
    fn set_name(&mut self, name: &AvatarName, texts: &mut Query<&mut Text, With<NameTag>>) {
        let agent = name.id;
        let resolved = name.legacy_name();
        for map in [&self.objects, &self.coarse] {
            if let Some(entities) = map.get(&agent)
                && let Ok(mut text) = texts.get_mut(entities.label)
            {
                *text = Text::new(resolved.clone());
            }
        }
        debug!("resolved avatar name {agent} = {resolved:?}");
        self.names.insert(agent, resolved);
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
const fn bake_service_slot_name(slot: usize) -> Option<&'static str> {
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

/// Spawn / move / despawn the placeholder of every avatar the simulator streams
/// as a full in-world object (`pcode` 47), requesting each avatar's legacy name
/// once.
pub(crate) fn update_avatar_objects(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarState>,
    body: Option<Res<AvatarBody>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut writer: MessageWriter<SlCommand>,
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
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        &mut writer,
                    );
                }
            }
            SlSessionEvent::ObjectRemoved { local_id, .. } => {
                state.forget_object(*local_id);
                state.remove_object(*local_id, &mut commands);
            }
            _other => {}
        }
    }
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut writer: MessageWriter<SlCommand>,
) {
    let origin = identity.region_handle;
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
                locations,
                *you,
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut writer,
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
            .map_or("<unresolved>", String::as_str);
        let ever_object = state.ever_full_object.contains(agent);
        let pos = state.coarse_pos.get(agent);
        info!(
            "  sphere agent={agent} name={name:?} ever_full_object={ever_object} coarse_pos={pos:?}"
        );
    }
}

/// R22b diagnostic: when `SL_VIEWER_LOG_AVATAR_INTEREST=1`, append each avatar's
/// distance from the agent's own avatar to its floating name tag (e.g.
/// `"Kamaeri (152m)"`), refreshed twice a second. Lets a live run read off exactly
/// where a full-body avatar gives way to a coarse "blue sphere" — i.e. the radius the
/// simulator streams full avatar objects within, and whether flying the camera moves
/// that boundary. A no-op unless the flag is set (`apply_avatar_names` restores the
/// plain tag on the next name refresh once it is off).
pub(crate) fn annotate_avatar_distances(
    time: Res<Time>,
    mut next_at: Local<f32>,
    state: Res<AvatarState>,
    identity: Res<SlIdentity>,
    anchors: Query<&GlobalTransform, With<AvatarAnchor>>,
    mut texts: Query<&mut Text, With<NameTag>>,
) {
    if !log_avatar_interest() {
        return;
    }
    let now = time.elapsed_secs();
    if now < *next_at {
        return;
    }
    *next_at = now + 0.5;
    let Some(own_agent) = identity.agent_id else {
        return;
    };
    let Some(own_pos) = state
        .objects
        .get(&own_agent)
        .and_then(|own| anchors.get(own.anchor).ok())
        .map(GlobalTransform::translation)
    else {
        return;
    };
    for (agent, entities) in state.objects.iter().chain(state.coarse.iter()) {
        if *agent == own_agent {
            continue;
        }
        let Ok(pos) = anchors
            .get(entities.anchor)
            .map(GlobalTransform::translation)
        else {
            continue;
        };
        let distance = own_pos.distance(pos);
        let name = state
            .names
            .get(agent)
            .cloned()
            .unwrap_or_else(|| provisional_label(*agent));
        if let Ok(mut text) = texts.get_mut(entities.label) {
            *text = Text::new(format!("{name} ({distance:.0}m)"));
        }
    }
}

/// Fold resolved legacy names (`UUIDNameReply`) into the name cache and refresh
/// the tag text of any avatar already on screen.
pub(crate) fn apply_avatar_names(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AvatarState>,
    mut texts: Query<&mut Text, With<NameTag>>,
) {
    for event in events.read() {
        if let SlSessionEvent::AvatarNames(names) = &event.0 {
            for name in names {
                state.set_name(name, &mut texts);
            }
        }
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
    materials: HashMap<(AgentKey, usize), Handle<StandardMaterial>>,
    /// Region materials parked on a not-yet-decoded baked texture id, filled by
    /// [`apply_avatar_bake_textures`] once it decodes.
    pending: HashMap<TextureKey, Vec<Handle<StandardMaterial>>>,
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
        let alpha = classify_bake_alpha(decoded.components, &decoded.pixels);
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
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let handle = self
            .materials
            .entry((agent, slot))
            .or_insert_with(|| materials.add(baked_region_material()))
            .clone();
        match self.ensure_bake(id, manager, images) {
            Some((image, alpha)) => {
                if let Some(mut material) = materials.get_mut(&handle) {
                    apply_bake_image(&mut material, image, alpha.alpha_mode());
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
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let handle = self
            .materials
            .entry((agent, slot))
            .or_insert_with(|| materials.add(baked_region_material()))
            .clone();
        if let Some(mut material) = materials.get_mut(&handle) {
            apply_bake_image(&mut material, image, alpha.alpha_mode());
        }
        handle
    }
}

/// The un-textured base material for a body region: the skin tint as a fallback
/// until the baked texture decodes and is draped over it (P14.2). Opaque until a
/// bake with alpha overrides it; once the bake fills `base_color_texture`,
/// [`apply_bake_image`] resets the tint to white and sets the region's
/// [`AlphaMode`] from the bake's composited alpha (P14.3).
fn baked_region_material() -> StandardMaterial {
    StandardMaterial {
        base_color: BODY_COLOR,
        perceptual_roughness: 0.9,
        // Single-sided, matching the prim / base-body surfaces: Second Life
        // renders a face only from its front.
        ..default()
    }
}

/// The initial material for a bake-on-mesh face (R22): each BoM face owns its
/// material (rather than sharing the region's) so [`apply_bom_face_materials`] can
/// give it the reference viewer's per-face tint / blend / hide on the sampled
/// bake. Until the wearer's bake resolves it shows the neutral
/// [`BOM_FALLBACK_COLOR`] (matching `IMG_DEFAULT`), multiplied by the face `tint`
/// alpha and placed by its `uv` transform. A fully-transparent tint is hidden by
/// visibility, not material, so its base colour is left neutral.
pub(crate) fn bom_face_material(tint: [u8; 4], uv: Affine2) -> StandardMaterial {
    StandardMaterial {
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
fn classify_bake_alpha(components: u16, pixels: &[u8]) -> BakeAlpha {
    // No alpha channel: the decoder filled alpha to fully opaque.
    if components < 4 {
        return BakeAlpha::Opaque;
    }
    let mut any_kept = false;
    let mut any_carved = false;
    for &alpha in pixels.iter().skip(3).step_by(4) {
        if alpha < BAKE_ALPHA_CUTOFF {
            any_carved = true;
        } else {
            any_kept = true;
        }
        // Once both kinds are seen the region is masked; stop scanning.
        if any_kept && any_carved {
            return BakeAlpha::Masked;
        }
    }
    match (any_kept, any_carved) {
        // Nothing carved (or no pixels at all) → opaque.
        (_, false) => BakeAlpha::Opaque,
        // Every pixel carved → wholly transparent.
        (false, true) => BakeAlpha::Transparent,
        // A mix is returned inside the loop; kept here for totality.
        (true, true) => BakeAlpha::Masked,
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    added: Query<&AvatarBodyPart, Added<AvatarBodyPart>>,
    mut parts: Query<(&AvatarBodyPart, &mut MeshMaterial3d<StandardMaterial>)>,
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
    mut materials: ResMut<Assets<StandardMaterial>>,
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
                apply_bake_image(&mut material, image.clone(), alpha.alpha_mode());
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
    /// Whether the composite has been built from the ready bake inputs (a one-shot
    /// per session — the worn outfit does not change in this passive viewer).
    built: bool,
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
    let layers = inputs.region_layers(region);
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
/// uploaded [`Image`] + alpha classification per baked slot: composite each bake
/// region ([`composite_own_region`]), classify the composited alpha (so an alpha
/// wearable carved into the bake renders masked, P14.3), and upload the RGBA to a
/// Bevy [`Image`]. A region with no worn layers is skipped.
fn build_local_bake(
    inputs: &OwnBakeInputs,
    images: &mut Assets<Image>,
) -> HashMap<usize, (Handle<Image>, BakeAlpha)> {
    let mut regions = HashMap::new();
    let mut summary: Vec<String> = Vec::new();
    for region in BakeRegion::ALL {
        let Some(decoded) = composite_own_region(inputs, region) else {
            continue;
        };
        let alpha = classify_bake_alpha(decoded.components, &decoded.pixels);
        let handle = images.add(to_bevy_image(&decoded));
        let _prev = regions.insert(region.slot(), (handle, alpha));
        summary.push(format!(
            "{}={} layer(s)/{alpha:?}",
            region.name(),
            inputs.region_layers(region).len()
        ));
    }
    info!(
        "composited client-side bake for own avatar: {}",
        summary.join(" ")
    );
    regions
}

/// Drape our own avatar's locally composited client-side bake (P15.3) over its
/// body regions when the grid publishes no server bake for us (OpenSim).
///
/// Once the bake inputs are assembled ([`OwnBakeInputs::is_ready`]) the composite
/// is built once ([`build_local_bake`]) and, for each of our own body parts whose
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut parts: Query<(&AvatarBodyPart, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    // Nothing to drape until the body assets loaded, the bake inputs are ready,
    // and we know which agent is our own avatar.
    if body.is_none() || !inputs.is_ready() {
        return;
    }
    let Some(agent) = identity.agent_id else {
        return;
    };
    if !local.built {
        local.regions = build_local_bake(&inputs, &mut images);
        local.built = true;
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
    mut meshes: ResMut<Assets<Mesh>>,
    added: Query<&AvatarBodyPart, Added<AvatarBodyPart>>,
    mut parts: Query<(&AvatarBodyPart, &mut Mesh3d)>,
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
    if state.appearance_dirty.is_empty() {
        return;
    }
    let Some(library) = library else {
        state.appearance_dirty.clear();
        return;
    };
    // Resolve each dirty avatar's appearance once into its morph weights and the
    // deformed joint transforms (both share one `ResolvedParams`).
    let log_geometry = std::env::var_os("SL_VIEWER_LOG_AVATAR_GEOMETRY").is_some();
    let mut morph_weights: HashMap<AgentKey, MorphWeights> = HashMap::new();
    let mut joint_transforms: HashMap<AgentKey, Vec<Transform>> = HashMap::new();
    let mut deformations: HashMap<AgentKey, SkeletalDeformations> = HashMap::new();
    // The rest deformed joint **world** matrices per avatar, kept only for the
    // geometry diagnostic (R13) so it can reproduce the GPU skinning on the CPU.
    let mut world_matrices: HashMap<AgentKey, Vec<Mat4>> = HashMap::new();
    for &agent in &state.appearance_dirty {
        if let Some(bytes) = state.appearances.get(&agent) {
            let resolved = ResolvedParams::from_appearance(library.params(), bytes);
            morph_weights.insert(
                agent,
                MorphWeights::from_resolved(library.params(), &resolved),
            );
            let deform = SkeletalDeformations::from_resolved(library.params(), &resolved);
            // Fold in the worn rigged meshes' joint position overrides (R1) so a
            // fitted mesh body/head poses the skeleton to the positions its
            // inverse-bind matrices were baked against, rather than the plain shape.
            let overrides = state.effective_joint_overrides(agent).unwrap_or_default();
            joint_transforms.insert(
                agent,
                library
                    .skeleton()
                    .deformed_local_transforms_with(&deform, &overrides),
            );
            if log_geometry {
                world_matrices.insert(
                    agent,
                    library.skeleton().deformed_world_matrices(
                        &deform,
                        &overrides,
                        &AnimationPose::default(),
                    ),
                );
            }
            deformations.insert(agent, deform);
        }
    }
    // Record each avatar's resolved deformations so the animation driver (P18.3)
    // can re-run the skeletal recurrence with the playing motion folded in, and
    // fold the worn shoes' heel / platform height into the body's plant height
    // (R17): the shoe raises the pelvis-to-foot distance, so an already-spawned
    // (possibly stationary) body is re-planted straight away rather than waiting
    // for its next position update.
    for (agent, deform) in deformations {
        let lift = shoe_lift(&deform);
        let previous = state.pelvis_lift.insert(agent, lift).unwrap_or(0.0);
        if (lift - previous).abs() > f32::EPSILON
            && let Some(entities) = state.objects.get(&agent)
            && let Ok(mut transform) = anchors.get_mut(entities.anchor)
        {
            transform.translation.y -= lift - previous;
        }
        let _prev = state.deformations.insert(agent, deform);
    }
    // Rebuild the mesh of every part belonging to a resolved avatar, masking its
    // clothing morphs by the region's decoded bake where one is available (P14.5).
    let mut morphed_parts = 0_usize;
    for (part, mut mesh) in &mut parts {
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
            *mesh = Mesh3d(meshes.add(to_bevy_morphed_mesh(&loaded.mesh, &morphed)));
            morphed_parts = morphed_parts.saturating_add(1);
        }
    }
    // Re-set every joint transform of a resolved avatar's skeleton instance.
    let mut deformed_joints = 0_usize;
    for (joint, mut transform) in &mut joints {
        if let Some(transforms) = joint_transforms.get(&joint.agent)
            && let Some(deformed) = transforms.get(joint.index)
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
    state.appearance_dirty.clear();
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    parts: Query<(&AvatarBodyPart, &MeshMaterial3d<StandardMaterial>), Without<BomFace>>,
    mut faces: Query<
        (&BomFace, &MeshMaterial3d<StandardMaterial>, &mut Visibility),
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
    let mut part_materials: HashMap<(AgentKey, usize), Handle<StandardMaterial>> = HashMap::new();
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
            && let Some(image) = material.base_color_texture.clone()
        {
            let has_alpha = !matches!(material.alpha_mode, AlphaMode::Opaque);
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
        // A mesh-body BoM face renders **opaque**, ignoring the bake's composited
        // alpha channel. The reference proves this: a 5-channel server bake never
        // satisfies `getPoolTypeFromTE`'s `getComponents()==4` alpha test, and with
        // an opaque tint and no material the face has no renderable alpha — so it is
        // batched into `sSimpleFaces` (`llvovolume.cpp`), the opaque simple pass,
        // which does not alpha-test. The bake's alpha carves the *system* avatar
        // body (and drives region hiding), not this attachment. Applying it here is
        // what made bare skin see-through (R22d) and cut UV-seam rings into the arm.
        // The per-face TE tint still applies: a non-opaque tint blends (the
        // reference's `color_alpha` → alpha pool); a fully-transparent tint is
        // hidden by visibility above.
        let alpha_mode = if face.tint[3] < 255 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        };
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
            match region_bake.get(&(face.agent, face.slot)) {
                Some((image, _bake_alpha)) => (Some(image.clone()), tint_color(face.tint)),
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
            current.base_color_texture == texture
                && current.base_color == base_color
                && current.alpha_mode == alpha_mode
                && current.uv_transform == face.uv
        });
        if up_to_date {
            continue;
        }
        let Some(mut material) = materials.get_mut(&material.0) else {
            continue;
        };
        material.base_color_texture = texture;
        material.base_color = base_color;
        material.alpha_mode = alpha_mode;
        material.uv_transform = face.uv;
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

/// Position each avatar name tag over its anchor by projecting the anchor's world
/// position (offset up by the tag's own height) to the screen and anchoring the
/// tag's *bottom-centre* on that point, so the text is centred over the avatar
/// and floats just above it; tags whose anchor is off-screen or behind the camera
/// are hidden.
///
/// The projection ([`Camera::world_to_viewport`](sl_client_bevy::Camera)) and the
/// UI `Val::Px` layout are both in logical pixels, but [`ComputedNode::size`] is
/// physical, so the tag's own size is scaled by its
/// [`inverse_scale_factor`](ComputedNode::inverse_scale_factor) before centring.
pub(crate) fn position_name_tags(
    cameras: Query<(&Camera, &GlobalTransform)>,
    anchors: Query<&GlobalTransform, With<AvatarAnchor>>,
    mut tags: Query<(&NameTag, &ComputedNode, &mut Node, &mut Visibility)>,
) {
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    for (tag, computed, mut node, mut visibility) in &mut tags {
        let Ok(anchor) = anchors.get(tag.anchor) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let base = anchor.translation();
        // Float the tag just above the avatar's head (per-component add to avoid
        // the `arithmetic_side_effects` lint on the glam `Vec3` operator).
        let head = Vec3::new(base.x, base.y + tag.tag_height, base.z);
        match camera.world_to_viewport(camera_transform, head) {
            Ok(screen) => {
                // The tag's own logical size, to anchor its bottom-centre on the
                // projected head point (previous frame's layout — one-frame lag is
                // imperceptible; a just-spawned tag has zero size for one frame).
                let size = computed.size();
                let inverse_scale = computed.inverse_scale_factor();
                let half_width = size.x * inverse_scale / 2.0;
                let height = size.y * inverse_scale;
                node.left = Val::Px(screen.x - half_width);
                node.top = Val::Px(screen.y - height);
                *visibility = Visibility::Inherited;
            }
            Err(_off_screen) => {
                *visibility = Visibility::Hidden;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AvatarState, BakeAlpha, PROVISIONAL_ID_CHARS, body_root_transform, classify_bake_alpha,
        coarse_translation, invisible_body_slots, provisional_label, should_refetch_bakes,
        used_baked_slots, visible_body_bakes,
    };
    use crate::avatar_assets::BodyRegion;
    use crate::coords::sl_to_bevy_rotation;
    use bevy::math::Vec3;
    use bevy::prelude::AlphaMode;
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
    /// swap and lowers it by the pelvis rest height, so the skeleton's pelvis
    /// (at that height) lands back at the reported position; with an identity
    /// facing rotation, the root carries just the basis change.
    #[test]
    fn body_root_plants_pelvis_at_the_object_position() {
        let pelvis_height = 1.067;
        let object = avatar_object_at(Vector {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        });
        // No worn shoes, so no extra plant height (R17).
        let transform = body_root_transform(&object, pelvis_height, 0.0);
        // Second Life (10, 20, 30) → Bevy (10, 30, -20), then lowered in Y by the
        // pelvis height.
        assert_eq!(
            transform.translation,
            Vec3::new(10.0, 30.0 - pelvis_height, -20.0)
        );
        // An identity object rotation leaves only the basis change at the root.
        assert!(
            transform
                .rotation
                .abs_diff_eq(sl_to_bevy_rotation(), 1.0e-6)
        );
    }

    /// A worn shoe raises the avatar (R17): a `Shoe_Heels`-style `param_skeleton`
    /// offsetting `mFootLeft` / `mFootRight` downward at full weight yields a
    /// positive plant lift equal to that downward offset, while no shoe yields
    /// none.
    #[test]
    fn shoe_offset_lifts_the_body() -> Result<(), Box<dyn core::error::Error>> {
        use sl_client_bevy::{SkeletalDeformations, VisualParams};
        // A transmitted shoe-heel param (id 0) offsetting the foot bones down by
        // 0.08 m at full weight, mirroring `avatar_lad.xml`'s `Shoe_Heels`.
        let lad = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <skeleton file_name="avatar_skeleton.xml">
    <param id="0" group="0" name="Shoe_Heels" value_min="0" value_max="1" value_default="0">
      <param_skeleton>
        <bone name="mFootLeft" scale="0 0 0" offset="0 0 -0.08"/>
        <bone name="mFootRight" scale="0 0 0" offset="0 0 -0.08"/>
      </param_skeleton>
    </param>
  </skeleton>
</linden_avatar>"#;
        let params = VisualParams::from_xml(lad)?;
        // Full heel → planted 0.08 m higher.
        let full = SkeletalDeformations::from_appearance(&params, &[255]);
        assert!((super::shoe_lift(&full) - 0.08).abs() < 1.0e-4);
        // No heel → no lift.
        let none = SkeletalDeformations::from_appearance(&params, &[0]);
        assert!(super::shoe_lift(&none).abs() < 1.0e-6);
        Ok(())
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
        // No alpha channel (RGB source): opaque regardless of the filled byte.
        assert_eq!(classify_bake_alpha(3, &[10, 20, 30, 0]), BakeAlpha::Opaque);
        // Every alpha at/above the cutoff → opaque.
        assert_eq!(
            classify_bake_alpha(4, &[0, 0, 0, 255, 1, 1, 1, 200]),
            BakeAlpha::Opaque
        );
        // Every alpha below the cutoff → wholly transparent (hide the region).
        assert_eq!(
            classify_bake_alpha(4, &[9, 9, 9, 0, 9, 9, 9, 10]),
            BakeAlpha::Transparent
        );
        // A mix of kept and carved pixels → masked.
        assert_eq!(
            classify_bake_alpha(4, &[9, 9, 9, 255, 9, 9, 9, 0]),
            BakeAlpha::Masked
        );
        // The cutoff is the reference `sMinimumAlpha` (0.2 → 51): a pixel at alpha
        // 60 is *kept* (opaque), where the old 0.5 cutoff (128) would have carved it
        // — which is what stopped bare mesh-body skin rendering see-through (R22d).
        assert_eq!(
            classify_bake_alpha(4, &[0, 0, 0, 60, 1, 1, 1, 255]),
            BakeAlpha::Opaque
        );
        // A pixel just below the cutoff (40 < 51) still carves, so it masks.
        assert_eq!(
            classify_bake_alpha(4, &[0, 0, 0, 40, 1, 1, 1, 255]),
            BakeAlpha::Masked
        );
        // No pixels at all → opaque (nothing is carved away).
        assert_eq!(classify_bake_alpha(4, &[]), BakeAlpha::Opaque);
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
