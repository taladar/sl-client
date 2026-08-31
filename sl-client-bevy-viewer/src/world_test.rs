//! The **headless fixture world** ([[viewer-world-test-harness]]): `SlEvent`
//! in, `SlCommand` out, no socket, no renderer.
//!
//! Everything downstream of the network consumes public
//! [`SlEvent`]`(SessionEvent)` messages and everything outbound is a
//! [`SlCommand`] message; the socket lives only in `sl-client-bevy`'s `drive`
//! system. So a test stands up [`crate::viewer_plugins::ViewerWorldPlugins`]
//! over the testkit's input stack, writes the events a grid would have sent,
//! and reads the commands (and open-menu requests) the viewer would have sent
//! back — the in-world counterpart of the UI tier's `InteractionTest`.
//!
//! Picking is the real pipeline with the CPU resolver swapped in
//! ([[viewer-cpu-pick-resolver]]): the same registry, the same
//! `GpuPickResolved` channel, a `MeshRayCast` in place of the ID-buffer
//! render. Target *classification* (avatar vs object vs attachment vs land)
//! is the logic under test; each fixture is a fat target the cursor cannot
//! miss, because geometric pick *accuracy* belongs to the render tiers.

use bevy::camera::visibility::VisibilityPlugin;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

use sl_client_bevy::{
    Object, ScopedObjectId, SlCommand, SlEvent, SlIdentity, SlSessionEvent as SessionEvent, pcode,
};
use sl_viewer_testkit::{interact, record};

use crate::face_material::FaceMaterial;
use crate::viewer_plugins::ViewerWorldPlugins;
use crate::world_api::ViewerCamera;

/// The fixture window's viewport, physical pixels (scale factor 1).
const VIEWPORT: UVec2 = UVec2::new(800, 600);

/// Build the fixture world: task pools and assets, the testkit input stack
/// (window, synthetic pointer, picking core, focus), visibility propagation,
/// the login-parameter resources `run_session` would have inserted, and the
/// world fold with the CPU pick resolver. No UI scaffold, no render app.
pub(crate) fn world_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        AssetPlugin::default(),
        TransformPlugin,
    ));
    // The synthetic pointer/keyboard stack (window + input + picking + focus);
    // it also installs the manually-stepped clock.
    interact::install_input_stack(&mut app, VIEWPORT, 1.0);
    // The asset stores the world fold and the ray cast read; `Shader` because
    // plugins in the world group `load_internal_asset!` their overlays.
    app.init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<Shader>()
        .init_asset::<FaceMaterial>()
        .init_asset::<sl_client_bevy::TerrainMaterial>()
        .init_asset::<SkinnedMeshInverseBindposes>();
    // Visibility propagation: `InheritedVisibility` is what the CPU pick
    // resolver (and `MeshRayCast` in general) reads; its bounds systems also
    // compute each mesh's `Aabb`, which the ray cast's culling needs.
    app.add_plugins(VisibilityPlugin);
    // The login-parameter resources the world group expects `run_session` to
    // have inserted.
    app.insert_resource(crate::settings::ViewerSettings::declared_for_test(
        crate::REGISTRARS,
    ));
    app.insert_resource(crate::animations::AnimationManager::new(None));
    app.init_resource::<crate::camera::CameraStart>();
    app.init_resource::<SlIdentity>();
    // Resources world-group systems read but other groups own: the edit
    // tools' selection (`detach_shared_face_materials` reads it) and the
    // derender list (`update_objects` reads it).
    app.init_resource::<crate::world_api::SelectionSet>();
    app.init_resource::<crate::world_api::DerenderList>();
    // The input-context state `world_has_keyboard`-gated systems read; its
    // owning plugin is in the input group, which the fixture world skips.
    app.init_resource::<crate::world_api::InputContext>();
    // The avatar-complexity model the name-tag/complexity readers consult;
    // its owner sits with the preferences UI, outside the world group.
    app.init_resource::<sl_viewer_world_avatar::avatar_complexity::AvatarComplexityModel>();
    // The camera-mode state machine (mouselook / third person / flycam) the
    // look-at systems read; its owner is the input group's `CameraPlugin`.
    app.init_resource::<crate::world_api::CameraMode>();
    // The agent's current parcel (a `SlClientPlugin` world resource) and the
    // friends model (people-plugin-owned): the context menus consult both.
    app.init_resource::<sl_client_bevy::SlAgentParcel>();
    app.init_resource::<crate::world_api::FriendsModel>();
    // The build-tool state the `edit_tool_inactive` run condition reads; its
    // owner (`EditToolPlugin`) is in the edit group. Tests that drive the
    // gizmos flip `active` on this same resource.
    app.init_resource::<crate::world_api::EditToolState>();
    // The avatar render-exception settings the avatar pie consults; owned by
    // the preferences-side plugin.
    app.init_resource::<crate::avatar_render_settings::AvatarRenderSettings>();
    // The inventory model the object pie's take/buy handlers consult; owned
    // by the inventory UI plugin.
    app.init_resource::<sl_viewer_inventory::inventory::InventoryModel>();
    // The named-action button map (`InputActionPlugin` normally maintains
    // it); empty here, so no world key binding ever reads as pressed.
    app.init_resource::<ButtonInput<crate::input_action::Action>>();
    // The Texture-tab material mode (Blinn-Phong vs PBR) the material
    // appliers consult; owned by the edit-texture UI.
    app.init_resource::<crate::world_api::MatModeState>();
    // The GPU-avatar pose resources, in the inactive mode (no render app):
    // the animesh / pose systems validate against them.
    crate::gpu_avatars::init_headless_pose_resources(&mut app);
    app.add_message::<SlEvent>();
    // The capability-announcement channel the `update_*_caps` systems read;
    // `SlClientPlugin` (not present — no socket) normally registers it.
    app.add_message::<sl_client_bevy::SlCapabilities>();
    // The pie/widget action channel the menu dispatchers read; the UI
    // scaffold normally registers it. Recorded, so tests can also assert
    // which actions fired.
    record::<sl_viewer_ui_core::ui_element::UiAction>(&mut app);
    // Channels the world pie-menu handlers write into, registered by UI /
    // edit plugins the fixture world leaves out ( `add_message` is
    // idempotent, so double registration by a later `with_*` is harmless).
    app.add_message::<crate::derender::RequestDerender>();
    app.add_message::<crate::world_api::OpenAvatarProfile>();
    app.add_message::<crate::world_api::OpenConversation>();
    app.add_message::<crate::world_api::OpenAddToContactSet>();
    app.add_message::<crate::about_land::OpenAboutLand>();
    app.add_message::<crate::edit_contents::OpenObjectContents>();
    app.add_message::<crate::avatar_render_settings::RequestRenderException>();
    app.add_message::<crate::contact_sets_panel::OpenSetPseudonym>();
    app.add_message::<crate::world_api::MediaWorldClick>();
    // The rest of the world-api message vocabulary, registered wholesale:
    // world systems write into these channels whose owning UI plugins the
    // fixture world leaves out, and an unregistered `Messages<T>` fails
    // system-param validation with a panic the moment such a system runs.
    app.add_message::<crate::world_api::OpenGroupProfile>();
    app.add_message::<crate::world_api::OpenAvatarPicker>();
    app.add_message::<crate::world_api::AvatarPicked>();
    app.add_message::<crate::world_api::OpenTexturePicker>();
    app.add_message::<crate::world_api::TexturePicked>();
    app.add_message::<crate::world_api::OpenWebBrowser>();
    app.add_message::<crate::world_api::BeginTeleportFlow>();
    app.add_message::<crate::world_api::ContentsMutated>();
    app.add_message::<crate::world_api::OpenNotecard>();
    app.add_message::<crate::world_api::OpenScript>();
    app.add_message::<crate::world_api::StartConference>();
    // UI-sound requests (typing / menu feedback); the audio bridge that
    // consumes them lives in the shell group.
    app.add_message::<sl_viewer_ui_core::ui_sounds::PlayUiSound>();
    // Notification toasts (the block/mute handlers raise them); the
    // notification UI that consumes them is in the UI group.
    app.add_message::<sl_viewer_notifications::ShowNotification>();
    record::<SlCommand>(&mut app);
    app.add_plugins(ViewerWorldPlugins::cpu_pick());
    app
}

/// Give the world a [`ViewerCamera`] the pick resolver and the
/// viewport-to-world conversions can use: a hand-filled projection (no
/// renderer computes one) at `eye`, looking at `target`, in Bevy world
/// coordinates.
pub(crate) fn install_camera(app: &mut App, eye: Vec3, target: Vec3) {
    let aspect = 800.0 / 600.0;
    let pose = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    let camera = Camera {
        computed: bevy::camera::ComputedCameraValues {
            clip_from_view: Mat4::perspective_infinite_reverse_rh(
                core::f32::consts::FRAC_PI_3,
                aspect,
                0.1,
            ),
            target_info: Some(bevy::camera::RenderTargetInfo {
                physical_size: VIEWPORT,
                scale_factor: 1.0,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    // The component form of the same projection: `place_gizmo_rig` and other
    // camera readers take `&Projection`, not the computed matrix.
    let projection = Projection::Perspective(PerspectiveProjection {
        fov: core::f32::consts::FRAC_PI_3,
        aspect_ratio: aspect,
        ..Default::default()
    });
    // Reuse the entity a camera plugin may already have spawned, so "the
    // ViewerCamera" stays unique; spawn one otherwise.
    let mut cameras = app
        .world_mut()
        .query_filtered::<Entity, With<ViewerCamera>>();
    match cameras.single(app.world()) {
        Ok(entity) => {
            app.world_mut().entity_mut(entity).insert((
                camera,
                projection,
                pose,
                GlobalTransform::from(pose),
            ));
        }
        Err(_none) => {
            app.world_mut().spawn((
                camera,
                projection,
                pose,
                GlobalTransform::from(pose),
                ViewerCamera,
            ));
        }
    }
}

/// The fixture world with the transform gizmos on top. A separate builder
/// because plugins must be added before the app's first update.
pub(crate) fn world_app_with_edit() -> App {
    let mut app = world_app();
    app.add_plugins(crate::gizmos::EditGizmoPlugin);
    app
}

/// The fixture world with the real avatar-asset library: the vendored
/// `viewer-assets/character/` satisfies `setup_hud_screen` (so the HUD screen,
/// its point nodes and the HUD camera spawn), and the render-layer
/// propagation — pure ECS, normally registered by the render group —
/// materialises the `RenderLayers` the HUD pick paths filter by.
///
/// # Errors
///
/// Returns the load error when the vendored character directory is missing
/// or unparsable — a broken checkout, not a scene condition.
pub(crate) fn world_app_with_hud() -> Result<App, Box<dyn core::error::Error>> {
    let mut app = world_app();
    app.add_plugins(bevy::app::HierarchyPropagatePlugin::<
        bevy::camera::visibility::RenderLayers,
    >::new(PostUpdate));
    let vendored =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../viewer-assets/character");
    let library = crate::avatar_assets::AvatarAssetLibrary::load(&vendored)?;
    app.insert_resource(library);
    Ok(app)
}

/// Hand-fill the HUD camera's computed projection (no renderer computes one):
/// the same fixed-vertical orthographic projection `setup_hud_screen` gives
/// it, updated for the fixture viewport, so the HUD right-click path's
/// `viewport_to_world` works. `None` when no HUD camera spawned.
pub(crate) fn install_hud_camera_projection(app: &mut App) -> Option<()> {
    use bevy::camera::CameraProjection as _;
    let mut projection = OrthographicProjection {
        scaling_mode: bevy::camera::ScalingMode::FixedVertical {
            viewport_height: 1.0,
        },
        near: 0.0,
        far: 128.0,
        ..OrthographicProjection::default_3d()
    };
    // Through `u16` so the widening to `f32` is a lossless `From` (the
    // workspace bans `as` casts); the fixture viewport is far below either
    // limit.
    projection.update(
        f32::from(u16::try_from(VIEWPORT.x).unwrap_or(u16::MAX)),
        f32::from(u16::try_from(VIEWPORT.y).unwrap_or(u16::MAX)),
    );
    let camera = Camera {
        computed: bevy::camera::ComputedCameraValues {
            clip_from_view: projection.get_clip_from_view(),
            target_info: Some(bevy::camera::RenderTargetInfo {
                physical_size: VIEWPORT,
                scale_factor: 1.0,
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut cameras = app
        .world_mut()
        .query_filtered::<Entity, With<crate::hud::HudCamera>>();
    let entity = cameras.single(app.world()).ok()?;
    app.world_mut().entity_mut(entity).insert(camera);
    Some(())
}

/// A fat, unmissable prim: the shared [`crate::objects::fixture_object`]
/// seed, placed at `position` (SL region-local metres), streamed to the
/// viewer as the grid would — one `ObjectAdded`. Returns its scoped id.
pub(crate) fn seed_prim(app: &mut App, position: sl_client_bevy::Vector) -> ScopedObjectId {
    let mut object: Object = crate::objects::fixture_object(pcode::PRIMITIVE);
    object.motion.position = position;
    // An ordinary editable prim: the edit gates read these agent flags, and a
    // zero mask would read as "may not move" and refuse a gizmo drag.
    object.update_flags = crate::world_api::FLAGS_OBJECT_MODIFY
        | crate::world_api::FLAGS_OBJECT_MOVE
        | crate::world_api::FLAGS_OBJECT_COPY
        | crate::world_api::FLAGS_OBJECT_YOU_OWNER;
    let scoped = ScopedObjectId::new(object.circuit, object.local_id);
    app.world_mut()
        .write_message(SlEvent(SessionEvent::ObjectAdded(Box::new(object))));
    scoped
}

/// Stream one already-shaped [`Object`] to the viewer as an `ObjectAdded`.
pub(crate) fn seed_object(app: &mut App, object: Object) -> ScopedObjectId {
    let scoped = ScopedObjectId::new(object.circuit, object.local_id);
    app.world_mut()
        .write_message(SlEvent(SessionEvent::ObjectAdded(Box::new(object))));
    scoped
}

/// A (placeholder-sphere) avatar for `agent` at `position`, streamed as the
/// grid would — an avatar-pcode object whose full id **is** the agent id.
pub(crate) fn seed_avatar(
    app: &mut App,
    agent: sl_client_bevy::AgentKey,
    local_id: u32,
    position: sl_client_bevy::Vector,
) -> ScopedObjectId {
    let mut object: Object = crate::objects::fixture_object(pcode::AVATAR);
    object.local_id = sl_client_bevy::RegionLocalObjectId(local_id);
    object.full_id = sl_client_bevy::ObjectKey::from(agent.uuid());
    object.motion.position = position;
    seed_object(app, object)
}

/// A fat prim worn on `wearer_local`'s avatar at attachment point `point`
/// (SL codes — `1` is the chest; the wire packs the point nibble-swapped
/// into the state byte). Its position is joint-relative.
pub(crate) fn seed_attachment(
    app: &mut App,
    wearer_local: u32,
    local_id: u32,
    point: u8,
    position: sl_client_bevy::Vector,
) -> ScopedObjectId {
    let mut object: Object = crate::objects::fixture_object(pcode::PRIMITIVE);
    object.local_id = sl_client_bevy::RegionLocalObjectId(local_id);
    object.full_id =
        sl_client_bevy::ObjectKey::from(sl_client_bevy::Uuid::from_u128(u128::from(local_id)));
    object.parent_id = sl_client_bevy::RegionLocalObjectId(wearer_local);
    // The wire's nibble-swapped attachment-point encoding
    // (`attachment_point_from_state` inverts this).
    object.state = (point & 0x0f).wrapping_shl(4) | (point & 0xf0).wrapping_shr(4);
    object.motion.position = position;
    object.update_flags = crate::world_api::FLAGS_OBJECT_MODIFY
        | crate::world_api::FLAGS_OBJECT_MOVE
        | crate::world_api::FLAGS_OBJECT_COPY;
    seed_object(app, object)
}

/// One flat 16×16 land patch at the region's south-west corner, `height`
/// metres up — enough ground for a land pick.
pub(crate) fn seed_terrain(app: &mut App, height: f32) {
    let patch = sl_client_bevy::TerrainPatch {
        region_handle: sl_client_bevy::RegionHandle(0),
        layer: sl_client_bevy::TerrainLayerType::Land,
        patch_x: 0,
        patch_y: 0,
        size: 16,
        values: vec![height; 256],
    };
    app.world_mut()
        .write_message(SlEvent(SessionEvent::TerrainPatch(Box::new(patch))));
}

/// The world position of `scoped`'s scene-object entity — where a camera
/// must look for the cursor centre to strike that fixture.
pub(crate) fn scene_position_of(app: &mut App, scoped: ScopedObjectId) -> Option<Vec3> {
    let mut objects = app
        .world_mut()
        .query::<(&crate::world_api::SceneObject, &GlobalTransform)>();
    objects
        .iter(app.world())
        .find(|(scene, _global)| scene.scoped_id == scoped)
        .map(|(_scene, global)| global.translation())
}

/// The world position of `agent`'s avatar anchor (its placeholder sphere).
pub(crate) fn avatar_position_of(app: &mut App, agent: sl_client_bevy::AgentKey) -> Option<Vec3> {
    let mut anchors = app
        .world_mut()
        .query_filtered::<(&crate::world_api::AvatarPickTarget, &GlobalTransform), With<Mesh3d>>();
    anchors
        .iter(app.world())
        .find(|(target, _global)| target.agent() == agent)
        .map(|(_target, global)| global.translation())
}

/// The world centre of the first terrain patch's bounds — a ground point the
/// cursor cannot miss, without assuming the mesh's origin convention.
pub(crate) fn terrain_centre(app: &mut App) -> Option<Vec3> {
    let mut patches = app.world_mut().query_filtered::<(
        &bevy::camera::primitives::Aabb,
        &GlobalTransform,
    ), With<crate::world_api::TerrainSurface>>();
    patches
        .iter(app.world())
        .next()
        .map(|(aabb, global)| global.transform_point(Vec3::from(aabb.center)))
}

/// Project a world position through the [`ViewerCamera`] to logical viewport
/// pixels. `None` when the point is behind the camera or no camera stands.
pub(crate) fn world_to_viewport(app: &mut App, position: Vec3) -> Option<Vec2> {
    let mut cameras = app
        .world_mut()
        .query_filtered::<(&Camera, &GlobalTransform), With<ViewerCamera>>();
    let (camera, transform) = cameras.single(app.world()).ok()?;
    camera.world_to_viewport(transform, position).ok()
}

/// Step `frames` updates — fixture events fold in, meshes build, tags assign.
pub(crate) fn settle(app: &mut App, frames: u32) {
    for _frame in 0..frames {
        app.update();
    }
}

/// The world position of the fixture prim's first pick-tagged face — where a
/// camera must look for the cursor centre to strike it.
pub(crate) fn first_tagged_face_position(app: &mut App) -> Option<Vec3> {
    let mut faces = app
        .world_mut()
        .query_filtered::<&GlobalTransform, With<bevy::mesh::MeshTag>>();
    faces
        .iter(app.world())
        .next()
        .map(|global| global.translation())
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use sl_client_bevy::Vector;
    use sl_viewer_testkit::{drain, interact, record};

    use super::{
        first_tagged_face_position, install_camera, seed_prim, settle, world_app, world_to_viewport,
    };
    use crate::object_menu::OpenObjectMenu;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// **The first pie-target test** ([[viewer-world-pie-target-tests]]): a
    /// right-click on an in-world prim resolves through the real pick
    /// pipeline (CPU resolver) and asks for the object pie — and a
    /// right-click on empty sky asks for nothing.
    #[test]
    fn a_right_click_on_a_prim_asks_for_the_object_pie() -> Result<(), TestError> {
        let mut app = world_app();
        record::<OpenObjectMenu>(&mut app);
        let _scoped = seed_prim(
            &mut app,
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            },
        );
        settle(&mut app, 5);

        // Aim the camera straight at the prim's built face, from 10 m away,
        // so the viewport centre cannot miss it.
        let target =
            first_tagged_face_position(&mut app).ok_or("the fixture prim never built a face")?;
        install_camera(&mut app, target + Vec3::new(0.0, 0.0, 10.0), target);
        settle(&mut app, 2);

        // A right *click* (press, release, no motion) at the centre.
        let centre = Vec2::new(400.0, 300.0);
        interact::hover(&mut app, centre);
        interact::press(&mut app, MouseButton::Right);
        interact::release(&mut app, MouseButton::Right);
        settle(&mut app, 3);

        let opened = drain::<OpenObjectMenu>(&mut app);
        assert!(
            opened.len() == 1,
            "one right-click on the prim must ask for exactly one object pie, got {}",
            opened.len()
        );

        // Beside every fixture: empty sky opens nothing.
        interact::hover(&mut app, Vec2::new(10.0, 10.0));
        interact::press(&mut app, MouseButton::Right);
        interact::release(&mut app, MouseButton::Right);
        settle(&mut app, 3);
        let opened = drain::<OpenObjectMenu>(&mut app);
        assert!(
            opened.is_empty(),
            "a right-click on empty sky must not ask for an object pie"
        );
        Ok(())
    }

    /// **The first gizmo test** (`viewer-edit-gizmo-interaction-tests`): a
    /// drag on the translate-X handle moves the selected prim along X only,
    /// and the release sends exactly one `UpdateObject` carrying a position
    /// (no rotation, no scale).
    #[test]
    fn a_translate_x_drag_moves_only_x_and_sends_one_update() -> Result<(), TestError> {
        let mut app = super::world_app_with_edit();
        let scoped = seed_prim(
            &mut app,
            sl_client_bevy::Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            },
        );
        settle(&mut app, 5);
        let target =
            first_tagged_face_position(&mut app).ok_or("the fixture prim never built a face")?;
        install_camera(&mut app, target + Vec3::new(0.0, 2.0, 10.0), target);

        // Select the prim and enter build mode: the rig spawns and mounts on
        // the selection pivot.
        let entity = {
            let mut objects = app
                .world_mut()
                .query::<(Entity, &crate::world_api::SceneObject)>();
            objects
                .iter(app.world())
                .find(|(_entity, scene)| scene.scoped_id == scoped)
                .map(|(entity, _scene)| entity)
                .ok_or("the fixture prim has no scene entity")?
        };
        let full = sl_client_bevy::ObjectKey::from(sl_client_bevy::Uuid::from_u128(1));
        app.world_mut()
            .resource_mut::<crate::world_api::SelectionSet>()
            .select_only(scoped, full, entity);
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .active = true;
        settle(&mut app, 3);

        // The +X arrow cone: of the three handles named `translate-x` (the
        // shaft and both cones), the one farthest from the pivot on the
        // positive side is a point the cursor cannot miss — and the
        // pivot→cone screen direction is exactly the drag direction.
        let pivot = app
            .world()
            .get::<GlobalTransform>(entity)
            .map(GlobalTransform::translation)
            .ok_or("the selected prim has no transform")?;
        let cone = {
            let mut handles = app.world_mut().query::<(&Name, &GlobalTransform)>();
            handles
                .iter(app.world())
                .filter(|(name, _global)| name.as_str() == "edit-gizmo:translate-x")
                .map(|(_name, global)| global.translation())
                .max_by(|a, b| {
                    a.distance_squared(pivot)
                        .total_cmp(&b.distance_squared(pivot))
                })
                .ok_or("no translate-x handle spawned")?
        };
        let from = world_to_viewport(&mut app, cone).ok_or("the cone projects off screen")?;
        let pivot_screen =
            world_to_viewport(&mut app, pivot).ok_or("the pivot projects off screen")?;
        // Component-wise plain `f32`, per the workspace convention: the
        // `arithmetic_side_effects` lint fires on `glam`'s operators.
        let direction = Vec2::new(from.x - pivot_screen.x, from.y - pivot_screen.y);
        let length = direction.length().max(1e-3);
        let to = Vec2::new(
            from.x + direction.x / length * 60.0,
            from.y + direction.y / length * 60.0,
        );

        let before = app
            .world()
            .get::<crate::objects::ObjectSlMotion>(entity)
            .map(|motion| motion.position.clone())
            .ok_or("the prim has no motion")?;
        interact::drag(&mut app, from, to, 6, MouseButton::Left);
        settle(&mut app, 2);
        let after = app
            .world()
            .get::<crate::objects::ObjectSlMotion>(entity)
            .map(|motion| motion.position.clone())
            .ok_or("the prim has no motion")?;

        assert!(
            (after.x - before.x).abs() > 0.01,
            "a translate-X drag must move the prim along X (before {before:?}, after {after:?})"
        );
        assert!(
            (after.y - before.y).abs() < 1e-3 && (after.z - before.z).abs() < 1e-3,
            "a translate-X drag must not touch Y or Z (before {before:?}, after {after:?})"
        );

        let commands = drain::<sl_client_bevy::SlCommand>(&mut app);
        let updates: Vec<_> = commands
            .iter()
            .filter_map(|sl_client_bevy::SlCommand(command)| match command {
                sl_client_bevy::Command::UpdateObject {
                    local_id,
                    transform,
                } => Some((local_id, transform)),
                _other => None,
            })
            .collect();
        assert!(
            updates.len() == 1,
            "one drag must send exactly one UpdateObject on release, got {}",
            updates.len()
        );
        let (local_id, transform) = updates.first().ok_or("just asserted one")?;
        assert!(
            **local_id == scoped,
            "the update must target the dragged prim"
        );
        assert!(
            transform.position.is_some()
                && transform.rotation.is_none()
                && transform.scale.is_none(),
            "a translate drag carries a position and nothing else (got {transform:?})"
        );
        Ok(())
    }

    /// Right-click `at` (a world position): aim the camera at it, click the
    /// viewport centre, settle — the shared shape of every pie-target test.
    fn right_click_at(app: &mut App, at: Vec3) {
        // Component-wise plain `f32`: the lint fires on `glam` operators.
        let eye = Vec3::new(at.x, at.y + 1.0, at.z + 8.0);
        super::install_camera(app, eye, at);
        settle(app, 2);
        let centre = Vec2::new(400.0, 300.0);
        interact::hover(app, centre);
        interact::press(app, MouseButton::Right);
        interact::release(app, MouseButton::Right);
        settle(app, 3);
    }

    /// **Another avatar's body opens the avatar pie**: a right-click on the
    /// placeholder sphere resolves through the class-1 avatar tag to exactly
    /// one `OpenAvatarMenu` naming that agent.
    #[test]
    fn a_right_click_on_another_avatar_asks_for_the_avatar_pie() -> Result<(), TestError> {
        let mut app = super::world_app();
        record::<crate::avatar_menu::OpenAvatarMenu>(&mut app);
        let other = sl_client_bevy::AgentKey::from(sl_client_bevy::Uuid::from_u128(0xB));
        super::seed_avatar(
            &mut app,
            other,
            2,
            sl_client_bevy::Vector {
                x: 120.0,
                y: 120.0,
                z: 30.0,
            },
        );
        settle(&mut app, 5);
        let at = super::avatar_position_of(&mut app, other)
            .ok_or("the avatar sphere never spawned or dressed")?;
        right_click_at(&mut app, at);
        let opened = drain::<crate::avatar_menu::OpenAvatarMenu>(&mut app);
        assert!(
            opened.len() == 1,
            "one right-click on an avatar must ask for exactly one avatar pie, got {}",
            opened.len()
        );
        let request = opened.first().ok_or("just asserted one")?;
        assert!(
            request.agent == other,
            "the pie must name the clicked avatar"
        );
        Ok(())
    }

    /// **The own avatar's body opens the avatar pie too** (the self pie is
    /// picked at open time from the same request): with `SlIdentity` naming
    /// the agent, the click still resolves to that agent's request.
    #[test]
    fn a_right_click_on_the_own_avatar_asks_for_the_avatar_pie() -> Result<(), TestError> {
        let mut app = super::world_app();
        record::<crate::avatar_menu::OpenAvatarMenu>(&mut app);
        let own = sl_client_bevy::AgentKey::from(sl_client_bevy::Uuid::from_u128(0xA));
        app.world_mut()
            .resource_mut::<sl_client_bevy::SlIdentity>()
            .agent_id = Some(own);
        super::seed_avatar(
            &mut app,
            own,
            2,
            sl_client_bevy::Vector {
                x: 120.0,
                y: 120.0,
                z: 30.0,
            },
        );
        settle(&mut app, 5);
        let at = super::avatar_position_of(&mut app, own)
            .ok_or("the own avatar sphere never spawned or dressed")?;
        right_click_at(&mut app, at);
        let opened = drain::<crate::avatar_menu::OpenAvatarMenu>(&mut app);
        assert!(
            opened.len() == 1 && opened.first().is_some_and(|request| request.agent == own),
            "a right-click on the own body must ask for the own avatar's pie (got {})",
            opened.len()
        );
        Ok(())
    }

    /// **A worn attachment opens the attachment pie**: the prim hangs off the
    /// avatar (nibble-swapped point in the state byte), and the object-face
    /// hit routes to `OpenAttachmentMenu` — a world attachment, not a HUD one.
    #[test]
    fn a_right_click_on_a_worn_attachment_asks_for_the_attachment_pie() -> Result<(), TestError> {
        let mut app = super::world_app();
        record::<crate::attachment_menu::OpenAttachmentMenu>(&mut app);
        let wearer = sl_client_bevy::AgentKey::from(sl_client_bevy::Uuid::from_u128(0xC));
        super::seed_avatar(
            &mut app,
            wearer,
            2,
            sl_client_bevy::Vector {
                x: 120.0,
                y: 120.0,
                z: 30.0,
            },
        );
        settle(&mut app, 3);
        // Chest (point 1), held out to the side so the ray meets the prim
        // and not the sphere.
        let attachment = super::seed_attachment(
            &mut app,
            2,
            3,
            1,
            sl_client_bevy::Vector {
                x: 4.0,
                y: 0.0,
                z: 0.0,
            },
        );
        settle(&mut app, 5);
        let at =
            super::scene_position_of(&mut app, attachment).ok_or("the attachment never spawned")?;
        right_click_at(&mut app, at);
        let opened = drain::<crate::attachment_menu::OpenAttachmentMenu>(&mut app);
        assert!(
            opened.len() == 1,
            "one right-click on a worn attachment must ask for exactly one attachment pie, \
             got {}",
            opened.len()
        );
        assert!(
            opened.first().is_some_and(|request| !request.hud),
            "a world attachment is not a HUD attachment"
        );
        Ok(())
    }

    /// **Bare land opens the land pie**: the flat fixture patch resolves
    /// through the class-3 terrain tag to exactly one `OpenLandMenu`.
    #[test]
    fn a_right_click_on_bare_land_asks_for_the_land_pie() -> Result<(), TestError> {
        let mut app = super::world_app();
        record::<crate::land_menu::OpenLandMenu>(&mut app);
        super::seed_terrain(&mut app, 25.0);
        settle(&mut app, 5);
        let at = super::terrain_centre(&mut app).ok_or("the land patch never built")?;
        right_click_at(&mut app, at);
        let opened = drain::<crate::land_menu::OpenLandMenu>(&mut app);
        assert!(
            opened.len() == 1,
            "one right-click on bare land must ask for exactly one land pie, got {}",
            opened.len()
        );
        Ok(())
    }

    /// **A HUD attachment opens the HUD attachment pie** — the sixth and last
    /// pie target, unblocked by the vendored character assets: the own
    /// avatar's HUD-Center prim routes onto the HUD screen, and a right-click
    /// over it resolves through the orthographic HUD pick to exactly one
    /// `OpenAttachmentMenu` with `hud: true` — synchronously, occluding the
    /// world behind it.
    #[test]
    fn a_right_click_on_a_hud_attachment_asks_for_the_hud_pie() -> Result<(), TestError> {
        let mut app = super::world_app_with_hud()?;
        record::<crate::attachment_menu::OpenAttachmentMenu>(&mut app);
        let own = sl_client_bevy::AgentKey::from(sl_client_bevy::Uuid::from_u128(0xD));
        app.world_mut()
            .resource_mut::<sl_client_bevy::SlIdentity>()
            .agent_id = Some(own);
        super::seed_avatar(
            &mut app,
            own,
            2,
            sl_client_bevy::Vector {
                x: 120.0,
                y: 120.0,
                z: 30.0,
            },
        );
        settle(&mut app, 3);
        // Worn on HUD Center (35), sitting on the point node itself — the
        // screen centre.
        super::seed_attachment(
            &mut app,
            2,
            3,
            35,
            sl_client_bevy::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        );
        settle(&mut app, 5);
        super::install_hud_camera_projection(&mut app)
            .ok_or("no HUD camera spawned — did the asset library load?")?;
        settle(&mut app, 2);

        interact::hover(&mut app, Vec2::new(400.0, 300.0));
        interact::press(&mut app, MouseButton::Right);
        interact::release(&mut app, MouseButton::Right);
        settle(&mut app, 3);

        let opened = drain::<crate::attachment_menu::OpenAttachmentMenu>(&mut app);
        assert!(
            opened.len() == 1,
            "one right-click on a HUD attachment must ask for exactly one attachment pie, \
             got {}",
            opened.len()
        );
        assert!(
            opened.first().is_some_and(|request| request.hud),
            "a HUD attachment's pie is the HUD one"
        );
        Ok(())
    }
}
