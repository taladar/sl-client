//! Screen-space overlay that renders avatar name tags **outside** `bevy_ui`.
//!
//! The tags used to be absolutely-positioned `bevy_ui` text nodes, and moving
//! them meant writing `Node.left`/`Node.top` for every avatar every frame —
//! each write dirtied taffy and re-triggered the change-driven parts of
//! `ui_layout_system`, and every tag inflated the unconditional per-frame
//! bevy_ui walks (layout geometry, stack rebuild, clipping, picking, styling).
//! See the `viewer-perf-ui-layout-per-frame-relayout` roadmap task.
//!
//! Now each tag is a [`Text2d`](bevy::sprite::Text2d) drawn by a dedicated
//! overlay [`Camera2d`] and positioned by writing its [`Transform`] — which
//! dirties nothing but that entity's own transform propagation. Only 2D
//! cameras can render `Text2d`, so the world cameras and the reflection-probe
//! capture cameras structurally cannot see tags; [`NAME_TAG_RENDER_LAYER`]
//! additionally keeps them apart from any future 2D content.
//!
//! This is deliberately the **minimal** extraction — same screen-space look as
//! the old UI tags (always on top of the world, constant size, no occlusion).
//! The full in-world treatment (backdrop bubble, outline, occlusion/depth,
//! size clamping, distance fade + cut-off) is the
//! `viewer-name-tags-billboard-render` roadmap task, which replaces the
//! projection in this module with true world-space placement.
//!
//! Because there is no `Text2d` picking backend in Bevy 0.19, cursor
//! interaction with tags is a manual rect test, [`NameTagHitTest`] — shared by
//! the right-click avatar-menu resolver today and built for reuse by the
//! planned inventory drag&drop-onto-avatar (any consumer with a screen
//! position can ask "which avatar's tag is under this point?").

use bevy::camera::Hdr;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::text::TextLayoutInfo;
use sl_client_bevy::AgentKey;

use crate::avatars::{AvatarPickTarget, NameTag};

/// The render layer name tags (and only name tags) live on, seen only by the
/// overlay camera. Distinct from the HUD layer (1, [`crate::hud`]) and the
/// edit-gizmo layer (3, [`crate::gizmos`]); the world lives on the default 0.
pub(crate) const NAME_TAG_RENDER_LAYER: usize = 2;

/// The overlay camera's render order: after the world camera (0), the
/// edit-gizmo overlay (1) and the HUD attachments (2), so tags draw over the
/// finished world frame — and *before* the UI pass, which bevy_ui runs on this
/// same camera (it is the [`IsDefaultUiCamera`]), so floaters and menus still
/// draw over tags exactly as they did when tags were low-z-index UI nodes.
const NAME_TAG_CAMERA_ORDER: isize = 3;

/// Marker for the tag overlay [`Camera2d`].
///
/// The camera is spawned at the **identity transform and never moved** — the
/// coordinate mapping in [`overlay_point_from_viewport`] relies on overlay
/// world space being the window's logical pixel grid centred on the origin.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct NameTagCamera;

/// Spawn the tag overlay camera (Startup).
///
/// [`IsDefaultUiCamera`] is **load-bearing**: without it, bevy_ui falls back
/// to "the highest-order window camera", which this camera silently becomes —
/// marking it makes the UI retarget explicit and keeps the UI pass rendering
/// after the tags (same window and scale factor, so no relayout results).
pub(crate) fn spawn_name_tag_overlay_camera(mut commands: Commands) {
    commands.spawn((
        Name::new("name tag overlay camera"),
        NameTagCamera,
        Camera2d,
        Camera {
            order: NAME_TAG_CAMERA_ORDER,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(NAME_TAG_RENDER_LAYER),
        IsDefaultUiCamera,
        // Must match the world camera's sample count and HDR-ness — all cameras
        // on the window share its view-target chain, and a mismatched (SDR)
        // camera splits off onto a *different* intermediate texture: the world
        // then never reaches the surface at all (black window) while this
        // pass's `ClearColorConfig::None` accumulates tag after-images. Same
        // constraint, same fix as the HUD camera (`crate::hud`).
        Msaa::Sample4,
        Hdr,
        // No tone mapping: the world's tonemap already ran over the frame this
        // camera draws onto, and plain white tag text needs none.
        Tonemapping::None,
    ));
}

/// Map a viewport/window position (logical pixels, top-left origin, +Y down —
/// what `Camera::world_to_viewport` and `Window::cursor_position` produce) to
/// the overlay camera's 2D world space (logical pixels, **centre** origin,
/// +Y **up** — the identity-transform `Camera2d` default).
pub(crate) fn overlay_point_from_viewport(viewport: Vec2, viewport_size: Vec2) -> Vec2 {
    Vec2::new(
        viewport.x - viewport_size.x / 2.0,
        viewport_size.y / 2.0 - viewport.y,
    )
}

/// The overlay-space rectangle a tag covers: its [`Anchor::BOTTOM_CENTER`]
/// (bevy::sprite::Anchor) translation plus its laid-out logical size (for
/// `Text2d`, [`TextLayoutInfo::size`] is already divided back to logical
/// units by the layout system).
pub(crate) fn tag_rect(translation: Vec2, size: Vec2) -> Rect {
    Rect::new(
        translation.x - size.x / 2.0,
        translation.y,
        translation.x + size.x / 2.0,
        translation.y + size.y,
    )
}

/// Cursor-vs-name-tag hit testing, shared by every screen-position
/// interaction with tags.
///
/// Consumers: the right-click avatar-menu resolver and the pick inspector
/// ([`crate::avatar_menu`]) today; built for the planned inventory
/// drag&drop-onto-avatar, which will call [`Self::agent_at`] with a drag
/// position instead of a click.
#[derive(SystemParam)]
pub(crate) struct NameTagHitTest<'w, 's> {
    /// The window, for the viewport→overlay coordinate mapping.
    windows: Query<'w, 's, &'static Window>,
    /// Every tag's identity, overlay position, laid-out size and visibility.
    /// Reads `Transform`, not `GlobalTransform`: tags are root entities (the
    /// two are identical) and the freshly-written `Transform` avoids the
    /// one-frame `GlobalTransform` propagation lag.
    tags: Query<
        'w,
        's,
        (
            &'static AvatarPickTarget,
            &'static Transform,
            &'static TextLayoutInfo,
            &'static Visibility,
        ),
        With<NameTag>,
    >,
}

impl NameTagHitTest<'_, '_> {
    /// The avatar whose visible tag contains `cursor` (window logical
    /// coordinates), if any.
    pub(crate) fn agent_at(&self, cursor: Vec2) -> Option<AgentKey> {
        let window = self.windows.single().ok()?;
        let point = overlay_point_from_viewport(cursor, window.size());
        self.tags
            .iter()
            .find_map(|(target, transform, layout, visibility)| {
                (*visibility != Visibility::Hidden
                    && tag_rect(transform.translation.truncate(), layout.size).contains(point))
                .then(|| target.agent())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{overlay_point_from_viewport, tag_rect};
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use crate::avatars::{AvatarAnchor, NameTag, position_name_tags};
    use crate::camera::ViewerCamera;

    /// How many tag `Transform`s were written (change-detected) this frame,
    /// recorded by [`count_tag_writes`] right after the projection ran.
    #[derive(Resource, Default)]
    struct TagWrites(usize);

    /// Record how many tags' `Transform` changed this frame (an `Added` counts,
    /// which is exactly right: the first projection is a write).
    fn count_tag_writes(
        mut writes: ResMut<TagWrites>,
        changed: Query<(), (Changed<Transform>, With<NameTag>)>,
    ) {
        writes.0 = changed.iter().count();
    }

    /// Headless end-to-end check of the projection: an on-screen anchor gets
    /// its tag placed at the overlay point matching its viewport projection and
    /// made visible — and a second frame with nothing moved writes **nothing**
    /// (the inequality guards), which is what keeps stationary scenes free of
    /// per-frame tag work.
    #[test]
    fn projection_places_tag_once_and_idles_when_stationary() {
        let mut app = App::new();
        app.init_resource::<TagWrites>();
        app.add_systems(Update, (position_name_tags, count_tag_writes).chain());
        app.world_mut().spawn(Window {
            resolution: UVec2::new(1280, 720).into(),
            ..default()
        });
        // A fake computed camera, the `ui_test` harness idiom: identity pose
        // looking down -Z with a symmetric orthographic clip sized to the
        // window, so a world point at (x, y, -z) projects to the viewport
        // point that maps back to overlay (x, y).
        app.world_mut().spawn((
            ViewerCamera,
            Camera {
                computed: ComputedCameraValues {
                    clip_from_view: Mat4::orthographic_rh(-640.0, 640.0, -360.0, 360.0, 0.1, 100.0),
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(1280, 720),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
            GlobalTransform::IDENTITY,
        ));
        let anchor = app
            .world_mut()
            .spawn((
                AvatarAnchor,
                GlobalTransform::from_translation(Vec3::new(100.0, 50.0, -10.0)),
            ))
            .id();
        let tag = app
            .world_mut()
            .spawn((
                Transform::default(),
                Visibility::Hidden,
                NameTag {
                    anchor,
                    tag_height: 0.0,
                },
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().get::<Transform>(tag).map(|t| t.translation),
            Some(Vec3::new(100.0, 50.0, 0.0))
        );
        assert_eq!(
            app.world().get::<Visibility>(tag).copied(),
            Some(Visibility::Inherited)
        );
        assert_eq!(app.world().resource::<TagWrites>().0, 1);

        // Nothing moved: the guarded writes must all skip.
        app.update();
        assert_eq!(app.world().resource::<TagWrites>().0, 0);
    }

    /// The viewport→overlay mapping recentres on the middle of the window and
    /// flips Y: top-left → (-w/2, +h/2), centre → origin, bottom-right →
    /// (+w/2, -h/2).
    #[test]
    fn overlay_mapping_recentres_and_flips_y() {
        let size = Vec2::new(1280.0, 720.0);
        assert_eq!(
            overlay_point_from_viewport(Vec2::ZERO, size),
            Vec2::new(-640.0, 360.0)
        );
        assert_eq!(
            overlay_point_from_viewport(Vec2::new(640.0, 360.0), size),
            Vec2::ZERO
        );
        assert_eq!(
            overlay_point_from_viewport(size, size),
            Vec2::new(640.0, -360.0)
        );
    }

    /// A bottom-centre-anchored tag's rect spans half its width either side of
    /// the anchor point and its full height *above* it (+Y up in overlay
    /// space).
    #[test]
    fn tag_rect_extends_up_from_bottom_centre() {
        assert_eq!(
            tag_rect(Vec2::new(10.0, 20.0), Vec2::new(100.0, 16.0)),
            Rect::new(-40.0, 20.0, 60.0, 36.0)
        );
    }

    /// The rect test that backs the hit test: points inside, on the anchor
    /// row, and just outside each edge.
    #[test]
    fn tag_rect_contains_expected_points() {
        let rect = tag_rect(Vec2::ZERO, Vec2::new(100.0, 16.0));
        assert!(rect.contains(Vec2::new(0.0, 8.0)));
        assert!(rect.contains(Vec2::new(-50.0, 0.0)));
        assert!(rect.contains(Vec2::new(50.0, 16.0)));
        assert!(!rect.contains(Vec2::new(0.0, -1.0)));
        assert!(!rect.contains(Vec2::new(0.0, 17.0)));
        assert!(!rect.contains(Vec2::new(-51.0, 8.0)));
    }
}
