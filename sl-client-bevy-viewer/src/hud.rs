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
/// node (P16.2). Which point a node stands for is [`HudState::point_entity`]'s
/// key; the marker is what makes the nodes queryable as a set.
#[derive(Component, Debug)]
pub(crate) struct HudPointNode;

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
                HudPointNode,
                Transform {
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
    info!(
        "spawned HUD screen {root} with {} point node(s) on render layer {HUD_RENDER_LAYER}",
        hud.points.len()
    );
}

#[cfg(test)]
mod tests {
    use super::{HUD_RENDER_LAYER, HudScreen, is_hud_point, on_hud_layer};
    use crate::coords::sl_to_bevy_rotation;
    use bevy::app::{App, HierarchyPropagatePlugin, PostUpdate, Propagate};
    use bevy::camera::visibility::RenderLayers;
    use bevy::ecs::hierarchy::ChildOf;
    use bevy::math::Vec3;
    use bevy::transform::components::Transform;

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
