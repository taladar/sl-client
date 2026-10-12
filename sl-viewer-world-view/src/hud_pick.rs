//! HUD picking and clicking (P35.3): the half of Phase 35 that makes a rendered
//! HUD ([`crate::hud`]) *usable*.
//!
//! [`crate::hud`] draws a worn HUD in screen space but nothing can touch it — a
//! HUD exists to be clicked. This module adds the pick path the reference viewer
//! runs alongside its world pick (`LLViewerWindow::cursorIntersect` with the HUD
//! matrices): a second, orthographic ray *through the HUD camera*, tried before
//! the world so a HUD covering half the screen never leaks a click to the ground
//! behind it.
//!
//! - **A free cursor, by default.** Outside mouselook the pointer is free
//!   ([`crate::camera`] only captures it in mouselook), so a left click has a
//!   cursor to pick with directly — the reference's model, where third person
//!   clicks the world. In mouselook the cursor is centred, so the pick falls back
//!   to the screen centre (the crosshair). The old `H` free-cursor toggle is gone:
//!   it only existed to escape the debug fly-camera's permanent grab.
//! - **A HUD pick, HUD before world.** On a left click ([`pick_and_touch`]) an
//!   orthographic ray is cast through the `HudCamera` at
//!   the cursor, restricted to the HUD render layer. If it hits, that HUD face is
//!   touched; only if nothing HUD-ward is under the cursor does the click fall
//!   through to a perspective world ray from the fly camera, exactly the
//!   reference's HUD-first order.
//! - **The surface the sim needs.** A touch carries a [`SurfaceInfo`] built from
//!   the hit ([`surface_info_from_hit`]): the picked face index, its texture
//!   (`UV`) and surface (`ST`) coordinates, and the intersection point / normal /
//!   binormal — the reference's `LLPickInfo::getSurfaceInfo`, which is what a
//!   script reads back through `llDetectedTouchFace` / `llDetectedTouchST` /
//!   `llDetectedTouchUV` / `llDetectedTouchPos` and friends.
//!
//! - **A touch lasts as long as the button is down.** The press sends an
//!   `ObjectGrab`, the release an `ObjectDeGrab` ([`hold_touch`]), so a
//!   script's `touch_start` and `touch_end` are as far apart as the user's
//!   press and release — and on Second Life, which hands a script a `touch`
//!   event on its own clock while a grab is open, a held button is a held
//!   touch. OpenSim hands a script a `touch` only for an `ObjectGrabUpdate`,
//!   so one follows the press and another every time the cursor moves to a
//!   different point of the surface, which is when the reference viewer
//!   sends them (`LLToolGrab::handleHoverNonPhysical`).
//! - **Updates only for a prim whose script takes touches**
//!   ([`FLAGS_HANDLE_TOUCH`] on it or its root). Both grids *move* a prim
//!   that is not physical to an update's grab position when its owner sends
//!   one and no script in it takes touches — the drag of the reference's grab
//!   tool — so an update for any other prim would carry it off. Measured on
//!   both grids, `book/src/gridspec/building.md` § Touch and grab.

use bevy::ecs::system::SystemParam;
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::ray_cast::RayMeshHit;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sl_client_bevy::{Command, ObjectKey, ScopedObjectId, SlCommand, SurfaceInfo, Vector};

use crate::hud::{HudCamera, HudScreen};
use sl_viewer_world_api::targeted_ray_cast::{TargetVisibility, TargetedRayCast};
use sl_viewer_world_api::{
    FLAGS_HANDLE_TOUCH, MediaWorldClick, ObjectState, SurfaceFrame, ViewerCamera,
    pointer_over_blocking_ui, surface_info_from_hit,
};
use sl_viewer_world_objects::objects::{FaceTextureDebug, PrimFaceEntity, SceneObject};

/// The mouse button a HUD (or fall-through world) touch is made with.
const TOUCH_BUTTON: MouseButton = MouseButton::Left;

/// No offset: a touch grabs a prim where the pick struck it.
const NO_OFFSET: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// The touch the left button is holding open, from its `ObjectGrab` until the
/// `ObjectDeGrab` its release sends.
#[derive(Debug, Resource, Default)]
pub struct HeldTouch {
    /// The touch, while the button is down on something.
    held: Option<Held>,
}

impl HeldTouch {
    /// The object the button is held on.
    #[must_use]
    pub fn object(&self) -> Option<ScopedObjectId> {
        self.held.as_ref().map(|held| held.scoped)
    }
}

/// One held touch.
#[derive(Debug)]
struct Held {
    /// The touched prim.
    scoped: ScopedObjectId,
    /// Its entity: what a moved cursor is tested against.
    object: Entity,
    /// The prim's key, where a script takes its touches — and so where the
    /// held button sends updates.
    updates: Option<ObjectKey>,
    /// Whether it is a HUD attachment, picked through the HUD camera.
    hud: bool,
    /// The surface the last message stated.
    surface: SurfaceInfo,
    /// Where the cursor was when the button was last looked at; `None` until
    /// it has been.
    cursor: Option<Vec2>,
    /// When the last update was sent.
    updated: core::time::Duration,
}

/// The surface a held button states when the cursor has left the prim: the
/// reference's "no intersection" values.
const fn no_surface() -> SurfaceInfo {
    SurfaceInfo {
        uv: [-1.0, -1.0],
        st: [-1.0, -1.0],
        face_index: -1,
        position: NO_OFFSET,
        normal: NO_OFFSET,
        binormal: NO_OFFSET,
    }
}

/// Everything the pick needs to identify and touch the object under a hit face.
type FaceQuery<'world, 'state> =
    Query<'world, 'state, (&'static PrimFaceEntity, &'static FaceTextureDebug)>;

/// The pointer a click is made with, bundled as one [`SystemParam`]: which
/// buttons and modifiers are down, where the cursor is, and whether it is over a
/// UI panel that claims the click.
#[derive(Debug, SystemParam)]
pub struct TouchPointer<'w, 's> {
    /// The mouse buttons.
    buttons: Res<'w, ButtonInput<MouseButton>>,
    /// The modifier keys — `Alt` makes a left-click a camera gesture, not a touch.
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    /// What the pointer is over this frame.
    hover_map: Res<'w, HoverMap>,
    /// Which of those entities are pickable.
    pickables: Query<'w, 's, &'static Pickable>,
    /// Their computed sizes, for the hit test.
    node_sizes: Query<'w, 's, &'static ComputedNode>,
    /// The window the cursor position is read from.
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}

impl TouchPointer<'_, '_> {
    /// Whether this frame begins a plain (un-modified) touch click. An
    /// `Alt`-held left-click is the camera focus / orbit gesture
    /// (`crate::camera::focus_on_object`), not a touch.
    fn touch_started(&self) -> bool {
        let alt =
            self.keyboard.pressed(KeyCode::AltLeft) || self.keyboard.pressed(KeyCode::AltRight);
        self.buttons.just_pressed(TOUCH_BUTTON) && !alt
    }

    /// Whether the cursor is over a solid UI element, which occludes the click.
    ///
    /// This pick casts its own ray instead of going through `bevy_picking`, so a
    /// floater's `should_block_lower` never gets to stop it — without this guard a
    /// click on a window drawn over the HUD would *also* touch the HUD attachment
    /// (or world object) behind it. Only `bevy_ui` nodes are in the
    /// [`HoverMap`] (no mesh-picking backend is installed), so a hovered
    /// block-lower `Pickable` means the cursor is over a solid UI element.
    fn over_blocking_ui(&self) -> bool {
        pointer_over_blocking_ui(&self.hover_map, &self.pickables, &self.node_sizes)
    }

    /// The free cursor's position, or the screen centre when it is captured
    /// (mouselook) — the crosshair the first-person view aims with.
    fn cursor(&self) -> Option<Vec2> {
        let window = self.windows.single().ok()?;
        Some(
            window
                .cursor_position()
                .unwrap_or_else(|| Vec2::new(window.width() * 0.5, window.height() * 0.5)),
        )
    }
}

/// What a touch pick resolves a hit through, bundled as one [`SystemParam`]: the
/// struck face's markers, the linkset's scene identities, their world transforms,
/// and the parent links walked between them.
#[derive(Debug, SystemParam)]
pub struct TouchTargets<'w, 's> {
    /// The face / submesh child the ray struck.
    faces: FaceQuery<'w, 's>,
    /// The object entities carrying the scene identity.
    scene: Query<'w, 's, &'static SceneObject>,
    /// Their world transforms, to map the hit into the object's own frame.
    globals: Query<'w, 's, &'static GlobalTransform>,
    /// The parent links walked up the linkset.
    parents: Query<'w, 's, &'static ChildOf>,
    /// The object graph: whether a script in the prim takes touches.
    state: Res<'w, ObjectState>,
}

impl TouchTargets<'_, '_> {
    /// The prim the face or submesh `entity` belongs to, with its entity.
    fn prim_of(&self, entity: Entity) -> Option<(ScopedObjectId, Entity)> {
        let mut current = entity;
        loop {
            if let Ok(scene) = self.scene.get(current) {
                return Some((scene.scoped_id, current));
            }
            current = self.parents.get(current).ok()?.parent();
        }
    }

    /// The surface `hit` struck on the face `entity` of the prim whose entity
    /// is `object`: in the prim's own frame for a HUD, in the region's for
    /// anything in the world.
    fn surface(&self, entity: Entity, object: Entity, hit: &RayMeshHit, hud: bool) -> SurfaceInfo {
        let face = self.faces.get(entity).ok();
        let frame = match self.globals.get(object) {
            Ok(global) if hud => SurfaceFrame::Object(global),
            _ => SurfaceFrame::Region,
        };
        surface_info_from_hit(
            hit,
            face.map(|(marker, _tf)| marker.face_id),
            face.map(|(_marker, FaceTextureDebug(tf))| tf),
            frame,
        )
    }
}

/// Where a resolved touch goes, bundled as one [`SystemParam`]: the wire command
/// channel, and the media-click channel a media-enabled face claims the click
/// through instead.
#[expect(
    missing_debug_implementations,
    reason = "a Bevy `MessageWriter` has no Debug; a hand-written impl could only print the \
              field names"
)]
#[derive(SystemParam)]
pub struct TouchOut<'w> {
    /// The command channel the touch is sent on.
    writer: MessageWriter<'w, SlCommand>,
    /// The media system's click channel.
    media_clicks: MessageWriter<'w, MediaWorldClick>,
    /// The touch the button is holding open.
    held: ResMut<'w, HeldTouch>,
    /// The mouse buttons: whether the press that made a touch is still down
    /// when its pick resolves.
    buttons: Res<'w, ButtonInput<MouseButton>>,
    /// The clock an update's time since the last is read off.
    time: Res<'w, Time>,
}

impl TouchOut<'_> {
    /// Ends the held touch, if there is one, with an `ObjectDeGrab`.
    fn release(&mut self) {
        if let Some(held) = self.held.held.take() {
            self.writer.write(SlCommand(Command::DegrabObject {
                local_id: held.scoped,
                surface: Some(held.surface),
            }));
        }
    }
}

/// On a left click, touch the HUD face under the cursor — or, if none,
/// request the **GPU ID-buffer pick** ([`crate::gpu_pick`]) of the world
/// under it (the reference's HUD-first pick order); [`resolve_touch_pick`]
/// dispatches the touch when the readback lands 1–2 frames later.
///
/// The cursor is free in every camera mode except mouselook, so the click has a
/// pointer position to use directly; in mouselook (a centred, captured cursor) the
/// pick falls back to the screen centre (the crosshair). The HUD ray is
/// orthographic (through the HUD camera) and restricted to the HUD render layer;
/// the world pick view excludes HUD-layer geometry, so the two passes never
/// poach each other's geometry.
pub fn pick_and_touch(
    pointer: TouchPointer,
    hud: HudRayCast,
    targets: TouchTargets,
    mut picker: ResMut<crate::gpu_pick::GpuPicker>,
    mut out: TouchOut,
) {
    // While the build tool is active a left click *selects* rather than
    // touches; that arbitration lives in the registration —
    // `pick_and_touch.run_if(edit_tool_inactive)` — because this system is at
    // Bevy's system-parameter limit.
    if !pointer.touch_started() || pointer.over_blocking_ui() {
        return;
    }
    let Some(cursor) = pointer.cursor() else {
        return;
    };

    // 1. HUD first: an orthographic ray through the HUD camera at the cursor,
    //    limited to the HUD subtree.
    if let Some((entity, hit)) = hud.hit(cursor)
        && touch_hit(entity, &hit, &targets, &mut out, true)
    {
        return;
    }

    // 2. Fall through to the world: the GPU ID-buffer pick at the cursor
    //    (HUD-layer geometry is excluded from the pick view by construction);
    //    `resolve_touch_pick` dispatches the touch on the readback.
    picker.request(cursor, crate::gpu_pick::PickPurpose::Touch);
}

/// Dispatch a resolved world touch pick: an object-face hit is surface-refined
/// against the one face the ID buffer named (recovering the face index /
/// ST / UV / position / normal the sim's surface block wants) and touched;
/// avatar / terrain / water hits touch nothing, exactly like the old ray
/// walk resolving to no object.
pub fn resolve_touch_pick(
    mut picks: MessageReader<crate::gpu_pick::GpuPickResolved>,
    ray_cast: TargetedRayCast,
    targets: TouchTargets,
    mut out: TouchOut,
) {
    for pick in picks.read() {
        if pick.purpose != crate::gpu_pick::PickPurpose::Touch {
            continue;
        }
        let Some(hit) = pick.hit.as_ref() else {
            continue;
        };
        let crate::gpu_pick::PickResolution::ObjectFace { entity, .. } = hit.resolution else {
            continue;
        };
        // Refine against the one named face for the exact struck surface.
        let Some((entity, ray_hit)) = ray_cast.nearest(pick.ray, [entity], TargetVisibility::Any)
        else {
            continue;
        };
        touch_hit(entity, &ray_hit, &targets, &mut out, false);
    }
}

/// The orthographic **HUD ray cast**: a ray through the [`HudCamera`] at the
/// cursor, tested against the HUD subtree's meshes only.
///
/// HUDs are 3D meshes on the HUD render layer, not `bevy_ui` nodes, so they never
/// appear in the [`HoverMap`]. The candidates are the descendants of the
/// [`HudScreen`] — the one subtree every routed HUD attachment hangs under — and
/// the test is a [`TargetedRayCast`] over them, so its cost is the handful of
/// HUD meshes. Neither a filtered `MeshRayCast` (a broad phase over every mesh
/// in the region, what the hover tooltip paid on every dwelt frame) nor a scan
/// of render-layered meshes (world geometry carries the main and probe layers,
/// so that is every mesh too) scales that way.
#[expect(
    missing_debug_implementations,
    reason = "it holds a `TargetedRayCast`, which has no Debug (Bevy's `Assets<Mesh>`)"
)]
#[derive(SystemParam)]
pub struct HudRayCast<'w, 's> {
    /// The HUD camera the ray is cast through.
    camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<HudCamera>>,
    /// The HUD subtree's root.
    screen: Query<'w, 's, Entity, With<HudScreen>>,
    /// Hierarchy links, to walk the HUD subtree.
    children: Query<'w, 's, &'static Children>,
    /// The candidate-restricted mesh ray caster.
    cast: TargetedRayCast<'w, 's>,
}

impl HudRayCast<'_, '_> {
    /// The orthographic HUD ray through `cursor` (logical px), when the HUD
    /// camera exists.
    #[must_use]
    pub fn ray(&self, cursor: Vec2) -> Option<Ray3d> {
        let (camera, camera_transform) = self.camera.single().ok()?;
        camera.viewport_to_world(camera_transform, cursor).ok()
    }

    /// The nearest **shown** HUD surface under `cursor`, with its entity.
    /// Inherited visibility, not per-view: the HUD is drawn by its own camera,
    /// and a HUD entity's `ViewVisibility` from the world camera (which never
    /// renders it) would read false.
    #[must_use]
    pub fn hit(&self, cursor: Vec2) -> Option<(Entity, RayMeshHit)> {
        let ray = self.ray(cursor)?;
        let screen = self.screen.single().ok()?;
        let hud = self.children.iter_descendants(screen);
        self.cast.nearest(ray, hud, TargetVisibility::Visible)
    }

    /// Whether the cursor is over a **HUD attachment**, which occludes the world
    /// behind it: a world pick — touch, hover, or the avatar context menu's body
    /// pick — must treat a HUD hit as occlusion and stop, so the occlusion order
    /// is **UI, then HUD attachments, then world**.
    #[must_use]
    pub fn over(&self, cursor: Vec2) -> bool {
        self.hit(cursor).is_some()
    }
}

/// Resolve a ray hit to its object and begin a touch of it, carrying the
/// surface the ray struck. Returns whether the hit was taken (a hit that
/// resolves to no object — e.g. an avatar's own mesh — is not).
///
/// The touch is an `ObjectGrab`, and for a prim whose script takes touches
/// the `ObjectGrabUpdate` the reference follows it with. It stays open while
/// the button is down ([`hold_touch`] ends it); a press that was let go
/// before its pick resolved — the world pick answers a frame or two late —
/// is ended here.
fn touch_hit(
    entity: Entity,
    hit: &RayMeshHit,
    targets: &TouchTargets,
    out: &mut TouchOut,
    hud: bool,
) -> bool {
    // The ray strikes a face/submesh child entity; the prim is the first
    // entity above it carrying the scene identity.
    let Some((scoped, object)) = targets.prim_of(entity) else {
        return false;
    };
    let surface = targets.surface(entity, object, hit, hud);
    // A face flagged for media-on-a-prim claims the click for the media
    // system (focus / interact, `crate::media_prim`) instead of a touch —
    // the reference's `handleMediaClick` running before `handleLeftClickPick`
    // dispatches the touch.
    if let Ok((marker, FaceTextureDebug(tf))) = targets.faces.get(entity)
        && tf.media_enabled()
    {
        out.media_clicks.write(MediaWorldClick {
            entity,
            scoped,
            face: marker.face_id,
            uv: Vec2::new(surface.uv[0], surface.uv[1]),
        });
        return true;
    }
    info!(
        "touch ({}) object {scoped:?} face={} pos=({:.2},{:.2},{:.2})",
        if hud { "HUD" } else { "world" },
        surface.face_index,
        surface.position.x,
        surface.position.y,
        surface.position.z,
    );
    // A touch already open is over before another begins.
    out.release();
    out.writer.write(SlCommand(Command::GrabObject {
        local_id: scoped,
        grab_offset: NO_OFFSET,
        surface: Some(surface.clone()),
    }));
    let updates = targets
        .state
        .pick_summary(scoped)
        .filter(|summary| summary.flags & FLAGS_HANDLE_TOUCH != 0)
        .map(|summary| summary.picked_full);
    let mut held = Held {
        scoped,
        object,
        updates,
        hud,
        surface,
        cursor: None,
        updated: out.time.elapsed(),
    };
    send_update(&mut out.writer, &mut held, None, out.time.elapsed());
    out.held.held = Some(held);
    if !out.buttons.pressed(TOUCH_BUTTON) {
        out.release();
    }
    true
}

/// Sends the `ObjectGrabUpdate` of a held touch — where a script takes the
/// prim's touches — stating `surface`, or the surface it already holds.
fn send_update(
    writer: &mut MessageWriter<SlCommand>,
    held: &mut Held,
    surface: Option<SurfaceInfo>,
    now: core::time::Duration,
) {
    let Some(object_id) = held.updates else {
        return;
    };
    if let Some(surface) = surface {
        held.surface = surface;
    }
    let since = now.saturating_sub(held.updated);
    held.updated = now;
    writer.write(SlCommand(Command::GrabObjectUpdate {
        object_id,
        grab_offset_initial: NO_OFFSET,
        // Where the grab is: the point of the surface under the cursor. A
        // prim whose script takes touches is not moved by it.
        grab_position: held.surface.position.clone(),
        time_since_last: u32::try_from(since.as_millis()).unwrap_or(u32::MAX),
        surface: Some(held.surface.clone()),
    }));
}

/// Keeps a held touch up to date and ends it: an `ObjectGrabUpdate` when the
/// cursor has moved to another point of the prim's surface (or off it), and
/// the `ObjectDeGrab` when the button is let go.
///
/// Not gated on the build tool as the pick is: a touch begun before the tool
/// was taken up still has to end.
pub fn hold_touch(
    pointer: TouchPointer,
    hud: HudRayCast,
    world_camera: Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
    targets: TouchTargets,
    mut out: TouchOut,
) {
    if out.held.held.is_none() {
        return;
    }
    if !out.buttons.pressed(TOUCH_BUTTON) {
        out.release();
        return;
    }
    let now = out.time.elapsed();
    let Some(held) = out.held.held.as_mut() else {
        return;
    };
    let Some(cursor) = pointer.cursor() else {
        return;
    };
    // The first look is the press itself: its surface went out with the grab.
    let moved = held.cursor.is_some_and(|last| last != cursor);
    held.cursor = Some(cursor);
    if held.updates.is_none() || !moved {
        return;
    }
    let ray = if held.hud {
        hud.ray(cursor)
    } else {
        world_camera
            .single()
            .ok()
            .and_then(|(camera, transform)| camera.viewport_to_world(transform, cursor).ok())
    };
    // The prim's own faces only, as the reference re-picks against the one
    // object it holds.
    let surface = ray
        .and_then(|ray| {
            let faces =
                core::iter::once(held.object).chain(hud.children.iter_descendants(held.object));
            hud.cast.nearest(ray, faces, TargetVisibility::Any)
        })
        .filter(|(entity, _hit)| {
            targets
                .prim_of(*entity)
                .is_some_and(|(scoped, _object)| scoped == held.scoped)
        })
        .map_or_else(no_surface, |(entity, hit)| {
            targets.surface(entity, held.object, &hit, held.hud)
        });
    if surface != held.surface {
        send_update(&mut out.writer, held, Some(surface), now);
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{Affine3A, Quat, Vec2, Vec3};
    use bevy::picking::mesh_picking::ray_cast::RayMeshHit;
    use bevy::transform::components::GlobalTransform;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::PrimFaceId;
    use sl_client_bevy::TextureFace;
    use sl_viewer_world_api::{SurfaceFrame, surface_info_from_hit, vscale, vsub};

    /// A ray hit with the given world point / normal / uv and a flat triangle in
    /// the XY plane, enough to exercise the surface-info build.
    fn hit(point: Vec3, normal: Vec3, uv: Option<Vec2>) -> RayMeshHit {
        RayMeshHit {
            point,
            normal,
            barycentric_coords: Vec3::ZERO,
            distance: 1.0,
            triangle: Some([
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ]),
            uv,
            triangle_index: Some(0),
        }
    }

    /// The face index passes through, and a hit with no face reports `-1` (the
    /// reference's "no intersection" value a script sees as `llDetectedTouchFace`).
    #[test]
    fn face_index_passes_through_or_defaults_to_minus_one() {
        let object = GlobalTransform::IDENTITY;
        let with_face = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, Some(Vec2::new(0.25, 0.5))),
            Some(PrimFaceId::new(4)),
            None,
            SurfaceFrame::Object(&object),
        );
        assert_eq!(with_face.face_index, 4);
        let no_face = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, None),
            None,
            None,
            SurfaceFrame::Object(&object),
        );
        assert_eq!(no_face.face_index, -1);
    }

    /// ST un-flips the Bevy (top-down) mesh coordinate back into Second Life's
    /// bottom-up space, and with an identity texture placement UV equals ST.
    #[test]
    fn st_unflips_and_uv_matches_without_placement() {
        let object = GlobalTransform::IDENTITY;
        let default_face =
            TextureFace::new(sl_client_bevy::TextureKey::from(sl_client_bevy::Uuid::nil()));
        let info = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, Some(Vec2::new(0.25, 0.75))),
            Some(PrimFaceId::new(0)),
            Some(&default_face),
            SurfaceFrame::Object(&object),
        );
        // ST: v flipped (1 - 0.75 = 0.25), u unchanged.
        assert!((info.st[0] - 0.25).abs() < 1e-6, "{:?}", info.st);
        assert!((info.st[1] - 0.25).abs() < 1e-6, "{:?}", info.st);
        // The identity placement leaves UV equal to ST.
        assert!((info.uv[0] - info.st[0]).abs() < 1e-6, "{:?}", info.uv);
        assert!((info.uv[1] - info.st[1]).abs() < 1e-6, "{:?}", info.uv);
    }

    /// A rotated, offset placement reports the UV the reference does — its
    /// `xform` (`llface.cpp`) of ST in Second Life space, which is what a script
    /// reads as `llDetectedTouchUV`. The placement acts on this viewer's flipped
    /// coordinates, so a UV built from the unflipped `xform` came out turned
    /// and slid the wrong way.
    #[test]
    fn uv_is_the_reference_xform_of_st() {
        let object = GlobalTransform::IDENTITY;
        let mut face =
            TextureFace::new(sl_client_bevy::TextureKey::from(sl_client_bevy::Uuid::nil()));
        face.rotation = core::f32::consts::FRAC_PI_2;
        face.offset_s = 0.1;
        face.offset_t = 0.25;
        let info = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, Some(Vec2::new(0.2, 0.3))),
            Some(PrimFaceId::new(0)),
            Some(&face),
            SurfaceFrame::Object(&object),
        );
        // The reference `xform` at a quarter turn: about the centre,
        // s' = t and t' = -s, then offset.
        let (s, t) = (info.st[0] - 0.5, info.st[1] - 0.5);
        let expected = [t + 0.5 + 0.1, -s + 0.5 + 0.25];
        assert!(
            (info.uv[0] - expected[0]).abs() < 1e-5,
            "{:?} vs {expected:?}",
            info.uv
        );
        assert!(
            (info.uv[1] - expected[1]).abs() < 1e-5,
            "{:?} vs {expected:?}",
            info.uv
        );
    }

    /// A doubled texture repeat tiles the sampled coordinate: UV moves twice as
    /// far from the face centre as ST does.
    #[test]
    fn uv_applies_the_face_texture_repeats() {
        let object = GlobalTransform::IDENTITY;
        let mut face =
            TextureFace::new(sl_client_bevy::TextureKey::from(sl_client_bevy::Uuid::nil()));
        face.scale_s = 2.0;
        face.scale_t = 2.0;
        let info = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, Some(Vec2::new(1.0, 1.0))),
            Some(PrimFaceId::new(0)),
            Some(&face),
            SurfaceFrame::Object(&object),
        );
        // ST is the un-flipped stored coordinate; UV tiles it about the centre, so
        // the two differ once the repeat is above one.
        let differs =
            (info.uv[0] - info.st[0]).abs() > 1e-3 || (info.uv[1] - info.st[1]).abs() > 1e-3;
        assert!(differs, "st {:?} uv {:?}", info.st, info.uv);
    }

    /// The hit point and normal are carried into the object's own Second Life
    /// frame: a translated / rotated object un-does its transform, so the surface
    /// data is object-relative rather than world.
    #[test]
    fn position_and_normal_are_object_local() {
        // An object translated to (10, 0, 0) and yawed 90° about Z.
        let object = GlobalTransform::from(Affine3A::from_rotation_translation(
            Quat::from_rotation_z(core::f32::consts::FRAC_PI_2),
            Vec3::new(10.0, 0.0, 0.0),
        ));
        // A world hit at the object's origin, normal along world +X.
        let info = surface_info_from_hit(
            &hit(Vec3::new(10.0, 0.0, 0.0), Vec3::X, Some(Vec2::ZERO)),
            Some(PrimFaceId::new(0)),
            None,
            SurfaceFrame::Object(&object),
        );
        // The hit is at the object's own origin.
        assert!(info.position.x.abs() < 1e-5, "{:?}", info.position);
        assert!(info.position.y.abs() < 1e-5, "{:?}", info.position);
        // World +X, un-yawed by 90° about Z, becomes object-local -Y.
        assert!((info.normal.y + 1.0).abs() < 1e-5, "{:?}", info.normal);
        // The normal is a unit vector.
        let len = (info.normal.x * info.normal.x
            + info.normal.y * info.normal.y
            + info.normal.z * info.normal.z)
            .sqrt();
        assert!((len - 1.0).abs() < 1e-5, "normal not unit: {len}");
    }

    /// The binormal comes out perpendicular to the normal and unit length, so the
    /// surface frame the sim receives is well-formed.
    #[test]
    fn binormal_is_unit_and_perpendicular_to_the_normal() {
        let object = GlobalTransform::IDENTITY;
        let info = surface_info_from_hit(
            &hit(Vec3::ZERO, Vec3::Z, Some(Vec2::ZERO)),
            Some(PrimFaceId::new(0)),
            None,
            SurfaceFrame::Object(&object),
        );
        let dot = info.normal.x * info.binormal.x
            + info.normal.y * info.binormal.y
            + info.normal.z * info.binormal.z;
        assert!(dot.abs() < 1e-5, "binormal not perpendicular: {dot}");
        let len = (info.binormal.x * info.binormal.x
            + info.binormal.y * info.binormal.y
            + info.binormal.z * info.binormal.z)
            .sqrt();
        assert!((len - 1.0).abs() < 1e-5, "binormal not unit: {len}");
    }

    /// The vector helpers do plain component arithmetic.
    #[test]
    fn vector_helpers() {
        assert_eq!(
            vsub(Vec3::new(3.0, 2.0, 1.0), Vec3::new(1.0, 1.0, 1.0)),
            Vec3::new(2.0, 1.0, 0.0)
        );
        assert_eq!(
            vscale(Vec3::new(1.0, 2.0, 3.0), 2.0),
            Vec3::new(2.0, 4.0, 6.0)
        );
    }
}
