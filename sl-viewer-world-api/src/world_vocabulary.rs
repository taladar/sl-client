//! Small world vocabulary the world's own layers share.
//!
//! Which render layer the HUD draws on and whether an entity sits on it, the
//! markers on an avatar's anchor and its pick target, a region's terrain
//! surface, the name-tag render layers, and what a click on a media prim
//! carries. Each is named by parts of the world that do not otherwise know
//! about each other, and none of them names anything back.

use std::collections::{HashMap, HashSet};

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, AvatarName, BodyPhysics, DisplayName, JointOverrides, MAX_FACES, Object, ObjectKey,
    PrimFaceId, RegionHandle, ScopedObjectId, SkeletalDeformations, TextureKey, Uuid,
    VolumeDeformations, avatar_texture, decode_texture_entry,
};

/// The render layer the whole HUD subtree lives on, and which the world (fly)
/// camera — on the default layer `0` — therefore does not render. P35.2's HUD
/// camera renders this layer and nothing else, so the HUD is drawn exactly once,
/// in screen space, and never leaks into the world pass (or into a reflection
/// probe's capture, which is likewise a default-layer camera).
pub const HUD_RENDER_LAYER: usize = 1;

/// Whether an entity's render layers put it on the HUD layer — i.e. whether it is
/// part of the HUD subtree rather than the world scene.
///
/// The HUD screen propagates `HUD_RENDER_LAYER` down its hierarchy, so every
/// entity of a routed HUD attachment (its object entity, its geometry holder, and
/// each face) carries it. The world's pixel-area render-priority / level-of-detail
/// pass uses this to recognise geometry it must not rank by on-screen size: a HUD
/// sits in its own space, where the world camera's distance to it is meaningless
/// (the reference viewer special-cases it the same way, treating every HUD face as
/// full-screen and pinning it to the finest level of detail).
///
/// `layers` is the entity's [`RenderLayers`] component, absent on a world entity
/// (which is then implicitly on the default layer `0`).
#[must_use]
pub fn on_hud_layer(layers: Option<&RenderLayers>) -> bool {
    layers.is_some_and(|layers| layers.intersects(&RenderLayers::layer(HUD_RENDER_LAYER)))
}

/// A marker component on the transform-bearing *anchor* entity of an avatar —
/// its placeholder sphere or the root of its rigged body — whose world position
/// the name-tag placement (`name_tag_billboard::follow_tag_anchors`)
/// follows to float the tag.
#[derive(Component, Debug, Clone, Copy)]
pub struct AvatarAnchor;

/// A component tagging an entity as **part of** a specific avatar, carrying that
/// avatar's [`AgentKey`] — the reusable "what avatar is this?" identity that
/// picking reads.
///
/// It sits on every pickable piece of an avatar: the placeholder sphere, each
/// rigged base-body part, each **worn rigged-mesh submesh** (on a modern
/// mesh-body avatar the base body is hidden, so the worn mesh *is* the
/// silhouette), and the floating name tag. That breadth is the point — a ray
/// that hits any body part, or a pointer over the name tag (resolved by the
/// `name_tag_billboard::NameTagHitTest` rect test — tags are custom
/// billboard meshes no picking backend covers), resolves to the
/// same agent through one component, so a caller never has to know *which* piece
/// it hit. Kept separate from `AvatarBodyPart` (which also holds an agent) so
/// non-mesh pieces (the sphere, the name tag) can carry the identity too, and
/// so consumers — the GPU pick-tag assignment
/// (`crate::gpu_pick::assign_avatar_pick_tags`) is the main one — read a
/// single, purpose-named component rather than three different markers.
#[derive(Component, Debug, Clone, Copy)]
pub struct AvatarPickTarget {
    /// The avatar this entity is part of.
    pub agent: AgentKey,
}

impl AvatarPickTarget {
    /// Tag a pickable piece of `agent` (used by the rigged-attachment spawn in
    /// `objects`, where the wearer is known only sometimes).
    #[must_use]
    pub const fn new(agent: AgentKey) -> Self {
        Self { agent }
    }

    /// The avatar this entity belongs to.
    #[must_use]
    pub const fn agent(&self) -> AgentKey {
        self.agent
    }
}

/// Marks a rendered land-patch entity as a **walkable ground surface**, so the
/// avatar ground probe (`ground`, P31.14) can accept it as something the
/// feet may plant on — the same role the reference viewer's
/// `LLWorld::resolveStepHeightGlobal` gives the land when its object raycast misses.
///
/// The probe only ever accepts geometry that is explicitly ground-like (this, and
/// object faces), so it never plants an avatar's feet on the water plane, a particle
/// billboard, the sky dome, or another avatar.
#[derive(Debug, Component)]
pub struct TerrainSurface;

/// One media face: the object (grid-wide key) and the Linden face index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaTarget {
    /// The object carrying the face.
    pub object: ObjectKey,
    /// The face index.
    pub face: PrimFaceId,
}

/// A left click on a media-capable prim face, claimed from the world touch
/// pick (`hud_pick::pick_and_touch`) before it becomes a touch.
#[derive(Message, Debug, Clone)]
pub struct MediaWorldClick {
    /// The face entity the ray struck.
    pub entity: Entity,
    /// The picked object's scoped id.
    pub scoped: ScopedObjectId,
    /// The struck face.
    pub face: PrimFaceId,
    /// The **sampled** texture coordinate of the hit (the `SurfaceInfo` UV:
    /// texture placement applied, Second Life bottom-up `v`).
    pub uv: Vec2,
}

/// The in-world media focus / hover state. Read by
/// `crate::input_context::compute_input_context` (a focused media face
/// takes the keyboard away from the world) and by the floating controls bar
/// (`crate::media_controls`).
#[derive(Resource, Debug, Default)]
pub struct MediaFocus {
    /// The face holding media keyboard focus, if any.
    pub focused: Option<MediaTarget>,
    /// Whether the focused face is a browser page that takes the keyboard
    /// away from the world (`input_context`); a focused *video*
    /// face keeps the bar visible but leaves the keyboard with the world —
    /// there is nothing to type at a video.
    pub focused_takes_keyboard: bool,
    /// The media face under the cursor this frame, if any.
    pub hover: Option<MediaTarget>,
    /// The surface pixel under the cursor on the hover face.
    pub hover_pixel: Option<(i32, i32)>,
    /// The world-space face normal at the **last** media hover hit (not
    /// cleared when the hover leaves), for the controls bar's camera zoom.
    pub hover_normal: Option<Vec3>,
    /// Whether a forwarded button press is outstanding (its release is
    /// forwarded to the same surface).
    pub pressed: Option<MediaTarget>,
}

/// How many leading hex characters of the agent id to show as a provisional tag
/// before the real name resolves.
const PROVISIONAL_ID_CHARS: usize = 8;

/// The user's **own** naming of a resident, overriding what the grid answers:
/// a contact-set pseudonym or display-name removal
/// (`crate::contact_sets`, `viewer-contact-set-pseudonyms`).
///
/// It is mirrored into the name cache rather than consulted beside it, so every
/// surface that resolves a name through a [`NameRecord`] — name tags, the radar,
/// tooltips, the inspectors, linkified names — shows the alias without knowing
/// that contact sets exist. The grid's own answer is never overwritten; it stays
/// in the record's fields and [`NameRecord::grid_name`] returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameAlias {
    /// Show this text in place of the resident's name. It carries the
    /// reference's **quoted** form (`'Nickname'`), which is what keeps an alias
    /// from being read as the grid's own answer.
    Pseudonym(String),
    /// Show this resident's legacy name only — the reference's display-name
    /// removal (`hasDisplayNameRemoved`), for someone whose chosen display name
    /// the user would rather not see.
    LegacyOnly,
}

/// One agent's resolved names, merged from every source: the instant
/// `ObjectUpdate` NameValue seed, the legacy `UUIDNameReply`, and the
/// `GetDisplayNames` cap (SL only — OpenSim generally lacks the cap, so the
/// legacy fields must always work on their own) — plus the user's own
/// [alias](NameAlias) for them, if they gave one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameRecord {
    /// The legacy `"First Last"` name (`"First"` alone for a single-name
    /// account), from whichever source arrived first.
    pub legacy: Option<String>,
    /// The immutable dotted SLID (`"first.last"`), display-name cap only.
    pub username: Option<String>,
    /// The chosen display name, display-name cap only (`None` on OpenSim).
    pub display_name: Option<String>,
    /// Whether the display name is just the legacy-derived default (a custom
    /// display name shows with the username line under it, the reference's
    /// `is_display_name_default` behaviour).
    pub is_display_name_default: bool,
    /// The user's own name for this resident, mirrored from the contact-set
    /// store by `crate::contact_sets::apply_name_aliases`. Not a grid answer,
    /// and never written by an ingest path.
    pub alias: Option<NameAlias>,
}

impl NameRecord {
    /// The name to show for this resident: the user's own alias when they gave
    /// one, else the display name when one resolved, else the legacy name.
    #[must_use]
    pub fn preferred_name(&self) -> Option<&str> {
        match self.alias {
            Some(NameAlias::Pseudonym(ref shown)) => Some(shown),
            Some(NameAlias::LegacyOnly) => self.legacy.as_deref(),
            None => self.grid_name(),
        }
    }

    /// [`Self::preferred_name`] with display names switched off (the name tags'
    /// `ShowDisplayNames`): the legacy name, but a **pseudonym still wins** —
    /// the toggle says which of the grid's two names to believe, and an alias is
    /// not one of the grid's answers.
    #[must_use]
    pub fn legacy_display_name(&self) -> Option<&str> {
        match self.alias {
            Some(NameAlias::Pseudonym(ref shown)) => Some(shown),
            Some(NameAlias::LegacyOnly) | None => self.legacy.as_deref(),
        }
    }

    /// The **grid's** own answer, with no alias applied — what a name filed in
    /// a store must remember, and what an action that names someone on the wire
    /// has to carry. A drawn name is not this: the reference folds the
    /// pseudonym into the name cache itself, so every surface that shows a
    /// resident — the profile included — reads [`Self::preferred_name`].
    #[must_use]
    pub fn grid_name(&self) -> Option<&str> {
        self.display_name.as_deref().or(self.legacy.as_deref())
    }

    /// Whether the shown name is something other than this resident's legacy
    /// name — a custom display name, or a pseudonym. It is what puts the
    /// username line under a name tag: the shown name does not say who this is,
    /// so the username has to.
    #[must_use]
    pub const fn has_custom_display_name(&self) -> bool {
        match self.alias {
            Some(NameAlias::Pseudonym(_)) => true,
            // The point of display-name removal is to be shown the legacy name,
            // which is the one name that needs no username under it.
            Some(NameAlias::LegacyOnly) => false,
            None => self.display_name.is_some() && !self.is_display_name_default,
        }
    }
}

/// One nearby avatar as the map surfaces (minimap, radar) consume it — see
/// [`AvatarState::map_avatars`].
#[derive(Debug, Clone, Copy)]
pub struct MapAvatar {
    /// The avatar's agent id.
    pub agent: AgentKey,
    /// The world entity whose transform places the avatar.
    pub anchor: Entity,
    /// For a coarse-only avatar, its last coarse altitude in metres (`0` /
    /// `1020` are the "unknown" sentinels); `None` for a precisely-known
    /// full-object avatar.
    pub coarse_z: Option<f32>,
}

/// The pair of entities rendering one avatar: its world-space anchor (a
/// placeholder sphere or the root of a rigged body) and its screen-space
/// name-tag text node.
#[derive(Debug, Clone, Copy)]
pub struct AvatarEntities {
    /// The anchor entity — a placeholder sphere or a rigged-body root. Despawned
    /// recursively, so a body's whole joint / mesh sub-hierarchy goes with it.
    pub anchor: Entity,
    /// The floating name-tag UI text entity.
    pub label: Entity,
}

/// Viewer-side avatar bookkeeping: the placeholder entities for every nearby
/// avatar, split by which stream it came from, plus a legacy-name cache.
///
/// A full-object avatar's `ObjectRemoved` carries only its scoped local id (not
/// its agent id), so `by_scoped` maps back to the agent id the
/// avatar is keyed by.
///
/// Everything here is **data about avatars**: entity ids, names, positions,
/// flags. The render machinery that produces it — the placeholder sphere's mesh
/// and material handles, the rigged base body, the texture fetches — stays in
/// the world layer (`avatars::AvatarPlaceholderAssets` and the free functions
/// beside it), which is what lets the bookkeeping sit below every surface that
/// reads it: the radar, the minimap, the name tags, the pickers, the profiles.
#[derive(Debug, Resource, Default)]
pub struct AvatarState {
    /// The region the Bevy scene is anchored at (origin `<0,0,0>`), so a **full
    /// object** avatar in a neighbour region is offset onto the right terrain
    /// (mirroring the coarse-dot and object offsets) and every avatar is re-based
    /// when this moves (`recenter_avatars`). `None` until the first region is
    /// known; kept in lockstep with the object/terrain origins (all follow
    /// `SlIdentity`'s root handle).
    pub origin: Option<RegionHandle>,
    /// Avatars known as a full in-world object (`pcode` 47), keyed by agent id;
    /// their sphere follows the object's precise position.
    pub objects: HashMap<AgentKey, AvatarEntities>,
    /// Avatars known only from coarse (minimap) locations — not (currently) a full
    /// object — keyed by agent id; their sphere sits at the 1 m coarse position.
    pub coarse: HashMap<AgentKey, AvatarEntities>,
    /// The source region of each coarse-only avatar (R24). `CoarseLocationUpdate`
    /// arrives per-region (root *and* each neighbour child circuit), so a coarse
    /// dot is reconciled only against its own region's update — a neighbour's
    /// update must not despawn the root region's dots. Also lets a region's dots be
    /// dropped when that region is disabled (an empty update for the region).
    pub coarse_region: HashMap<AgentKey, RegionHandle>,
    /// A reverse map from an object's scoped id to its agent id, so an
    /// `ObjectRemoved` can find the avatar to despawn.
    pub by_scoped: HashMap<ScopedObjectId, AgentKey>,
    /// The per-avatar attachment-point node entities, keyed by agent id then by
    /// raw attachment-point id (P16.2). Each node is a child of its skeleton joint
    /// carrying the fixed `avatar_lad.xml` offset; a worn attachment parents to the
    /// node for its point so it seats at the stored local offset from the joint.
    /// Absent for a sphere-only (no `--viewer-assets`) avatar.
    pub attachment_nodes: HashMap<AgentKey, HashMap<u8, Entity>>,
    /// The camera's head-focus socket entity per rigged avatar (Phase 4 §5.4):
    /// a root child the pose driver's socket writer places at the posed `mHead`
    /// joint each frame, so the camera holds the animated head without a head
    /// joint entity. Absent for a sphere-only avatar; despawned with the anchor.
    pub head_sockets: HashMap<AgentKey, Entity>,
    /// Resolved names, keyed by agent id — the "simple name cache" that keeps
    /// a repeatedly-seen avatar from being re-requested; merged from the
    /// NameValue seed, the legacy `UUIDNameReply` and the display-name cap.
    pub names: HashMap<AgentKey, NameRecord>,
    /// The user's own [aliases](NameAlias), mirrored from the contact-set store
    /// by `crate::contact_sets::apply_name_aliases`. Held beside the records
    /// (as well as folded into them) so an avatar seen *after* the alias was
    /// given still shows it: [`Self::name_entry`] folds it in as the record is
    /// created. Session state — the store is what persists.
    name_aliases: HashMap<AgentKey, NameAlias>,
    /// Bumped whenever the name cache is written — a record ingested, the
    /// aliases replaced. Read through [`names_revision`](Self::names_revision).
    names_revision: u64,
    /// Group titles from each avatar object's NameValue `Title` — the classic
    /// mechanism the reference reads for other avatars' tags. (The own
    /// avatar's fresher title comes from `ActiveGroupChanged` via
    /// `sl_viewer_social::GroupsModel`.)
    titles: HashMap<AgentKey, String>,
    /// Agents whose name has already been requested (but has not necessarily
    /// arrived), so the same request is never sent twice.
    requested: HashSet<AgentKey>,
    /// Agents queued for this frame's batched name request
    /// (`flush_name_requests`): one `UUIDNameRequest` **and** one
    /// `GetDisplayNames` cap call per frame, however many avatars appeared
    /// (each cap call costs an HTTP request; cap absence — OpenSim — is a
    /// silent no-op, which is why the legacy request always goes out too).
    pub pending_name_requests: HashSet<AgentKey>,
    /// The latest `AvatarAppearance.visual_params` byte vector per avatar, kept so
    /// a body spawned after (or re-spawned) can be morphed from the last known
    /// appearance (P13.3).
    pub appearances: HashMap<AgentKey, Vec<u8>>,
    /// Avatars whose rigged body needs its appearance (re)applied — its morphs
    /// re-blended and its skeleton re-deformed — set on a fresh appearance and on
    /// a newly spawned body, drained by `apply_avatar_appearance`.
    pub appearance_dirty: HashSet<AgentKey>,
    /// The debounce ledger behind [`appearance_dirty`](Self::appearance_dirty):
    /// per still-unserviced avatar, when (app elapsed seconds) it was first and
    /// last marked dirty. `apply_avatar_appearance` folds fresh marks in each
    /// frame and picks avatars from here under its per-frame budget — a
    /// never-shaped avatar immediately, a re-marked one only after a quiet
    /// window, so the appearance → body-spawn → bake-decode trigger cascade
    /// resolves once instead of once per trigger.
    pub appearance_pending: HashMap<AgentKey, AppearanceDirtyStamps>,
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
    pub skirt_visible: HashMap<AgentKey, bool>,
    /// Each avatar's ingested body-physics (`WT_PHYSICS`) configuration (P34.1),
    /// resolved from its latest appearance: the six breast / belly / butt
    /// spring-damper motions, their settings, and the runtime morph params each
    /// one drives. The per-frame simulation (P34.2) reads it; an avatar whose
    /// appearance switches physics off keeps an entry whose motions are all
    /// inactive.
    pub body_physics: HashMap<AgentKey, BodyPhysics>,
    /// The visible baked-texture id in each base-body region slot per avatar,
    /// from its latest appearance (P14.1): the published baked UUIDs the viewer
    /// fetches through the shared `TextureManager` and (from P14.2) drapes over
    /// the system body. Keyed by baked slot (`BODY_BAKE_SLOTS`); a slot with no
    /// real bake is simply absent.
    pub baked_textures: HashMap<AgentKey, HashMap<usize, TextureKey>>,
    /// The base-body region slots each avatar has baked **invisible**
    /// (`IMG_INVISIBLE`) via a worn system alpha layer, from its latest appearance
    /// (R22). These regions are hidden outright (`apply_avatar_part_visibility`),
    /// matching the reference viewer's `isTextureVisible`, so the system body does
    /// not render and z-fight a non-BOM mesh body worn over it.
    pub invisible_regions: HashMap<AgentKey, HashSet<usize>>,
    /// The Current Outfit Folder version whose bakes were last fetched per avatar
    /// (P14.4), so a later `AvatarAppearance` with a strictly-older `cof_version`
    /// (an out-of-order / duplicate resend) is skipped and cannot clobber a newer
    /// bake. Absent means none seen yet; an appearance with no `cof_version`
    /// (OpenSim / the older path) is always ingested.
    pub baked_cof_version: HashMap<AgentKey, i32>,
    /// Avatars whose body-region bake materials need (re)assigning — set on a
    /// fresh appearance and on a newly spawned body, drained by
    /// `assign_avatar_bake_materials` (P14.2).
    pub bake_dirty: HashSet<AgentKey>,
    /// The parent scoped id of every tracked non-root object (linkset children and
    /// attachments), so an attachment's chain can be chased up to its avatar root
    /// (P13.5 `IMG_USE_BAKED_*` region hide).
    pub object_parents: HashMap<ScopedObjectId, ScopedObjectId>,
    /// For every tracked non-root object whose texture entry carries
    /// `IMG_USE_BAKED_*` sentinels, the baked slots it replaces — aggregated up the
    /// attachment chain to hide the matching base-avatar mesh regions.
    pub baked_hides: HashMap<ScopedObjectId, Vec<usize>>,
    /// Non-root objects whose texture entry has already been scanned for
    /// `IMG_USE_BAKED_*` sentinels, so a motion-only update never re-decodes it.
    scanned_objects: HashSet<ScopedObjectId>,
    /// Each rigged avatar's resolved skeletal deformations, the shape
    /// `apply_avatar_appearance` last applied — kept so the animation driver
    /// (P18.3) can re-run the Second Life skeletal recurrence with the playing
    /// motion folded in and write each joint's world matrix straight to its
    /// `GlobalTransform` (avoiding the limb-shear a rotation overlaid onto the
    /// baked-scale rest transform would cause). Absent for a sphere-only
    /// (no `--viewer-assets`) avatar, or before its first appearance.
    pub deformations: HashMap<AgentKey, SkeletalDeformations>,
    /// Each rigged avatar's resolved **collision-volume** displacements (P34.3):
    /// the shape morphs' `<volume_morph>` children, which move the volumes a worn
    /// rigged-mesh body is rigged to. Resolved and folded into the skeletal
    /// recurrence alongside [`deformations`](Self::deformations).
    pub volume_deformations: HashMap<AgentKey, VolumeDeformations>,
    /// Each avatar's resolved **root drop** (R23): how far below the reported
    /// wire Z its body-root entity is planted, in Second Life Z-up metres —
    /// `root_drop_from_metrics` of the shape's `computeBodySize` quantities
    /// (the wire Z is the physics-capsule *centre*, so the drop is half the
    /// shape-scaled body height, corrected for the pelvis sitting above the
    /// root and any hover). Shoe heel / platform offsets (R17) fold in through
    /// the foot term of those metrics, as in the reference. Absent (the rest
    /// shape's `AvatarBody::rest_root_drop` applies) until an appearance
    /// resolves, or for a sphere-only avatar.
    pub root_drops: HashMap<AgentKey, f32>,
    /// Each avatar's resolved **seat drop** (R23 counterpart): the pelvis's
    /// shape-scaled local height above the body root (`pelvis_local_z`), keyed by
    /// agent. A sit offset targets the avatar **root** (hips), so a seated avatar's
    /// anchor is dropped by this so the hips land on the sit target
    /// (`place_seated_avatars`) — unlike the standing [`root_drops`](Self::root_drops),
    /// which also folds in the capsule-centre correction that does not apply while
    /// seated. Absent (the rest `AvatarBody::rest_seat_drop` applies, seeded on
    /// body spawn) until an appearance resolves, or for a sphere-only avatar.
    pub seat_drops: HashMap<AgentKey, f32>,
    /// R22b diagnostic: every agent the session has *ever* surfaced a full avatar
    /// object (`pcode` 47) for, so the `log_avatar_interest`-gated census can
    /// tell a "the simulator never streamed this avatar" case (agent absent here)
    /// from a "we received it but failed to render it" case (agent present here yet
    /// still a coarse sphere). Never pruned — it is a cumulative diagnostic marker.
    pub ever_full_object: HashSet<AgentKey>,
    /// The last coarse (minimap) position `(x, y, z)` seen per coarse-only
    /// agent — `x`/`y` region-local metres (0..255), `z` already in metres
    /// (0..1020, the `u8 × 4` coarse scale). A `z` at the 1020 ceiling is the
    /// simulator's "height unknown / off this region" sentinel; a `0` from some
    /// simulators means the same. Read by the R22b census diagnostic and by the
    /// minimap's dot layer (the unknown-altitude glyph).
    pub coarse_pos: HashMap<AgentKey, (u8, u8, u16)>,
    /// Avatars currently **seated on an object** (their full-object `ObjectUpdate`
    /// carries a non-zero `ParentID`), keyed by agent id — self and others alike
    /// (several avatars share one boat). The value is the seat and the avatar's
    /// pose **in the seat's frame** (the parent-relative wire transform, the
    /// `llSitTarget` offset): `place_seated_avatars` composes it onto the seat's
    /// live world transform each frame so the avatar rides the moving seat, and
    /// `drive_avatar_motion` leaves a
    /// [`Seated`] anchor alone (its motion is the seat's, not region dead-reckoned).
    /// Entries clear the instant an update arrives with `ParentID` zero (a stand).
    pub seated: HashMap<AgentKey, SeatedTarget>,
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
pub struct SeatedTarget {
    /// The seat object's scoped id — resolved to its scene entity through
    /// `ObjectState::entity_by_scoped`
    /// each frame (the seat may stream in after, or independently of, the avatar).
    pub seat: ScopedObjectId,
    /// The avatar's pose in the seat's local frame, as a pure-SL [`Transform`].
    pub offset: Transform,
}

/// Marker on a seated avatar's anchor: its world pose is driven by
/// `place_seated_avatars` from its seat, so the region-space dead-reckoner
/// (`drive_avatar_motion`) must leave it be.
#[derive(Component, Debug, Clone, Copy)]
pub struct Seated;

/// The maximum attachment/linkset depth chased when attributing an object's
/// `IMG_USE_BAKED_*` hide to its avatar, a guard against a malformed parent cycle.
const MAX_ATTACHMENT_DEPTH: usize = 32;

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
    /// How many times the name cache has been written — a record ingested, the
    /// aliases replaced.
    ///
    /// The signal a view that **draws resolved names** should rebuild on: it
    /// stores the value it resolved at and compares, the way the friends and
    /// groups models' `revision` is already used.
    ///
    /// This resource's own change tick will not do that job. `AvatarState` is
    /// written by every avatar that moves, streams in, changes appearance or is
    /// re-costed, so `Res<AvatarState>::is_changed()` is true on most frames in
    /// a crowded region and says nothing whatever about names — About Land
    /// rebuilt every owner and access row, a `translator.get()` and a `format!`
    /// apiece, many times a second while it was open.
    #[must_use]
    pub const fn names_revision(&self) -> u64 {
        self.names_revision
    }

    /// The tag text for an agent: its display name when resolved, else its
    /// legacy name, else a provisional id fragment until either arrives.
    pub fn label_text(&self, agent: AgentKey) -> String {
        self.names
            .get(&agent)
            .and_then(NameRecord::preferred_name)
            .map_or_else(|| provisional_label(agent), str::to_owned)
    }

    /// Every labelled avatar: `(agent, anchor entity, label entity)` — full
    /// objects first, then the coarse-only spheres (the object path despawns
    /// a coarse twin, but the filter keeps a mid-frame overlap harmless).
    /// The tag-content composer iterates this.
    pub fn labelled_avatars(&self) -> impl Iterator<Item = (AgentKey, Entity, Entity)> + '_ {
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
    #[must_use]
    pub fn name_of(&self, agent: AgentKey) -> Option<&str> {
        self.names
            .get(&agent)
            .and_then(|record| record.legacy.as_deref())
    }

    /// Record a name learned from **traffic** rather than a name lookup — an
    /// instant message's sender, a chat-session invitation's inviter, a
    /// server-history line's speaker. The wire carries these names alongside
    /// the message, so the person is nameable without asking.
    ///
    /// Only fills a name that is **not** already known: a lookup reply (and the
    /// display-name cap behind it) is the better-defined answer, and this must
    /// not overwrite it with whatever a message happened to be stamped with.
    pub fn note_legacy_name(&mut self, agent: AgentKey, name: &str) {
        if name.is_empty() {
            return;
        }
        let record = self.name_entry(agent);
        if record.legacy.is_none() {
            record.legacy = Some(name.to_owned());
        }
    }

    /// This agent's full name record, if any of its sources answered yet —
    /// the tag-content composer reads the display name / username / default
    /// flag from it.
    #[must_use]
    pub fn name_record(&self, agent: AgentKey) -> Option<&NameRecord> {
        self.names.get(&agent)
    }

    /// The name to **show** for this agent — the user's alias, else the display
    /// name, else the legacy name — or `None` while nothing has resolved.
    ///
    /// This is the accessor a drawn name wants; [`Self::name_of`] is the grid's
    /// legacy answer, which is what a wire action (a mute entry naming the muted
    /// avatar) has to carry.
    pub fn shown_name_of(&self, agent: AgentKey) -> Option<&str> {
        self.names.get(&agent).and_then(NameRecord::preferred_name)
    }

    /// Replace the user's name aliases, re-folding every cached record so a
    /// pseudonym given (or cleared) now shows (or stops showing) everywhere at
    /// once. The one way an alias reaches the name cache.
    pub fn set_name_aliases(&mut self, aliases: HashMap<AgentKey, NameAlias>) {
        for (agent, record) in &mut self.names {
            record.alias = aliases.get(agent).cloned();
        }
        self.name_aliases = aliases;
        self.names_revision = self.names_revision.wrapping_add(1);
    }

    /// The record for `agent`, created if this is the first thing known about
    /// them, with the user's alias folded in — the one way an ingest path takes
    /// a record, so a name that arrives after the alias was given is aliased
    /// too.
    fn name_entry(&mut self, agent: AgentKey) -> &mut NameRecord {
        let alias = self.name_aliases.get(&agent).cloned();
        // Bumped for the *touch*, not for a proven change: the caller takes a
        // `&mut` and decides for itself whether to write through it, so this is
        // an upper bound on "a name moved". Erring that way is the cheap
        // direction — a name ingest is a rare, bounded event, and a spurious
        // rebuild of a few table rows costs nothing, while missing a real one
        // leaves a resident showing as a UUID until something else moves.
        self.names_revision = self.names_revision.wrapping_add(1);
        let record = self.names.entry(agent).or_default();
        if record.alias != alias {
            record.alias = alias;
        }
        record
    }

    /// This agent's group title (from its avatar object's NameValue `Title`),
    /// if it has one.
    pub fn title_of(&self, agent: AgentKey) -> Option<&str> {
        self.titles.get(&agent).map(String::as_str)
    }

    /// The agent whose avatar object carries the region-scoped id `scoped`, if
    /// this viewer tracks one — the reverse of the object stream's view of an
    /// avatar. An attachment names its wearer only by that scoped id, so this is
    /// how a worn linkset is attributed to the avatar it is worn on
    /// (`crate::avatar_complexity`).
    #[must_use]
    pub fn agent_of_scoped(&self, scoped: ScopedObjectId) -> Option<AgentKey> {
        self.by_scoped.get(&scoped).copied()
    }

    /// Mark this avatar's body-region materials for (re)assignment by
    /// `assign_avatar_bake_materials` — how a pass that borrowed those
    /// materials hands them back. The jellydoll render
    /// (`crate::avatar_complexity`) paints a limited avatar's body flat and
    /// calls this when it stops, so the real bakes are draped again.
    pub fn mark_bake_dirty(&mut self, agent: AgentKey) {
        let _fresh = self.bake_dirty.insert(agent);
    }

    /// How many base-body regions this avatar has a **visible** bake in, from its
    /// latest appearance — the count the render-cost model charges its per-region
    /// body cost for. Zero before an appearance arrives (or for a sphere-only
    /// avatar); a region baked invisible by a worn system alpha layer was already
    /// filtered out when the appearance was ingested, exactly as the reference
    /// skips an `IMG_INVISIBLE` slot.
    pub fn visible_bake_count(&self, agent: AgentKey) -> usize {
        self.baked_textures.get(&agent).map_or(0, HashMap::len)
    }

    /// Every avatar this viewer currently knows in-world, with the anchor
    /// entity whose transform places it — full objects first, then the
    /// coarse-only dots. The avatar picker's Near Me tab reads this.
    #[must_use]
    pub fn known_agents(&self) -> Vec<(AgentKey, Entity)> {
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
    /// sentinel (`crate::minimap_math::coarse_altitude_unknown`).
    #[must_use]
    pub fn map_avatars(&self) -> Vec<MapAvatar> {
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
    #[must_use]
    pub fn root_entity_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects
            .get(&agent)
            .or_else(|| self.coarse.get(&agent))
            .map(|entities| entities.anchor)
    }

    /// Seed the name cache and title map from an avatar object's NameValue
    /// pairs (`FirstName` / `LastName` / `Title` — the classic mechanism; the
    /// simulator sends them with every avatar `ObjectUpdate`, so the legacy
    /// name and group title arrive *with the object*, zero round trips).
    /// Never clobbers a legacy name another source already resolved, and only
    /// touches the title when a `Title` pair is actually present (a present
    /// but empty title means "title taken off").
    pub fn seed_from_name_values(&mut self, agent: AgentKey, object: &Object) {
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
    pub fn seed_name_fields(
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
                let record = self.name_entry(agent);
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
    pub fn set_display_name(&mut self, resolved: &DisplayName) {
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
    pub fn merge_display_name_record(&mut self, resolved: &DisplayName) -> bool {
        if resolved.missing {
            return false;
        }
        let record = self.name_entry(resolved.id);
        record.legacy = Some(resolved.legacy_name());
        record.username = Some(resolved.username.clone());
        record.display_name = Some(resolved.display_name.clone());
        record.is_display_name_default = resolved.is_display_name_default;
        true
    }

    /// Queue a name request for `agent` once — a no-op if it is already in
    /// flight or answered. The actual wire traffic goes out batched, once per
    /// frame, in `flush_name_requests`. `pub(crate)` for the build
    /// floater's General tab, which resolves a selected object's creator /
    /// owner through the same cache.
    pub fn request_name(&mut self, agent: AgentKey) {
        if !self.requested.insert(agent) {
            return;
        }
        self.pending_name_requests.insert(agent);
    }

    /// Despawn the full-object avatar tracked under `scoped` because it was
    /// derendered — the scoped-id counterpart of `derender_agent`,
    /// for the suppression index (which works in region-scoped ids). A no-op
    /// when `scoped` is not an avatar.
    pub fn derender_scoped(&mut self, scoped: ScopedObjectId, commands: &mut Commands) {
        self.remove_object(scoped, commands);
    }

    /// Despawn the placeholder of the full-object avatar that left the scene under
    /// `scoped`, if one is tracked.
    pub fn remove_object(&mut self, scoped: ScopedObjectId, commands: &mut Commands) {
        let Some(agent) = self.by_scoped.remove(&scoped) else {
            return;
        };
        if let Some(entities) = self.objects.remove(&agent) {
            despawn_avatar(entities, commands);
        }
        // The body's attachment-point nodes and head socket are despawned with
        // its anchor; drop the stores so a later attachment can no longer resolve
        // them (P16.2). The recorded joint overrides go too, so a re-spawn
        // rebuilds them from the meshes that re-bind (R1).
        let _dropped_nodes = self.attachment_nodes.remove(&agent);
        let _dropped_head = self.head_sockets.remove(&agent);
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
    #[must_use]
    pub fn is_seated(&self, agent: AgentKey) -> bool {
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
    pub fn unseat_from_seat(&mut self, seat: ScopedObjectId, commands: &mut Commands) {
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
    /// seat drop)`, for `place_seated_avatars` to drive the anchor from its seat's
    /// live world transform. The seat drop is the pelvis's height above the body
    /// root (zero for a sphere), applied so the hips land on the sit target. Skips
    /// any avatar whose anchor is not (yet) a tracked full object.
    pub fn seated_placements(
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
    #[must_use]
    pub fn agent_of(&self, avatar_scoped: ScopedObjectId) -> Option<AgentKey> {
        self.by_scoped.get(&avatar_scoped).copied()
    }

    /// The attachment-point node entity a worn attachment parents to (P16.2): the
    /// node for raw attachment-point `point_id` on the rigged body of the avatar
    /// tracked under `avatar_scoped`, carrying the fixed `avatar_lad.xml` offset
    /// from its skeleton joint. `None` if that avatar is not a tracked full-object
    /// rigged body yet, or the point has no body joint (a HUD point) — in which
    /// case the caller holds the attachment pending and retries.
    #[must_use]
    pub fn attachment_point_entity(
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
    #[must_use]
    pub fn label_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects
            .get(&agent)
            .or_else(|| self.coarse.get(&agent))
            .map(|entities| entities.label)
    }

    /// The rigged-body root (anchor) entity of `agent`'s full-object avatar,
    /// if one is rendered.
    #[must_use]
    pub fn body_root_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects.get(&agent).map(|entities| entities.anchor)
    }

    /// Whether `agent` has a spawned rigged body (Phase 4: keyed on the presence
    /// of the head socket, spawned with the body — the joint entities the old
    /// `joint_entities_of` returned are gone). `false` for a sphere-only
    /// (no-`--viewer-assets`) avatar or one not spawned yet.
    #[must_use]
    pub fn is_rigged(&self, agent: AgentKey) -> bool {
        self.head_sockets.contains_key(&agent)
    }

    /// The per-point attachment-point node entities of `agent`'s rigged avatar
    /// as `(raw attachment-point id, node entity)` pairs — the socket writer
    /// (§5.4) places each worn node at its joint's posed world composed with the
    /// point's fixed `avatar_lad.xml` offset each frame. Empty for an avatar
    /// with no rigged body.
    pub fn attachment_nodes_of(&self, agent: AgentKey) -> impl Iterator<Item = (u8, Entity)> + '_ {
        self.attachment_nodes
            .get(&agent)
            .into_iter()
            .flat_map(|nodes| nodes.iter().map(|(&point_id, &entity)| (point_id, entity)))
    }

    /// The camera's head-focus socket entity of `agent`'s rigged avatar
    /// (Phase 4 §5.4): a root child the socket writer places at the posed
    /// `mHead` joint each frame, so the camera reads the animated head without a
    /// head joint entity. `None` for a sphere-only avatar or before the body
    /// spawns.
    #[must_use]
    pub fn head_socket_of(&self, agent: AgentKey) -> Option<Entity> {
        self.head_sockets.get(&agent).copied()
    }

    /// The resolved skeletal deformations the animation driver (P18.3) folds a
    /// playing motion into when recomputing each joint's world matrix, as last
    /// shaped by `apply_avatar_appearance`. `None` for an avatar with no rigged
    /// body, or before its first appearance.
    #[must_use]
    pub fn deformations(&self, agent: AgentKey) -> Option<&SkeletalDeformations> {
        self.deformations.get(&agent)
    }

    /// The resolved collision-volume displacements (P34.3) the animation driver
    /// folds into the same recurrence, as last shaped by
    /// `apply_avatar_appearance`. An avatar whose shape displaces no volume has
    /// no entry, which is the same as the (empty) default.
    #[must_use]
    pub fn volume_deformations(&self, agent: AgentKey) -> Option<&VolumeDeformations> {
        self.volume_deformations.get(&agent)
    }

    /// Every avatar with a spawned rigged body (Phase 4: keyed on the head
    /// socket, since the joint entities are gone). The pose driver publishes each
    /// one's root + adjuster corrections and places its sockets every frame; the
    /// GPU samples, blends and FK-poses the skinning in place.
    #[must_use]
    pub fn rigged_agents(&self) -> Vec<AgentKey> {
        self.head_sockets.keys().copied().collect()
    }

    /// Note that `agent` wears the rigged mesh asset `mesh` (for the avatar-state
    /// dump). Idempotent; forgotten with the avatar on despawn.
    pub fn record_worn_rigged_mesh(&mut self, agent: AgentKey, mesh: Uuid) {
        let _new = self
            .worn_rigged_meshes
            .entry(agent)
            .or_default()
            .insert(mesh);
    }

    /// The anchor entity (rigged-body root, or placeholder sphere) of `agent`'s
    /// full-object avatar, if one is tracked — the world pose the replay test rig
    /// (an orbiting light, a reflection probe) centres itself on.
    #[must_use]
    pub fn anchor_of(&self, agent: AgentKey) -> Option<Entity> {
        self.objects.get(&agent).map(|entities| entities.anchor)
    }

    /// Record the joint position overrides that worn rigged `mesh` imposes on
    /// `agent`'s skeleton (R1), replacing any previous contribution from that mesh
    /// (a rebind is idempotent). Flags the avatar for a skeleton re-deform **only
    /// when the contribution actually changed**, so re-binding identical rig parts
    /// (a mesh body's many same-rigged pieces) does not thrash the appearance pass.
    pub fn record_joint_overrides(
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
    #[must_use]
    pub fn body_physics(&self, agent: AgentKey) -> Option<&BodyPhysics> {
        self.body_physics.get(&agent)
    }

    /// The current pose-inputs generation (see the field doc): the pose gate
    /// stores it per avatar and re-evaluates when it moved.
    #[must_use]
    pub const fn pose_inputs_generation(&self) -> u64 {
        self.pose_inputs_generation
    }

    /// Record that an input the skeleton pose fold consumes (deformations, volume
    /// deformations, joint overrides, body physics) was (re)applied. Over-bumping
    /// is harmless — an extra bump costs one frame of full re-evaluation.
    pub const fn bump_pose_inputs(&mut self) {
        self.pose_inputs_generation = self.pose_inputs_generation.wrapping_add(1);
    }

    /// The effective joint position overrides for `agent` (R1): the per-joint winner
    /// across every worn rigged mesh, resolved to the **highest mesh id** on a
    /// conflict (the reference viewer's `findActiveOverride`) with the scale lock
    /// sticky. `None` when the avatar wears no position-carrying rig.
    #[must_use]
    pub fn effective_joint_overrides(&self, agent: AgentKey) -> Option<JointOverrides> {
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
    #[must_use]
    pub fn wearer_of(&self, scoped: ScopedObjectId) -> Option<AgentKey> {
        self.avatar_root_of(scoped)
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
    /// destination streams. Also drops the origin anchor so `recenter_avatars`
    /// re-anchors on the destination without a spurious re-base shift.
    pub fn purge(&mut self, own: Option<AgentKey>, commands: &mut Commands) {
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
        //
        // **Names are not scene state and are deliberately kept.** A name is
        // knowledge about a person, not about a presence: most of the names
        // this viewer shows are for avatars nowhere near it — group members and
        // group chat, an object's or parcel's owner and creator, an inventory
        // item's creator, an open conversation's peer. Dropping the cache
        // because the *region* changed would re-ask the grid for names it
        // already knew, and blank every one of those surfaces until the replies
        // land. It cannot grow enough to matter over a session; if it ever
        // did, the bound would be least-recently-used, not "is standing near
        // me". The request bookkeeping stays with it, so a name already
        // resolved is never re-requested.
        self.coarse_region.retain(|agent, _| keep(agent));
        self.coarse_pos.retain(|agent, _| keep(agent));
        self.attachment_nodes.retain(|agent, _| keep(agent));
        self.head_sockets.retain(|agent, _| keep(agent));
        self.titles.retain(|agent, _| keep(agent));
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

    /// Record a resolved legacy name. (The tag itself refreshes via the
    /// content composer, which recomposes whenever this state changes.)
    pub fn set_name(&mut self, name: &AvatarName) {
        let agent = name.id;
        let resolved = name.legacy_name();
        self.name_entry(agent).legacy = Some(resolved.clone());
        debug!("resolved avatar name {agent} = {resolved:?}");
    }

    /// Record the parenting of an in-world object and, once, scan its texture
    /// entry for the `IMG_USE_BAKED_*` sentinels a worn attachment uses to hide a
    /// base-avatar region. Called for every object; a *root* object (no parent)
    /// can never be an attachment, so it is ignored.
    pub fn track_object(&mut self, object: &Object) {
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
    pub fn forget_object(&mut self, scoped: ScopedObjectId) {
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

    /// Diagnostic form of `avatar_root_of`: `Ok(agent)` when the parent chain
    /// reaches a recognised avatar, else `Err((terminus, hops))` — the object the
    /// walk stopped at (a root with no recorded parent, or the last hop when the
    /// depth cap is hit) and how many hops it took. Lets a stuck rigged
    /// attachment's `wearer not resolved` failure be classified against the object
    /// state: a *tracked in-world* terminus means it is genuinely not worn (an
    /// in-world rigged mesh), while an *untracked* terminus means the wearer /
    /// linkset-root object never arrived (a parenting / ordering gap).
    ///
    /// # Errors
    ///
    /// `Err((terminus, hops))` when the walk reaches no avatar: `terminus` is the
    /// object it stopped at — a root with no recorded parent, or the last hop
    /// when the depth cap is reached — and `hops` how many parent links it
    /// followed to get there.
    pub fn avatar_root_walk(
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
    #[must_use]
    pub fn hidden_slots_per_agent(&self) -> HashMap<AgentKey, HashSet<usize>> {
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

impl crate::world_scoped::WorldScoped for AvatarState {
    fn purge_world(&mut self, purge: crate::world_scoped::WorldPurge, commands: &mut Commands) {
        self.purge(purge.own_agent, commands);
    }
}

/// Despawn both entities of an avatar (its anchor — sphere or body root, whose
/// sub-hierarchy goes with it — and its name tag).
pub fn despawn_avatar(entities: AvatarEntities, commands: &mut Commands) {
    commands.entity(entities.anchor).try_despawn();
    commands.entity(entities.label).try_despawn();
}

/// When an avatar was first and last marked appearance-dirty, in
/// [`Time::elapsed_secs_f64`] seconds (see
/// [`AvatarState::appearance_pending`]).
#[derive(Debug, Clone, Copy)]
pub struct AppearanceDirtyStamps {
    /// When the avatar entered the pending set (unchanged by re-marks).
    pub first: f64,
    /// When the avatar was most recently marked (each re-mark refreshes it).
    pub last: f64,
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{AvatarState, PROVISIONAL_ID_CHARS, provisional_label, used_baked_slots};
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{
        AgentKey, RegionHandle, TextureEntry, TextureFace, TextureKey, Uuid, avatar_texture,
        encode_texture_entry,
    };

    /// The provisional tag is the agent id's leading hex fragment, so two distinct
    /// avatars read differently before their names resolve.
    #[test]
    fn provisional_label_is_a_short_id_fragment() {
        let agent = AgentKey::from(Uuid::from_u128(0x1234_5678_9abc));
        let label = provisional_label(agent);
        assert_eq!(label.chars().count(), PROVISIONAL_ID_CHARS);
        assert!(agent.uuid().simple().to_string().starts_with(&label));
    }

    /// The name-cache revision moves for a name and for nothing else.
    ///
    /// The distinction a table of resolved names lives on: `AvatarState` is
    /// written by every avatar that moves or streams in, so its change tick is
    /// set on most frames in a crowded region and says nothing about names. A
    /// view that rebuilds on the tick rebuilds constantly for no reason
    /// (`viewer-audit-about-land-row-rebuild`); one that rebuilds on this
    /// rebuilds when a name arrives.
    #[test]
    fn the_name_revision_moves_for_names_and_nothing_else() {
        let agent = AgentKey::from(Uuid::from_u128(0xa1));
        let mut avatars = AvatarState::default();
        let quiet = avatars.names_revision();

        // An avatar arriving where the viewer can see it is not a name.
        let _previous = avatars
            .coarse_region
            .insert(agent, RegionHandle::new(0x1234));
        assert_eq!(
            avatars.names_revision(),
            quiet,
            "an avatar moving into view is not a name resolving"
        );

        // A name learned from traffic is.
        avatars.note_legacy_name(agent, "Somebody Resident");
        let named = avatars.names_revision();
        assert_ne!(named, quiet);
        assert_eq!(avatars.label_text(agent), "Somebody Resident");

        // So is an alias replacing one, which renames them everywhere at once.
        avatars.set_name_aliases(HashMap::new());
        assert_ne!(avatars.names_revision(), named);
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
}
