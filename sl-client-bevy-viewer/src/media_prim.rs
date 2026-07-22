//! **Media-on-a-prim** (`viewer-media-prim-browser`, in-world half): drive
//! offscreen web-media surfaces ([`crate::media_engine`]) onto prim faces
//! whose `TextureEntry` media flag and `ObjectMedia` capability data say they
//! carry media, and route world input (hover, clicks, wheel, keyboard) into
//! the page under the pick — the reference viewer's `LLViewerMedia` /
//! `LLViewerMediaFocus` pair.
//!
//! Life cycle: an object update whose `MediaURL` version string is new
//! triggers a `RequestObjectMedia` fetch; the resulting per-face
//! [`MediaEntry`] set is held in [`MediaData`]. A periodic driver ranks the
//! media faces by camera distance (the focused face always first), keeps at
//! most [`MAX_MEDIA_SURFACES`] engine surfaces alive for the auto-play (or
//! user-started) entries, tiers their paint rates by rank, and swaps each
//! face's material for an unlit one sampling the surface's image (the
//! original material handle is restored when the surface goes away —
//! deliberately the *handle*, so the PBR / legacy-material / texture-anim
//! pipelines that keep mutating the old material by handle stay coherent).
//!
//! Input follows the reference's focus model: the first click on a media face
//! **focuses** it (interacting immediately only with `first_click_interact`),
//! further clicks interact; keyboard goes to the focused face
//! ([`crate::input_context`]'s `Media` context suppresses world movement);
//! `Escape` releases. Navigation is bounced back when it violates the
//! entry's white-list (`MediaEntry::check_candidate_url`), and interaction is
//! gated on `perms_interact` ([`media_permission_allows`]).

use std::collections::HashMap;

use bevy::input::keyboard::KeyboardInput;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use sl_client_bevy::{
    Command, MEDIA_PERM_ANYONE, MEDIA_PERM_OWNER, MediaEntry, ObjectKey, PrimFaceId,
    ScopedObjectId, SlCommand, SlEvent, SlSessionEvent, texture_face_uv_transform,
};

use crate::camera::ViewerCamera;
use crate::hud_pick::{pointer_over_blocking_ui, surface_info_from_hit};
use crate::input_context::InputContext;
use crate::media_engine::{
    MediaEngine, MediaEngineKind, MediaEngineSystems, MediaSurfaceId, MediaSurfaces,
};
use crate::media_keys::{current_modifiers, is_printable_text, vk_for_key_code};
use crate::objects::{FaceTextureDebug, ObjectState, PrimFaceEntity, SceneObject};
use sl_cef::{KeyInput, MediaKind, SurfaceConfig, classify_url};

/// The hard cap on simultaneously live in-world media surfaces (the
/// reference's `PluginInstancesTotal`).
pub(crate) const MAX_MEDIA_SURFACES: usize = 8;

/// The `FLAGS_OBJECT_YOU_OWNER` update-flags bit (the agent owns the object).
const FLAGS_OBJECT_YOU_OWNER: u32 = 1 << 5;

/// Paint-rate tiers by interest rank (the reference's 100/50/25/1 Hz idea,
/// scaled to CEF's 60 fps ceiling): the focused / nearest surfaces paint
/// fast, far ones idle.
const FPS_TIERS: [u8; 3] = [30, 15, 5];

/// One media face: the object (grid-wide key) and the Linden face index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MediaTarget {
    /// The object carrying the face.
    pub(crate) object: ObjectKey,
    /// The face index.
    pub(crate) face: PrimFaceId,
}

/// A left click on a media-capable prim face, claimed from the world touch
/// pick ([`crate::hud_pick::pick_and_touch`]) before it becomes a touch.
#[derive(Message, Debug, Clone)]
pub(crate) struct MediaWorldClick {
    /// The face entity the ray struck.
    pub(crate) entity: Entity,
    /// The picked object's scoped id.
    pub(crate) scoped: ScopedObjectId,
    /// The struck face.
    pub(crate) face: PrimFaceId,
    /// The **sampled** texture coordinate of the hit (the `SurfaceInfo` UV:
    /// texture placement applied, Second Life bottom-up `v`).
    pub(crate) uv: Vec2,
}

/// The in-world media focus / hover state. Read by
/// [`crate::input_context::compute_input_context`] (a focused media face
/// takes the keyboard away from the world) and by the floating controls bar
/// ([`crate::media_controls`]).
#[derive(Resource, Debug, Default)]
pub(crate) struct MediaFocus {
    /// The face holding media keyboard focus, if any.
    pub(crate) focused: Option<MediaTarget>,
    /// Whether the focused face is a browser page that takes the keyboard
    /// away from the world ([`crate::input_context`]); a focused *video*
    /// face keeps the bar visible but leaves the keyboard with the world —
    /// there is nothing to type at a video.
    pub(crate) focused_takes_keyboard: bool,
    /// The media face under the cursor this frame, if any.
    pub(crate) hover: Option<MediaTarget>,
    /// The surface pixel under the cursor on the hover face.
    pub(crate) hover_pixel: Option<(i32, i32)>,
    /// The world-space face normal at the **last** media hover hit (not
    /// cleared when the hover leaves), for the controls bar's camera zoom.
    pub(crate) hover_normal: Option<Vec3>,
    /// Whether a forwarded button press is outstanding (its release is
    /// forwarded to the same surface).
    pressed: Option<MediaTarget>,
}

/// Per-object media data from the `ObjectMedia` capability.
#[derive(Debug, Clone)]
pub(crate) struct ObjectMediaData {
    /// The media version string the data corresponds to.
    pub(crate) version: String,
    /// Per-face media entries (one slot per face; `None` = no media).
    pub(crate) faces: Vec<Option<MediaEntry>>,
}

/// All known per-object media data, plus fetch bookkeeping.
#[derive(Resource, Debug, Default)]
pub(crate) struct MediaData {
    /// Media data by object key.
    pub(crate) objects: HashMap<ObjectKey, ObjectMediaData>,
    /// The media version string last *requested* per object, so one version
    /// is fetched once.
    requested: HashMap<ObjectKey, String>,
}

impl MediaData {
    /// The media entry for `target`, if any.
    pub(crate) fn entry(&self, target: MediaTarget) -> Option<&MediaEntry> {
        self.objects
            .get(&target.object)?
            .faces
            .get(target.face.as_usize())?
            .as_ref()
    }
}

/// The runtime state of one face whose surface is live.
pub(crate) struct ActiveMedia {
    /// Which engine the surface runs on (browser page vs video playback) —
    /// decides the input routing here and the control set the floating bar
    /// shows.
    pub(crate) kind: MediaEngineKind,
    /// The engine surface.
    pub(crate) surface: MediaSurfaceId,
    /// The face entity currently wearing the media material.
    pub(crate) face_entity: Entity,
    /// The material handle the face wore before (restored on close).
    restore: Handle<StandardMaterial>,
    /// The media material (samples the surface image).
    pub(crate) material: Handle<StandardMaterial>,
    /// The surface image size the media material was last built against. A
    /// size change re-creates the material and re-inserts the component: a
    /// changed `MeshMaterial3d` is the one path guaranteed to rebind the new
    /// GPU texture (touching the material asset alone proved unreliable).
    applied_size: UVec2,
    /// Whether the user interacted (a user-started surface survives the
    /// auto-play gate).
    user_started: bool,
    /// Consecutive white-list bounce-backs (a loop closes the surface).
    bounces: u8,
    /// The last URL a white-list check accepted (bounce-back destination).
    last_good_url: Option<String>,
}

/// All live in-world media surfaces by target.
#[derive(Resource, Default)]
pub(crate) struct MediaPrimState {
    /// The live surfaces.
    pub(crate) active: HashMap<MediaTarget, ActiveMedia>,
}

/// System set for the media-on-a-prim frame work, for consumers (the
/// controls bar) to order after.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MediaPrimSystems {
    /// Ingest, surface driving and input routing.
    Drive,
}

/// The media-on-a-prim plugin.
pub(crate) struct MediaPrimPlugin;

impl Plugin for MediaPrimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MediaData>()
            .init_resource::<MediaFocus>()
            .init_resource::<MediaPrimState>()
            .add_message::<MediaWorldClick>()
            .add_systems(
                Update,
                (
                    ingest_media_events,
                    drive_media_surfaces,
                    hover_media_faces,
                    handle_media_clicks,
                    forward_media_release,
                    route_media_keyboard,
                    release_media_focus_on_escape,
                    enforce_media_whitelists,
                )
                    .chain()
                    .in_set(MediaPrimSystems::Drive)
                    .after(MediaEngineSystems::Pump),
            )
            // The wheel claim must run before the camera consumes the same
            // scroll accumulator for its orbit zoom.
            .add_systems(
                Update,
                claim_media_wheel.before(crate::camera::orbit_third_person),
            );
    }
}

/// Forward the scroll wheel to the **focused** media face while the cursor
/// hovers it, claiming the accumulated scroll so the camera's orbit zoom does
/// not also fire — the reference forwards wheel events only to the focused
/// media (`LLViewerMediaFocus::handleScrollWheel`).
fn claim_media_wheel(
    mut wheel: ResMut<bevy::input::mouse::AccumulatedMouseScroll>,
    focus: Res<MediaFocus>,
    state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
) {
    if wheel.delta == Vec2::ZERO {
        return;
    }
    let Some(target) = focus.focused else {
        return;
    };
    if focus.hover != Some(target) {
        return;
    }
    let Some(active) = state.active.get(&target) else {
        return;
    };
    if active.kind != MediaEngineKind::Web {
        // A video surface has nothing to scroll; leave the wheel to the
        // camera zoom.
        return;
    }
    let Some(slot) = surfaces.get(active.surface) else {
        return;
    };
    let (x, y) = focus.hover_pixel.unwrap_or((0, 0));
    let scale = match wheel.unit {
        bevy::input::mouse::MouseScrollUnit::Line => 40.0,
        bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
    };
    slot.surface.mouse_wheel(
        x,
        y,
        crate::browser_widget::float_to_pixel(wheel.delta.x * scale),
        crate::browser_widget::float_to_pixel(wheel.delta.y * scale),
    );
    wheel.delta = Vec2::ZERO;
}

/// Whether the agent may act on a media-permission bitfield: the *anyone*
/// bit, or the *owner* bit when the agent owns the object. The *group* bit is
/// not resolvable client-side yet (the viewer does not track the agent's
/// membership of the object's group here) and counts as denied — the
/// conservative reading.
pub(crate) const fn media_permission_allows(perms: u8, is_owner: bool) -> bool {
    if perms & MEDIA_PERM_ANYONE != 0 {
        return true;
    }
    (perms & MEDIA_PERM_OWNER != 0) && is_owner
}

/// The surface pixel for a sampled texture coordinate `uv` (placement
/// applied, Second Life bottom-up `v`), wrapping repeats — the reference's
/// `scaleTextureCoords`, without its power-of-two padding correction (our
/// surface image is exactly the media size).
pub(crate) fn media_pixel_from_uv(uv: Vec2, size: UVec2) -> (i32, i32) {
    let wrap = |value: f32| {
        let fractional = value.fract();
        if fractional < 0.0 {
            fractional + 1.0
        } else {
            fractional
        }
    };
    let width = u16::try_from(size.x.clamp(1, 8192)).unwrap_or(u16::MAX);
    let height = u16::try_from(size.y.clamp(1, 8192)).unwrap_or(u16::MAX);
    let x = (wrap(uv.x) * f32::from(width)).round();
    // Second Life's `v` runs bottom-up, a page's `y` top-down.
    let y = ((1.0 - wrap(uv.y)) * f32::from(height)).round();
    (
        crate::browser_widget::float_to_pixel(x),
        crate::browser_widget::float_to_pixel(y),
    )
}

/// Ingest protocol events: `ObjectMedia` capability data into [`MediaData`],
/// and object adds / updates whose `MediaURL` version is new into a
/// `RequestObjectMedia` fetch (a login sees every scene object as an add).
fn ingest_media_events(
    mut events: MessageReader<SlEvent>,
    mut data: ResMut<MediaData>,
    mut state: ResMut<MediaPrimState>,
    mut surfaces: NonSendMut<MediaSurfaces>,
    mut commands: MessageWriter<SlCommand>,
    mut entity_commands: Commands,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object) => {
                let Some(version) = object.media_url.as_ref().map(url::Url::as_str) else {
                    continue;
                };
                if version.is_empty() {
                    continue;
                }
                let key = object.full_id;
                let known = data.objects.get(&key).map(|media| media.version.as_str());
                let requested = data.requested.get(&key).map(String::as_str);
                if known == Some(version) || requested == Some(version) {
                    continue;
                }
                debug!("media version {version} on object {key:?}: fetching ObjectMedia");
                data.requested.insert(key, version.to_owned());
                commands.write(SlCommand(Command::RequestObjectMedia { object_id: key }));
            }
            SlSessionEvent::ObjectMedia {
                object_id,
                version,
                faces,
            } => {
                // A server-side navigation shows up as a changed current_url:
                // follow it on the live surface — unless the new URL belongs
                // to the *other* engine (a page navigated to a direct video
                // URL, or back), in which case the surface is closed and the
                // driver restarts it on the right engine.
                for (index, entry) in faces.iter().enumerate() {
                    let Some(entry) = entry else { continue };
                    let Ok(face) = u16::try_from(index) else {
                        continue;
                    };
                    let target = MediaTarget {
                        object: *object_id,
                        face: PrimFaceId::new(face),
                    };
                    let previous = data.entry(target).and_then(|old| old.current_url.clone());
                    if entry.current_url != previous
                        && let Some(active) = state.active.get(&target)
                        && let Some(url) = &entry.current_url
                    {
                        let wanted = match classify_url(url) {
                            MediaKind::Web => MediaEngineKind::Web,
                            MediaKind::Video | MediaKind::Audio => MediaEngineKind::Video,
                        };
                        if wanted == active.kind {
                            if let Some(slot) = surfaces.get(active.surface) {
                                slot.surface.navigate(url.as_str());
                            }
                        } else {
                            close_media_surface(
                                target,
                                &mut state,
                                &mut surfaces,
                                &mut entity_commands,
                            );
                        }
                    }
                }
                debug!(
                    "ObjectMedia for {object_id:?}: version {version}, {} media face(s)",
                    faces.iter().filter(|face| face.is_some()).count()
                );
                data.objects.insert(
                    *object_id,
                    ObjectMediaData {
                        version: version.clone(),
                        faces: faces.clone(),
                    },
                );
            }
            _ => {}
        }
    }
}

/// The face entity of `target`, resolved through [`ObjectState`].
fn resolve_face_entity(
    objects: &ObjectState,
    target: MediaTarget,
    faces: &Query<(&PrimFaceEntity, &FaceTextureDebug)>,
) -> Option<Entity> {
    objects
        .face_entities_by_key(target.object)?
        .iter()
        .copied()
        .find(|&entity| {
            faces
                .get(entity)
                .is_ok_and(|(face, _tf)| face.face_id == target.face)
        })
}

/// One ranked media face the surface driver considers.
struct Candidate {
    /// The face.
    target: MediaTarget,
    /// Squared camera distance (the interest metric).
    distance: f32,
    /// Whether a surface may exist for it (auto-play, or user-started).
    startable: bool,
}

/// The periodic surface driver: rank media faces by interest, keep surfaces
/// for the top auto-play / user-started entries within the cap, tier their
/// paint rates, apply / restore face materials, and reap dead faces.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the media data \
              and runtime state, the engine and surface tables, the object/face/camera lookups, \
              and the asset stores the face materials live in"
)]
fn drive_media_surfaces(
    time: Res<Time>,
    mut timer: Local<f32>,
    data: Res<MediaData>,
    mut state: ResMut<MediaPrimState>,
    mut focus: ResMut<MediaFocus>,
    mut engine: NonSendMut<MediaEngine>,
    mut surfaces: NonSendMut<MediaSurfaces>,
    objects: Res<ObjectState>,
    faces: Query<(&PrimFaceEntity, &FaceTextureDebug)>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    transforms: Query<&GlobalTransform>,
    cameras: Query<&GlobalTransform, With<ViewerCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    *timer += time.delta_secs();
    if *timer < 0.5 {
        return;
    }
    *timer = 0.0;

    let camera_position = cameras
        .single()
        .map(|transform| transform.translation())
        .unwrap_or_default();

    let mut candidates: Vec<Candidate> = Vec::new();
    for (key, media) in &data.objects {
        for (index, entry) in media.faces.iter().enumerate() {
            let Some(entry) = entry else { continue };
            if entry.current_url.is_none() && entry.home_url.is_none() {
                continue;
            }
            let Ok(face) = u16::try_from(index) else {
                continue;
            };
            let target = MediaTarget {
                object: *key,
                face: PrimFaceId::new(face),
            };
            let user_started = state
                .active
                .get(&target)
                .is_some_and(|active| active.user_started);
            let startable = entry.auto_play || user_started;
            let distance = objects
                .entity_of(*key)
                .and_then(|entity| transforms.get(entity).ok())
                .map_or(f32::MAX, |transform| {
                    let d = transform.translation();
                    let dx = d.x - camera_position.x;
                    let dy = d.y - camera_position.y;
                    let dz = d.z - camera_position.z;
                    dx.mul_add(dx, dy.mul_add(dy, dz * dz))
                });
            candidates.push(Candidate {
                target,
                distance,
                startable,
            });
        }
    }
    candidates.sort_by(|a, b| {
        let a_focused = focus.focused == Some(a.target);
        let b_focused = focus.focused == Some(b.target);
        b_focused
            .cmp(&a_focused)
            .then(a.distance.total_cmp(&b.distance))
    });

    // The wanted set: startable candidates within the cap.
    let wanted: Vec<MediaTarget> = candidates
        .iter()
        .filter(|candidate| candidate.startable)
        .take(MAX_MEDIA_SURFACES)
        .map(|candidate| candidate.target)
        .collect();

    // Close surfaces that fell out of the wanted set or whose data vanished.
    let stale: Vec<MediaTarget> = state
        .active
        .keys()
        .copied()
        .filter(|target| !wanted.contains(target))
        .collect();
    for target in stale {
        close_media_surface(target, &mut state, &mut surfaces, &mut commands);
        if focus.focused == Some(target) {
            focus.focused = None;
            focus.focused_takes_keyboard = false;
        }
    }

    // Create / repair surfaces for the wanted set and tier their rates.
    for (rank, target) in wanted.iter().enumerate() {
        let Some(entry) = data.entry(*target).cloned() else {
            continue;
        };
        let face_entity = resolve_face_entity(&objects, *target, &faces);
        if let Some(active) = state.active.get_mut(target) {
            let fps = if focus.focused == Some(*target) {
                FPS_TIERS[0]
            } else {
                *FPS_TIERS.get(rank / 2).unwrap_or(&1)
            };
            let mut surface_size = active.applied_size;
            if let Some(slot) = surfaces.get(active.surface) {
                slot.surface.set_max_fps(fps);
                surface_size = slot.size;
            }
            // Re-apply the media material when the face entity was rebuilt (a
            // shape change) or the surface image was re-allocated at a new
            // size (its first real paint, or a resize): a fresh material on a
            // changed component is what rebinds the new GPU texture.
            match face_entity {
                Some(entity)
                    if entity == active.face_entity && surface_size == active.applied_size => {}
                Some(entity) => {
                    apply_media_material(
                        entity,
                        target,
                        active,
                        &faces,
                        &mesh_materials,
                        &mut surfaces,
                        &mut materials,
                        &mut commands,
                    );
                }
                None => {
                    close_media_surface(*target, &mut state, &mut surfaces, &mut commands);
                }
            }
            continue;
        }
        let Some(entity) = face_entity else { continue };
        start_media_surface(
            *target,
            &entry,
            entity,
            false,
            &mut state,
            &mut engine,
            &mut surfaces,
            &mut images,
            &mut materials,
            &faces,
            &mesh_materials,
            &mut commands,
        );
    }
}

/// Create the engine surface for `target` and put its image on the face.
#[expect(
    clippy::too_many_arguments,
    reason = "threaded resources from the driver / click systems that own them"
)]
fn start_media_surface(
    target: MediaTarget,
    entry: &MediaEntry,
    face_entity: Entity,
    user_started: bool,
    state: &mut MediaPrimState,
    engine: &mut MediaEngine,
    surfaces: &mut MediaSurfaces,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    faces: &Query<(&PrimFaceEntity, &FaceTextureDebug)>,
    mesh_materials: &Query<&MeshMaterial3d<StandardMaterial>>,
    commands: &mut Commands,
) -> bool {
    let url = entry.current_url.as_ref().or(entry.home_url.as_ref());
    let Some(url) = url else {
        return false;
    };
    // The mime_types.xml dispatch: direct video / audio URLs go to the
    // GStreamer engine, everything else to the browser.
    let kind = match classify_url(url) {
        MediaKind::Web => MediaEngineKind::Web,
        MediaKind::Video | MediaKind::Audio => MediaEngineKind::Video,
    };
    let url = url.to_string();
    let width = u32::try_from(entry.width_pixels.clamp(0, 4096)).unwrap_or(0);
    let height = u32::try_from(entry.height_pixels.clamp(0, 4096)).unwrap_or(0);
    let config = SurfaceConfig {
        width: if width == 0 { 1024 } else { width },
        height: if height == 0 { 768 } else { height },
        initial_url: url.clone(),
        isolated: true,
        max_fps: 15,
        muted: false,
        loop_media: entry.auto_loop,
    };
    let Some(id) = surfaces.create_kind(engine, images, &config, kind) else {
        return false;
    };
    debug!("media surface started for {target:?} at {url} ({kind:?})");
    let mut active = ActiveMedia {
        kind,
        surface: id,
        face_entity,
        restore: Handle::default(),
        material: Handle::default(),
        applied_size: UVec2::ONE,
        user_started,
        bounces: 0,
        last_good_url: Some(url),
    };
    apply_media_material(
        face_entity,
        &target,
        &mut active,
        faces,
        mesh_materials,
        surfaces,
        materials,
        commands,
    );
    state.active.insert(target, active);
    true
}

/// Swap `entity`'s material for the media material sampling the surface's
/// image (recording the original for restore). Also used to re-apply after a
/// face rebuild.
#[expect(
    clippy::too_many_arguments,
    reason = "threaded resources from the driver / click systems that own them"
)]
fn apply_media_material(
    entity: Entity,
    target: &MediaTarget,
    active: &mut ActiveMedia,
    faces: &Query<(&PrimFaceEntity, &FaceTextureDebug)>,
    mesh_materials: &Query<&MeshMaterial3d<StandardMaterial>>,
    surfaces: &mut MediaSurfaces,
    materials: &mut Assets<StandardMaterial>,
    commands: &mut Commands,
) {
    let Some(slot) = surfaces.get_mut(active.surface) else {
        return;
    };
    active.applied_size = slot.size;
    let uv_transform = faces
        .get(entity)
        .map(|(_face, FaceTextureDebug(tf))| texture_face_uv_transform(tf))
        .unwrap_or_default();
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(slot.image.clone()),
        // Media renders fullbright in the reference viewer.
        unlit: true,
        uv_transform,
        ..default()
    });
    slot.touch_materials.push(material.clone());
    let Ok(mut entity_commands) = commands.get_entity(entity) else {
        return;
    };
    entity_commands.insert(MediaFace { target: *target });
    // Record the face's current (non-media) material as the restore point: on
    // first application that is the object's own material, and on a re-apply
    // after a face rebuild the rebuilt entity carries a fresh original too.
    if let Ok(current) = mesh_materials.get(entity)
        && current.0 != active.material
    {
        active.restore = current.0.clone();
    }
    active.face_entity = entity;
    active.material = material.clone();
    entity_commands.insert(MeshMaterial3d(material));
}

/// Marks a face entity currently showing a live media surface, carrying its
/// target for the pick paths.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct MediaFace {
    /// The media face this entity renders.
    pub(crate) target: MediaTarget,
}

/// Close `target`'s surface and restore the face's original material.
fn close_media_surface(
    target: MediaTarget,
    state: &mut MediaPrimState,
    surfaces: &mut MediaSurfaces,
    commands: &mut Commands,
) {
    let Some(active) = state.active.remove(&target) else {
        return;
    };
    surfaces.close(active.surface);
    if let Ok(mut entity_commands) = commands.get_entity(active.face_entity) {
        entity_commands.remove::<MediaFace>();
        if active.restore != Handle::default() {
            entity_commands.insert(MeshMaterial3d(active.restore.clone()));
        }
    }
}

/// The pick inputs of the hover ray, bundled to stay within Bevy's
/// system-parameter arity.
#[derive(bevy::ecs::system::SystemParam)]
struct HoverPick<'w, 's> {
    /// The primary window (for the cursor position).
    windows: Query<'w, 's, &'static Window>,
    /// The world camera the ray is cast from.
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<ViewerCamera>>,
    /// The UI hover map (occlusion).
    hover_map: Res<'w, HoverMap>,
    /// UI pickables (occlusion).
    pickables: Query<'w, 's, &'static Pickable>,
    /// UI node sizes (occlusion).
    node_sizes: Query<'w, 's, &'static ComputedNode>,
    /// World transforms, for the object-local surface info.
    globals: Query<'w, 's, &'static GlobalTransform>,
    /// The parent chain up to the object root.
    parents: Query<'w, 's, &'static ChildOf>,
    /// The object roots.
    scene: Query<'w, 's, &'static SceneObject>,
}

/// Hover: cast the cursor ray each frame (outside mouselook, world/media
/// context), find the media face under it, update [`MediaFocus`]'s hover
/// state, and forward pointer motion to its page when interaction is allowed.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the pick \
              plumbing plus the media state the hover updates"
)]
fn hover_media_faces(
    pick: HoverPick,
    mut ray_cast: MeshRayCast,
    media_faces: Query<&MediaFace>,
    faces: Query<(&PrimFaceEntity, &FaceTextureDebug)>,
    mut focus: ResMut<MediaFocus>,
    state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    data: Res<MediaData>,
    objects: Res<ObjectState>,
) {
    let previous = focus.hover;
    focus.hover = None;
    focus.hover_pixel = None;
    // hover_normal is deliberately kept: the controls bar's zoom wants the
    // last face normal even while the cursor is over the bar itself.

    'pick: {
        if pointer_over_blocking_ui(&pick.hover_map, &pick.pickables, &pick.node_sizes) {
            break 'pick;
        }
        let Ok(window) = pick.windows.single() else {
            break 'pick;
        };
        let Some(cursor) = window.cursor_position() else {
            break 'pick;
        };
        let Ok((camera, camera_transform)) = pick.cameras.single() else {
            break 'pick;
        };
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
            break 'pick;
        };
        let settings = MeshRayCastSettings::default();
        let Some((entity, hit)) = ray_cast.cast_ray(ray, &settings).first().cloned() else {
            break 'pick;
        };
        let Ok(media_face) = media_faces.get(entity) else {
            break 'pick;
        };
        let target = media_face.target;
        // Object-local surface info for the sampled UV (wraps the repeats).
        let mut object_entity = entity;
        while pick.scene.get(object_entity).is_err() {
            let Ok(child_of) = pick.parents.get(object_entity) else {
                break;
            };
            object_entity = child_of.parent();
        }
        let Ok(object_global) = pick.globals.get(object_entity) else {
            break 'pick;
        };
        let face = faces.get(entity).ok();
        let info = surface_info_from_hit(
            &hit,
            face.map(|(marker, _tf)| marker.face_id),
            face.map(|(_marker, FaceTextureDebug(tf))| tf),
            object_global,
        );
        focus.hover = Some(target);
        focus.hover_normal = Some(hit.normal);
        let Some(active) = state.active.get(&target) else {
            break 'pick;
        };
        let Some(slot) = surfaces.get(active.surface) else {
            break 'pick;
        };
        let pixel = media_pixel_from_uv(Vec2::new(info.uv[0], info.uv[1]), slot.size);
        focus.hover_pixel = Some(pixel);
        // Forward motion when the face is focused, or first-click-interact
        // (with permission) would let a click through anyway.
        let is_owner = objects
            .update_flags_by_key(target.object)
            .is_some_and(|flags| flags & FLAGS_OBJECT_YOU_OWNER != 0);
        let may_interact = data
            .entry(target)
            .is_some_and(|entry| media_permission_allows(entry.perms_interact, is_owner));
        if may_interact
            && (focus.focused == Some(target)
                || data
                    .entry(target)
                    .is_some_and(|entry| entry.first_click_interact))
        {
            slot.surface
                .mouse_move(pixel.0, pixel.1, current_modifiers(&keyboard, &mouse));
        }
    }

    if previous.is_some()
        && focus.hover != previous
        && let Some(target) = previous
        && let Some(active) = state.active.get(&target)
        && let Some(slot) = surfaces.get(active.surface)
    {
        slot.surface.mouse_leave();
    }
}

/// Handle a claimed media click: focus the face, start its surface when
/// needed, and forward the press when interaction is allowed.
#[expect(
    clippy::too_many_arguments,
    reason = "threaded resources: the click stream, media data / state, the engine tables, \
              object / face lookups and the asset stores for surface start"
)]
fn handle_media_clicks(
    mut clicks: MessageReader<MediaWorldClick>,
    mut data: ResMut<MediaData>,
    mut state: ResMut<MediaPrimState>,
    mut focus: ResMut<MediaFocus>,
    mut engine: NonSendMut<MediaEngine>,
    mut surfaces: NonSendMut<MediaSurfaces>,
    objects: Res<ObjectState>,
    faces: Query<(&PrimFaceEntity, &FaceTextureDebug)>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    for click in clicks.read() {
        let Some(object) = objects.full_key(&click.scoped) else {
            continue;
        };
        let target = MediaTarget {
            object,
            face: click.face,
        };
        let Some(entry) = data.entry(target).cloned() else {
            // Media flag set but no data yet: fetch it, ignore the click.
            if let std::collections::hash_map::Entry::Vacant(vacant) = data.requested.entry(object)
            {
                vacant.insert(String::new());
                sl_commands.write(SlCommand(Command::RequestObjectMedia { object_id: object }));
            }
            continue;
        };
        let is_owner = objects
            .update_flags_by_key(object)
            .is_some_and(|flags| flags & FLAGS_OBJECT_YOU_OWNER != 0);
        if !media_permission_allows(entry.perms_interact, is_owner) {
            continue;
        }
        let was_focused = focus.focused == Some(target);
        focus.focused = Some(target);
        focus.focused_takes_keyboard = false;
        if !state.active.contains_key(&target) {
            // First interaction starts the media (the reference's click-to-play).
            let started = start_media_surface(
                target,
                &entry,
                click.entity,
                true,
                &mut state,
                &mut engine,
                &mut surfaces,
                &mut images,
                &mut materials,
                &faces,
                &mesh_materials,
                &mut commands,
            );
            if !started {
                continue;
            }
        } else if let Some(active) = state.active.get_mut(&target) {
            active.user_started = true;
        }
        focus.focused_takes_keyboard = state
            .active
            .get(&target)
            .is_some_and(|active| active.kind == MediaEngineKind::Web);
        // Forward the press when already focused, or on the first click with
        // `first_click_interact`.
        if (was_focused || entry.first_click_interact)
            && let Some(active) = state.active.get(&target)
            && let Some(slot) = surfaces.get(active.surface)
        {
            let pixel = media_pixel_from_uv(click.uv, slot.size);
            slot.surface.set_focus(true);
            slot.surface.mouse_button(
                pixel.0,
                pixel.1,
                sl_cef::MouseButton::Left,
                true,
                1,
                current_modifiers(&keyboard, &mouse),
            );
            focus.pressed = Some(target);
        }
    }
}

/// Forward the release of a media press to the surface it went down on (the
/// reference's mouse-capture semantics: the up goes to the pressed media even
/// if the cursor left the face).
fn forward_media_release(
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<MediaFocus>,
    state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
) {
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some(target) = focus.pressed.take() else {
        return;
    };
    let Some(active) = state.active.get(&target) else {
        return;
    };
    let Some(slot) = surfaces.get(active.surface) else {
        return;
    };
    let pixel = focus.hover_pixel.unwrap_or((0, 0));
    slot.surface.mouse_button(
        pixel.0,
        pixel.1,
        sl_cef::MouseButton::Left,
        false,
        1,
        current_modifiers(&keyboard, &mouse),
    );
}

/// Route keyboard input to the focused media face while the media context
/// holds (the world's movement keys are already suppressed by
/// [`crate::input_context`]).
fn route_media_keyboard(
    context: Res<InputContext>,
    focus: Res<MediaFocus>,
    state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
    mut keys: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    if *context != InputContext::Media {
        keys.clear();
        return;
    }
    let Some(target) = focus.focused else {
        return;
    };
    let Some(active) = state.active.get(&target) else {
        return;
    };
    let Some(slot) = surfaces.get(active.surface) else {
        return;
    };
    let modifiers = current_modifiers(&keyboard, &mouse);
    for input in keys.read() {
        if matches!(input.key_code, KeyCode::Escape) {
            continue;
        }
        let down = input.state.is_pressed();
        if let Some(vk) = vk_for_key_code(input.key_code) {
            slot.surface.key(KeyInput {
                down,
                vk,
                modifiers,
            });
        }
        if down
            && let Some(text) = &input.text
            && is_printable_text(text)
        {
            slot.surface.insert_text(text);
        }
    }
}

/// `Escape` releases media focus (the reference also resets its camera zoom;
/// [`crate::media_controls`] owns the zoom state and watches the same edge).
fn release_media_focus_on_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    mut focus: ResMut<MediaFocus>,
    state: Res<MediaPrimState>,
    surfaces: NonSend<MediaSurfaces>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }
    // Web focus owns the keyboard (Media context); a focused video face
    // leaves the keyboard with the world, so accept its Escape there too.
    let releasable = *context == InputContext::Media
        || (*context == InputContext::World && focus.focused.is_some());
    if !releasable {
        return;
    }
    if let Some(target) = focus.focused.take()
        && let Some(active) = state.active.get(&target)
        && let Some(slot) = surfaces.get(active.surface)
    {
        slot.surface.set_focus(false);
    }
    focus.focused_takes_keyboard = false;
    focus.pressed = None;
}

/// Enforce each entry's navigation white-list on the live surfaces: a page
/// that navigated somewhere the white-list rejects is bounced back to the
/// last accepted URL (then the home URL); a bounce loop closes the surface —
/// the reference's `mediaNavigateBounceBack`.
fn enforce_media_whitelists(
    data: Res<MediaData>,
    mut state: ResMut<MediaPrimState>,
    mut surfaces: NonSendMut<MediaSurfaces>,
    mut commands: Commands,
) {
    let mut to_close: Vec<MediaTarget> = Vec::new();
    for (target, active) in &mut state.active {
        let Some(entry) = data.entry(*target) else {
            continue;
        };
        let Some(slot) = surfaces.get(active.surface) else {
            continue;
        };
        let current = &slot.status.url;
        if current.is_empty() || slot.status.loading {
            continue;
        }
        let Ok(parsed) = url::Url::parse(current) else {
            continue;
        };
        if entry.check_candidate_url(&parsed) {
            active.bounces = 0;
            active.last_good_url = Some(current.clone());
            continue;
        }
        active.bounces = active.bounces.saturating_add(1);
        if active.bounces > 2 {
            to_close.push(*target);
            continue;
        }
        let back_to = active
            .last_good_url
            .clone()
            .or_else(|| entry.home_url.as_ref().map(url::Url::to_string));
        if let Some(back_to) = back_to {
            warn!("media white-list bounced {current} back to {back_to}");
            slot.surface.navigate(&back_to);
        } else {
            to_close.push(*target);
        }
    }
    for target in to_close {
        close_media_surface(target, &mut state, &mut surfaces, &mut commands);
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{UVec2, Vec2};
    use pretty_assertions::assert_eq;

    use super::{media_permission_allows, media_pixel_from_uv};

    #[test]
    fn permissions_gate_anyone_and_owner() {
        // MEDIA_PERM: none=0, owner=1, group=2, anyone=4.
        assert!(media_permission_allows(4, false));
        assert!(media_permission_allows(7, false));
        assert!(!media_permission_allows(0, true));
        assert!(media_permission_allows(1, true));
        assert!(!media_permission_allows(1, false));
        // Group-only is conservatively denied (membership unknown here).
        assert!(!media_permission_allows(2, false));
    }

    #[test]
    fn pixel_mapping_flips_v_and_wraps_repeats() {
        let size = UVec2::new(1000, 500);
        // Top-left of the media is SL uv (0, 1).
        assert_eq!(media_pixel_from_uv(Vec2::new(0.0, 1.0), size), (0, 500));
        // uv v=1 wraps to 0 → y = (1-0)*500? No: fract(1.0) = 0 → y = 500.
        // Centre.
        assert_eq!(media_pixel_from_uv(Vec2::new(0.5, 0.5), size), (500, 250));
        // A repeat outside [0,1] wraps: uv.x = 1.25 samples x = 0.25.
        assert_eq!(media_pixel_from_uv(Vec2::new(1.25, 0.5), size), (250, 250));
        // Negative wraps upward: uv.x = -0.25 samples x = 0.75.
        assert_eq!(media_pixel_from_uv(Vec2::new(-0.25, 0.5), size), (750, 250));
    }
}
