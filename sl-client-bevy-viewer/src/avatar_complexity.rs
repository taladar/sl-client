//! **Avatar complexity limiting** (`viewer-avatar-complexity-limit`): score what
//! each nearby avatar costs to draw, and past a budget draw them as a flat
//! silhouette — the *jellydoll* — instead of their real, attachment-laden self.
//!
//! This is the third and most automatic of the crowded-event render filters. The
//! derender blacklist ([`crate::derender`]) is aimed by hand at one target;
//! Show Friends Only is a blunt "hide everyone I do not know"; this one asks
//! only *what does this avatar cost me*, and answers per avatar, continuously.
//! It is the filter that keeps a single griefer-built avatar — or one very
//! enthusiastic mesh outfit — from sinking the frame rate for everybody.
//!
//! # The score
//!
//! [`avatar_complexity`] is a faithful port of the reference viewer's
//! `LLVOAvatar::calculateUpdateRenderComplexity` / `LLVOVolume::getRenderCost`.
//! The reference comments that calculation "should not be modified by third
//! party viewers, since it is used to limit rendering and should be uniform for
//! everyone", and it is right for a social reason as much as a technical one:
//! residents quote their ARC ("avatar rendering cost") to each other and compare
//! it against a shared idea of what is polite to wear. A number that meant
//! something different here would be worse than useless. So the constants, the
//! multipliers and the order they apply in are the reference's:
//!
//! - **200 per visible baked body region** — the system avatar itself.
//! - **Per attachment linkset**, clamped to at most a million so one item cannot
//!   swamp the total: `max(5 · triangles, 2)` scaled by the per-face and
//!   per-prim multipliers (planar tex-gen, animated texture ×4, alpha ×4,
//!   invisible ×1.2, glow ×1.5, bump ×1.25, shiny ×1.6, rigged mesh ×1.2, flexi
//!   ×5), plus the additive charges (light 500, media face 1500, particles by
//!   burst size, animesh 1000 + a per-linkset base), plus `256 + 16·(w + h)/128`
//!   for each **unique** texture in the linkset.
//! - **Triangles** are the reference's *radius-weighted* estimate across all
//!   four levels of detail, not what this viewer happens to be drawing: a mesh's
//!   per-level counts come from its asset header's block byte sizes, a prim's
//!   from [`lod_triangle_counts`]. That matters — the weighting is dominated by
//!   the coarse levels for a small attachment, so scoring the *drawn* geometry
//!   would both diverge from every other viewer and make the score wobble as the
//!   camera moves.
//!
//! Scoring is **debounced and budgeted** ([`recompute_avatar_complexity`]): an
//! avatar is re-scored at most every [`RESCORE_INTERVAL_SECS`], at most
//! [`RESCORE_BUDGET`] avatars per frame, and only when something it is made of
//! changed. An avatar whose score is waiting on an asset (a mesh header, a
//! texture's real dimensions) is re-scored when that asset lands, so the number
//! converges as the avatar rezzes rather than being wrong forever.
//!
//! # The decision, and the overrides
//!
//! [`jelly_reason`] applies the reference's priority order: yourself is never
//! jellied, then the per-avatar override wins, then the complexity mode, then
//! the budget and the attachment surface-area trigger. The per-avatar override
//! ([`RenderOverride`]) is this session's — Render Fully exempts an avatar from
//! every automatic rule, Never Render pins them to the jellydoll. Persisting
//! those overrides across relogs, and the floater that manages them, are
//! `viewer-avatar-render-settings-manager`, which builds on the machinery here.
//!
//! # The jellydoll
//!
//! [`apply_jellydoll`] renders it the way the reference does *in effect*, by the
//! means this viewer has:
//!
//! - **Every attachment is hidden**, including the rigged faces that hang off the
//!   wearer's body root rather than off the attachment object. Hidden geometry is
//!   not extracted, so it is not skinned, batched or drawn — that is where the
//!   frame time comes back, and attachments are nearly all of it.
//! - **The system body is drawn in a flat colour** ([`JELLY_COLOR`], the
//!   reference's `grey4`), unlit, so what is left is a silhouette rather than a
//!   naked avatar.
//! - **Its base regions are forced visible** and its hair hidden, exactly as the
//!   reference's `updateMeshVisibility` does for a jellydoll. Without this a
//!   mesh-body wearer would vanish entirely: their system body is baked
//!   invisible precisely because a mesh body covers it, and we just hid the mesh
//!   body. (That override lives in
//!   [`apply_avatar_part_visibility`](crate::avatars::apply_avatar_part_visibility),
//!   which reads this module's jellied set.)
//!
//! Presence is untouched: a jellied avatar keeps its name tag, its place on the
//! radar and the minimap, and its animations. The reference stops a jellydoll's
//! animations and forces a stand — worth it there because its animation system
//! runs on the CPU per avatar; ours poses on the GPU, so a jellied avatar can
//! keep moving like a person for free.
//!
//! # Where the number deliberately differs
//!
//! Three inputs the reference reads off its own renderer are approximated here,
//! each in a way that can only move a score by a small factor:
//!
//! - **Transparency** is judged from the face's tint alpha rather than from
//!   whether the face landed in the alpha draw pool, so a fully-opaque tint over
//!   a texture that happens to carry an alpha channel is not charged the ×4.
//! - **Attachment surface area** is the plain square-metre area of each prim's
//!   scaled bounding box, not the reference's unit-volume surface area times the
//!   largest scale axis. Ours is the more literal answer to "how much of the
//!   screen can this smear over", and it catches the one enormous alpha sheet
//!   the reference's measure lets through.
//! - **An animated object's** streaming-cost term uses its finest level's
//!   triangle estimate rather than the reference's charged-versus-allowed
//!   refinement.
//!
//! Reference (Firestorm, read-only): `LLVOAvatar::calculateUpdateRenderComplexity`,
//! `accountRenderComplexityForObject`, `LLVOVolume::getRenderCost` /
//! `getTextureCost`, `LLVOAvatar::isTooComplex` / `getOverallAppearance` /
//! `updateMeshVisibility`, `LLMeshCostData` (`llmeshrepository.cpp`), and the
//! `RenderAvatarMaxComplexity` / `RenderAvatarComplexityMode` /
//! `RenderAutoMuteSurfaceAreaLimit` settings.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use sl_settings::SettingValue;
use tracing::{debug, info};

use sl_client_bevy::{
    AgentKey, MeshHeader, MeshKey, PRIM_LOD_COUNT, PrimShapeParams, ScopedObjectId, SlEvent,
    SlIdentity, SlSessionEvent, TextureEntry, TextureKey, decode_texture_entry,
    lod_triangle_counts,
};

use crate::avatar_assets::BodyRegion;
use crate::avatars::{AvatarBodyPart, AvatarState};
use crate::face_material::{FaceMaterial, inert_face_material};
use crate::meshes::{MeshDecoded, MeshManager};
use crate::objects::{ObjectState, PrimComplexityFacts};
use crate::particles::ObjectParticleSystem;
use crate::people::FriendsModel;
use crate::settings::ViewerSettings;
use crate::textures::{TextureDecoded, TextureManager};

// ---------------------------------------------------------------------------
// Settings.
// ---------------------------------------------------------------------------

/// The persisted-settings section these knobs live in — the same `[render]`
/// section the graphics tab's own settings use.
const RENDER_SECTION: &[&str] = &["render"];

/// The complexity budget (the reference's `RenderAvatarMaxComplexity`): an
/// avatar scoring above it is drawn as a jellydoll. `0` disables the limit —
/// and, as in the reference, the surface-area trigger with it.
pub(crate) const SETTING_MAX_COMPLEXITY: &str = "RenderAvatarMaxComplexity";

/// How the budget is applied (the reference's `RenderAvatarComplexityMode`); see
/// [`ComplexityMode`].
pub(crate) const SETTING_COMPLEXITY_MODE: &str = "RenderAvatarComplexityMode";

/// The attachment surface-area trigger (the reference's
/// `RenderAutoMuteSurfaceAreaLimit`), in square metres; `0` turns it off. It
/// catches the content a triangle count cannot: one enormous alpha sheet is
/// cheap to *transform* and ruinous to *fill*.
pub(crate) const SETTING_SURFACE_AREA_LIMIT: &str = "RenderAutoMuteSurfaceAreaLimit";

/// The default budget: **off**, as the reference ships it. The limit hides
/// people, so it is opt-in — the Quick Preferences slider is how you reach for
/// it when a region turns out to be too much.
const DEFAULT_MAX_COMPLEXITY: u32 = 0;

/// The largest budget the sliders offer. Above roughly this an avatar is
/// unrenderable on any hardware, so a higher setting would only mean "off",
/// which `0` already says.
pub(crate) const MAX_COMPLEXITY_SLIDER_MAX: f32 = 500_000.0;

/// The budget slider's step — fine enough to tune, coarse enough that dragging
/// it does not re-decide every avatar on every pixel.
pub(crate) const MAX_COMPLEXITY_SLIDER_STEP: f32 = 5_000.0;

/// The default surface-area trigger, in square metres (the reference's value).
const DEFAULT_SURFACE_AREA_LIMIT: f32 = 1000.0;

/// The largest surface-area limit the slider offers.
pub(crate) const SURFACE_AREA_SLIDER_MAX: f32 = 5000.0;

/// The surface-area slider's step.
pub(crate) const SURFACE_AREA_SLIDER_STEP: f32 = 100.0;

/// How the complexity budget is applied to friends — the reference's
/// `RenderAvatarComplexityMode`, whose stored numbering this keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ComplexityMode {
    /// Judge everyone by the budget alone, friends included.
    #[default]
    ByComplexity,
    /// Friends are always drawn in full, whatever they cost.
    AlwaysShowFriends,
    /// Only friends are drawn in full; everyone else is a jellydoll. Distinct
    /// from Show Friends Only ([`crate::derender`]), which does not draw the
    /// non-friends at all — this keeps their silhouette.
    OnlyShowFriends,
}

impl ComplexityMode {
    /// The mode for a stored setting value, defaulting to
    /// [`ByComplexity`](Self::ByComplexity) for anything unrecognised.
    const fn from_stored(value: u32) -> Self {
        match value {
            1 => Self::AlwaysShowFriends,
            2 => Self::OnlyShowFriends,
            _other => Self::ByComplexity,
        }
    }

    /// The stored setting value for this mode.
    pub(crate) const fn stored(self) -> u32 {
        match self {
            Self::ByComplexity => 0,
            Self::AlwaysShowFriends => 1,
            Self::OnlyShowFriends => 2,
        }
    }
}

/// A per-avatar override of the automatic rules (the reference's
/// `VisualMuteSettings`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RenderOverride {
    /// No override: the automatic rules decide.
    #[default]
    Normal,
    /// Always draw this avatar in full, whatever they cost.
    AlwaysFull,
    /// Never draw this avatar in full — pin them to the jellydoll.
    Never,
}

// ---------------------------------------------------------------------------
// The render-cost constants (the reference's, unchanged).
// ---------------------------------------------------------------------------

/// The cost of one visible baked body region (`COMPLEXITY_BODY_PART_COST`).
const BODY_PART_COST: f32 = 200.0;

/// The base charge for any texture, before its resolution term
/// (`getTextureCost`).
const TEXTURE_BASE_COST: f32 = 256.0;

/// The resolution multiplier in the texture charge (`ARC_TEXTURE_COST`), applied
/// to `(width + height) / 128`.
const TEXTURE_RESOLUTION_COST: f32 = 16.0;

/// The reference's per-particle charge (`ARC_PARTICLE_COST`).
const PARTICLE_COST: f32 = 1.0;

/// The cap on the particle count a system is charged for (`ARC_PARTICLE_MAX`).
const PARTICLE_MAX: f32 = 2048.0;

/// The flat charge for a light-producing prim (`ARC_LIGHT_COST`).
const LIGHT_COST: f32 = 500.0;

/// The flat charge per media-enabled face (`ARC_MEDIA_FACE_COST`).
const MEDIA_FACE_COST: f32 = 1500.0;

/// The planar tex-gen multiplier (`ARC_PLANAR_COST`; deliberately neutral in the
/// reference, kept so the formula reads the same).
const PLANAR_MULT: f32 = 1.0;

/// The animated-texture multiplier (`ARC_ANIM_TEX_COST`).
const ANIM_TEX_MULT: f32 = 4.0;

/// The transparency multiplier (`ARC_ALPHA_COST`).
const ALPHA_MULT: f32 = 4.0;

/// The invisiprim multiplier (`ARC_INVISI_COST`).
const INVISI_MULT: f32 = 1.2;

/// The glow multiplier (`ARC_GLOW_MULT`).
const GLOW_MULT: f32 = 1.5;

/// The bump-map multiplier (`ARC_BUMP_MULT`).
const BUMP_MULT: f32 = 1.25;

/// The shininess multiplier (`ARC_SHINY_MULT`).
const SHINY_MULT: f32 = 1.6;

/// The rigged-mesh multiplier (`ARC_WEIGHTED_MESH`).
const WEIGHTED_MESH_MULT: f32 = 1.2;

/// The flexible-prim multiplier (`ARC_FLEXI_MULT`) — by far the heaviest, and
/// deservedly: a flexi prim is re-tessellated on the CPU every frame.
const FLEXI_MULT: f32 = 5.0;

/// The per-triangle charge before the multipliers (`shame = num_triangles * 5`).
const COST_PER_TRIANGLE: f32 = 5.0;

/// The floor a prim's pre-multiplier cost is raised to.
const MIN_PRIM_COST: f32 = 2.0;

/// The triangle count a prim with no usable geometry estimate is charged for.
const FALLBACK_TRIANGLES: f32 = 4.0;

/// The ceiling one attachment linkset's cost is clamped to
/// (`DEFAULT_MAX_ATTACHMENT_COMPLEXITY`), so a single absurd item cannot
/// overflow the wearer's total.
const MAX_ATTACHMENT_COMPLEXITY: f32 = 1.0e6;

/// The surcharge for an animated-object (animesh) attachment
/// (`animated_object_attachment_surcharge`).
const ANIMESH_ATTACHMENT_SURCHARGE: f32 = 1000.0;

/// The animated-object per-linkset base cost (`ANIMATED_OBJECT_BASE_COST`),
/// charged as `base / 0.06 * 5` triangles' worth on the linkset root.
const ANIMESH_BASE_COST: f32 = 15.0;

/// The per-thousand-triangle animated-object cost (`ANIMATED_OBJECT_COST_PER_KTRI`).
const ANIMESH_COST_PER_KTRI: f32 = 1.5;

/// The streaming-cost denominator the animated-object charges are normalised by
/// (the reference's literal `0.06`).
const ANIMESH_COST_SCALE: f32 = 0.06;

/// Bytes discounted from each mesh level's block size before triangles are
/// estimated (`MeshMetaDataDiscount`).
const MESH_METADATA_DISCOUNT: f32 = 384.0;

/// The floor a mesh level's discounted size is raised to (`MeshMinimumByteSize`),
/// so nothing is free.
const MESH_MINIMUM_BYTE_SIZE: f32 = 16.0;

/// Bytes per triangle in the mesh size → triangle estimate
/// (`MeshBytesPerTriangle`).
const MESH_BYTES_PER_TRIANGLE: f32 = 16.0;

/// The bytes-per-triangle a **prim**'s analytic level counts are inflated to
/// before going through the same estimator (`LLVOVolume::getCostData`'s
/// `counts[i] * 10`).
const PRIM_BYTES_PER_TRIANGLE: f32 = 10.0;

/// The largest area, in square metres, any level-of-detail band is credited with
/// in the radius weighting — the area of a circle enclosing a region.
const LOD_MAX_AREA: f32 = 102_944.0;

/// The smallest area a level band is credited with.
const LOD_MIN_AREA: f32 = 1.0;

/// The farthest distance a level-of-detail switch is placed at, in metres.
const LOD_MAX_DISTANCE: f32 = 512.0;

/// The level-of-detail switch factors (`radius / factor` is the switch distance)
/// for the lowest, low and medium levels.
const LOD_SWITCH_FACTORS: [f32; 3] = [0.03, 0.06, 0.24];

// ---------------------------------------------------------------------------
// Scoring cadence.
// ---------------------------------------------------------------------------

/// How long an avatar's score is trusted before a fresh mark can re-score it.
/// A rezzing crowd marks the same avatars over and over as their attachments
/// stream in; this collapses that into one score per avatar per second.
const RESCORE_INTERVAL_SECS: f64 = 1.0;

/// How many avatars may be re-scored in one frame. Scoring walks an avatar's
/// whole worn linkset and decodes each prim's texture entry, so it is not free —
/// but at this rate a two-hundred-avatar region settles in under a second.
const RESCORE_BUDGET: usize = 4;

/// How often the jellydoll's hidden set is re-derived while the decision itself
/// has not moved — the safety net for an attachment that streams in for an
/// avatar who is already a jellydoll.
const JELLY_SWEEP_SECONDS: f64 = 0.5;

/// The jellydoll's colour — the reference's `LLColor4::grey4`, drawn unlit so
/// the result reads as a silhouette rather than an oddly-lit person.
const JELLY_COLOR: Color = Color::srgb(0.3, 0.3, 0.3);

// ---------------------------------------------------------------------------
// The model.
// ---------------------------------------------------------------------------

/// What one avatar costs to draw, as [`avatar_complexity`] last measured it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct AvatarComplexity {
    /// The total render cost — the number residents call ARC.
    pub(crate) score: u32,
    /// The part of it charged for the system body's visible baked regions.
    pub(crate) body_cost: u32,
    /// The worn (non-HUD) attachment linksets counted.
    pub(crate) attachments: usize,
    /// The total surface area of those attachments, in square metres.
    pub(crate) surface_area: f32,
}

/// Why an avatar is being drawn as a jellydoll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JellyReason {
    /// The user pinned this avatar to "never render in full".
    Override,
    /// Only friends are drawn in full, and this avatar is not one.
    NotAFriend,
    /// Their score is over the budget.
    TooComplex,
    /// Their attachments cover too much surface area.
    TooMuchArea,
}

/// The limits the jelly decision is made against, mirrored from the settings so
/// the decision is a pure function of them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct ComplexityLimits {
    /// The complexity budget; `0` disables the automatic rules entirely.
    pub(crate) max: u32,
    /// How the budget treats friends.
    pub(crate) mode: ComplexityMode,
    /// The attachment surface-area trigger in square metres; `0` disables it.
    pub(crate) area: f32,
}

/// The avatar render-cost model: every nearby avatar's score, the per-avatar
/// overrides, and who is currently drawn as a jellydoll.
#[derive(Resource, Debug, Default)]
pub(crate) struct AvatarComplexityModel {
    /// The last measured cost per avatar.
    scores: HashMap<AgentKey, AvatarComplexity>,
    /// The per-avatar overrides this session
    /// (`viewer-avatar-render-settings-manager` will persist them).
    overrides: HashMap<AgentKey, RenderOverride>,
    /// Who is currently jellied, and why.
    jellied: HashMap<AgentKey, JellyReason>,
    /// Avatars whose score is stale and needs re-measuring.
    dirty: HashSet<AgentKey>,
    /// When (app elapsed seconds) each avatar was last scored, for the debounce.
    scored_at: HashMap<AgentKey, f64>,
    /// Avatars whose last score was missing a mesh's asset header, keyed by the
    /// mesh they are waiting for — re-scored when it lands, so the number
    /// converges as the avatar rezzes.
    awaiting_mesh: HashMap<MeshKey, HashSet<AgentKey>>,
    /// The same for a texture whose real dimensions were not known yet.
    awaiting_texture: HashMap<TextureKey, HashSet<AgentKey>>,
    /// The limits mirrored from the settings store.
    limits: ComplexityLimits,
    /// What the jelly render hid, and the visibility each entity had before —
    /// restored exactly when the avatar stops being a jellydoll, so a face
    /// another system deliberately hid stays hidden.
    hidden: HashMap<Entity, Visibility>,
    /// The shared flat jellydoll material, built on first use.
    material: Option<Handle<FaceMaterial>>,
    /// Whose body parts currently wear that material, so the bake materials can
    /// be handed back the moment an avatar stops being a jellydoll.
    painted: HashSet<AgentKey>,
    /// Bumped whenever a score or the jellied set moves, so a view over them
    /// (the radar's Complexity column) rebuilds exactly when it needs to. Also
    /// the gate the decision pass skips on — every input it reads bumps it.
    revision: u64,
    /// The avatars currently in-world, mirrored from
    /// [`AvatarState`](crate::avatars::AvatarState) by the scoring pass so the
    /// decision pass needs no query of its own.
    known: HashSet<AgentKey>,
    /// The friends the mode-dependent rules spare, mirrored from
    /// [`FriendsModel`] when its revision moves.
    friends: HashSet<AgentKey>,
    /// The friends-roster revision this mirror was taken at.
    friends_revision: Option<u64>,
}

impl AvatarComplexityModel {
    /// The revision of the scores and the jellied set — a view stores the value
    /// it last built at and rebuilds when it advances.
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    /// This avatar's measured cost, if it has been scored.
    pub(crate) fn complexity(&self, agent: AgentKey) -> Option<AvatarComplexity> {
        self.scores.get(&agent).copied()
    }

    /// Why this avatar is drawn as a jellydoll, or `None` when they are drawn in
    /// full. The hot query the avatar render paths make.
    pub(crate) fn jelly_reason_for(&self, agent: AgentKey) -> Option<JellyReason> {
        self.jellied.get(&agent).copied()
    }

    /// Whether this avatar is currently drawn as a jellydoll.
    pub(crate) fn is_jellied(&self, agent: AgentKey) -> bool {
        self.jellied.contains_key(&agent)
    }

    /// This avatar's per-avatar override.
    pub(crate) fn override_of(&self, agent: AgentKey) -> RenderOverride {
        self.overrides.get(&agent).copied().unwrap_or_default()
    }

    /// Set (or clear) an avatar's per-avatar override.
    pub(crate) fn set_override(&mut self, agent: AgentKey, over: RenderOverride) {
        if over == RenderOverride::Normal {
            let _dropped = self.overrides.remove(&agent);
        } else {
            let _previous = self.overrides.insert(agent, over);
        }
        self.bump();
    }

    /// Whether the jellydoll render has anything to do — someone to paint,
    /// something to keep hidden, or a body to hand back.
    ///
    /// `painted` has to be in this test, not just `hidden`: an avatar wearing no
    /// attachments hides nothing, so on the frame it stops being a jellydoll both
    /// of the other two sets are already empty — and skipping then would strand
    /// its body on the flat silhouette material for good.
    fn has_jelly_work(&self) -> bool {
        !self.jellied.is_empty() || !self.hidden.is_empty() || !self.painted.is_empty()
    }

    /// Note that something the decision pass reads has moved.
    const fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Mark `agent`'s score stale, so it is re-measured (subject to the
    /// debounce).
    fn mark_dirty(&mut self, agent: AgentKey) {
        let _fresh = self.dirty.insert(agent);
    }

    /// Forget everything about avatars that are no longer around, and mirror the
    /// set that remains for the decision pass.
    fn retain_known(&mut self, known: HashSet<AgentKey>) {
        if self.known == known {
            return;
        }
        self.scores.retain(|agent, _score| known.contains(agent));
        self.jellied.retain(|agent, _why| known.contains(agent));
        self.scored_at.retain(|agent, _when| known.contains(agent));
        self.dirty.retain(|agent| known.contains(agent));
        self.painted.retain(|agent| known.contains(agent));
        self.known = known;
        self.bump();
        // The overrides are the user's standing intent, not scene state: an
        // avatar who walks away and comes back keeps the setting they were given.
    }
}

/// The reference's priority order for whether an avatar is drawn as a jellydoll:
/// yourself never is; then the per-avatar override; then the complexity mode;
/// then the budget and the surface-area trigger. `None` means "draw them
/// normally".
///
/// A budget of `0` disables the automatic rules *including* the surface-area
/// trigger — the reference is explicit that "unlimited" must mean unlimited,
/// griefing content included, because that is what the user asked for.
pub(crate) fn jelly_reason(
    is_self: bool,
    is_friend: bool,
    over: RenderOverride,
    score: Option<AvatarComplexity>,
    limits: ComplexityLimits,
) -> Option<JellyReason> {
    if is_self {
        return None;
    }
    match over {
        RenderOverride::AlwaysFull => return None,
        RenderOverride::Never => return Some(JellyReason::Override),
        RenderOverride::Normal => {}
    }
    // Both friend-aware modes spare a friend from the budget entirely (the
    // reference's `render_friend = isBuddy() && mode > AV_RENDER_LIMIT_BY_COMPLEXITY`);
    // the stricter one additionally jellies everyone else on sight.
    match limits.mode {
        ComplexityMode::AlwaysShowFriends | ComplexityMode::OnlyShowFriends if is_friend => {
            return None;
        }
        ComplexityMode::OnlyShowFriends => return Some(JellyReason::NotAFriend),
        ComplexityMode::ByComplexity | ComplexityMode::AlwaysShowFriends => {}
    }
    if limits.max == 0 {
        return None;
    }
    let score = score?;
    if score.score > limits.max {
        return Some(JellyReason::TooComplex);
    }
    if limits.area > 0.0 && score.surface_area > limits.area {
        return Some(JellyReason::TooMuchArea);
    }
    None
}

// ---------------------------------------------------------------------------
// Scoring.
// ---------------------------------------------------------------------------

/// What a scoring pass could not resolve yet, so the avatar can be re-scored
/// when it lands.
#[derive(Debug, Default)]
pub(crate) struct PendingCostAssets {
    /// Meshes whose asset header had not been fetched.
    pub(crate) meshes: Vec<MeshKey>,
    /// Textures whose full-resolution dimensions were not known.
    pub(crate) textures: Vec<TextureKey>,
}

/// Everything the scorer needs to look up while it walks an avatar's
/// attachments, so [`avatar_complexity`] itself stays a pure function of the
/// scene state (and is unit-testable against hand-built inputs).
pub(crate) trait CostLookup {
    /// The per-level block byte sizes of a mesh asset, coarsest level first, or
    /// `None` while its header has not been fetched.
    fn mesh_lod_bytes(&self, mesh: MeshKey) -> Option<[u32; PRIM_LOD_COUNT]>;
    /// Whether a mesh is rigged (carries a skin block).
    fn mesh_rigged(&self, mesh: MeshKey) -> bool;
    /// A texture's full-resolution dimensions, or `None` while unknown.
    fn texture_dimensions(&self, texture: TextureKey) -> Option<(u32, u32)>;
    /// The particle burst a prim emits, if it is a live particle source.
    fn particles(&self, prim: Entity) -> Option<ParticleBurst>;
}

/// The two facts about a particle system the render cost is charged from: how
/// many particles it keeps alive and how big they are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ParticleBurst {
    /// Particles alive at once — `burst_part_count · ceil(part_max_age / burst_rate)`,
    /// the reference's estimate.
    pub(crate) count: f32,
    /// The mean of the larger of the start / end scale in each axis.
    pub(crate) size: f32,
}

impl ParticleBurst {
    /// The burst a decoded particle system amounts to.
    pub(crate) fn of(system: &sl_client_bevy::ParticleSystem) -> Self {
        let bursts = if system.burst_rate > 0.0 {
            (system.part_max_age / system.burst_rate).ceil()
        } else {
            1.0
        };
        let count = f32::from(system.burst_part_count) * bursts.max(1.0);
        let start = system.part_start_scale;
        let end = system.part_end_scale;
        let axis_x = start.first().copied().unwrap_or(0.0);
        let axis_y = start.last().copied().unwrap_or(0.0);
        let end_x = end.first().copied().unwrap_or(0.0);
        let end_y = end.last().copied().unwrap_or(0.0);
        Self {
            count,
            size: f32::midpoint(axis_x.max(end_x), axis_y.max(end_y)),
        }
    }
}

/// The estimated triangle count of each level of detail, from the per-level
/// block byte sizes — the reference's `LLMeshCostData::init`.
fn est_tris_by_lod(lod_bytes: [u32; PRIM_LOD_COUNT]) -> [f32; PRIM_LOD_COUNT] {
    // A level the asset omits inherits the next finer one's size, so a
    // single-level mesh is charged for that level at every distance.
    let mut bytes = lod_bytes;
    for index in (0..PRIM_LOD_COUNT.saturating_sub(1)).rev() {
        let finer = bytes.get(index.saturating_add(1)).copied().unwrap_or(0);
        if let Some(slot) = bytes.get_mut(index)
            && *slot == 0
        {
            *slot = finer;
        }
    }
    bytes.map(|size| {
        (f32_from_u32(size) - MESH_METADATA_DISCOUNT).max(MESH_MINIMUM_BYTE_SIZE)
            / MESH_BYTES_PER_TRIANGLE
    })
}

/// The reference's `LLMeshCostData::getRadiusWeightedTris`: the triangle count an
/// object of this radius costs *on average over a region*, weighting each level
/// by the area of the annulus it is displayed in. For a small attachment the
/// coarse levels dominate — which is exactly why the score must not be taken
/// from the level this viewer happens to be drawing.
fn radius_weighted_tris(tris: [f32; PRIM_LOD_COUNT], radius: f32) -> f32 {
    let switch = |factor: f32| (radius / factor).min(LOD_MAX_DISTANCE);
    let lowest_switch = switch(LOD_SWITCH_FACTORS.first().copied().unwrap_or(0.03));
    let low_switch = switch(LOD_SWITCH_FACTORS.get(1).copied().unwrap_or(0.06));
    let mid_switch = switch(LOD_SWITCH_FACTORS.last().copied().unwrap_or(0.24));

    let mut high_area = (core::f32::consts::PI * mid_switch * mid_switch).min(LOD_MAX_AREA);
    let mut mid_area = (core::f32::consts::PI * low_switch * low_switch).min(LOD_MAX_AREA);
    let mut low_area = (core::f32::consts::PI * lowest_switch * lowest_switch).min(LOD_MAX_AREA);
    let mut lowest_area = LOD_MAX_AREA;

    lowest_area -= low_area;
    low_area -= mid_area;
    mid_area -= high_area;

    high_area = high_area.clamp(LOD_MIN_AREA, LOD_MAX_AREA);
    mid_area = mid_area.clamp(LOD_MIN_AREA, LOD_MAX_AREA);
    low_area = low_area.clamp(LOD_MIN_AREA, LOD_MAX_AREA);
    lowest_area = lowest_area.clamp(LOD_MIN_AREA, LOD_MAX_AREA);

    let total = high_area + mid_area + low_area + lowest_area;
    if total <= 0.0 {
        return tris.last().copied().unwrap_or(0.0);
    }
    let weight = |index: usize, area: f32| tris.get(index).copied().unwrap_or(0.0) * (area / total);
    weight(0, lowest_area) + weight(1, low_area) + weight(2, mid_area) + weight(3, high_area)
}

/// The reference's `LLVOVolume::getTextureCost`: a flat base plus a term in the
/// texture's full resolution. An undecoded texture is charged the base alone —
/// as in the reference, whose full width / height are zero until the asset
/// header lands.
fn texture_cost(dimensions: Option<(u32, u32)>) -> f32 {
    let (width, height) = dimensions.unwrap_or((0, 0));
    TEXTURE_BASE_COST
        + TEXTURE_RESOLUTION_COST * (f32_from_u32(height) / 128.0 + f32_from_u32(width) / 128.0)
}

/// One prim's render cost, adding every texture it uses to `textures` (so the
/// linkset charges each unique texture once, as the reference does) and its
/// surface area to `area`.
///
/// The reference's `LLVOVolume::getRenderCost`, in its order: triangles → the
/// per-face multipliers → the per-prim multipliers → the additive charges.
fn prim_render_cost(
    facts: &PrimComplexityFacts<'_>,
    lookup: &impl CostLookup,
    textures: &mut HashSet<TextureKey>,
    pending: &mut PendingCostAssets,
) -> f32 {
    let radius = facts.scale.length() * 0.5;
    let rigged = facts.mesh.is_some_and(|mesh| lookup.mesh_rigged(mesh));
    let animesh_mesh = facts.animated && rigged;

    let triangles = match facts.mesh {
        Some(mesh) => match lookup.mesh_lod_bytes(mesh) {
            Some(bytes) => {
                let tris = est_tris_by_lod(bytes);
                if animesh_mesh {
                    // An animated object is charged proportionally to its
                    // streaming cost rather than its on-screen size.
                    ANIMESH_COST_PER_KTRI * 0.001 * est_tris_for_streaming(tris)
                        / ANIMESH_COST_SCALE
                } else {
                    radius_weighted_tris(tris, radius)
                }
            }
            None => {
                // The header has not landed; charge the fallback for now and
                // re-score this avatar when it does.
                pending.meshes.push(mesh);
                FALLBACK_TRIANGLES
            }
        },
        None => radius_weighted_tris(prim_est_tris(facts.shape), radius),
    };

    let mut cost = (triangles * COST_PER_TRIANGLE).max(MIN_PRIM_COST);

    // A legacy sculpt's map counts as one of the prim's textures.
    if let Some(map) = facts.sculpt_map {
        let _fresh = textures.insert(map);
    }

    // The blob is run-length encoded with a default that applies to every face,
    // so decoding it needs a face count it does not itself carry; the most a prim
    // can have is the safe ask (a simpler prim just repeats its default).
    let entry = decode_texture_entry(facts.texture_entry, MAX_PRIM_FACES);
    let flags = FaceFlags::of(&entry, textures);

    if flags.planar {
        cost *= PLANAR_MULT;
    }
    if facts.texture_animated {
        cost *= ANIM_TEX_MULT;
    }
    if flags.alpha {
        cost *= ALPHA_MULT;
    }
    if flags.invisible {
        cost *= INVISI_MULT;
    }
    if flags.glow {
        cost *= GLOW_MULT;
    }
    if flags.bump {
        cost *= BUMP_MULT;
    }
    if flags.shiny {
        cost *= SHINY_MULT;
    }
    if rigged {
        cost *= WEIGHTED_MESH_MULT;
    }
    if facts.flexi {
        cost *= FLEXI_MULT;
    }

    if let Some(burst) = lookup.particles(facts.entity) {
        cost += burst.count.min(PARTICLE_MAX) * burst.size * PARTICLE_COST;
    }
    if facts.light {
        cost += LIGHT_COST;
    }
    cost += f32_from_u32(flags.media_faces) * MEDIA_FACE_COST;
    if facts.animated && facts.is_root {
        cost += ANIMESH_BASE_COST / ANIMESH_COST_SCALE * COST_PER_TRIANGLE;
    }

    cost
}

/// The triangle count an animated object's streaming cost is computed from — the
/// finest level's, which is what the simulator charges it for.
fn est_tris_for_streaming(tris: [f32; PRIM_LOD_COUNT]) -> f32 {
    tris.last().copied().unwrap_or(0.0)
}

/// A non-mesh prim's per-level triangle estimate, taken through the same
/// byte-size estimator the reference feeds it through
/// (`LLVOVolume::getCostData`'s `counts[i] * 10`), so a prim and a mesh are
/// weighted on one scale.
fn prim_est_tris(shape: PrimShapeParams) -> [f32; PRIM_LOD_COUNT] {
    let counts = lod_triangle_counts(&sl_client_bevy::PrimShapeFloat::from_params(&shape));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "a prim's triangle count times ten is non-negative and far inside u32"
    )]
    est_tris_by_lod(counts.map(|count| (f32_from_u32(count) * PRIM_BYTES_PER_TRIANGLE) as u32))
}

/// The per-face render-cost flags of one prim, folded over its texture entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per multiplier the reference applies; folding them into enums \
              would obscure the formula this mirrors"
)]
struct FaceFlags {
    /// Any face is (partly) transparent.
    alpha: bool,
    /// Any face uses a water-exclusion ("invisiprim") texture.
    invisible: bool,
    /// Any face glows.
    glow: bool,
    /// Any face carries a bump map.
    bump: bool,
    /// Any face is shiny.
    shiny: bool,
    /// Any face uses planar texture-coordinate generation.
    planar: bool,
    /// How many faces have media enabled.
    media_faces: u32,
}

impl FaceFlags {
    /// Fold a decoded texture entry into its cost flags, collecting each face's
    /// texture into `textures`.
    fn of(entry: &TextureEntry, textures: &mut HashSet<TextureKey>) -> Self {
        let mut flags = Self::default();
        for face in &entry.faces {
            let _fresh = textures.insert(face.texture_id);
            // The reference tests whether the face landed in the alpha draw
            // pool; a non-opaque tint is what puts it there for a prim face.
            if face.color.last().copied().unwrap_or(u8::MAX) < u8::MAX {
                flags.alpha = true;
            } else if face.is_water_exclusion() {
                flags.invisible = true;
            }
            if face.media_enabled() {
                flags.media_faces = flags.media_faces.saturating_add(1);
            }
            if face.bumpmap() != 0 {
                flags.bump = true;
            }
            if face.shininess() != 0 {
                flags.shiny = true;
            }
            if face.glow > 0.0 {
                flags.glow = true;
            }
            if face.is_planar_texgen() {
                flags.planar = true;
            }
        }
        flags
    }
}

/// The most faces a single prim can have (the reference's `MAX_TES`).
const MAX_PRIM_FACES: usize = 9;

/// One prim's contribution to the wearer's attachment surface area, in square
/// metres: the surface area of its scaled bounding box.
fn prim_surface_area(scale: Vec3) -> f32 {
    2.0 * scale
        .x
        .mul_add(scale.y, scale.y.mul_add(scale.z, scale.z * scale.x))
}

/// Score one avatar: their visible body regions plus every worn (non-HUD)
/// attachment linkset, each clamped as the reference clamps it.
///
/// Pure in its inputs, so the whole formula is unit-testable: `linksets` is the
/// avatar's worn linksets, each a list of that linkset's prims (the root first).
pub(crate) fn avatar_complexity(
    visible_bake_regions: usize,
    linksets: &[Vec<PrimComplexityFacts<'_>>],
    lookup: &impl CostLookup,
    pending: &mut PendingCostAssets,
) -> AvatarComplexity {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "the body cost is a small multiple of a region count"
    )]
    let body_cost = (BODY_PART_COST * usize_as_f32(visible_bake_regions)) as u32;
    let mut total = f32_from_u32(body_cost);
    let mut surface_area = 0.0_f32;
    for linkset in linksets {
        let mut textures: HashSet<TextureKey> = HashSet::new();
        let mut linkset_cost = 0.0_f32;
        let mut animesh = false;
        for prim in linkset {
            linkset_cost += prim_render_cost(prim, lookup, &mut textures, pending);
            surface_area += prim_surface_area(prim.scale);
            animesh |= prim.animated;
        }
        if animesh {
            linkset_cost += ANIMESH_ATTACHMENT_SURCHARGE;
        }
        for texture in &textures {
            let dimensions = lookup.texture_dimensions(*texture);
            if dimensions.is_none() {
                // Charged the base for now; the wearer is re-scored when the
                // texture's real size is known.
                pending.textures.push(*texture);
            }
            linkset_cost += texture_cost(dimensions);
        }
        total += linkset_cost.clamp(0.0, MAX_ATTACHMENT_COMPLEXITY);
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "the total is clamped into u32's range before the cast"
    )]
    let score = total.clamp(0.0, f32_from_u32(u32::MAX)) as u32;
    AvatarComplexity {
        score,
        body_cost,
        attachments: linksets.len(),
        surface_area,
    }
}

// ---------------------------------------------------------------------------
// Numeric helpers.
// ---------------------------------------------------------------------------

/// A `u32` as `f32`. Costs and sizes are far below the precision threshold that
/// would matter here.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "cost arithmetic is approximate by construction; f32 covers the range"
)]
const fn f32_from_u32(value: u32) -> f32 {
    value as f32
}

/// A `usize` count as `f32` (a handful of body regions).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "the value is a small count"
)]
const fn usize_as_f32(value: usize) -> f32 {
    value as f32
}

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// Registers the avatar render-cost model, its settings, the scoring pass and
/// the jellydoll render.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AvatarComplexityPlugin;

impl Plugin for AvatarComplexityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AvatarComplexityModel>()
            .add_systems(Startup, register_complexity_settings)
            .add_systems(
                Update,
                // The scoring inputs are marked before the scene mirror folds the
                // frame's events (a removal still resolves to its wearer then),
                // and the score / decision follow it.
                mark_complexity_dirty
                    .before(crate::objects::update_objects)
                    .before(crate::avatars::update_avatar_objects),
            )
            .add_systems(
                Update,
                (
                    sync_complexity_settings,
                    sync_complexity_friends,
                    recompute_avatar_complexity,
                    decide_avatar_appearance,
                )
                    .chain()
                    .after(crate::objects::update_objects)
                    .after(crate::avatars::update_avatar_objects)
                    // The base-region visibility override reads the decision, so
                    // it must already be made this frame.
                    .before(crate::avatars::apply_avatar_part_visibility),
            )
            .add_systems(
                Update,
                // Applied last: the jellydoll overrides the materials and the
                // visibilities every other avatar pass has just settled.
                apply_jellydoll
                    .after(decide_avatar_appearance)
                    .after(crate::avatars::assign_avatar_bake_materials)
                    .after(crate::avatars::apply_avatar_part_visibility)
                    .after(crate::avatars::apply_bom_face_materials),
            );
    }
}

/// Register the complexity-limiting settings.
fn register_complexity_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        RENDER_SECTION,
        SETTING_MAX_COMPLEXITY,
        SettingValue::U32(DEFAULT_MAX_COMPLEXITY),
        "Draw an avatar costing more than this as a flat silhouette (0 = no limit)",
    );
    settings.register_in(
        RENDER_SECTION,
        SETTING_COMPLEXITY_MODE,
        SettingValue::U32(ComplexityMode::ByComplexity.stored()),
        "How the avatar complexity limit treats friends: 0 by complexity alone, \
         1 always draw friends fully, 2 draw only friends fully",
    );
    settings.register_in(
        RENDER_SECTION,
        SETTING_SURFACE_AREA_LIMIT,
        SettingValue::F32(DEFAULT_SURFACE_AREA_LIMIT),
        "Draw an avatar whose attachments cover more than this many square metres \
         as a flat silhouette (0 = no area limit)",
    );
}

/// Mirror the settings into the model, so the decision reads one plain struct.
pub(crate) fn sync_complexity_settings(
    mut model: ResMut<AvatarComplexityModel>,
    settings: Option<Res<ViewerSettings>>,
) {
    let Some(settings) = settings else {
        return;
    };
    let store = settings.store();
    let wanted = ComplexityLimits {
        max: store.get_u32(SETTING_MAX_COMPLEXITY).unwrap_or(0),
        mode: ComplexityMode::from_stored(store.get_u32(SETTING_COMPLEXITY_MODE).unwrap_or(0)),
        area: store.get_f32(SETTING_SURFACE_AREA_LIMIT).unwrap_or(0.0),
    };
    if model.limits != wanted {
        info!(?wanted, "avatar complexity limits changed");
        model.limits = wanted;
        model.bump();
    }
}

/// Keep the friends mirror current, so the decision pass reads one hash set
/// rather than rebuilding the roster every frame.
pub(crate) fn sync_complexity_friends(
    mut model: ResMut<AvatarComplexityModel>,
    friends: Option<Res<FriendsModel>>,
) {
    let revision = friends.as_deref().map(FriendsModel::revision);
    if model.friends_revision == revision {
        return;
    }
    model.friends_revision = revision;
    model.friends = friends
        .as_deref()
        .map(|friends| {
            friends
                .friend_ids()
                .into_iter()
                .map(AgentKey::from)
                .collect()
        })
        .unwrap_or_default();
    model.bump();
}

/// Mark an avatar's score stale whenever something it is made of changed: one of
/// its attachments arrived, moved between linksets or left; its appearance (and
/// so its baked body regions) changed; or an asset a previous score was waiting
/// on finally decoded.
///
/// Runs **before** the scene mirror folds the frame's events, so a removed
/// object can still be chased up to the avatar that was wearing it.
pub(crate) fn mark_complexity_dirty(
    mut events: MessageReader<SlEvent>,
    mut meshes: MessageReader<MeshDecoded>,
    mut textures: MessageReader<TextureDecoded>,
    mut model: ResMut<AvatarComplexityModel>,
    objects: Res<ObjectState>,
    avatars: Res<AvatarState>,
) {
    let mark_scoped = |model: &mut AvatarComplexityModel, wearer: Option<ScopedObjectId>| {
        if let Some(agent) = wearer.and_then(|scoped| avatars.agent_of_scoped(scoped)) {
            model.mark_dirty(agent);
        }
    };
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                // An attachment root names its avatar directly; a linked child of
                // one has to be chased up through the objects already tracked.
                let wearer = if object.attachment_point_id().is_some() {
                    Some(object.scoped_parent_id())
                } else {
                    objects.wearer_of(object.scoped_id())
                };
                mark_scoped(&mut model, wearer);
            }
            SlSessionEvent::ObjectRemoved { local_id, .. } => {
                let wearer = objects.wearer_of(*local_id);
                mark_scoped(&mut model, wearer);
            }
            SlSessionEvent::AvatarAppearance(appearance) => {
                model.mark_dirty(appearance.avatar_id);
            }
            _other => {}
        }
    }
    for MeshDecoded(mesh) in meshes.read() {
        if let Some(waiting) = model.awaiting_mesh.remove(mesh) {
            for agent in waiting {
                model.mark_dirty(agent);
            }
        }
    }
    for TextureDecoded(texture) in textures.read() {
        if let Some(waiting) = model.awaiting_texture.remove(texture) {
            for agent in waiting {
                model.mark_dirty(agent);
            }
        }
    }
}

/// The scene-backed [`CostLookup`]: the mesh and texture stores plus the live
/// particle sources.
struct SceneLookup<'world> {
    /// The mesh store, for asset headers and skin blocks.
    meshes: &'world MeshManager,
    /// The texture store, for full-resolution dimensions.
    textures: &'world TextureManager,
    /// Every live particle source in the scene, by prim entity. Collected up
    /// front (there are few of them) rather than held as a query, so the lookup
    /// is a plain borrow-free map.
    particles: HashMap<Entity, ParticleBurst>,
}

impl CostLookup for SceneLookup<'_> {
    fn mesh_lod_bytes(&self, mesh: MeshKey) -> Option<[u32; PRIM_LOD_COUNT]> {
        let header: &MeshHeader = self.meshes.header(mesh)?;
        let mut bytes = [0_u32; PRIM_LOD_COUNT];
        for (slot, block) in bytes.iter_mut().zip(header.lods.iter()) {
            *slot = block.map_or(0, |block| u32::try_from(block.size).unwrap_or(u32::MAX));
        }
        Some(bytes)
    }

    fn mesh_rigged(&self, mesh: MeshKey) -> bool {
        self.meshes.skin(mesh).is_some()
    }

    fn texture_dimensions(&self, texture: TextureKey) -> Option<(u32, u32)> {
        self.textures.native_dimensions(texture)
    }

    fn particles(&self, prim: Entity) -> Option<ParticleBurst> {
        self.particles.get(&prim).copied()
    }
}

/// Re-score the avatars whose cost went stale — at most [`RESCORE_BUDGET`] per
/// frame, and no more often than [`RESCORE_INTERVAL_SECS`] apiece.
pub(crate) fn recompute_avatar_complexity(
    time: Res<Time>,
    mut model: ResMut<AvatarComplexityModel>,
    objects: Res<ObjectState>,
    avatars: Res<AvatarState>,
    meshes: Res<MeshManager>,
    textures: Res<TextureManager>,
    particles: Query<(Entity, &ObjectParticleSystem)>,
) {
    let known: HashSet<AgentKey> = avatars
        .known_agents()
        .into_iter()
        .map(|(agent, _anchor)| agent)
        .collect();
    model.retain_known(known);
    if model.dirty.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    let due: Vec<AgentKey> = model
        .dirty
        .iter()
        .filter(|agent| model.known.contains(agent))
        .filter(|agent| {
            model
                .scored_at
                .get(agent)
                .is_none_or(|last| now - last >= RESCORE_INTERVAL_SECS)
        })
        .take(RESCORE_BUDGET)
        .copied()
        .collect();
    if due.is_empty() {
        // Everything still marked is inside its debounce window (or gone); the
        // marks stay so the next frame can pick them up.
        return;
    }
    let worn = objects.attachment_roots_by_wearer();
    let lookup = SceneLookup {
        meshes: &meshes,
        textures: &textures,
        particles: particles
            .iter()
            .map(|(prim, source)| (prim, ParticleBurst::of(&source.system)))
            .collect(),
    };
    for agent in due {
        let mut pending = PendingCostAssets::default();
        let linksets: Vec<Vec<ScopedObjectId>> = worn
            .iter()
            .filter(|(wearer, _roots)| avatars.agent_of_scoped(**wearer) == Some(agent))
            .flat_map(|(_wearer, roots)| roots.iter().map(|root| objects.linkset_members(root)))
            .collect();
        let facts: Vec<Vec<PrimComplexityFacts<'_>>> = linksets
            .iter()
            .map(|members| {
                members
                    .iter()
                    .filter_map(|scoped| objects.complexity_facts(scoped))
                    .collect()
            })
            .collect();
        let complexity = avatar_complexity(
            avatars.visible_bake_count(agent),
            &facts,
            &lookup,
            &mut pending,
        );
        for mesh in pending.meshes {
            let _fresh = model.awaiting_mesh.entry(mesh).or_default().insert(agent);
        }
        for texture in pending.textures {
            let _fresh = model
                .awaiting_texture
                .entry(texture)
                .or_default()
                .insert(agent);
        }
        let changed = model.scores.get(&agent) != Some(&complexity);
        let _previous = model.scores.insert(agent, complexity);
        let _stamped = model.scored_at.insert(agent, now);
        let _serviced = model.dirty.remove(&agent);
        if changed {
            model.bump();
            debug!(
                %agent,
                score = complexity.score,
                body = complexity.body_cost,
                attachments = complexity.attachments,
                area = complexity.surface_area,
                "avatar render cost measured"
            );
        }
    }
}

/// Re-decide who is drawn as a jellydoll, from the current scores, limits,
/// overrides and friends list. Cheap — a handful of comparisons per avatar — and
/// run every frame so a settings change, a new friend or a fresh score takes
/// effect at once.
pub(crate) fn decide_avatar_appearance(
    mut model: ResMut<AvatarComplexityModel>,
    identity: Res<SlIdentity>,
    mut decided_at: Local<Option<(u64, Option<AgentKey>)>>,
) {
    let own = identity.agent_id;
    // Every input the decision reads bumps the model revision, so an unchanged
    // revision (and own agent) means an unchanged answer.
    let inputs = (model.revision(), own);
    if *decided_at == Some(inputs) {
        return;
    }
    *decided_at = Some(inputs);
    let limits = model.limits;
    let mut decided: HashMap<AgentKey, JellyReason> = HashMap::new();
    for agent in &model.known {
        let reason = jelly_reason(
            own == Some(*agent),
            model.friends.contains(agent),
            model.override_of(*agent),
            model.scores.get(agent).copied(),
            limits,
        );
        if let Some(reason) = reason {
            let _held = decided.insert(*agent, reason);
        }
    }
    for (agent, reason) in &decided {
        if model.jellied.get(agent) != Some(reason) {
            info!(
                %agent,
                ?reason,
                score = model.scores.get(agent).map_or(0, |score| score.score),
                "drawing avatar as a jellydoll"
            );
        }
    }
    for agent in model.jellied.keys() {
        if !decided.contains_key(agent) {
            info!(%agent, "drawing avatar in full again");
        }
    }
    if model.jellied != decided {
        model.jellied = decided;
        model.bump();
    }
}

/// Draw the decision: hide every jellied avatar's attachments (their own faces
/// included, since a rigged one's hang off the wearer's body root), paint their
/// system body flat, and put both back exactly as they were when they are drawn
/// in full again.
///
/// Runs after the bake / BoM material passes and the base-region visibility
/// pass, so it overrides what they settled rather than racing them.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system: the scene state it reads plus the two things it writes"
)]
pub(crate) fn apply_jellydoll(
    time: Res<Time>,
    mut model: ResMut<AvatarComplexityModel>,
    objects: Res<ObjectState>,
    mut avatars: ResMut<AvatarState>,
    mut materials: ResMut<Assets<FaceMaterial>>,
    mut visibilities: Query<&mut Visibility>,
    mut parts: Query<(&AvatarBodyPart, &mut MeshMaterial3d<FaceMaterial>)>,
    mut applied_at: Local<Option<(u64, f64)>>,
) {
    // Nothing to draw and nothing to put back: the overwhelmingly common case
    // (the limit is off by default), and the one that must cost nothing.
    if !model.has_jelly_work() {
        return;
    }
    // Finding the hidden set walks every tracked object, so it runs when the
    // decision moved and otherwise only on a slow sweep — which catches an
    // attachment that streamed in for an already-jellied avatar without its
    // wearer's score changing.
    let now = time.elapsed_secs_f64();
    let stale = applied_at.is_none_or(|(revision, at)| {
        revision != model.revision() || now - at >= JELLY_SWEEP_SECONDS
    });
    if !stale {
        return;
    }
    *applied_at = Some((model.revision(), now));

    // Restore whatever is no longer hidden — before hiding this frame's set, so
    // an entity that stays hidden is never restored and re-hidden.
    let worn = objects.attachment_roots_by_wearer();
    let mut wanted: HashSet<Entity> = HashSet::new();
    for (wearer, roots) in &worn {
        let Some(agent) = avatars.agent_of_scoped(*wearer) else {
            continue;
        };
        if !model.jellied.contains_key(&agent) {
            continue;
        }
        for root in roots {
            for member in objects.linkset_members(root) {
                if let Some(entity) = objects.entity_by_scoped(&member) {
                    let _fresh = wanted.insert(entity);
                }
                for face in objects.face_entities_of(&member) {
                    let _fresh = wanted.insert(*face);
                }
            }
        }
    }
    let released: Vec<Entity> = model
        .hidden
        .keys()
        .filter(|entity| !wanted.contains(entity))
        .copied()
        .collect();
    for entity in released {
        let restored = model.hidden.remove(&entity);
        if let (Some(restored), Ok(mut visibility)) = (restored, visibilities.get_mut(entity)) {
            visibility.set_if_neq(restored);
        }
    }
    for entity in wanted {
        let Ok(mut visibility) = visibilities.get_mut(entity) else {
            continue;
        };
        if let std::collections::hash_map::Entry::Vacant(slot) = model.hidden.entry(entity) {
            let _remembered = slot.insert(*visibility);
        }
        visibility.set_if_neq(Visibility::Hidden);
    }

    // The flat silhouette material, built once.
    let jelly = match model.material.clone() {
        Some(handle) => handle,
        None => {
            let handle = materials.add(jelly_material());
            model.material = Some(handle.clone());
            handle
        }
    };
    for (part, mut material) in &mut parts {
        if !model.jellied.contains_key(&part.agent()) {
            // Not jellied: the bake pass owns this material — leave it alone.
            continue;
        }
        if material.0 != jelly {
            *material = MeshMaterial3d(jelly.clone());
        }
    }
    // Hand the region materials back for everyone who stopped being a jellydoll:
    // the bake pass only re-drapes an avatar it is told is dirty, so without this
    // an un-limited avatar would keep the flat silhouette for good.
    let restored: Vec<AgentKey> = model
        .painted
        .iter()
        .filter(|agent| !model.jellied.contains_key(agent))
        .copied()
        .collect();
    for agent in restored {
        let _dropped = model.painted.remove(&agent);
        avatars.mark_bake_dirty(agent);
    }
    let jellied: Vec<AgentKey> = model.jellied.keys().copied().collect();
    model.painted.extend(jellied);
}

/// The flat, unlit jellydoll material.
fn jelly_material() -> FaceMaterial {
    inert_face_material(StandardMaterial {
        base_color: JELLY_COLOR,
        unlit: true,
        ..default()
    })
}

/// How a jellied avatar's base body regions are shown: every region the avatar
/// has (so a mesh-body wearer whose system body is baked invisible still has a
/// silhouette) except the hair, which the reference also drops.
///
/// This is the reference's `updateMeshVisibility` jellydoll branch, which clears
/// the whole `bake_flag` array — the attachment-driven region hides — and its
/// `getOverallAppearance() != AOA_JELLYDOLL` guard on the hair mesh.
pub(crate) const fn jellied_region_visible(region: BodyRegion) -> bool {
    !matches!(region, BodyRegion::Hair)
}

#[cfg(test)]
mod tests {
    use super::{
        AvatarComplexity, AvatarComplexityModel, ComplexityLimits, ComplexityMode, JellyReason,
        ParticleBurst, PendingCostAssets, RenderOverride, avatar_complexity, est_tris_by_lod,
        jelly_reason, radius_weighted_tris, texture_cost,
    };
    use crate::objects::PrimComplexityFacts;
    use bevy::prelude::{Entity, Vec3};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, MeshKey, PRIM_LOD_COUNT, PrimShapeParams, TextureKey, Uuid};
    use std::collections::HashMap;

    /// A lookup with nothing in it: no mesh headers, no texture sizes, no
    /// particles — the state a freshly streamed crowd is in.
    #[derive(Default)]
    struct TestLookup {
        /// The per-level block sizes of each known mesh.
        meshes: HashMap<MeshKey, [u32; PRIM_LOD_COUNT]>,
        /// The meshes that carry a skin block.
        rigged: Vec<MeshKey>,
        /// The known texture dimensions.
        textures: HashMap<TextureKey, (u32, u32)>,
    }

    impl super::CostLookup for TestLookup {
        fn mesh_lod_bytes(&self, mesh: MeshKey) -> Option<[u32; PRIM_LOD_COUNT]> {
            self.meshes.get(&mesh).copied()
        }

        fn mesh_rigged(&self, mesh: MeshKey) -> bool {
            self.rigged.contains(&mesh)
        }

        fn texture_dimensions(&self, texture: TextureKey) -> Option<(u32, u32)> {
            self.textures.get(&texture).copied()
        }

        fn particles(&self, _prim: Entity) -> Option<ParticleBurst> {
            None
        }
    }

    /// The wire params for a plain unit box.
    fn box_params() -> PrimShapeParams {
        PrimShapeParams {
            path_curve: 0x10,
            profile_curve: 0x01,
            path_scale_x: 100,
            path_scale_y: 100,
            ..PrimShapeParams::default()
        }
    }

    /// A plain one-prim attachment: an un-textured, un-flagged box.
    fn plain_prim() -> PrimComplexityFacts<'static> {
        PrimComplexityFacts {
            entity: Entity::PLACEHOLDER,
            scale: Vec3::splat(0.5),
            shape: box_params(),
            mesh: None,
            sculpt_map: None,
            texture_entry: &[],
            flexi: false,
            light: false,
            animated: false,
            is_root: true,
            texture_animated: false,
        }
    }

    /// A body with no attachments costs exactly its visible baked regions.
    #[test]
    fn body_regions_are_the_floor() {
        let lookup = TestLookup::default();
        let mut pending = PendingCostAssets::default();
        let complexity = avatar_complexity(5, &[], &lookup, &mut pending);
        assert_eq!(complexity.score, 1000, "five regions at 200 apiece");
        assert_eq!(complexity.body_cost, 1000);
        assert_eq!(complexity.attachments, 0);
        assert!(complexity.surface_area.abs() < f32::EPSILON);
    }

    /// The per-prim multipliers compound the way the reference applies them, and
    /// each is worth what it says: a flexi, glowing, transparent prim costs
    /// 4 × 1.5 × 5 = thirty times the same prim plain.
    #[test]
    fn multipliers_compound() {
        let lookup = TestLookup::default();
        let mut pending = PendingCostAssets::default();
        let plain = avatar_complexity(0, &[vec![plain_prim()]], &lookup, &mut pending);

        // The same prim, but half-transparent and glowing, and flexi.
        let entry = sl_client_bevy::encode_texture_entry(&sl_client_bevy::TextureEntry {
            faces: vec![sl_client_bevy::TextureFace {
                color: [255, 255, 255, 128],
                glow: 0.5,
                ..sl_client_bevy::TextureFace::new(TextureKey::from(Uuid::from_u128(9)))
            }],
        });
        let mut loud = plain_prim();
        loud.texture_entry = entry.leak();
        loud.flexi = true;
        let loud = avatar_complexity(0, &[vec![loud]], &lookup, &mut pending);

        // The loud prim carries one texture's charge the plain one does not, so
        // take that off before comparing the geometry halves.
        let plain_geometry = f64::from(plain.score);
        let loud_geometry = f64::from(loud.score) - 256.0;
        let ratio = loud_geometry / plain_geometry;
        assert!(
            (ratio - 30.0).abs() < 1.0,
            "alpha (4) x glow (1.5) x flexi (5) = 30, got {ratio} \
             (plain {plain_geometry}, loud {loud_geometry})"
        );
    }

    /// A texture is charged once per linkset however many faces use it, and its
    /// resolution term is the reference's.
    #[test]
    fn textures_are_charged_once_per_linkset() {
        assert!(
            (texture_cost(None) - 256.0).abs() < 0.01,
            "an unknown texture is the base"
        );
        assert!(
            (texture_cost(Some((1024, 1024))) - (256.0 + 16.0 * 16.0)).abs() < 0.01,
            "1024 square is 256 + 16 x (8 + 8)"
        );

        let shared = TextureKey::from(Uuid::from_u128(7));
        let entry = sl_client_bevy::encode_texture_entry(&sl_client_bevy::TextureEntry {
            faces: vec![sl_client_bevy::TextureFace::new(shared)],
        });
        let leaked: &'static [u8] = entry.leak();
        let mut first = plain_prim();
        first.texture_entry = leaked;
        let mut second = plain_prim();
        second.texture_entry = leaked;
        second.is_root = false;

        let lookup = TestLookup {
            textures: HashMap::from([(shared, (512, 512))]),
            ..TestLookup::default()
        };
        let mut pending = PendingCostAssets::default();
        let one = avatar_complexity(0, &[vec![first]], &lookup, &mut pending);
        let two = avatar_complexity(0, &[vec![plain_prim(), second]], &lookup, &mut pending);
        let texture_charge = 256.0 + 16.0 * 8.0;
        assert!(
            f64::from(two.score) - f64::from(one.score) < texture_charge,
            "the second prim adds geometry, not a second charge for the same texture"
        );
    }

    /// A mesh whose header has not landed is charged the fallback and recorded
    /// as pending, so the avatar is re-scored when it does.
    #[test]
    fn a_missing_mesh_header_is_pending() {
        let mesh = MeshKey::from(Uuid::from_u128(3));
        let mut prim = plain_prim();
        prim.mesh = Some(mesh);
        let lookup = TestLookup::default();
        let mut pending = PendingCostAssets::default();
        let _unresolved = avatar_complexity(0, &[vec![prim]], &lookup, &mut pending);
        assert_eq!(pending.meshes, vec![mesh]);

        // With the header, a big mesh costs far more than the fallback.
        let lookup = TestLookup {
            meshes: HashMap::from([(mesh, [4_000, 40_000, 400_000, 900_000])]),
            rigged: vec![mesh],
            ..TestLookup::default()
        };
        let mut pending = PendingCostAssets::default();
        let mut prim = plain_prim();
        prim.mesh = Some(mesh);
        let scored = avatar_complexity(0, &[vec![prim]], &lookup, &mut pending);
        assert!(pending.meshes.is_empty(), "the header resolved");
        assert!(
            scored.score > 1000,
            "a heavy rigged mesh is expensive, got {}",
            scored.score
        );
    }

    /// The level estimator fills an absent level from the next finer one and
    /// discounts the header bytes, as the reference does.
    #[test]
    fn level_estimates_backfill_and_discount() {
        let tris = est_tris_by_lod([0, 0, 0, 0x4000]);
        let finest = tris.last().copied().unwrap_or(0.0);
        assert!((finest - (16_384.0 - 384.0) / 16.0).abs() < 0.01);
        assert!(
            (tris.first().copied().unwrap_or(0.0) - finest).abs() < 0.01,
            "a mesh with only a high block is charged for it at every distance"
        );

        // The radius weighting is dominated by the coarse levels for a small
        // attachment and by the fine ones for a huge object.
        let tris = [10.0, 100.0, 1000.0, 10_000.0];
        let small = radius_weighted_tris(tris, 0.25);
        let huge = radius_weighted_tris(tris, 64.0);
        assert!(
            small < 100.0,
            "a small attachment is mostly seen at its coarse levels, got {small}"
        );
        assert!(
            huge > small * 10.0,
            "a huge object is mostly seen at its fine ones, got {huge}"
        );
    }

    /// The decision's priority order, straight from the reference.
    #[test]
    fn the_decision_follows_the_reference_priority() {
        let heavy = Some(AvatarComplexity {
            score: 500_000,
            surface_area: 10.0,
            ..AvatarComplexity::default()
        });
        let limits = ComplexityLimits {
            max: 100_000,
            mode: ComplexityMode::ByComplexity,
            area: 1000.0,
        };

        assert_eq!(
            jelly_reason(true, false, RenderOverride::Normal, heavy, limits),
            None,
            "you are never jellied to yourself"
        );
        assert_eq!(
            jelly_reason(false, false, RenderOverride::Normal, heavy, limits),
            Some(JellyReason::TooComplex)
        );
        assert_eq!(
            jelly_reason(false, false, RenderOverride::AlwaysFull, heavy, limits),
            None,
            "Render Fully beats the budget"
        );
        assert_eq!(
            jelly_reason(false, true, RenderOverride::Never, None, limits),
            Some(JellyReason::Override),
            "Never Render beats being a friend and needs no score"
        );

        // A budget of zero disables the automatic rules, area trigger included.
        let unlimited = ComplexityLimits { max: 0, ..limits };
        let sprawling = Some(AvatarComplexity {
            score: 1,
            surface_area: 100_000.0,
            ..AvatarComplexity::default()
        });
        assert_eq!(
            jelly_reason(false, false, RenderOverride::Normal, sprawling, unlimited),
            None,
            "unlimited means unlimited"
        );
        assert_eq!(
            jelly_reason(false, false, RenderOverride::Normal, sprawling, limits),
            Some(JellyReason::TooMuchArea),
            "the area trigger catches what a triangle count cannot"
        );
    }

    /// The friends modes: one spares friends from the budget, the other jellies
    /// everyone who is not one — without needing a score at all.
    #[test]
    fn friend_modes_short_circuit_the_budget() {
        let heavy = Some(AvatarComplexity {
            score: 500_000,
            ..AvatarComplexity::default()
        });
        let spare_friends = ComplexityLimits {
            max: 100_000,
            mode: ComplexityMode::AlwaysShowFriends,
            area: 0.0,
        };
        assert_eq!(
            jelly_reason(false, true, RenderOverride::Normal, heavy, spare_friends),
            None
        );
        assert_eq!(
            jelly_reason(false, false, RenderOverride::Normal, heavy, spare_friends),
            Some(JellyReason::TooComplex)
        );

        let only_friends = ComplexityLimits {
            mode: ComplexityMode::OnlyShowFriends,
            ..spare_friends
        };
        assert_eq!(
            jelly_reason(false, false, RenderOverride::Normal, None, only_friends),
            Some(JellyReason::NotAFriend),
            "an unscored stranger is jellied on sight in this mode"
        );
        assert_eq!(
            jelly_reason(false, true, RenderOverride::Normal, heavy, only_friends),
            None,
            "a friend is drawn in full whatever they cost"
        );
    }

    /// The jelly render must still run on the frame an avatar stops being a
    /// jellydoll, even when it hid nothing for them — otherwise the body it
    /// painted flat is never handed back to the bake pass. (A live run caught
    /// exactly this: an avatar wearing no attachments hides nothing, so both
    /// other sets were already empty and the pass skipped itself.)
    #[test]
    fn a_painted_body_alone_is_still_work() {
        let mut model = AvatarComplexityModel::default();
        assert!(!model.has_jelly_work(), "an idle viewer does nothing");
        model.painted.insert(AgentKey::from(Uuid::from_u128(1)));
        assert!(
            model.has_jelly_work(),
            "a body still wearing the silhouette must be handed back"
        );
    }

    /// The stored mode numbering round-trips (a Firestorm value ports across).
    #[test]
    fn mode_numbering_round_trips() {
        for mode in [
            ComplexityMode::ByComplexity,
            ComplexityMode::AlwaysShowFriends,
            ComplexityMode::OnlyShowFriends,
        ] {
            assert_eq!(ComplexityMode::from_stored(mode.stored()), mode);
        }
        assert_eq!(
            ComplexityMode::from_stored(99),
            ComplexityMode::ByComplexity,
            "an unrecognised value falls back to the plain budget"
        );
    }
}
