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
    // The camera frusta `check_visibility` culls against. `bevy_camera`'s own
    // `CameraPlugin` owns this system and the fixture world has no reason to
    // add the rest of it — but without a computed frustum every entity is
    // culled, `ViewVisibility` never becomes true, and every ray cast left at
    // the default `RayCastVisibility::VisibleInView` quietly hits nothing.
    // That is the left-click's `ObjectPicker::pick` (and so the selection
    // gesture), while the pick resolver's own `Visible` cast goes on working —
    // a difference that would read as "selection is broken", not as "the
    // harness never computed a frustum".
    app.add_systems(
        PostUpdate,
        bevy::camera::visibility::update_frusta
            .in_set(bevy::camera::visibility::VisibilitySystems::UpdateFrusta),
    );
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

/// The fixture world with the build tools on top — the selection gesture and
/// the transform gizmos. A separate builder because plugins must be added
/// before the app's first update.
pub(crate) fn world_app_with_edit() -> App {
    let mut app = world_app();
    add_edit_plugins(&mut app);
    app
}

/// The build-tool half of [`world_app_with_edit`], on its own — the selection
/// gesture and the transform gizmos — so a fixture world that is still being
/// composed (the UI fold, which must add its plugins before the app's first
/// update) can take the edit tools too.
fn add_edit_plugins(app: &mut App) {
    app.add_plugins((
        crate::gizmos::EditGizmoPlugin,
        crate::edit_selection::EditSelectionPlugin,
    ));
    // The per-frame "a widget took this press" flag the selection gesture
    // consults; its owner is the combo widget, over in the UI scaffold.
    app.init_resource::<sl_viewer_ui_core::ui::UiPointerClaim>();
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

/// The fixture world with **the UI on top of it**: the layout stack, the
/// scaffold's `UiRoot`, and the UI half of the interaction stack, composed onto
/// the world fold the way the running viewer composes them.
///
/// Built over [`world_app_with_hud`] rather than [`world_app`] because that is
/// where the viewer's UI camera comes from: `setup_hud_screen` spawns the HUD
/// camera carrying `IsDefaultUiCamera`, and `DefaultUiCamera` is what decides
/// which camera the UI root's size and scale factor are read from. A UI
/// composed onto a world with no such marker would target whichever camera won
/// a `max_by_key` — a different answer from the viewer's, arrived at by entity
/// order.
///
/// The returned app has already run **one** update: the two `Startup` halves
/// (the HUD screen and its camera, the UI root) have to stand up before the HUD
/// camera's computed values can be filled in, and the UI cannot lay out until
/// they are.
///
/// # Errors
///
/// Returns the load error when the vendored character directory is missing, and
/// a message when no HUD camera stood up.
pub(crate) fn world_app_with_ui() -> Result<App, Box<dyn core::error::Error>> {
    compose_ui_over(world_app_with_hud()?)
}

/// [`world_app_with_ui`] **with the build tools underneath it** — the fixture
/// world a test needs when a manipulator drag has to meet a real UI panel (the
/// gizmos' blocking-UI guard). The edit plugins go on before the UI's first
/// update, which is why this cannot be `world_app_with_ui()` plus a later
/// `add_plugins`.
///
/// # Errors
///
/// As [`world_app_with_ui`].
pub(crate) fn world_app_with_ui_and_edit() -> Result<App, Box<dyn core::error::Error>> {
    let mut app = world_app_with_hud()?;
    add_edit_plugins(&mut app);
    compose_ui_over(app)
}

/// Compose the UI fold onto an already-built (never-updated) fixture world:
/// the layout stack, the interaction and text halves, the first update the two
/// `Startup` halves need, and the HUD camera's hand-filled projection.
///
/// # Errors
///
/// Returns a message when no HUD camera stood up.
fn compose_ui_over(mut app: App) -> Result<App, Box<dyn core::error::Error>> {
    // The layout half: `Hosted`, because the world app already propagates
    // transforms and brings the cameras.
    sl_viewer_testkit::LayoutTest::new().install(&mut app, sl_viewer_testkit::UiHost::Hosted);
    // The interaction half: the UI stack, the UI picking backend and the widget
    // systems. The input stack underneath it is already in `world_app`.
    interact::install_ui_interaction(&mut app);
    // …and the editable-text path on top of it, so a floater or a chat bar
    // standing in this world can actually be typed into
    // ([[viewer-ui-keyboard-text-harness]]).
    interact::install_text_editing(&mut app);
    app.update();
    install_hud_camera_projection(&mut app)
        .ok_or("no HUD camera spawned — did the vendored character assets load?")?;
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
    seed_prim_with_flags(app, position, 0)
}

/// [`seed_prim`] with `extra_flags` set on top of the ordinary editable mask —
/// the update-flag bits a menu's conditions read (a touch handler, say).
pub(crate) fn seed_prim_with_flags(
    app: &mut App,
    position: sl_client_bevy::Vector,
    extra_flags: u32,
) -> ScopedObjectId {
    seed_object(app, fixture_prim(1, position, extra_flags))
}

/// [`seed_prim`] under an explicit region-local id — what a fixture world with
/// **more than one** prim needs. The shared seed's id is `1`, so two prims
/// streamed through [`seed_prim`] would be one object updated twice, silently:
/// the second `ObjectAdded` folds onto the first's entity and the scene holds
/// one box where the test believes there are two.
pub(crate) fn seed_prim_numbered(
    app: &mut App,
    local_id: u32,
    position: sl_client_bevy::Vector,
) -> ScopedObjectId {
    seed_object(app, fixture_prim(local_id, position, 0))
}

/// A fat prim **linked** under `parent_local`'s root — a two-prim linkset, the
/// shape whole-linkset selection and *Edit Linked Parts* differ over. Its
/// position is parent-relative (a linkset child's transform is), and the root
/// must already be tracked when this folds in, so seed and settle the root
/// first.
pub(crate) fn seed_child_prim(
    app: &mut App,
    parent_local: u32,
    local_id: u32,
    position: sl_client_bevy::Vector,
) -> ScopedObjectId {
    let mut object = fixture_prim(local_id, position, 0);
    object.parent_id = sl_client_bevy::RegionLocalObjectId(parent_local);
    seed_object(app, object)
}

/// The shared editable-prim seed: [`crate::objects::fixture_object`] under
/// `local_id` (whose grid-wide key is the same number, so every fixture prim is
/// distinct on both), at `position`, with the ordinary editable agent-flag mask
/// plus `extra_flags`. A zero mask would read as "may not move" and refuse a
/// gizmo drag.
fn fixture_prim(local_id: u32, position: sl_client_bevy::Vector, extra_flags: u32) -> Object {
    let mut object: Object = crate::objects::fixture_object(pcode::PRIMITIVE);
    object.local_id = sl_client_bevy::RegionLocalObjectId(local_id);
    object.full_id =
        sl_client_bevy::ObjectKey::from(sl_client_bevy::Uuid::from_u128(u128::from(local_id)));
    object.motion.position = position;
    object.update_flags = crate::world_api::FLAGS_OBJECT_MODIFY
        | crate::world_api::FLAGS_OBJECT_MOVE
        | crate::world_api::FLAGS_OBJECT_COPY
        | crate::world_api::FLAGS_OBJECT_YOU_OWNER
        | extra_flags;
    object
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

/// The scene entity a fixture's scoped id folded into — what a test needs to
/// read the prim's motion, select it, or parent something to it.
pub(crate) fn entity_of(app: &mut App, scoped: ScopedObjectId) -> Option<Entity> {
    let mut objects = app
        .world_mut()
        .query::<(Entity, &crate::world_api::SceneObject)>();
    objects
        .iter(app.world())
        .find(|(_entity, scene)| scene.scoped_id == scoped)
        .map(|(entity, _scene)| entity)
}

/// Select whatever is under `at` (logical viewport pixels) the way a user
/// does: the real selection tool's click gesture, press and release within its
/// slop, resolved by the real world pick.
///
/// The build tool has to be active first — `handle_select_pointer` bails on an
/// inactive tool before it looks at the pointer at all — and a click on empty
/// world clears the selection rather than leaving it alone, which is what makes
/// this worth driving instead of writing [`crate::world_api::SelectionSet`] by
/// hand: a badly aimed camera *deselects* rather than quietly selecting
/// nothing.
pub(crate) fn select_by_click(app: &mut App, at: Vec2) {
    interact::hover(app, at);
    interact::press(app, MouseButton::Left);
    interact::release(app, MouseButton::Left);
    settle(app, 2);
}

/// Every [`sl_client_bevy::Command`] the viewer has sent since the last drain —
/// the outbound half of the fixture world's seam, unwrapped from its message.
pub(crate) fn drain_commands(app: &mut App) -> Vec<sl_client_bevy::Command> {
    sl_viewer_testkit::drain::<SlCommand>(app)
        .into_iter()
        .map(|SlCommand(command)| command)
        .collect()
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

    use sl_client_bevy::{ScopedObjectId, Vector};
    use sl_viewer_testkit::{drain, find_by_name, interact, record};

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

        // Enter build mode and select the prim by clicking it: the camera looks
        // straight at the fixture, so the viewport centre is on it. The rig
        // spawns and mounts on the selection pivot.
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .active = true;
        settle(&mut app, 2);
        super::select_by_click(&mut app, Vec2::new(400.0, 300.0));
        let entity = super::entity_of(&mut app, scoped).ok_or("the fixture prim has no entity")?;
        assert!(
            app.world()
                .resource::<crate::world_api::SelectionSet>()
                .is_selected(scoped),
            "a click on the prim must select it — the gizmo has nothing to mount on otherwise"
        );
        settle(&mut app, 3);

        // The +X arrow cone, and the pivot→cone screen direction that is
        // exactly the drag direction.
        let pivot = app
            .world()
            .get::<GlobalTransform>(entity)
            .map(GlobalTransform::translation)
            .ok_or("the selected prim has no transform")?;
        let cone = translate_x_cone(&mut app, pivot).ok_or("no translate-x handle spawned")?;
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

    /// The far **+X translate cone** of the spawned gizmo rig: of the three
    /// parts named `edit-gizmo:translate-x` (the shaft and both cones), the one
    /// farthest from `pivot` on the positive side — a point the cursor cannot
    /// miss, and one that sits **off** the selected prim, out over empty world.
    fn translate_x_cone(app: &mut App, pivot: Vec3) -> Option<Vec3> {
        handle_toward(app, "edit-gizmo:translate-x", pivot, Vec3::X)
    }

    /// The rig part named `edit-gizmo:<slug>` reaching **furthest from `pivot`
    /// along `direction`** (Bevy world space), by its world position.
    ///
    /// A handle's test address is its *part*, and a part can be several
    /// entities: a translate arrow is a shaft and a cone at each end, all three
    /// named `edit-gizmo:translate-x`. Picking by distance alone leaves the two
    /// cones tied and the winner decided by spawn order, so a test that means
    /// "the **+X** cone" says which way it means.
    fn handle_toward(app: &mut App, slug: &str, pivot: Vec3, direction: Vec3) -> Option<Vec3> {
        let mut handles = app.world_mut().query::<(&Name, &GlobalTransform)>();
        handles
            .iter(app.world())
            .filter(|(name, _global)| name.as_str() == slug)
            .map(|(_name, global)| global.translation())
            .max_by(|a, b| {
                let reach = |point: &Vec3| {
                    Vec3::new(point.x - pivot.x, point.y - pivot.y, point.z - pivot.z)
                        .dot(direction)
                };
                reach(a).total_cmp(&reach(b))
            })
    }

    /// A point on the **rotate ring** named `edit-gizmo:rotate-<axis>`, at
    /// `angle` radians around it and `radius` in rig units (the drawn torus'
    /// major radius is `1.0`, so `1.0` is a point on the ring the cursor can
    /// press and anything larger is out where the detents engage).
    ///
    /// Read through the ring's own [`GlobalTransform`], so the rig's live
    /// constant-screen-size scale and the grid frame's rotation are already in
    /// it — and, because the returned point lies in the ring's plane (which is
    /// the drag plane, through the pivot), the ray through its projection hits
    /// exactly it. A drag between two such points turns by exactly the angle
    /// between them.
    fn ring_point(app: &mut App, slug: &str, angle: f32, radius: f32) -> Option<Vec3> {
        let mut handles = app.world_mut().query::<(&Name, &GlobalTransform)>();
        let global = handles
            .iter(app.world())
            .find(|(name, _global)| name.as_str() == slug)
            .map(|(_name, global)| *global)?;
        // The torus is authored about its own local Y: (cos, 0, sin) traces its
        // circle, and `transform_point` carries the rig's scale and pose.
        Some(global.transform_point(Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius)))
    }

    /// Drag a handle at `handle` (world) outward from `pivot`: `along` logical
    /// pixels further along the pivot→handle screen direction, and `across`
    /// pixels perpendicular to it — the off-axis excursion that decides whether
    /// a drag stays free or crosses into the reference's **snap regime**.
    fn drag_handle(
        app: &mut App,
        pivot: Vec3,
        handle: Vec3,
        (along, across): (f32, f32),
        steps: u32,
    ) -> Result<(), TestError> {
        let from = world_to_viewport(app, handle).ok_or("the handle projects off screen")?;
        let pivot_at = world_to_viewport(app, pivot).ok_or("the pivot projects off screen")?;
        // Component-wise plain `f32`, per the workspace convention.
        let direction = Vec2::new(from.x - pivot_at.x, from.y - pivot_at.y);
        let length = direction.length().max(1e-3);
        let unit = Vec2::new(direction.x / length, direction.y / length);
        let to = Vec2::new(
            from.x + unit.x * along - unit.y * across,
            from.y + unit.y * along + unit.x * across,
        );
        interact::drag(app, from, to, steps, MouseButton::Left);
        settle(app, 2);
        Ok(())
    }

    /// The prim's live wire-frame motion — the position, rotation and scale the
    /// simulator would next be told about.
    fn motion_of(app: &App, entity: Entity) -> Option<crate::objects::ObjectSlMotion> {
        app.world()
            .get::<crate::objects::ObjectSlMotion>(entity)
            .cloned()
    }

    /// Every `UpdateObject` sent since the last drain, as `(id, transform)` —
    /// the edit half of the wire, the reaction a simulator would see.
    fn drain_updates(app: &mut App) -> Vec<(ScopedObjectId, sl_client_bevy::ObjectTransform)> {
        super::drain_commands(app)
            .into_iter()
            .filter_map(|command| match command {
                sl_client_bevy::Command::UpdateObject {
                    local_id,
                    transform,
                } => Some((local_id, transform)),
                _other => None,
            })
            .collect()
    }

    /// Every `DuplicateObjects` sent since the last drain, as `(ids, offset)` —
    /// the wire form of the copy a Shift-drag leaves behind.
    fn drain_duplicates(app: &mut App) -> Vec<(Vec<ScopedObjectId>, Vector)> {
        super::drain_commands(app)
            .into_iter()
            .filter_map(|command| match command {
                sl_client_bevy::Command::DuplicateObjects {
                    local_ids,
                    offset,
                    group_id: _group_id,
                } => Some((local_ids, offset)),
                _other => None,
            })
            .collect()
    }

    /// Stand a **selected fixture prim** up under `tool`: seed a fat prim,
    /// frame it head-on from ten metres, enter build mode with that
    /// manipulator, and select it with a real click, so the rig spawns and
    /// mounts on it. Returns the prim's scoped id and its scene entity — the
    /// opening move of every gizmo-drag test.
    fn selected_fixture(
        app: &mut App,
        tool: crate::world_api::EditTool,
    ) -> Result<(ScopedObjectId, Entity), TestError> {
        selected_fixture_object(app, tool, super::fixture_prim(1, FIXTURE_AT, 0))
    }

    /// [`selected_fixture`] over an already-shaped object — what a test that
    /// needs a *rotated* prim (the local grid frame) seeds.
    fn selected_fixture_object(
        app: &mut App,
        tool: crate::world_api::EditTool,
        object: sl_client_bevy::Object,
    ) -> Result<(ScopedObjectId, Entity), TestError> {
        let scoped = super::seed_object(app, object);
        settle(app, 5);
        let target =
            first_tagged_face_position(app).ok_or("the fixture prim never built a face")?;
        // Component-wise plain `f32`, per the workspace convention: the
        // `arithmetic_side_effects` lint fires on `glam`'s operators.
        let eye = Vec3::new(target.x, target.y + 2.0, target.z + 10.0);
        install_camera(app, eye, target);
        {
            let mut state = app
                .world_mut()
                .resource_mut::<crate::world_api::EditToolState>();
            state.active = true;
            state.tool = tool;
        }
        settle(app, 2);
        super::select_by_click(app, Vec2::new(400.0, 300.0));
        settle(app, 3);
        assert!(
            is_selected(app, scoped),
            "a click on the prim must select it — the gizmo has nothing to mount on otherwise"
        );
        let entity = super::entity_of(app, scoped).ok_or("the fixture prim has no entity")?;
        Ok((scoped, entity))
    }

    /// Where every gizmo test's fixture prim stands (SL region-local metres) —
    /// on the half-metre grid, so a snapped drag that lands back on it is the
    /// grid's doing and not the start position's.
    const FIXTURE_AT: Vector = Vector {
        x: 128.0,
        y: 128.0,
        z: 30.0,
    };

    /// The selected prim's pivot — where the rig mounts, and the point every
    /// handle direction is measured from.
    fn pivot_of(app: &App, entity: Entity) -> Option<Vec3> {
        app.world()
            .get::<GlobalTransform>(entity)
            .map(GlobalTransform::translation)
    }

    /// Whether `value` sits on a multiple of `grid`, within a millimetre.
    fn on_grid(value: f32, grid: f32) -> bool {
        ((value / grid).round() * grid - value).abs() < 1e-3
    }

    /// The prim's rotation as an angle about the Second Life Z axis, in
    /// degrees, with the off-axis components alongside it for the "and nothing
    /// else turned" half of a rotate assertion.
    fn twist_about_z(rotation: &sl_client_bevy::Rotation) -> (f32, f32) {
        let angle = rotation.z.atan2(rotation.s).to_degrees() * 2.0;
        let off_axis = rotation.x.abs().max(rotation.y.abs());
        (angle, off_axis)
    }

    /// **Snapping is what lands the drag on the grid**: the *same* cursor path
    /// — out along the +X arrow and far enough off its axis to cross the white
    /// snap-guide ruler — leaves the prim on a grid multiple with
    /// `EditToolState::snap` on and off it with snapping off.
    ///
    /// The pair is the point: one run alone could not tell a snapped landing
    /// from a lucky one.
    #[test]
    fn snapping_lands_a_translate_drag_on_the_grid() -> Result<(), TestError> {
        /// The drag both runs make: out along the arrow, and well past the snap
        /// ruler (which sits at `0.45` rig units, about fifty pixels).
        const PATH: (f32, f32) = (55.0, 140.0);

        let mut snapped_x = None;
        let mut free_x = None;
        for snap in [true, false] {
            let mut app = super::world_app_with_edit();
            let (_scoped, entity) = selected_fixture(&mut app, crate::world_api::EditTool::Move)?;
            {
                let mut state = app
                    .world_mut()
                    .resource_mut::<crate::world_api::EditToolState>();
                state.snap = snap;
                state.grid_unit = 0.5;
            }
            settle(&mut app, 1);
            let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
            let cone = translate_x_cone(&mut app, pivot).ok_or("no translate-x handle spawned")?;
            drag_handle(&mut app, pivot, cone, PATH, 8)?;
            let after = motion_of(&app, entity).ok_or("the prim has no motion")?;
            if snap {
                snapped_x = Some(after.position.x);
            } else {
                free_x = Some(after.position.x);
            }
        }
        let snapped = snapped_x.ok_or("the snapped run never ran")?;
        let free = free_x.ok_or("the free run never ran")?;
        assert!(
            (snapped - FIXTURE_AT.x).abs() > 0.1,
            "the drag must have moved the prim at all (from {} to {snapped})",
            FIXTURE_AT.x
        );
        assert!(
            on_grid(snapped, 0.5),
            "a drag past the snap ruler must land the prim on the half-metre grid, got {snapped}"
        );
        assert!(
            !on_grid(free, 0.5),
            "the same drag with snapping off must NOT land on the grid, got {free} — \
             pick a drag distance whose free landing is off-grid, or the pair proves nothing"
        );
        Ok(())
    }

    /// **The grid frame decides which way a handle points**: the same
    /// translate-X arrow moves the prim along the *world* X in
    /// `GridFrame::World` and along the prim's *own* X — here world Z, the
    /// prim standing on its side — in `GridFrame::Local`.
    #[test]
    fn the_grid_frame_decides_the_translate_axis() -> Result<(), TestError> {
        for frame in [
            crate::world_api::GridFrame::World,
            crate::world_api::GridFrame::Local,
        ] {
            let mut app = super::world_app_with_edit();
            // A quarter turn about the Second Life Y axis: the prim's own X
            // now points along world -Z (down), well clear of the world X the
            // world frame would use.
            let mut object = super::fixture_prim(1, FIXTURE_AT, 0);
            let half = core::f32::consts::FRAC_PI_4;
            object.motion.rotation = sl_client_bevy::Rotation {
                x: 0.0,
                y: half.sin(),
                z: 0.0,
                s: half.cos(),
            };
            let (_scoped, entity) =
                selected_fixture_object(&mut app, crate::world_api::EditTool::Move, object)?;
            {
                let mut state = app
                    .world_mut()
                    .resource_mut::<crate::world_api::EditToolState>();
                state.frame = frame;
                // Free dragging: the snap regime is the snapping pair's
                // subject, not this one's.
                state.snap = false;
            }
            settle(&mut app, 2);
            let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
            // The frame's +X in Bevy space: world X, or (rotated) straight down.
            let bevy_axis = match frame {
                crate::world_api::GridFrame::Local => Vec3::NEG_Y,
                _world => Vec3::X,
            };
            let cone = handle_toward(&mut app, "edit-gizmo:translate-x", pivot, bevy_axis)
                .ok_or("no translate-x handle spawned")?;
            let before = motion_of(&app, entity).ok_or("the prim has no motion")?;
            drag_handle(&mut app, pivot, cone, (60.0, 0.0), 8)?;
            let after = motion_of(&app, entity).ok_or("the prim has no motion")?;
            let moved = Vec3::new(
                after.position.x - before.position.x,
                after.position.y - before.position.y,
                after.position.z - before.position.z,
            );
            // Which Second Life axis the frame's X is: world X, or the turned
            // prim's own X, which is world -Z.
            let (wanted, others) = match frame {
                crate::world_api::GridFrame::Local => (moved.z, moved.x.abs().max(moved.y.abs())),
                _world => (moved.x, moved.y.abs().max(moved.z.abs())),
            };
            assert!(
                wanted.abs() > 0.2,
                "in {frame:?} the translate-X arrow must move the prim along that frame's X \
                 (moved {moved:?})"
            );
            assert!(
                others < 1e-3,
                "in {frame:?} the translate-X arrow must move the prim along nothing else \
                 (moved {moved:?})"
            );
        }
        Ok(())
    }

    /// **A rotate ring turns the prim about its own axis, and only that**: a
    /// drag along the Z ring's circle — inside the detent circle, so free —
    /// turns the prim by exactly the angle swept, leaves its position alone,
    /// and sends one update carrying a rotation but no scale.
    #[test]
    fn a_rotate_ring_drag_turns_the_prim_about_that_axis_alone() -> Result<(), TestError> {
        /// How far around the ring the cursor travels, radians.
        const SWEEP: f32 = 0.4;

        let mut app = super::world_app_with_edit();
        let (scoped, entity) = selected_fixture(&mut app, crate::world_api::EditTool::Rotate)?;
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .snap = false;
        settle(&mut app, 2);

        let before = motion_of(&app, entity).ok_or("the prim has no motion")?;
        let from = ring_point(&mut app, "edit-gizmo:rotate-z", 0.0, 1.0)
            .ok_or("no rotate-z ring spawned")?;
        let to = ring_point(&mut app, "edit-gizmo:rotate-z", SWEEP, 1.0)
            .ok_or("no rotate-z ring spawned")?;
        let from_at = world_to_viewport(&mut app, from).ok_or("the ring projects off screen")?;
        let to_at = world_to_viewport(&mut app, to).ok_or("the ring projects off screen")?;
        let _opening = drain_updates(&mut app);
        interact::drag(&mut app, from_at, to_at, 8, MouseButton::Left);
        settle(&mut app, 2);

        let after = motion_of(&app, entity).ok_or("the prim has no motion")?;
        let (angle, off_axis) = twist_about_z(&after.rotation);
        assert!(
            (angle.abs() - SWEEP.to_degrees()).abs() < 1.0,
            "a ring drag turns by the angle the cursor swept ({}\u{b0} wanted, {angle}\u{b0} got)",
            SWEEP.to_degrees()
        );
        assert!(
            off_axis < 1e-3,
            "the Z ring must not tilt the prim about X or Y (rotation {:?})",
            after.rotation
        );
        assert!(
            (after.position.x - before.position.x).abs() < 1e-3
                && (after.position.y - before.position.y).abs() < 1e-3
                && (after.position.z - before.position.z).abs() < 1e-3,
            "turning a lone prim about its own centre must not move it \
             (before {:?}, after {:?})",
            before.position,
            after.position
        );

        let updates = drain_updates(&mut app);
        assert!(
            updates.len() == 1,
            "one ring drag sends exactly one update on release, got {}",
            updates.len()
        );
        let (local_id, transform) = updates.first().ok_or("just asserted one")?;
        assert!(
            *local_id == scoped,
            "the update must target the turned prim"
        );
        assert!(
            transform.rotation.is_some()
                && transform.position.is_some()
                && transform.scale.is_none(),
            "a rotate drag carries a rotation (and the pivot-relative position), never a scale \
             (got {transform:?})"
        );
        Ok(())
    }

    /// **Outside the detent circle a rotation snaps**: the same ring drag,
    /// pulled out past the tick circle, lands the prim on a multiple of the
    /// reference's `5.625°` detent rather than on the angle swept.
    #[test]
    fn a_rotate_drag_past_the_detents_lands_on_one() -> Result<(), TestError> {
        /// The angle the cursor sweeps: `22.9°`, whose nearest detent is the
        /// fourth (`22.5°`) — far enough from it to tell the two apart.
        const SWEEP: f32 = 0.4;
        /// The reference's rotation detent, degrees.
        const DETENT_DEG: f32 = 5.625;

        let mut app = super::world_app_with_edit();
        let (_scoped, entity) = selected_fixture(&mut app, crate::world_api::EditTool::Rotate)?;
        {
            let mut state = app
                .world_mut()
                .resource_mut::<crate::world_api::EditToolState>();
            state.snap = true;
        }
        settle(&mut app, 2);

        // Press ON the ring (the mesh is the pick target), release well
        // outside the detent tick circle at `1.35` rig units.
        let from = ring_point(&mut app, "edit-gizmo:rotate-z", 0.0, 1.0)
            .ok_or("no rotate-z ring spawned")?;
        let to = ring_point(&mut app, "edit-gizmo:rotate-z", SWEEP, 1.8)
            .ok_or("no rotate-z ring spawned")?;
        let from_at = world_to_viewport(&mut app, from).ok_or("the ring projects off screen")?;
        let to_at = world_to_viewport(&mut app, to).ok_or("the ring projects off screen")?;
        interact::drag(&mut app, from_at, to_at, 8, MouseButton::Left);
        settle(&mut app, 2);

        let after = motion_of(&app, entity).ok_or("the prim has no motion")?;
        let (angle, _off_axis) = twist_about_z(&after.rotation);
        assert!(
            on_grid(angle, DETENT_DEG),
            "a rotation dragged past the tick circle must land on a {DETENT_DEG}\u{b0} detent, \
             got {angle}\u{b0}"
        );
        assert!(
            (angle.abs() - SWEEP.to_degrees()).abs() > 0.1,
            "…and the detent must be what decided it, not the swept angle ({}\u{b0} swept, \
             {angle}\u{b0} landed)",
            SWEEP.to_degrees()
        );
        Ok(())
    }

    /// **A stretch streams while it drags**: a long face drag sends the
    /// reference's ~10 Hz `MultipleObjectUpdate`s along the way *and* a final
    /// one on release, while a drag over in two frames sends only the release.
    #[test]
    fn a_stretch_drag_streams_updates_and_a_short_one_does_not() -> Result<(), TestError> {
        let mut app = super::world_app_with_edit();
        let (_scoped, entity) = selected_fixture(&mut app, crate::world_api::EditTool::Stretch)?;
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .snap = false;
        settle(&mut app, 2);

        // A drag that is over inside one stream interval: the release alone.
        let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
        let handle = handle_toward(&mut app, "edit-gizmo:scale-face-x-pos", pivot, Vec3::X)
            .ok_or("no +X stretch face handle spawned")?;
        let _opening = drain_updates(&mut app);
        drag_handle(&mut app, pivot, handle, (30.0, 0.0), 2)?;
        let short = drain_updates(&mut app);
        assert!(
            short.len() == 1,
            "a stretch that is over within one stream interval sends only its release update, \
             got {}",
            short.len()
        );

        // A long one: the fixture clock steps 16 ms a frame, so thirty frames
        // span four of the reference's 100 ms intervals.
        let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
        let handle = handle_toward(&mut app, "edit-gizmo:scale-face-x-pos", pivot, Vec3::X)
            .ok_or("no +X stretch face handle spawned")?;
        let before = motion_of(&app, entity).ok_or("the prim has no motion")?;
        drag_handle(&mut app, pivot, handle, (60.0, 0.0), 30)?;
        let streamed = drain_updates(&mut app);
        assert!(
            streamed.len() >= 3,
            "a thirty-frame stretch must stream updates as it goes, got {}",
            streamed.len()
        );
        assert!(
            streamed
                .iter()
                .all(|(_id, transform)| transform.scale.is_some() && transform.rotation.is_none()),
            "every streamed stretch update carries the scale and no rotation (got {streamed:?})"
        );
        assert!(
            streamed.iter().all(|(_id, transform)| !transform.uniform),
            "a FACE stretch never sets the uniform bit — the simulator could take it as licence \
             to scale every axis (got {streamed:?})"
        );
        let after = motion_of(&app, entity).ok_or("the prim has no motion")?;
        assert!(
            after.scale.x - before.scale.x > 0.1,
            "the stretch must have grown the prim on X (before {:?}, after {:?})",
            before.scale,
            after.scale
        );
        assert!(
            streamed.last().is_some_and(|(_id, transform)| transform
                .scale
                .as_ref()
                .is_some_and(|scale| { (scale.x - after.scale.x).abs() < 1e-4 })),
            "the release update carries the size the prim ended at"
        );
        Ok(())
    }

    /// **A corner stretch scales every axis by one factor**: the diagonal
    /// handle grows the prim's three sizes in the same proportion — the
    /// reference's shared factor, which is what keeps a selection's shape — and
    /// with stretch-both-sides on the update carries the `UNIFORM` bit that a
    /// face drag never sets.
    #[test]
    fn a_corner_stretch_scales_every_axis_by_one_factor() -> Result<(), TestError> {
        let mut app = super::world_app_with_edit();
        let (_scoped, entity) = selected_fixture(&mut app, crate::world_api::EditTool::Stretch)?;
        {
            let mut state = app
                .world_mut()
                .resource_mut::<crate::world_api::EditToolState>();
            state.snap = false;
            state.stretch_both = true;
        }
        settle(&mut app, 2);

        let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
        // The +X+Y+Z corner cube: the one part of the rig with that address.
        let handle = handle_toward(&mut app, "edit-gizmo:scale-corner-ppp", pivot, Vec3::ONE)
            .ok_or("no +++ stretch corner handle spawned")?;
        let before = motion_of(&app, entity).ok_or("the prim has no motion")?;
        let _opening = drain_updates(&mut app);
        drag_handle(&mut app, pivot, handle, (40.0, 0.0), 8)?;
        let after = motion_of(&app, entity).ok_or("the prim has no motion")?;

        let factor = after.scale.x / before.scale.x;
        assert!(
            factor > 1.05,
            "dragging the corner outward must grow the prim (before {:?}, after {:?})",
            before.scale,
            after.scale
        );
        for (grown, started) in [
            (after.scale.y, before.scale.y),
            (after.scale.z, before.scale.z),
        ] {
            assert!(
                (grown / started - factor).abs() < 1e-3,
                "every axis grows by the SAME factor ({factor} on X, {} here — before {:?}, \
                 after {:?})",
                grown / started,
                before.scale,
                after.scale
            );
        }
        let updates = drain_updates(&mut app);
        assert!(
            updates
                .iter()
                .all(|(_id, transform)| transform.scale.is_some() && transform.uniform),
            "a corner stretch with stretch-both-sides on is the one drag that sets the uniform \
             bit (got {updates:?})"
        );
        Ok(())
    }

    /// **Stretch both sides doubles the size change and holds the centre**: the
    /// same face drag grows the prim twice as much with
    /// `EditToolState::stretch_both` on, and leaves its position alone
    /// instead of shifting it half the growth (the opposite face stays put
    /// otherwise).
    #[test]
    fn stretch_both_sides_doubles_the_size_and_holds_the_centre() -> Result<(), TestError> {
        let mut grown = Vec::new();
        let mut shifted = Vec::new();
        for stretch_both in [false, true] {
            let mut app = super::world_app_with_edit();
            let (_scoped, entity) =
                selected_fixture(&mut app, crate::world_api::EditTool::Stretch)?;
            {
                let mut state = app
                    .world_mut()
                    .resource_mut::<crate::world_api::EditToolState>();
                state.snap = false;
                state.stretch_both = stretch_both;
            }
            settle(&mut app, 2);
            let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
            let handle = handle_toward(&mut app, "edit-gizmo:scale-face-x-pos", pivot, Vec3::X)
                .ok_or("no +X stretch face handle spawned")?;
            let before = motion_of(&app, entity).ok_or("the prim has no motion")?;
            drag_handle(&mut app, pivot, handle, (60.0, 0.0), 8)?;
            let after = motion_of(&app, entity).ok_or("the prim has no motion")?;
            grown.push(after.scale.x - before.scale.x);
            shifted.push((after.position.x - before.position.x).abs());
        }
        let (single, both) = (
            *grown.first().ok_or("the one-sided run never ran")?,
            *grown.get(1).ok_or("the both-sides run never ran")?,
        );
        assert!(
            single > 0.1,
            "the one-sided stretch must have grown the prim at all, got {single}"
        );
        assert!(
            (both - single * 2.0).abs() < single * 0.05,
            "stretch-both-sides moves both faces, so the same cursor travel changes the size \
             twice as much (one-sided {single}, both {both})"
        );
        let (single_shift, both_shift) = (
            *shifted.first().ok_or("the one-sided run never ran")?,
            *shifted.get(1).ok_or("the both-sides run never ran")?,
        );
        assert!(
            (single_shift - single * 0.5).abs() < 1e-3,
            "a one-sided stretch pins the opposite face: the centre shifts half the growth \
             (grew {single}, shifted {single_shift})"
        );
        assert!(
            both_shift < 1e-3,
            "stretching both sides leaves the centre where it was, got a shift of {both_shift}"
        );
        Ok(())
    }

    /// **A Shift-drag leaves exactly one copy behind**: the reference's
    /// `MASK_COPY` translate branch — one `ObjectDuplicate` at zero offset, on
    /// the drag's first movement and never again, while the original follows
    /// the cursor. The same drag without `Shift` copies nothing.
    #[test]
    fn a_shift_drag_leaves_exactly_one_copy_behind() -> Result<(), TestError> {
        use bevy::input::keyboard::Key;

        let mut app = super::world_app_with_edit();
        let (scoped, entity) = selected_fixture(&mut app, crate::world_api::EditTool::Move)?;
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .snap = false;
        settle(&mut app, 2);

        // A plain drag first: it copies nothing.
        let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
        let cone = translate_x_cone(&mut app, pivot).ok_or("no translate-x handle spawned")?;
        drag_handle(&mut app, pivot, cone, (50.0, 0.0), 8)?;
        let plain = drain_duplicates(&mut app);
        assert!(
            plain.is_empty(),
            "a drag with no modifier must leave no copy behind, got {plain:?}"
        );

        // …and the same drag with `Shift` held.
        let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
        let cone = translate_x_cone(&mut app, pivot).ok_or("no translate-x handle spawned")?;
        let before = motion_of(&app, entity).ok_or("the prim has no motion")?;
        let mut dragged = Ok(());
        interact::with_modifier(&mut app, KeyCode::ShiftLeft, Key::Shift, |app| {
            dragged = drag_handle(app, pivot, cone, (50.0, 0.0), 8);
        });
        dragged?;
        settle(&mut app, 2);

        let copies = drain_duplicates(&mut app);
        assert!(
            copies.len() == 1,
            "a Shift-drag leaves exactly one copy behind, got {copies:?}"
        );
        let (local_ids, offset) = copies.first().ok_or("just asserted one")?;
        assert!(
            local_ids == &vec![scoped],
            "the copy is of the dragged linkset root (got {local_ids:?})"
        );
        assert!(
            offset.x.abs() < 1e-6 && offset.y.abs() < 1e-6 && offset.z.abs() < 1e-6,
            "the copy is dropped in place — the original is what moved (offset {offset:?})"
        );
        let after = motion_of(&app, entity).ok_or("the prim has no motion")?;
        assert!(
            (after.position.x - before.position.x).abs() > 0.1,
            "the original still follows the cursor (before {:?}, after {:?})",
            before.position,
            after.position
        );
        Ok(())
    }

    /// **`Alt` yields the pointer to the camera**: with `Alt` held the gizmo
    /// never even hovers, so a press on a handle starts no drag — the prim
    /// stands still and nothing reaches the wire.
    ///
    /// The plain drag first is the control: without it, a fixture whose
    /// manipulator was never grabbable at all would pass this silently.
    #[test]
    fn alt_held_yields_the_pointer_to_the_camera() -> Result<(), TestError> {
        use bevy::input::keyboard::Key;

        let mut app = super::world_app_with_edit();
        let (_scoped, entity) = selected_fixture(&mut app, crate::world_api::EditTool::Move)?;
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .snap = false;
        settle(&mut app, 2);

        // The control: the same drag, no modifier, does move the prim.
        let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
        let cone = translate_x_cone(&mut app, pivot).ok_or("no translate-x handle spawned")?;
        let before = motion_of(&app, entity).ok_or("the prim has no motion")?;
        drag_handle(&mut app, pivot, cone, (60.0, 0.0), 8)?;
        let control = motion_of(&app, entity).ok_or("the prim has no motion")?;
        assert!(
            (control.position.x - before.position.x).abs() > 0.1,
            "this handle IS grabbable without a modifier — otherwise the Alt case below \
             proves nothing (before {:?}, after {:?})",
            before.position,
            control.position
        );
        let _control_updates = drain_updates(&mut app);

        // …and with `Alt` held it is the camera's.
        let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
        let cone = translate_x_cone(&mut app, pivot).ok_or("no translate-x handle spawned")?;
        let mut dragged = Ok(());
        interact::with_modifier(&mut app, KeyCode::AltLeft, Key::Alt, |app| {
            dragged = drag_handle(app, pivot, cone, (60.0, 0.0), 8);
        });
        dragged?;
        settle(&mut app, 2);

        let after = motion_of(&app, entity).ok_or("the prim has no motion")?;
        assert!(
            (after.position.x - control.position.x).abs() < 1e-4,
            "an Alt-drag belongs to the camera: the prim must not move \
             (before {:?}, after {:?})",
            control.position,
            after.position
        );
        let updates = drain_updates(&mut app);
        assert!(
            updates.is_empty(),
            "an Alt-drag on a handle sends no object update, got {updates:?}"
        );
        Ok(())
    }

    /// **A press over blocking UI never begins a drag**: with a real panel
    /// under the cursor the manipulator does not hover, so the same press that
    /// would have grabbed the +X arrow does nothing to the prim — the guard
    /// that keeps a click on a floater over the gizmo from dragging the world
    /// behind it.
    ///
    /// The bare-world drag first is the control: in a fixture world carrying a
    /// whole UI, "the prim did not move" is exactly what a broken pick would
    /// also say.
    #[test]
    fn a_press_over_blocking_ui_never_begins_a_drag() -> Result<(), TestError> {
        let mut app = super::world_app_with_ui_and_edit()?;
        let (_scoped, entity) = selected_fixture(&mut app, crate::world_api::EditTool::Move)?;
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .snap = false;
        settle(&mut app, 2);

        // The control: with nothing over it, the handle drags.
        let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
        let cone = translate_x_cone(&mut app, pivot).ok_or("no translate-x handle spawned")?;
        let opening = motion_of(&app, entity).ok_or("the prim has no motion")?;
        drag_handle(&mut app, pivot, cone, (60.0, 0.0), 8)?;
        let before = motion_of(&app, entity).ok_or("the prim has no motion")?;
        assert!(
            (before.position.x - opening.position.x).abs() > 0.1,
            "this handle IS grabbable with the UI standing but nothing over the cursor — \
             otherwise the blocked case below proves nothing (before {:?}, after {:?})",
            opening.position,
            before.position
        );
        let _control_updates = drain_updates(&mut app);
        let pivot = pivot_of(&app, entity).ok_or("the selected prim has no transform")?;
        let cone = translate_x_cone(&mut app, pivot).ok_or("no translate-x handle spawned")?;

        // A panel over the whole viewport — a floater the user has parked over
        // the manipulator. Spawned after the selection, which had to reach the
        // world to put a rig on screen at all.
        app.world_mut().spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..Default::default()
            },
            Name::new("blocking-panel"),
        ));
        settle(&mut app, 3);

        drag_handle(&mut app, pivot, cone, (60.0, 0.0), 8)?;
        let after = motion_of(&app, entity).ok_or("the prim has no motion")?;
        assert!(
            (after.position.x - before.position.x).abs() < 1e-4,
            "a press that landed on a UI panel must not drag the prim behind it \
             (before {:?}, after {:?})",
            before.position,
            after.position
        );
        let updates = drain_updates(&mut app);
        assert!(
            updates.is_empty(),
            "a press over blocking UI sends no object update, got {updates:?}"
        );
        Ok(())
    }

    /// Enter build mode: the selection gesture bails on an inactive tool before
    /// it looks at the pointer at all, so every selection test opens with this.
    fn enter_build_mode(app: &mut App) {
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .active = true;
        settle(app, 2);
    }

    /// Whether `scoped` is in the selection right now.
    fn is_selected(app: &App, scoped: ScopedObjectId) -> bool {
        app.world()
            .resource::<crate::world_api::SelectionSet>()
            .is_selected(scoped)
    }

    /// The primary selection's scoped id — the object the numeric fields and
    /// the local-frame gizmos follow.
    fn primary(app: &App) -> Option<ScopedObjectId> {
        app.world()
            .resource::<crate::world_api::SelectionSet>()
            .primary()
            .map(|node| node.scoped)
    }

    /// The **wire** half of the selection since the last drain: the ids sent in
    /// an `ObjectSelect` (which is a properties request) and those sent in an
    /// `ObjectDeselect`. What the simulator would have seen.
    fn wire_selection(app: &mut App) -> (Vec<ScopedObjectId>, Vec<ScopedObjectId>) {
        let mut selected = Vec::new();
        let mut deselected = Vec::new();
        for command in super::drain_commands(app) {
            match command {
                sl_client_bevy::Command::RequestObjectProperties { local_ids } => {
                    selected.extend(local_ids);
                }
                sl_client_bevy::Command::DeselectObjects { local_ids } => {
                    deselected.extend(local_ids);
                }
                _other => {}
            }
        }
        (selected, deselected)
    }

    /// Two fat prims side by side, framed head-on by one camera, in build mode:
    /// each one's scoped id and where its centre lands on screen. Eight metres
    /// apart in region X and fourteen from the camera, so their projected boxes
    /// are ~300 px apart — far enough that a rubber band can cover one and miss
    /// the other by more than its own width.
    #[expect(
        clippy::type_complexity,
        reason = "two (id, screen position) pairs is the tuple this returns; naming a struct for \
                  one test setup helper would not read better"
    )]
    fn two_prims_in_view(
        app: &mut App,
    ) -> Result<((ScopedObjectId, Vec2), (ScopedObjectId, Vec2)), TestError> {
        let left = super::seed_prim_numbered(
            app,
            1,
            Vector {
                x: 124.0,
                y: 128.0,
                z: 30.0,
            },
        );
        let right = super::seed_prim_numbered(
            app,
            2,
            Vector {
                x: 132.0,
                y: 128.0,
                z: 30.0,
            },
        );
        settle(app, 6);
        let left_at = super::scene_position_of(app, left).ok_or("the left prim never spawned")?;
        let right_at =
            super::scene_position_of(app, right).ok_or("the right prim never spawned")?;
        // Component-wise plain `f32`: the lint fires on `glam` operators.
        let mid = Vec3::new(
            f32::midpoint(left_at.x, right_at.x),
            f32::midpoint(left_at.y, right_at.y),
            f32::midpoint(left_at.z, right_at.z),
        );
        install_camera(app, Vec3::new(mid.x, mid.y, mid.z + 14.0), mid);
        settle(app, 2);
        enter_build_mode(app);
        let left_screen =
            world_to_viewport(app, left_at).ok_or("the left prim projects off screen")?;
        let right_screen =
            world_to_viewport(app, right_at).ok_or("the right prim projects off screen")?;
        Ok(((left, left_screen), (right, right_screen)))
    }

    /// **A click selects, a click on nothing deselects** — the first
    /// selection-gesture test ([[viewer-edit-selection-interaction-tests]]),
    /// driven through the real `handle_select_pointer` and the real world pick,
    /// asserting both the local set and what the simulator would have seen.
    #[test]
    fn a_click_selects_a_prim_and_a_click_on_nothing_deselects_it() -> Result<(), TestError> {
        let mut app = super::world_app_with_edit();
        let scoped = seed_prim(
            &mut app,
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            },
        );
        settle(&mut app, 5);
        let target =
            first_tagged_face_position(&mut app).ok_or("the fixture prim never built a face")?;
        install_camera(&mut app, target + Vec3::new(0.0, 0.0, 10.0), target);
        enter_build_mode(&mut app);
        let _before = wire_selection(&mut app);

        super::select_by_click(&mut app, Vec2::new(400.0, 300.0));
        assert!(
            is_selected(&app, scoped) && primary(&app) == Some(scoped),
            "a click on the prim selects it and makes it primary"
        );
        let (selected, deselected) = wire_selection(&mut app);
        assert!(
            selected == vec![scoped] && deselected.is_empty(),
            "selecting sends exactly one ObjectSelect for the clicked prim \
             (selected {selected:?}, deselected {deselected:?})"
        );

        // A click on empty sky is a deselect-all, not a no-op.
        super::select_by_click(&mut app, Vec2::new(10.0, 10.0));
        assert!(
            app.world()
                .resource::<crate::world_api::SelectionSet>()
                .is_empty(),
            "a click on empty world clears the selection"
        );
        let (selected, deselected) = wire_selection(&mut app);
        assert!(
            deselected == vec![scoped] && selected.is_empty(),
            "clearing sends exactly one ObjectDeselect for the dropped prim \
             (selected {selected:?}, deselected {deselected:?})"
        );
        Ok(())
    }

    /// **Shift-click accumulates, and toggles back out**: the reference's
    /// `LLToolSelect` extend semantics under a real held modifier — the second
    /// prim joins the set and becomes primary, and shift-clicking it again
    /// removes it (leaving the first, which never moved).
    #[test]
    fn a_shift_click_accumulates_and_then_toggles_the_second_prim() -> Result<(), TestError> {
        use bevy::input::keyboard::Key;

        let mut app = super::world_app_with_edit();
        let ((left, left_at), (right, right_at)) = two_prims_in_view(&mut app)?;
        let _before = wire_selection(&mut app);

        super::select_by_click(&mut app, left_at);
        assert!(
            is_selected(&app, left) && !is_selected(&app, right),
            "a plain click selects only what it hit"
        );

        interact::with_modifier(&mut app, KeyCode::ShiftLeft, Key::Shift, |app| {
            interact::hover(app, right_at);
            interact::press(app, MouseButton::Left);
            interact::release(app, MouseButton::Left);
        });
        settle(&mut app, 2);
        assert!(
            is_selected(&app, left) && is_selected(&app, right),
            "a shift-click adds to the selection instead of replacing it"
        );
        assert!(
            primary(&app) == Some(right),
            "the last-clicked object is the primary"
        );
        let (selected, _deselected) = wire_selection(&mut app);
        assert!(
            selected.contains(&left) && selected.contains(&right),
            "both selections reached the wire (got {selected:?})"
        );

        // The same shift-click again toggles it back out.
        interact::with_modifier(&mut app, KeyCode::ShiftLeft, Key::Shift, |app| {
            interact::hover(app, right_at);
            interact::press(app, MouseButton::Left);
            interact::release(app, MouseButton::Left);
        });
        settle(&mut app, 2);
        assert!(
            is_selected(&app, left) && !is_selected(&app, right),
            "a shift-click on a selected object removes it, leaving the rest"
        );
        let (_selected, deselected) = wire_selection(&mut app);
        assert!(
            deselected == vec![right],
            "only the toggled-out object is deselected on the wire (got {deselected:?})"
        );
        Ok(())
    }

    /// **A rubber band selects what it sweeps, and nothing else**: a left drag
    /// that starts on empty world grows past the click slop into a sweep
    /// (`sweep_candidates` projecting each object's scale box), and the release
    /// commits exactly the covered prim.
    #[test]
    fn a_rubber_band_drag_selects_only_the_prim_it_covers() -> Result<(), TestError> {
        let mut app = super::world_app_with_edit();
        let ((left, left_at), (right, right_at)) = two_prims_in_view(&mut app)?;
        // A band around the right prim only: it starts in empty sky well above
        // both boxes (an object press would be a click, not a sweep), and its
        // near edge stays clear of the left prim's projected box.
        let from = Vec2::new(right_at.x - 90.0, 60.0);
        let to = Vec2::new(right_at.x + 90.0, 520.0);
        assert!(
            from.x - left_at.x > 100.0,
            "the band must miss the left prim by more than its projected width \
             (left {left_at:?}, band from {from:?})"
        );

        interact::drag(&mut app, from, to, 8, MouseButton::Left);
        settle(&mut app, 2);
        assert!(
            is_selected(&app, right) && !is_selected(&app, left),
            "the sweep commits the covered prim and only that one"
        );
        let (selected, _deselected) = wire_selection(&mut app);
        assert!(
            selected == vec![right],
            "the swept selection reaches the wire like any other (got {selected:?})"
        );
        Ok(())
    }

    /// **Edit Linked Parts picks the prim; whole-linkset picks the root** — and
    /// switching back promotes the part up to its root
    /// (`promote_selection_to_roots`, whose child→root jump needs a populated
    /// `ObjectState` and so could not be exercised until this fixture world).
    #[test]
    fn edit_linked_parts_selects_the_picked_prim_not_its_root() -> Result<(), TestError> {
        let mut app = super::world_app_with_edit();
        let root = seed_prim(
            &mut app,
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            },
        );
        settle(&mut app, 5);
        // Linked under the root and held well out to the side, so the ray that
        // strikes the child cannot also graze the root's box.
        let child = super::seed_child_prim(
            &mut app,
            1,
            2,
            Vector {
                x: 6.0,
                y: 0.0,
                z: 0.0,
            },
        );
        settle(&mut app, 6);
        let at = super::scene_position_of(&mut app, child).ok_or("the child prim never spawned")?;
        install_camera(&mut app, Vec3::new(at.x, at.y, at.z + 10.0), at);
        enter_build_mode(&mut app);
        let centre = Vec2::new(400.0, 300.0);

        // Whole-linkset mode (the default): the click lands on the child and
        // selects its root.
        super::select_by_click(&mut app, centre);
        assert!(
            is_selected(&app, root) && !is_selected(&app, child),
            "a whole-linkset click on a child prim selects the linkset root"
        );

        // Edit Linked Parts: the same click selects the part itself.
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .edit_linked = true;
        settle(&mut app, 2);
        super::select_by_click(&mut app, centre);
        assert!(
            is_selected(&app, child) && !is_selected(&app, root),
            "an edit-linked click selects the picked part, not its root"
        );

        // Leaving Edit Linked Parts promotes the part back up to its root.
        let promoted = app.world_mut().resource_scope(
            |world, mut selection: Mut<crate::world_api::SelectionSet>| {
                let objects = world.resource::<crate::world_api::ObjectState>();
                crate::edit_selection::promote_selection_to_roots(&mut selection, objects)
            },
        );
        assert!(
            promoted,
            "a selected child prim has a root to be promoted to"
        );
        assert!(
            is_selected(&app, root) && !is_selected(&app, child),
            "promoting a part selection yields its linkset root"
        );
        Ok(())
    }

    /// **Select Face picks one face, and shift toggles it back off**: the
    /// distinct `LLToolFace` mode, where a click resolves to a prim *face*
    /// rather than sweeping or driving a gizmo — and where toggling an object's
    /// last face out drops the object from the selection with it.
    #[test]
    fn select_face_mode_picks_one_face_and_shift_toggles_it_off() -> Result<(), TestError> {
        use bevy::input::keyboard::Key;

        let mut app = super::world_app_with_edit();
        let scoped = seed_prim(
            &mut app,
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            },
        );
        settle(&mut app, 5);
        let target =
            first_tagged_face_position(&mut app).ok_or("the fixture prim never built a face")?;
        install_camera(&mut app, target + Vec3::new(0.0, 0.0, 10.0), target);
        {
            let mut tool = app
                .world_mut()
                .resource_mut::<crate::world_api::EditToolState>();
            tool.active = true;
            tool.tool = crate::world_api::EditTool::SelectFace;
        }
        settle(&mut app, 2);

        let centre = Vec2::new(400.0, 300.0);
        super::select_by_click(&mut app, centre);
        let picked = {
            let selection = app.world().resource::<crate::world_api::SelectionSet>();
            assert!(
                selection.is_selected(scoped),
                "a face click selects the prim carrying the face"
            );
            let faces = selection
                .primary_faces()
                .ok_or("a face click must leave a face set, not the whole object")?;
            assert!(
                faces.len() == 1,
                "a plain face click selects exactly one face, got {}",
                faces.len()
            );
            *faces.iter().next().ok_or("just asserted one")?
        };

        // Shift on the same face toggles it out — and it was the object's last,
        // so the object goes with it.
        interact::with_modifier(&mut app, KeyCode::ShiftLeft, Key::Shift, |app| {
            interact::hover(app, centre);
            interact::press(app, MouseButton::Left);
            interact::release(app, MouseButton::Left);
        });
        settle(&mut app, 2);
        assert!(
            app.world()
                .resource::<crate::world_api::SelectionSet>()
                .is_empty(),
            "toggling an object's last face off drops the object (face {picked:?})"
        );
        Ok(())
    }

    /// **A press on a gizmo handle is never a selection click**: the ordering
    /// `handle_select_pointer.after(drive_gizmo_interaction)` plus the
    /// `claims_pointer` guard. The +X cone hangs out over empty world, so
    /// without the guard this press would classify as an empty-world gesture
    /// and its release would clear the very selection the rig is mounted on.
    #[test]
    fn a_press_on_a_gizmo_handle_never_changes_the_selection() -> Result<(), TestError> {
        let mut app = super::world_app_with_edit();
        let scoped = seed_prim(
            &mut app,
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            },
        );
        settle(&mut app, 5);
        let target =
            first_tagged_face_position(&mut app).ok_or("the fixture prim never built a face")?;
        install_camera(&mut app, target + Vec3::new(0.0, 2.0, 10.0), target);
        enter_build_mode(&mut app);
        super::select_by_click(&mut app, Vec2::new(400.0, 300.0));
        let entity = super::entity_of(&mut app, scoped).ok_or("the fixture prim has no entity")?;
        settle(&mut app, 3);

        let pivot = app
            .world()
            .get::<GlobalTransform>(entity)
            .map(GlobalTransform::translation)
            .ok_or("the selected prim has no transform")?;
        let cone = translate_x_cone(&mut app, pivot).ok_or("no translate-x handle spawned")?;
        let at = world_to_viewport(&mut app, cone).ok_or("the cone projects off screen")?;
        // A world pick at the cone must find nothing — otherwise the guard is
        // not what keeps the selection, and this test proves nothing.
        assert!(
            world_to_viewport(&mut app, pivot).is_some_and(|centre| (at.x - centre.x).abs() > 20.0),
            "the cone must sit clear of the prim's own screen position"
        );
        let _before = wire_selection(&mut app);

        interact::hover(&mut app, at);
        interact::press(&mut app, MouseButton::Left);
        interact::release(&mut app, MouseButton::Left);
        settle(&mut app, 2);

        assert!(
            is_selected(&app, scoped) && primary(&app) == Some(scoped),
            "a press on a manipulator handle leaves the selection exactly as it was"
        );
        let (selected, deselected) = wire_selection(&mut app);
        assert!(
            selected.is_empty() && deselected.is_empty(),
            "an unchanged selection sends no select / deselect \
             (selected {selected:?}, deselected {deselected:?})"
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

    /// **The whole loop, through the UI** — the `with_ui` composition
    /// ([[viewer-world-test-harness]]) and the second consumer of the
    /// synthetic pointer ([[viewer-ui-interaction-harness]]).
    ///
    /// Every other pie-target test above stops at the *request*: it asserts
    /// that a right-click asked for a pie. This one runs the request through
    /// the real widget — the pie spawns under the scaffold's root, measures
    /// its labels, sizes its ring and places itself at the cursor — and then
    /// clicks the label a user would aim at. What comes out the far end is a
    /// `TouchObject` on the wire, so the whole path is under test at once:
    /// `SlEvent` in, pick, pie, layout, pointer, dispatch, `SlCommand` out.
    #[test]
    fn a_pie_slice_clicked_in_world_sends_its_command() -> Result<(), TestError> {
        let mut app = super::world_app_with_ui()?;
        // Touch is the object pie's north slice, and it is enabled only for an
        // object whose linkset handles touch.
        let scoped = super::seed_prim_with_flags(
            &mut app,
            Vector {
                x: 128.0,
                y: 128.0,
                z: 30.0,
            },
            crate::object_menu::FLAGS_HANDLE_TOUCH,
        );
        settle(&mut app, 5);
        let target =
            first_tagged_face_position(&mut app).ok_or("the fixture prim never built a face")?;
        install_camera(&mut app, target + Vec3::new(0.0, 0.0, 10.0), target);
        settle(&mut app, 2);

        // The right-click that opens the pie, at the viewport centre.
        let centre = Vec2::new(400.0, 300.0);
        interact::hover(&mut app, centre);
        interact::press(&mut app, MouseButton::Right);
        interact::release(&mut app, MouseButton::Right);
        // The pick resolves, the pie spawns hidden, its labels are measured and
        // the ring fitted around them, and `place_pie_menu` waits for two
        // agreeing frames before revealing it and starting the flick.
        settle(&mut app, 8);
        // Opening the pie also asks for the object's properties (for the Mute
        // entry's name); that is not what this test is about.
        let _opening = super::drain_commands(&mut app);

        // Where the user sees `Touch`, not where the maths says north is.
        let touch_at = interact::centre_of(&mut app, "pie-label:north")
            .ok_or("the object pie drew no north label — is the prim touchable?")?;
        interact::hover(&mut app, touch_at);
        interact::press(&mut app, MouseButton::Left);
        interact::release(&mut app, MouseButton::Left);
        settle(&mut app, 2);

        let touches: Vec<_> = super::drain_commands(&mut app)
            .into_iter()
            .filter_map(|command| match command {
                sl_client_bevy::Command::TouchObject {
                    local_id,
                    surface: _surface,
                } => Some(local_id),
                _other => None,
            })
            .collect();
        assert!(
            touches.len() == 1 && touches.first() == Some(&scoped),
            "clicking the pie's Touch label must send exactly one TouchObject for the \
             right-clicked prim, got {touches:?}"
        );
        assert!(
            find_by_name(&mut app, "pie-menu").is_none(),
            "committing a slice closes the menu"
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
