//! HUD attachments (P35.1): the screen-space layer a worn HUD hangs off, and the
//! classification that routes an attachment onto it instead of into the world.
//!
//! An attachment worn on one of the eight **HUD points** (raw attachment-point
//! ids `31`..=`38` — `Center 2`, `Top Right`, `Top`, `Top Left`, `Center`,
//! `Bottom Left`, `Bottom`, `Bottom Right`) is not part of the world at all: the
//! reference viewer hangs those points off a pseudo-joint `mScreen` that sits
//! outside the skeleton and renders them in a separate screen-space pass
//! (`LLVOVolume::isHUDAttachment` → `PARTITION_HUD`, the id range checked
//! verbatim there for the same backwards-compatibility reason). Everything else
//! in the viewer parents an attachment to a *body* joint, so before this phase a
//! HUD attachment never resolved a joint at all: it was held pending forever and
//! its geometry sat parentless at the world origin.
//!
//! This module owns the other end of that route:
//!
//! - [`setup_hud_screen`] spawns the **HUD screen** — the `mScreen` equivalent —
//!   plus one node per HUD point at the point's fixed `avatar_lad.xml` offset,
//!   exactly as [`crate::avatars`] spawns the body's attachment-point nodes off
//!   their joints (P16.2). The screen carries the single Second Life → Bevy basis
//!   change, so the subtree below it stays in Second Life space like every other
//!   attachment subtree;
//! - the whole subtree is put on the dedicated [`HUD_RENDER_LAYER`] (propagated
//!   down the hierarchy), which no camera renders yet — so a HUD is *out of the
//!   world scene* but not yet drawn. P35.2 adds the orthographic HUD camera that
//!   renders that layer, anchors the screen to the viewport, and scales it by the
//!   aspect ratio (the reference's `mScreen` scale of `(1, aspect, 1)`).
//!
//! Only the **agent's own** HUD attachments are routed here. The reference viewer
//! creates the HUD joints for `isSelf()` only (`LLVOAvatar::initAttachmentPoints`
//! skips every `hud="true"` point on another avatar), so another avatar's HUD
//! attachment never attaches and never renders — the grid does not normally send
//! them at all, but a HUD worn by someone else must not turn into world geometry
//! if one arrives. [`crate::objects::adopt_pending_attachments`] hides those.

use std::collections::HashMap;

use bevy::app::Propagate;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Hdr, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use sl_client_bevy::AttachmentPoint;

use crate::avatar_assets::AvatarAssetLibrary;
use crate::coords::{sl_euler_deg_to_quat, sl_to_bevy_rotation};

/// The render layer the whole HUD subtree lives on, and which the world (fly)
/// camera — on the default layer `0` — therefore does not render. P35.2's HUD
/// camera renders this layer and nothing else, so the HUD is drawn exactly once,
/// in screen space, and never leaks into the world pass (or into a reflection
/// probe's capture, which is likewise a default-layer camera).
pub(crate) const HUD_RENDER_LAYER: usize = 1;

/// Whether a raw attachment-point id names a HUD (screen-space) slot rather than
/// a body joint — the reference viewer's `LLVOVolume::isHUDAttachment`, which
/// tests the same `31..=38` id range.
pub(crate) const fn is_hud_point(point_id: u8) -> bool {
    AttachmentPoint::from_code(point_id).is_hud()
}

/// Whether an entity's render layers put it on the HUD layer — i.e. whether it is
/// part of the HUD subtree rather than the world scene.
///
/// The HUD screen propagates [`HUD_RENDER_LAYER`] down its hierarchy, so every
/// entity of a routed HUD attachment (its object entity, its geometry holder, and
/// each face) carries it. The world's pixel-area render-priority / level-of-detail
/// pass uses this to recognise geometry it must not rank by on-screen size: a HUD
/// sits in its own space, where the world camera's distance to it is meaningless
/// (the reference viewer special-cases it the same way, treating every HUD face as
/// full-screen and pinning it to the finest level of detail).
///
/// `layers` is the entity's [`RenderLayers`] component, absent on a world entity
/// (which is then implicitly on the default layer `0`).
pub(crate) fn on_hud_layer(layers: Option<&RenderLayers>) -> bool {
    layers.is_some_and(|layers| layers.intersects(&RenderLayers::layer(HUD_RENDER_LAYER)))
}

/// The HUD screen: the root of the screen-space HUD hierarchy, standing in for the
/// reference viewer's `mScreen` pseudo-joint (`LLVOAvatarSelf::buildSkeletonSelf`).
///
/// It carries the Second Life → Bevy basis change, so its children — the HUD point
/// nodes and, below them, the routed attachments — stay in Second Life space, the
/// same convention a world linkset (and a body attachment subtree) follows. P35.2
/// places and scales it against the HUD camera.
#[derive(Component, Debug)]
pub(crate) struct HudScreen;

/// One HUD attachment point (raw id `31`..=`38`): a child of the [`HudScreen`] at
/// the point's fixed `avatar_lad.xml` offset, onto which an attachment worn on
/// that point is parented — the screen-space mirror of a body attachment-point
/// node (P16.2).
///
/// It carries its own **unscaled** offset because the live one is aspect-dependent:
/// [`fit_hud_points`] re-derives the node's translation from it whenever the window
/// aspect changes, so the corner points stay in the viewport's corners (P35.2).
#[derive(Component, Debug)]
pub(crate) struct HudPointNode {
    /// The point's fixed `avatar_lad.xml` offset from the screen (Second Life
    /// Z-up: `+y` screen-left, `+z` screen-up), before the aspect anchoring.
    offset: Vec3,
}

/// The HUD camera (P35.2): the orthographic, screen-relative view that draws the
/// HUD layer — and only it — over the finished world frame.
#[derive(Component, Debug)]
pub(crate) struct HudCamera;

/// The spawned HUD point nodes, keyed by raw attachment-point id, so an
/// attachment can be routed to the node for its point.
///
/// Empty when the run has no avatar assets (no `--viewer-assets`): the HUD point
/// offsets come from `avatar_lad.xml`, so without it there is no HUD screen and a
/// HUD attachment is hidden rather than routed (the same degradation that leaves
/// avatars as placeholder spheres).
#[derive(Resource, Debug, Default)]
pub(crate) struct HudState {
    /// The HUD point node entities, keyed by raw attachment-point id.
    points: HashMap<u8, Entity>,
}

impl HudState {
    /// The node entity a HUD attachment worn on `point_id` parents to, or `None`
    /// if there is no HUD screen (no avatar assets) or the id is not a HUD point.
    pub(crate) fn point_entity(&self, point_id: u8) -> Option<Entity> {
        self.points.get(&point_id).copied()
    }
}

/// Startup system: spawn the [`HudScreen`] and its eight HUD point nodes from the
/// loaded avatar assets (P35.1).
///
/// Each node sits at its point's `avatar_lad.xml` `position` / `rotation` offset
/// from the screen — the same offsets the reference viewer hangs off `mScreen`
/// (`Top` at `(0, 0, 0.5)`, `Bottom Left` at `(0, 0.5, -0.5)`, …, in Second Life's
/// Z-up frame, where `+y` is screen-left and `+z` is screen-up). A run without
/// avatar assets has no attachment-point table, so no screen is spawned and HUD
/// attachments are hidden instead of routed.
pub(crate) fn setup_hud_screen(
    mut commands: Commands,
    library: Option<Res<AvatarAssetLibrary>>,
    mut hud: ResMut<HudState>,
) {
    let Some(library) = library else {
        info!("no avatar assets: no HUD screen; HUD attachments stay hidden");
        return;
    };
    let points = library.hud_attachment_points();
    if points.is_empty() {
        warn!("avatar assets define no HUD attachment points; HUD attachments stay hidden");
        return;
    }
    let root = commands
        .spawn((
            HudScreen,
            // The single Second Life → Bevy basis change for the HUD subtree, so
            // the point offsets (and each attachment's own local transform) stay in
            // Second Life space, exactly as under a world linkset root or an avatar.
            Transform::from_rotation(sl_to_bevy_rotation()),
            Visibility::default(),
            // Put the whole subtree — the point nodes, the attachments routed onto
            // them, and their faces — on the HUD layer, so the world camera does not
            // render it and P35.2's HUD camera does.
            Propagate(RenderLayers::layer(HUD_RENDER_LAYER)),
        ))
        .id();
    for (point_id, offset) in points {
        let node = commands
            .spawn((
                HudPointNode {
                    offset: Vec3::from_array(offset.position),
                },
                Transform {
                    // The aspect anchoring is applied by `fit_hud_points` on the
                    // first frame (it needs the window); the unscaled offset is the
                    // right thing to start from.
                    translation: Vec3::from_array(offset.position),
                    rotation: sl_euler_deg_to_quat(offset.rotation_euler_deg),
                    scale: Vec3::ONE,
                },
                Visibility::default(),
                ChildOf(root),
            ))
            .id();
        hud.points.insert(point_id, node);
    }
    // The camera that draws them (P35.2): orthographic, looking down the HUD's
    // depth axis, rendering the HUD layer over the finished world frame.
    let camera = commands
        .spawn((
            HudCamera,
            Camera3d::default(),
            Camera {
                // After the world camera (order 0), and without clearing what it
                // drew: the HUD is composited over the finished frame, exactly where
                // the reference viewer draws it — `render_hud_attachments` runs in
                // `render_ui`, *after* `renderFinalize`'s tonemap and post effects.
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            // The reference's HUD projection (`get_hud_matrices`):
            // `ortho(-0.5 * aspect, 0.5 * aspect, -0.5, 0.5, …)` — a fixed **1.0**
            // vertical extent, the horizontal one widening with the aspect. So HUD
            // space is `[-0.5, 0.5]` top-to-bottom whatever the window, and geometry
            // keeps its proportions (a HUD metre is the same many pixels either way).
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: 1.0,
                },
                near: 0.0,
                far: 2.0 * HUD_CAMERA_DEPTH,
                ..OrthographicProjection::default_3d()
            }),
            // Down the HUD's depth axis: Second Life `+x` (into the screen) with
            // `+z` up — the reference's `OGL_TO_CFR_ROTATION` HUD modelview, and the
            // basis the screen's children are already expressed in. Under the
            // screen's basis change that is Bevy `+x` forward, `+y` up. Standing
            // `HUD_CAMERA_DEPTH` back of the screen origin (with `near = 0`) keeps
            // content *behind* the screen plane visible too, the way the reference's
            // bounding-box-fitted near plane does.
            Transform::from_xyz(-HUD_CAMERA_DEPTH, 0.0, 0.0).looking_to(Vec3::X, Vec3::Y),
            // The HUD layer, and nothing else: the world is invisible to this camera
            // and this camera is the only one that sees the HUD.
            RenderLayers::layer(HUD_RENDER_LAYER),
            // Must match the world camera's sample count and HDR-ness — the two share
            // the window's view-target chain, and this camera draws *into* the frame
            // the world camera left there (`ClearColorConfig::None` above).
            Msaa::Sample4,
            Hdr,
            // No tone mapping (the world's `SlTonemap` already ran over the frame this
            // camera draws onto) and no underwater fog: the reference likewise skips
            // atmospherics on HUDs (`sRenderingHUDs`).
            Tonemapping::None,
        ))
        .id();
    info!(
        "spawned HUD screen {root} with {} point node(s) and HUD camera {camera} on render layer {HUD_RENDER_LAYER}",
        hud.points.len()
    );
}

/// How far back of the screen origin the HUD camera stands, in HUD metres (P35.2):
/// the depth range it sees is `[-HUD_CAMERA_DEPTH, +HUD_CAMERA_DEPTH]` around the
/// screen plane. Ample for any HUD (whose content sits within a metre or two of the
/// screen) and, being orthographic, it costs nothing in apparent size — the
/// reference instead fits the range to the HUD's bounding box each frame, purely so
/// its near plane can sit at the content.
const HUD_CAMERA_DEPTH: f32 = 64.0;

/// Keep each HUD point node anchored to its corner of the viewport as the window's
/// aspect ratio changes (P35.2).
///
/// The reference viewer scales the `mScreen` joint by `(1, aspect, 1)`, and a
/// joint's scale multiplies its **children's position offsets** (`LLXformMatrix`'s
/// `mScaleChildOffset`) — but never their geometry, which keeps its own scale. So a
/// point at `y = ±0.5` lands at `±0.5 * aspect`, the edge of the
/// `[-0.5 * aspect, 0.5 * aspect]` projection, while a HUD prim stays square.
///
/// A Bevy `Transform` scale *would* reach the geometry below, so the anchoring is
/// applied to the node translations here instead — the same arithmetic, at the only
/// place the reference actually applies it.
pub(crate) fn fit_hud_points(
    windows: Query<&Window>,
    mut points: Query<(&HudPointNode, &mut Transform)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let height = window.height();
    if height <= 0.0 {
        return;
    }
    let aspect = window.width() / height;
    for (point, mut transform) in &mut points {
        let anchored = anchored_point_offset(point.offset, aspect);
        if transform.translation != anchored {
            transform.translation = anchored;
        }
    }
}

/// A HUD point's `avatar_lad.xml` `offset` anchored for the viewport's `aspect`
/// ratio: its across-screen component (Second Life `y`) scaled by the aspect, so a
/// point at `y = ±0.5` sits at the edge of the `[-0.5 * aspect, 0.5 * aspect]`
/// projection whatever the window's shape. The reference viewer's `mScreen` scale of
/// `(1, aspect, 1)`, which reaches exactly this — its children's offsets — and
/// nothing else.
fn anchored_point_offset(offset: Vec3, aspect: f32) -> Vec3 {
    Vec3::new(offset.x, offset.y * aspect, offset.z)
}

/// The faces [`apply_hud_fullbright`] reconsiders: those whose material handle or
/// render layer just changed — a freshly spawned face, a face the propagation just
/// carried onto the HUD layer, and a face whose material a later render-material
/// pipeline swapped.
type RelitFaces<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static MeshMaterial3d<StandardMaterial>,
        &'static RenderLayers,
    ),
    Or<(
        Changed<MeshMaterial3d<StandardMaterial>>,
        Changed<RenderLayers>,
    )>,
>;

/// Render every HUD face fullbright (P35.2).
///
/// The reference viewer forces `LLFace::FULLBRIGHT` on the faces of a HUD
/// attachment (`LLVOVolume::setupFaces`) and skips atmospherics for them
/// (`LLPipeline::sRenderingHUDs`): a HUD is a screen overlay, and it would be absurd
/// for it to darken at dusk. The same holds here for a second reason — a Bevy light
/// only lights the layers it is on, and the world's sun is on the world layer, so a
/// lit HUD material would simply render black.
///
/// [`face_material`](crate::textures::face_material) builds a fresh, unshared
/// `StandardMaterial` per face, so flipping `unlit` here cannot leak into world
/// geometry. It runs on faces whose material or layer *changed*, which covers a face
/// spawned under an already-routed attachment (its layer arrives a frame later, with
/// the propagation) and one whose material is swapped later by the render-material
/// pipelines.
///
/// Deviation, deliberately: the reference exempts a face with a **PBR** material
/// (`isHUDAttachment() && !is_pbr`), leaving it lit. Here that would render it black,
/// so every HUD face goes fullbright.
pub(crate) fn apply_hud_fullbright(
    faces: RelitFaces,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (face, layers) in &faces {
        if !on_hud_layer(Some(layers)) {
            continue;
        }
        if let Some(mut material) = materials.get_mut(&face.0)
            && !material.unlit
        {
            material.unlit = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HUD_CAMERA_DEPTH, HUD_RENDER_LAYER, HudScreen, anchored_point_offset, is_hud_point,
        on_hud_layer,
    };
    use crate::coords::sl_to_bevy_rotation;
    use bevy::app::{App, HierarchyPropagatePlugin, PostUpdate, Propagate};
    use bevy::camera::visibility::RenderLayers;
    use bevy::ecs::hierarchy::ChildOf;
    use bevy::math::Vec3;
    use bevy::transform::components::Transform;

    /// A 16:9 viewport, the shape the layout is most often seen in.
    const WIDE_ASPECT: f32 = 16.0 / 9.0;

    /// Only the eight screen slots (31–38) are HUD points; the body points on
    /// either side of the range are not.
    #[test]
    fn hud_points_are_31_to_38() {
        assert!(!is_hud_point(0));
        assert!(!is_hud_point(30), "Left Pec is a body point");
        for point_id in 31..=38 {
            assert!(is_hud_point(point_id), "{point_id} is a HUD point");
        }
        assert!(!is_hud_point(39), "Neck is a body point");
        assert!(!is_hud_point(55));
    }

    /// The HUD layer is not the default (world) layer, so a world entity — which
    /// carries no `RenderLayers` at all — is never taken for HUD geometry, and the
    /// world camera (default layer) does not render the HUD subtree.
    #[test]
    fn hud_layer_is_not_the_world_layer() {
        assert!(!on_hud_layer(None), "a world entity has no render layers");
        assert!(!on_hud_layer(Some(&RenderLayers::default())));
        assert!(on_hud_layer(Some(&RenderLayers::layer(HUD_RENDER_LAYER))));
    }

    /// The HUD layer reaches the whole routed subtree — the attachment's object
    /// entity, its geometry holder, and each face — including one parented *after*
    /// the screen was spawned, which is the only way it ever happens: an attachment
    /// is routed frames (or minutes) after startup, and its faces are spawned later
    /// still, when the mesh decodes. Were the layer not propagated down, the HUD
    /// geometry would keep rendering in the world.
    #[test]
    fn hud_layer_propagates_to_the_routed_subtree() {
        let mut app = App::new();
        app.add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate));
        let screen = app
            .world_mut()
            .spawn((
                HudScreen,
                Transform::default(),
                Propagate(RenderLayers::layer(HUD_RENDER_LAYER)),
            ))
            .id();
        app.update();

        // An attachment routed onto a point node, and a face spawned under it a
        // frame later (the mesh / texture decode arriving late).
        let point = app
            .world_mut()
            .spawn((Transform::default(), ChildOf(screen)))
            .id();
        let object = app
            .world_mut()
            .spawn((Transform::default(), ChildOf(point)))
            .id();
        app.update();
        let face = app
            .world_mut()
            .spawn((Transform::default(), ChildOf(object)))
            .id();
        app.update();

        for entity in [point, object, face] {
            let layers = app.world().entity(entity).get::<RenderLayers>();
            assert!(
                on_hud_layer(layers),
                "entity {entity} of the HUD subtree is not on the HUD layer: {layers:?}"
            );
        }
        // A world entity outside the subtree keeps the default (world) layer.
        let world_entity = app.world_mut().spawn(Transform::default()).id();
        app.update();
        assert!(!on_hud_layer(
            app.world().entity(world_entity).get::<RenderLayers>()
        ));
    }

    /// A corner point lands in the corner of the viewport, whatever its shape: the
    /// projection is `1.0` tall and `aspect` wide, and the across-screen offset is
    /// scaled to match (the reference's `mScreen` scale). The *up*-screen offset is
    /// never touched — the vertical extent is fixed — and neither is depth.
    #[test]
    fn corner_points_anchor_to_the_viewport_corners() {
        // "Top Left" (id 34), `position="0 0.5 0.5"`: half a screen up, and hard
        // against the left edge — which is `0.5 * aspect` in a projection that spans
        // `[-0.5 * aspect, 0.5 * aspect]` across.
        let top_left = anchored_point_offset(Vec3::new(0.0, 0.5, 0.5), WIDE_ASPECT);
        assert!(
            (top_left.y - 0.5 * WIDE_ASPECT).abs() < 1e-6,
            "{top_left:?}"
        );
        assert!((top_left.z - 0.5).abs() < 1e-6, "{top_left:?}");
        // A square viewport is the one shape where the anchoring is a no-op.
        let square = anchored_point_offset(Vec3::new(0.0, 0.5, 0.5), 1.0);
        assert!(
            (square - Vec3::new(0.0, 0.5, 0.5)).length() < 1e-6,
            "{square:?}"
        );
        // "Center" (id 35) is the screen centre in every viewport.
        let centre = anchored_point_offset(Vec3::ZERO, WIDE_ASPECT);
        assert!(centre.length() < 1e-6, "{centre:?}");
    }

    /// The HUD camera looks down the HUD's depth axis with the screen the right way
    /// up and the right way round: a point that is *up* in Second Life's HUD frame
    /// (`+z`) is up in view space (`+y`), and one that is *screen-left* (`+y`) is
    /// left of the view centre (`-x`). Getting this wrong mirrors every HUD.
    #[test]
    fn the_camera_frames_the_screen_upright_and_unmirrored() {
        let camera = Transform::from_xyz(-HUD_CAMERA_DEPTH, 0.0, 0.0).looking_to(Vec3::X, Vec3::Y);
        let view = camera.to_matrix().inverse();
        let to_bevy = sl_to_bevy_rotation();
        // "Top Left" in Second Life HUD space, carried into Bevy by the screen's
        // basis change, then into the camera's view space.
        let top_left = view.transform_point3(to_bevy * Vec3::new(0.0, 0.5, 0.5));
        assert!(
            top_left.y > 0.0,
            "top-left is above the centre: {top_left:?}"
        );
        assert!(top_left.x < 0.0, "top-left is left of centre: {top_left:?}");
        // The camera stands back of the screen plane, so the screen (and content
        // behind it) is in front of it — a negative view-space z, Bevy's forward.
        assert!(top_left.z < 0.0, "the screen is in front: {top_left:?}");
        let bottom_right = view.transform_point3(to_bevy * Vec3::new(0.0, -0.5, -0.5));
        assert!(bottom_right.y < 0.0, "{bottom_right:?}");
        assert!(bottom_right.x > 0.0, "{bottom_right:?}");
    }

    /// The `avatar_lad.xml` HUD point offsets are in Second Life's Z-up frame
    /// (`+y` screen-left, `+z` screen-up); the HUD screen's basis change carries
    /// them into Bevy, where screen-up becomes `+y` and screen-left `-z` (the HUD
    /// camera P35.2 spawns looks along `+x`, so `x` is depth and `z` is across).
    #[test]
    fn point_offsets_land_in_screen_space() {
        let to_bevy = sl_to_bevy_rotation();
        // "Top" (id 33), `position="0 0 0.5"`: straight up, no depth, not across.
        let top = to_bevy * Vec3::new(0.0, 0.0, 0.5);
        assert!((top - Vec3::new(0.0, 0.5, 0.0)).length() < 1e-6, "{top:?}");
        // "Bottom Right" (id 38), `position="0 -0.5 -0.5"`: down and — since Second
        // Life's `+y` is screen-left — across to `+z` in Bevy.
        let bottom_right = to_bevy * Vec3::new(0.0, -0.5, -0.5);
        assert!(
            (bottom_right - Vec3::new(0.0, -0.5, 0.5)).length() < 1e-6,
            "{bottom_right:?}"
        );
        // "Center" (id 35), `position="0 0 0"`: the screen origin itself.
        let center = to_bevy * Vec3::ZERO;
        assert!((center - Vec3::ZERO).length() < 1e-6, "{center:?}");
    }
}
