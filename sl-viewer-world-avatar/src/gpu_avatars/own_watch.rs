//! A frame-by-frame watch on whether the **own** avatar is drawn in the main
//! view (`SL_VIEWER_LOG_OWN_AVATAR_VISIBILITY=1`), for the roadmap bug
//! `viewer-own-avatar-vanishes-near-ground`: the own avatar disappearing for a
//! moment near the ground while flying or falling.
//!
//! A vanish lasting a few frames is invisible to the once-per-second
//! `SL_VIEWER_LOG_AVATAR_BOUNDS` census, and each suspect leaves a different
//! trace in one frame's state, so the watch records that state every frame
//! and logs in full only around an anomaly: the frames leading up to it, every
//! frame it lasts, and the one it ends on. Between anomalies it logs a line a
//! second and each change of the playing animation set, so a vanish seen live
//! can be placed on the timeline even when no trigger fired.
//!
//! `ViewVisibility` is not enough to judge a cull by: it is the union over
//! every view, and the reflection-probe, mirror and minimap cameras render the
//! avatar too. So the watch tests each part's `Aabb` against the main camera's
//! frustum itself. The anomalies:
//!
//! - **culled** — the body root is in front of the camera, yet a visible part's
//!   `Aabb` misses the frustum (a wrong bound: compare the placed read-back box
//!   with the root);
//! - **camera inside** — the camera is within arm's reach of the body root, so
//!   the body is seen from inside and back-face culled;
//! - **jumped** — the body root moved faster over one frame than any flight or
//!   fall does;
//! - **respawned** — the anchor entity or the part count changed.
//!
//! A non-finite pose shows as `nonfinite` corrections, a non-finite feed root or
//! a non-finite read-back box on any line. A vanish seen live with none of
//! these points past culling and placement, at the draw itself.

use std::collections::VecDeque;

use bevy::camera::primitives::{Aabb, Frustum, Sphere};
use bevy::prelude::*;
use sl_client_bevy::{AgentKey, SlIdentity};

use super::render::{GpuAvatarBounds, bounds_at};
use super::stage::{GpuAvatarPoseFeed, GpuAvatarRegistry, GpuSkinBinding};
use crate::animations::{AnimationManager, AnimationPlayback};
use sl_viewer_world_api::{AvatarState, PoseSlotKey, ViewerCamera};

/// The env flag turning the watch on.
const ENV_LOG_OWN_VISIBILITY: &str = "SL_VIEWER_LOG_OWN_AVATAR_VISIBILITY";

/// How many frames before an anomaly are logged with it.
const LEAD_FRAMES: usize = 12;

/// The most frames of one anomaly logged line by line; a longer one is summed
/// up when it ends.
const MAX_EPISODE_LINES: u32 = 240;

/// The radius (metres) of the sphere around the body root that stands for "the
/// avatar is in front of the camera" when judging a cull.
const ROOT_SPHERE_RADIUS_METRES: f32 = 1.0;

/// The camera-to-root distance (metres) under which the camera is taken to be
/// inside the body.
const CAMERA_INSIDE_METRES: f32 = 0.6;

/// The root speed (metres per second) over one frame that counts as a jump —
/// twice the ~46 m/s an avatar falling from 135 m reaches on aditi, so a fall
/// at a low frame rate is not flagged.
const ROOT_JUMP_METRES_PER_SEC: f32 = 100.0;

/// Seconds between the timeline lines logged while nothing is wrong.
const TIMELINE_INTERVAL_SECS: f32 = 1.0;

/// The watch's state between frames.
#[derive(Default)]
pub(crate) struct OwnWatch {
    /// Whether the env flag is set, read once.
    enabled: Option<bool>,
    /// The frame counter.
    frame: u64,
    /// The last [`LEAD_FRAMES`] frames' lines.
    lead: VecDeque<String>,
    /// The anchor entity last frame.
    anchor: Option<Entity>,
    /// The body root's position last frame.
    root: Option<Vec3>,
    /// The part count last frame.
    parts: u32,
    /// The playing animation set last frame.
    animations: String,
    /// When the next timeline line is due.
    next_timeline: f32,
    /// The running anomaly, if any: what it is, its first frame, its start
    /// time and how many lines it has logged.
    episode: Option<Episode>,
}

/// One running anomaly.
struct Episode {
    /// What was wrong on its first frame.
    what: String,
    /// Its first frame.
    first: u64,
    /// Its start time.
    start: f32,
    /// How many of its frames were logged.
    logged: u32,
}

/// The skinned submeshes the watch reads.
type PartQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static GpuSkinBinding,
        &'static GlobalTransform,
        Option<&'static Aabb>,
        &'static InheritedVisibility,
        &'static ViewVisibility,
    ),
>;

/// The own avatar's parts, counted for one frame.
#[derive(Default)]
struct PartCensus {
    /// Skinned submeshes posed by the own slot.
    total: u32,
    /// Of those, inherited-visible (not hidden).
    shown: u32,
    /// Of those, view-visible in some view.
    view_visible: u32,
    /// Of the shown ones, those whose `Aabb` meets the main camera's frustum.
    in_main_view: u32,
    /// Of the shown ones, those with no `Aabb` yet.
    unbounded: u32,
}

/// Record the own avatar's draw state for this frame, and log it around an
/// anomaly — see the module docs. Runs after `CheckVisibility`. Inert unless
/// the env flag is set.
#[expect(
    clippy::too_many_arguments,
    reason = "a diagnostic joining the avatar, its pose feed, its read-back bound, its \
              parts, the camera and the playing animations"
)]
pub(crate) fn watch_own_avatar_visibility(
    time: Res<Time>,
    identity: Res<SlIdentity>,
    state: Res<AvatarState>,
    registry: Res<GpuAvatarRegistry>,
    bounds: Res<GpuAvatarBounds>,
    feed: Res<GpuAvatarPoseFeed>,
    (playback, manager): (Res<AnimationPlayback>, Res<AnimationManager>),
    parts: PartQuery<'_, '_>,
    globals: Query<&GlobalTransform>,
    camera: Query<(&GlobalTransform, &Frustum), With<ViewerCamera>>,
    mut watch: Local<OwnWatch>,
) {
    let on = *watch
        .enabled
        .get_or_insert_with(|| std::env::var(ENV_LOG_OWN_VISIBILITY).as_deref() == Ok("1"));
    if !on {
        return;
    }
    let Some(own) = identity.agent_id else {
        return;
    };
    // A plain reborrow, so the episode and the counters borrow as disjoint
    // fields.
    let watch: &mut OwnWatch = &mut watch;
    watch.frame = watch.frame.saturating_add(1);
    let now = time.elapsed_secs();
    let slot = PoseSlotKey::Avatar(own);
    let anchor = state.body_root_of(own);
    let root = anchor
        .and_then(|entity| globals.get(entity).ok())
        .map(GlobalTransform::translation);
    let view = camera.single().ok();
    let census = count_parts(&parts, slot, view.map(|(_camera, frustum)| frustum));
    let root_in_view = root.zip(view).is_some_and(|(root, (_camera, frustum))| {
        frustum.intersects_sphere(
            &Sphere {
                center: root.into(),
                radius: ROOT_SPHERE_RADIUS_METRES,
            },
            false,
        )
    });
    let camera_distance = root
        .zip(view)
        .map(|(root, (camera, _frustum))| root.distance(camera.translation()));
    let travel = root
        .zip(watch.root)
        .map(|(root, previous)| root.distance(previous));
    let animations = describe_animations(&playback, &manager, own, now);

    let line = format!(
        "frame={} t={now:.3} dt_ms={:.1} anchor={anchor:?} root={} travel={} parts={} shown={} \
         view_visible={} in_main_view={} unbounded={} root_in_view={root_in_view} \
         camera_distance={} bound={} feed={} anims=[{animations}]",
        watch.frame,
        time.delta_secs() * 1000.0,
        root.map_or_else(|| "none".to_owned(), format_vec),
        travel.map_or_else(|| "none".to_owned(), |travel| format!("{travel:.2}")),
        census.total,
        census.shown,
        census.view_visible,
        census.in_main_view,
        census.unbounded,
        camera_distance.map_or_else(|| "none".to_owned(), |d| format!("{d:.2}")),
        describe_bound(&registry, &bounds, &feed, slot, root),
        describe_feed(&feed, slot, root),
    );

    // What, if anything, is wrong this frame.
    let mut wrong: Vec<&str> = Vec::new();
    let culled = census
        .shown
        .saturating_sub(census.in_main_view)
        .saturating_sub(census.unbounded);
    if root_in_view && culled > 0 {
        wrong.push("culled");
    }
    if camera_distance.is_some_and(|distance| distance < CAMERA_INSIDE_METRES) {
        wrong.push("camera-inside");
    }
    if travel.is_some_and(|travel| travel > ROOT_JUMP_METRES_PER_SEC * time.delta_secs()) {
        wrong.push("jumped");
    }
    if watch.anchor.is_some() && watch.anchor != anchor {
        wrong.push("anchor-changed");
    }
    if watch.frame > 1 && watch.parts != census.total {
        wrong.push("part-count-changed");
    }
    let what = wrong.join("+");

    if animations != watch.animations {
        info!(
            "own avatar watch: animations -> [{animations}] at frame={}",
            watch.frame
        );
    }
    match watch.episode.as_mut() {
        None if !wrong.is_empty() => {
            info!(
                "own avatar watch: ANOMALY {what} — the {} frames before it:",
                watch.lead.len()
            );
            for lead in &watch.lead {
                info!("own avatar watch:   {lead}");
            }
            info!("own avatar watch: > {what} {line}");
            watch.episode = Some(Episode {
                what,
                first: watch.frame,
                start: now,
                logged: 1,
            });
        }
        Some(episode) if wrong.is_empty() => {
            info!(
                "own avatar watch: CLEARED {} after {} frame(s), {:.3} s ({} logged): {line}",
                episode.what,
                watch.frame.saturating_sub(episode.first),
                now - episode.start,
                episode.logged,
            );
            watch.episode = None;
        }
        Some(episode) => {
            if episode.logged < MAX_EPISODE_LINES {
                info!("own avatar watch: > {what} {line}");
                episode.logged = episode.logged.saturating_add(1);
            }
        }
        None => {
            if now >= watch.next_timeline {
                info!("own avatar watch: timeline {line}");
            }
        }
    }
    if now >= watch.next_timeline {
        watch.next_timeline = now + TIMELINE_INTERVAL_SECS;
    }
    watch.lead.push_back(line);
    while watch.lead.len() > LEAD_FRAMES {
        let _oldest = watch.lead.pop_front();
    }
    watch.anchor = anchor;
    watch.root = root;
    watch.parts = census.total;
    watch.animations = animations;
}

/// Count the own avatar's skinned submeshes, testing the shown ones against
/// the main camera's `frustum` the way the cull does.
fn count_parts(
    parts: &PartQuery<'_, '_>,
    slot: PoseSlotKey,
    frustum: Option<&Frustum>,
) -> PartCensus {
    let mut census = PartCensus::default();
    for (binding, global, aabb, inherited, view_visibility) in parts {
        if binding.slot != slot {
            continue;
        }
        census.total = census.total.saturating_add(1);
        if view_visibility.get() {
            census.view_visible = census.view_visible.saturating_add(1);
        }
        if !inherited.get() {
            continue;
        }
        census.shown = census.shown.saturating_add(1);
        match (aabb, frustum) {
            (None, _) => census.unbounded = census.unbounded.saturating_add(1),
            (Some(aabb), Some(frustum)) => {
                if frustum.intersects_obb(aabb, &global.affine(), true, false) {
                    census.in_main_view = census.in_main_view.saturating_add(1);
                }
            }
            (Some(_aabb), None) => {}
        }
    }
    census
}

/// A vector with centimetre precision.
fn format_vec(value: Vec3) -> String {
    format!("({:.2},{:.2},{:.2})", value.x, value.y, value.z)
}

/// The own slot's read-back bound: absent, non-finite, or its box placed on
/// the root published this frame (as the cull places it) and how far the body
/// root lies outside that (0 inside).
fn describe_bound(
    registry: &GpuAvatarRegistry,
    bounds: &GpuAvatarBounds,
    feed: &GpuAvatarPoseFeed,
    slot: PoseSlotKey,
    root: Option<Vec3>,
) -> String {
    let Some(index) = registry.slot_index(slot) else {
        return "no-slot".to_owned();
    };
    let Some((min, max)) = bounds_at(&bounds.bytes, index) else {
        return format!("slot{index}:none");
    };
    if !(min.is_finite() && max.is_finite()) {
        return format!("slot{index}:NONFINITE min={min} max={max}");
    }
    let Some(origin) = feed.root_translation(slot) else {
        return format!("slot{index}:unplaced");
    };
    let min = Vec3::new(min.x + origin.x, min.y + origin.y, min.z + origin.z);
    let max = Vec3::new(max.x + origin.x, max.y + origin.y, max.z + origin.z);
    let outside = root.map_or_else(
        || "?".to_owned(),
        |root| format!("{:.2}", root.distance(root.clamp(min, max))),
    );
    format!(
        "slot{index}:min={} max={} root_outside={outside}",
        format_vec(min),
        format_vec(max)
    )
}

/// The own slot's published pose-feed entry: how far its root lies from the
/// body root, how many corrections it carries and how many are non-finite.
fn describe_feed(feed: &GpuAvatarPoseFeed, slot: PoseSlotKey, root: Option<Vec3>) -> String {
    let Some((feed_root, corrections)) = feed.template_entry(slot) else {
        return "none".to_owned();
    };
    let nonfinite = corrections
        .iter()
        .filter(|(_joint, pose)| !(pose.rot.is_finite() && pose.pos.is_finite()))
        .count();
    let root_offset = if feed_root.is_finite() {
        root.map_or_else(
            || "?".to_owned(),
            |root| format!("{:.2}", feed_root.w_axis.truncate().distance(root)),
        )
    } else {
        "NONFINITE".to_owned()
    };
    format!(
        "root_offset={root_offset} corrections={} nonfinite={nonfinite}",
        corrections.len()
    )
}

/// The own avatar's playing animations, by built-in name where it has one, a
/// `*` marking one easing out.
fn describe_animations(
    playback: &AnimationPlayback,
    manager: &AnimationManager,
    own: AgentKey,
    now: f32,
) -> String {
    playback
        .playing_animations(own, now, manager)
        .iter()
        .map(|anim| {
            let name = sl_anim::builtin_animation(anim.id)
                .map_or_else(|| anim.id.to_string(), |builtin| builtin.name.to_owned());
            if anim.stopping {
                format!("{name}*")
            } else {
                name
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
