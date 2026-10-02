//! [`GroundAim`]: a ground action's wait for the point of the ground it acts
//! on — find the region, read the ground's height there, and find where on
//! screen a click lands on *that* ground, framing it with the camera when no
//! click reaches it.
//!
//! The ground has no id to resolve, so its address is a position: a region,
//! by name, and a point in it ([`GroundPoint`]). The aim projects that point
//! (at the height of the terrain the viewer has) and asks the viewer's own
//! pick resolver what a click there would hit ([`PickProbes`]) — the same
//! check [`WorldAim`](crate::WorldAim) makes of an object. Only the ground,
//! near the point, counts: an object or an avatar standing on it, water over
//! it, or a ridge in front of it all mean a click there lands elsewhere.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use sl_automation_proto::{ActionabilityCheck, AutomationError, Deadline, GroundPoint};
use sl_client_bevy::{RegionHandle, SlRegion, SlRegionIdentity, Uuid, Vector};
use sl_viewer_kit::coords::{bevy_to_sl_vec, region_offset_bevy, sl_to_bevy_vec};
use sl_viewer_ui_core::synthetic_input::InputAction;
use sl_viewer_world_api::{
    EditTool, EditToolState, FrameObject, ObjectState, PickProbes, ProbeId, ProbeTarget,
    TerrainState,
};

use crate::pursuit::{DEFAULT_DEADLINE, DEFAULT_DEADLINE_FRAMES, PursuitError};
use crate::world_aim::{
    AimStage, CameraStill, REVEAL_SETTLE_FRAMES, WorldIntent, full_id_of, gesture, reach_the_world,
    view,
};

/// The radius of the sphere a reveal frames around the point, metres: the
/// camera stands close enough that a click near the point is unambiguous.
const FRAME_RADIUS: f32 = 2.0;

/// How far from the point a pick may land and still be on it, metres — at
/// least this, and more where a few pixels cover more ground
/// ([`FOOTPRINT_PIXELS`]).
const NEAR_TOLERANCE: f32 = 0.25;

/// How many pixels' worth of ground around the aim point a pick may land in
/// and still be on the point: the pick reads one pixel, and seen far off and
/// nearly edge-on, one pixel covers metres of ground.
const FOOTPRINT_PIXELS: f32 = 3.0;

/// The width of a region, metres.
const REGION_WIDTH: f32 = 256.0;

/// Whether `ground` names a point a region has: finite, and inside the
/// region's 256 m square.
#[must_use]
pub fn ground_is_in_a_region(ground: &GroundPoint) -> bool {
    let inside = |metres: f32| metres.is_finite() && (0.0..REGION_WIDTH).contains(&metres);
    inside(ground.x) && inside(ground.y)
}

/// The point a [`GroundAim`] found, and where to aim at it.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundTarget {
    /// The ground aimed at.
    pub ground: GroundPoint,
    /// The point a click lands on it, logical pixels.
    pub aim: Vec2,
    /// Where the pick resolver says that click lands, in the named region's
    /// own metres.
    pub hit_point: [f32; 3],
    /// What will be done there.
    intent: WorldIntent,
}

impl GroundTarget {
    /// The gesture the intent is, aimed at this point, for the synthetic
    /// input to play.
    #[must_use]
    pub fn input(&self) -> InputAction {
        gesture(self.intent, self.aim)
    }
}

/// Where a [`GroundAim`] stands after a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum GroundProgress {
    /// The point and a verified aim at it.
    Ready(GroundTarget),
    /// Not yet: poll again next frame.
    Waiting(AimStage),
}

/// The point in the world: its region and where it is in Bevy space.
#[derive(Debug, Clone, Copy)]
struct Located {
    /// The region the point is in.
    region: RegionHandle,
    /// The point, Bevy world space: on the ground.
    point: Vec3,
}

/// The probe in flight.
#[derive(Debug, Clone, Copy)]
struct Probing {
    /// The question.
    id: ProbeId,
    /// Where it was asked, logical pixels.
    aim: Vec2,
    /// The point it is about, Bevy world space.
    point: Vec3,
}

/// A ground action's wait for its point, polled once a frame.
///
/// Each [`poll`](Self::poll) moves it along: the region is found by name and
/// the ground's height read where the point is; the camera must hold still;
/// the point is projected — on screen and not under a UI node that takes the
/// click — and probed; and a probe that finds the ground at the point is the
/// aim. With the point off screen, or the probe landing elsewhere, the camera
/// frames the point once and the aim starts over — unless
/// [`without_reveal`](Self::without_reveal) said not to.
#[derive(Debug)]
pub struct GroundAim {
    /// The ground wanted.
    ground: GroundPoint,
    /// What will be done there.
    intent: WorldIntent,
    /// Whether the camera may frame the point.
    reveal: bool,
    /// The most frames to wait.
    max_frames: u32,
    /// The most wall-clock time to wait.
    max_time: Duration,
    /// When the first poll ran.
    started: Option<Instant>,
    /// The polls so far.
    frames: u32,
    /// Whether the camera holds still.
    still: CameraStill,
    /// The probe in flight.
    probing: Option<Probing>,
    /// Whether the camera has framed the point already.
    revealed: bool,
    /// No pose is judged before this poll.
    settle_from: u32,
}

impl GroundAim {
    /// Aim at `ground` for `intent`, under the default deadline.
    #[must_use]
    pub fn new(ground: GroundPoint, intent: WorldIntent) -> Self {
        Self {
            ground,
            intent,
            reveal: true,
            max_frames: DEFAULT_DEADLINE_FRAMES,
            max_time: DEFAULT_DEADLINE,
            started: None,
            frames: 0,
            still: CameraStill::default(),
            probing: None,
            revealed: false,
            settle_from: 0,
        }
    }

    /// Give up after `deadline`; a limit it leaves unset keeps the default.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Deadline) -> Self {
        if let Some(frames) = deadline.frames {
            self.max_frames = frames;
        }
        if let Some(millis) = deadline.millis {
            self.max_time = Duration::from_millis(millis);
        }
        self
    }

    /// Never move the camera: fail where a reveal would have framed the
    /// point.
    #[must_use]
    pub const fn without_reveal(mut self) -> Self {
        self.reveal = false;
        self
    }

    /// Look at the world once — a frame's worth of the wait. May queue a pick
    /// probe, or ask the camera to frame the point, on the way.
    ///
    /// # Errors
    ///
    /// [`AutomationError::GroundNotActionable`] when no click reaches the
    /// point and no reveal was allowed; [`AutomationError::GroundTimedOut`]
    /// with the failing check when the deadline passes — also while the
    /// region or its ground is not known; and [`PursuitError::Model`] when the
    /// UI model cannot be read.
    pub fn poll(&mut self, world: &mut World) -> Result<GroundProgress, PursuitError> {
        let started = *self.started.get_or_insert_with(Instant::now);
        self.frames = self.frames.saturating_add(1);
        let progress = match self.step(world) {
            Ok(progress) => progress,
            Err(error) => {
                self.abandon(world);
                return Err(error);
            }
        };
        let GroundProgress::Waiting(stage) = &progress else {
            return Ok(progress);
        };
        let waited = started.elapsed();
        if self.frames >= self.max_frames || waited >= self.max_time {
            self.abandon(world);
            return Err(AutomationError::GroundTimedOut {
                ground: self.ground.clone(),
                failed_check: stage.check(),
                frames: self.frames,
                millis: u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
            }
            .into());
        }
        Ok(progress)
    }

    /// One poll's work, deadline aside.
    fn step(&mut self, world: &mut World) -> Result<GroundProgress, PursuitError> {
        let Some(located) = locate(world, &self.ground) else {
            self.abandon(world);
            return Ok(GroundProgress::Waiting(AimStage::Resolving));
        };
        if self.intent == WorldIntent::Place
            && !world
                .get_resource::<EditToolState>()
                .is_some_and(|tool| tool.active && tool.tool == EditTool::Create)
        {
            self.abandon(world);
            return Ok(GroundProgress::Waiting(AimStage::CreateTool));
        }
        let still = self.still.observe(world, &[located.point]);
        if let Some(probing) = self.probing {
            if !still {
                // The camera moved since the point was projected: the
                // question no longer says where the point is.
                self.abandon(world);
                return Ok(GroundProgress::Waiting(AimStage::Settling));
            }
            return self.read(world, located, probing);
        }
        if !still || self.frames < self.settle_from {
            return Ok(GroundProgress::Waiting(AimStage::Settling));
        }
        let Some((camera, transform)) = view(world) else {
            return Ok(GroundProgress::Waiting(AimStage::Settling));
        };
        let Some(viewport) = camera.logical_viewport_size() else {
            return Ok(GroundProgress::Waiting(AimStage::Settling));
        };
        let projected = camera
            .world_to_viewport(&transform, located.point)
            .ok()
            .filter(|at| at.x >= 0.0 && at.y >= 0.0 && at.x < viewport.x && at.y < viewport.y);
        let Some(aim) = projected else {
            return self.fail(world, located, ActionabilityCheck::InViewport, None);
        };
        if reach_the_world(world, vec![aim])?.is_empty() {
            debug!(?aim, "ground aim: a UI node takes a click at the point");
            return self.fail(world, located, ActionabilityCheck::ReceivesEvents, None);
        }
        let Some(mut probes) = world.get_resource_mut::<PickProbes>() else {
            // No resolver in the app: nothing can say where a click lands,
            // and the deadline will report the check as failing.
            return Ok(GroundProgress::Waiting(AimStage::Probing));
        };
        self.probing = Some(Probing {
            id: probes.request(aim),
            aim,
            point: located.point,
        });
        Ok(GroundProgress::Waiting(AimStage::Probing))
    }

    /// Read the probe's answer: the ground at the point is the aim.
    fn read(
        &mut self,
        world: &mut World,
        located: Located,
        probing: Probing,
    ) -> Result<GroundProgress, PursuitError> {
        let answer = world
            .get_resource_mut::<PickProbes>()
            .and_then(|mut probes| probes.take_answer(probing.id));
        let Some(hit) = answer else {
            return Ok(GroundProgress::Waiting(AimStage::Probing));
        };
        self.probing = None;
        let tolerance = footprint(world, probing).max(NEAR_TOLERANCE);
        if hit.is_none() {
            debug!(aim = ?probing.aim, "ground aim: the probe at the point hit nothing");
        }
        if let Some(hit) = hit {
            debug!(
                target = ?hit.target,
                missed_by = hit.world_point.distance(probing.point),
                tolerance,
                "ground aim: the probe at the point found {:?}",
                hit.target
            );
        }
        match hit {
            Some(hit)
                if hit.target == ProbeTarget::Ground
                    && hit.world_point.distance(probing.point) <= tolerance =>
            {
                let origin = world
                    .get_resource::<ObjectState>()
                    .and_then(|objects| objects.origin);
                let offset = region_offset_bevy(located.region, origin);
                let local = bevy_to_sl_vec(Vec3::new(
                    hit.world_point.x - offset.x,
                    hit.world_point.y - offset.y,
                    hit.world_point.z - offset.z,
                ));
                Ok(GroundProgress::Ready(GroundTarget {
                    ground: self.ground.clone(),
                    aim: probing.aim,
                    hit_point: [local.x, local.y, local.z],
                    intent: self.intent,
                }))
            }
            other => {
                let covered_by = other.and_then(|hit| full_id_of(world, &hit.target));
                self.fail(
                    world,
                    located,
                    ActionabilityCheck::ReceivesEvents,
                    covered_by,
                )
            }
        }
    }

    /// No click reaches the point: frame it once and start over, or fail.
    fn fail(
        &mut self,
        world: &mut World,
        located: Located,
        check: ActionabilityCheck,
        covered_by: Option<Uuid>,
    ) -> Result<GroundProgress, PursuitError> {
        if self.reveal && !self.revealed && world.contains_resource::<Messages<FrameObject>>() {
            world.write_message(FrameObject {
                center: located.point,
                radius: FRAME_RADIUS,
                passes_through: Vec::new(),
            });
            self.revealed = true;
            self.settle_from = self.frames.saturating_add(REVEAL_SETTLE_FRAMES);
            self.still.reset();
            return Ok(GroundProgress::Waiting(AimStage::Revealing(check)));
        }
        if self.reveal && self.revealed {
            // The camera eases into a framing: look again until the deadline,
            // as an object's aim does.
            self.still.reset();
            return Ok(GroundProgress::Waiting(AimStage::Revealing(check)));
        }
        Err(AutomationError::GroundNotActionable {
            ground: self.ground.clone(),
            check,
            covered_by,
        }
        .into())
    }

    /// Give up the probe in flight.
    fn abandon(&mut self, world: &mut World) {
        if let Some(probing) = self.probing.take()
            && let Some(mut probes) = world.get_resource_mut::<PickProbes>()
        {
            probes.abandon(probing.id);
        }
    }
}

/// How much ground [`FOOTPRINT_PIXELS`] around the aim point cover, metres:
/// where the rays through the aim point and through a point that many pixels
/// away meet the level of the point. Zero without a camera, or where a ray
/// does not come down to that level.
fn footprint(world: &mut World, probing: Probing) -> f32 {
    let Some((camera, transform)) = view(world) else {
        return 0.0;
    };
    let level = probing.point.y;
    let meet = |at: Vec2| {
        let ray = camera.viewport_to_world(&transform, at).ok()?;
        let fall = ray.direction.y;
        if fall.abs() <= f32::EPSILON {
            return None;
        }
        let along = (level - ray.origin.y) / fall;
        (along > 0.0).then(|| ray.get_point(along))
    };
    let aside = Vec2::new(probing.aim.x, probing.aim.y + FOOTPRINT_PIXELS);
    match (meet(probing.aim), meet(aside)) {
        (Some(here), Some(there)) => here.distance(there),
        _other => 0.0,
    }
}

/// Where `ground` is: the region the viewer knows by that name, and the point
/// on its ground in Bevy space. `None` while no region of that name is known,
/// or its ground there has not arrived.
fn locate(world: &mut World, ground: &GroundPoint) -> Option<Located> {
    let mut regions = world.query::<(&SlRegion, &SlRegionIdentity)>();
    let region = regions
        .iter(world)
        .find(|(_region, identity)| {
            identity
                .0
                .sim_name
                .as_ref()
                .is_some_and(|name| name.as_ref().eq_ignore_ascii_case(&ground.region))
        })
        .map(|(region, _identity)| region.handle)?;
    let height = world
        .get_resource::<TerrainState>()?
        .land_height(region, ground.x, ground.y)?;
    let origin = world
        .get_resource::<ObjectState>()
        .and_then(|objects| objects.origin);
    let offset = region_offset_bevy(region, origin);
    let local = sl_to_bevy_vec(&Vector {
        x: ground.x,
        y: ground.y,
        z: height,
    });
    Some(Located {
        region,
        point: Vec3::new(offset.x + local.x, offset.y + local.y, offset.z + local.z),
    })
}

#[cfg(test)]
mod tests {
    use sl_automation_proto::GroundPoint;

    use super::ground_is_in_a_region;

    /// A point is in a region when it lies in its 256 m square: the far edge
    /// belongs to the next one, and nothing is outside the square.
    #[test]
    fn a_ground_point_lies_in_its_region() {
        assert!(ground_is_in_a_region(&GroundPoint::new("Home", 0.0, 255.9)));
        assert!(!ground_is_in_a_region(&GroundPoint::new(
            "Home", 256.0, 8.0
        )));
        assert!(!ground_is_in_a_region(&GroundPoint::new("Home", 8.0, -0.5)));
        assert!(!ground_is_in_a_region(&GroundPoint::new(
            "Home",
            f32::NAN,
            8.0
        )));
    }
}
