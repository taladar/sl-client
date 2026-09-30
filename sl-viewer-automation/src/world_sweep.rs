//! [`WorldSweep`]: the build tool's rubber band, drawn so that it selects
//! exactly the things a locator names.
//!
//! The band is the smallest rectangle round the things' projected boxes,
//! padded a little. It must start on empty world — a press on an object would
//! select that object instead, and one on a transform handle would drag it —
//! so each corner is put to the selection gesture's own press classification
//! ([`SelectionProbes`]) and the first empty one starts the band. Then the
//! gesture's own rectangle test says what that band would select, and the
//! sweep is ready only if that is exactly the things named: a band that
//! misses one or catches another fails, naming both.

use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use sl_automation_proto::{ActionabilityCheck, AutomationError, Deadline, WorldLocator, WorldNode};
use sl_client_bevy::Uuid;
use sl_viewer_ui_core::synthetic_input::InputAction;
use sl_viewer_world_api::{
    EditToolState, PressOutcome, ProbeTicket, SelectionAnswer, SelectionProbes, SelectionQuery,
};

use crate::pursuit::{DEFAULT_DEADLINE, DEFAULT_DEADLINE_FRAMES, PursuitError};
use crate::world_aim::{
    AimStage, CameraStill, object_full_id, reach_the_world, screen_projection, view,
};
use crate::world_query::{WorldProgress, WorldQuery, WorldWant};

/// The padding round the things' projected boxes, logical pixels: clear of
/// their edges, so the band's corners are not on them.
const MARGIN: f32 = 6.0;

/// The frames the band is drawn over.
const SWEEP_STEPS: u32 = 8;

/// The band, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepTarget {
    /// The things it selects.
    pub nodes: Vec<WorldNode>,
    /// Where the drag starts: a corner on empty world.
    pub from: Vec2,
    /// Where it ends: the opposite corner.
    pub to: Vec2,
}

impl SweepTarget {
    /// The drag, for the synthetic input to play.
    #[must_use]
    pub fn input(&self) -> InputAction {
        InputAction::drag(self.from, self.to, SWEEP_STEPS, MouseButton::Left)
    }
}

/// Where a [`WorldSweep`] stands after a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum SweepProgress {
    /// The band.
    Ready(SweepTarget),
    /// Not yet: poll again next frame.
    Waiting(AimStage),
}

/// Where the sweep stands, with its state.
#[derive(Debug)]
enum Stage {
    /// Resolving the things.
    Resolve,
    /// Waiting for build mode and a still camera.
    Settle,
    /// Asking which corner is empty world: each corner and its opposite.
    Corners(VecDeque<(ProbeTicket, Vec2, Vec2)>, Option<Uuid>),
    /// Asking what the band from the first corner to the second selects.
    Check(ProbeTicket, Vec2, Vec2),
}

/// A rubber band over the things a locator names, polled once a frame.
#[derive(Debug)]
pub struct WorldSweep {
    /// What is to be selected.
    locator: WorldLocator,
    /// Its resolution: every match.
    query: WorldQuery,
    /// The most frames to wait.
    max_frames: u32,
    /// The most wall-clock time to wait.
    max_time: Duration,
    /// When the first poll ran.
    started: Option<Instant>,
    /// The polls so far.
    frames: u32,
    /// The things, once resolved.
    nodes: Vec<WorldNode>,
    /// Where it stands.
    stage: Stage,
    /// The camera's stillness.
    still: CameraStill,
}

impl WorldSweep {
    /// Sweep the things `locator` names, under the default deadline.
    #[must_use]
    pub fn new(locator: WorldLocator) -> Self {
        Self {
            query: WorldQuery::new(locator.clone(), WorldWant::All),
            locator,
            max_frames: DEFAULT_DEADLINE_FRAMES,
            max_time: DEFAULT_DEADLINE,
            started: None,
            frames: 0,
            nodes: Vec::new(),
            stage: Stage::Resolve,
            still: CameraStill::default(),
        }
    }

    /// Give up after `deadline`; a limit it leaves unset keeps the default.
    #[must_use]
    pub fn with_deadline(mut self, deadline: Deadline) -> Self {
        if let Some(frames) = deadline.frames {
            self.max_frames = frames;
        }
        if let Some(millis) = deadline.millis {
            self.max_time = Duration::from_millis(millis);
        }
        self.query = WorldQuery::new(self.locator.clone(), WorldWant::All).with_deadline(deadline);
        self
    }

    /// Move the sweep along by a frame.
    ///
    /// # Errors
    ///
    /// [`AutomationError::WorldNotActionable`] when one of the things is off
    /// screen (`in_viewport`) or no corner of the band is empty world
    /// (`receives_events`, naming what the first corner is on);
    /// [`AutomationError::SweepInexact`] when the band would not select exactly
    /// the things; [`AutomationError::WorldTimedOut`] with the failing check
    /// when the deadline passes; and the resolution's and the UI model's
    /// errors.
    pub fn poll(&mut self, world: &mut World) -> Result<SweepProgress, PursuitError> {
        let started = *self.started.get_or_insert_with(Instant::now);
        self.frames = self.frames.saturating_add(1);
        let progress = match self.step(world) {
            Ok(progress) => progress,
            Err(error) => {
                self.abandon(world);
                return Err(error);
            }
        };
        let SweepProgress::Waiting(stage) = &progress else {
            return Ok(progress);
        };
        let waited = started.elapsed();
        if self.frames >= self.max_frames || waited >= self.max_time {
            self.abandon(world);
            return Err(AutomationError::WorldTimedOut {
                locator: self.locator.clone(),
                failed_check: Some(stage.check()),
                unresolved: Vec::new(),
                last_observed: self.nodes.clone(),
                frames: self.frames,
                millis: u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
            }
            .into());
        }
        Ok(progress)
    }

    /// One poll's work, deadline aside.
    fn step(&mut self, world: &mut World) -> Result<SweepProgress, PursuitError> {
        match core::mem::replace(&mut self.stage, Stage::Resolve) {
            Stage::Resolve => {
                match self.query.poll(world)? {
                    WorldProgress::Ready(nodes) if !nodes.is_empty() => {
                        self.nodes = nodes;
                        self.still.reset();
                        self.stage = Stage::Settle;
                        return Ok(SweepProgress::Waiting(AimStage::Settling));
                    }
                    // Nothing yet: look again next frame.
                    WorldProgress::Ready(_nothing) => {
                        self.query = WorldQuery::new(self.locator.clone(), WorldWant::All);
                    }
                    WorldProgress::Waiting { .. } => {}
                }
                Ok(SweepProgress::Waiting(AimStage::Resolving))
            }
            Stage::Settle => self.settle(world),
            Stage::Corners(pending, covered_by) => self.corners(world, pending, covered_by),
            Stage::Check(ticket, from, to) => self.check(world, ticket, from, to),
        }
    }

    /// Wait for build mode and a still camera, then ask about the corners.
    fn settle(&mut self, world: &mut World) -> Result<SweepProgress, PursuitError> {
        self.stage = Stage::Settle;
        if !world
            .get_resource::<EditToolState>()
            .is_some_and(|tool| tool.active && tool.tool.selects_objects())
        {
            self.still.reset();
            return Ok(SweepProgress::Waiting(AimStage::BuildMode));
        }
        if !self.still.observe(world) {
            return Ok(SweepProgress::Waiting(AimStage::Settling));
        }
        let Some(viewport) =
            view(world).and_then(|(camera, _transform)| camera.logical_viewport_size())
        else {
            return Ok(SweepProgress::Waiting(AimStage::Settling));
        };
        let mut band: Option<Rect> = None;
        for node in self.nodes.clone() {
            let Some(projection) = screen_projection(world, &node).filter(|p| p.on_screen) else {
                return Err(self.not_actionable(&node, ActionabilityCheck::InViewport, None));
            };
            band = Some(band.map_or(projection.bounds, |band| band.union(projection.bounds)));
        }
        let Some(band) = band else {
            return Ok(SweepProgress::Waiting(AimStage::Resolving));
        };
        let padded = Rect::from_corners(
            Vec2::new(band.min.x - MARGIN, band.min.y - MARGIN),
            Vec2::new(band.max.x + MARGIN, band.max.y + MARGIN),
        );
        let screen = Rect::from_corners(Vec2::ZERO, Vec2::new(viewport.x - 1.0, viewport.y - 1.0));
        let clipped = padded.intersect(screen);
        let corners = [
            clipped.min,
            Vec2::new(clipped.max.x, clipped.min.y),
            clipped.max,
            Vec2::new(clipped.min.x, clipped.max.y),
        ];
        let open = reach_the_world(world, corners.to_vec())?;
        let Some(mut probes) = world.get_resource_mut::<SelectionProbes>() else {
            // No build tool to ask: the deadline will say so.
            return Ok(SweepProgress::Waiting(AimStage::Probing));
        };
        let pending = corners
            .iter()
            .zip(corners.iter().cycle().skip(2))
            .filter(|(corner, _opposite)| open.contains(corner))
            .map(|(corner, opposite)| {
                (
                    probes.request(SelectionQuery::Press(*corner)),
                    *corner,
                    *opposite,
                )
            })
            .collect();
        self.stage = Stage::Corners(pending, None);
        Ok(SweepProgress::Waiting(AimStage::Probing))
    }

    /// Read the corners in order; the first on empty world starts the band.
    fn corners(
        &mut self,
        world: &mut World,
        mut pending: VecDeque<(ProbeTicket, Vec2, Vec2)>,
        mut covered_by: Option<Uuid>,
    ) -> Result<SweepProgress, PursuitError> {
        while let Some(&(ticket, corner, opposite)) = pending.front() {
            let answer = world
                .get_resource_mut::<SelectionProbes>()
                .and_then(|mut probes| probes.take_answer(ticket));
            let Some(answer) = answer else {
                self.stage = Stage::Corners(pending, covered_by);
                return Ok(SweepProgress::Waiting(AimStage::Probing));
            };
            let _read = pending.pop_front();
            match answer {
                SelectionAnswer::Press(PressOutcome::EmptyWorld) => {
                    abandon_corners(world, &mut pending);
                    let Some(mut probes) = world.get_resource_mut::<SelectionProbes>() else {
                        return Ok(SweepProgress::Waiting(AimStage::Probing));
                    };
                    let ticket = probes.request(SelectionQuery::Sweep {
                        from: corner,
                        to: opposite,
                    });
                    self.stage = Stage::Check(ticket, corner, opposite);
                    return Ok(SweepProgress::Waiting(AimStage::Probing));
                }
                SelectionAnswer::Press(PressOutcome::Object(scoped)) => {
                    if covered_by.is_none() {
                        covered_by = object_full_id(world, &scoped);
                    }
                }
                SelectionAnswer::Press(PressOutcome::Handle) | SelectionAnswer::Sweep(_) => {}
            }
        }
        let first = self.nodes.first().cloned();
        match first {
            Some(node) => {
                Err(self.not_actionable(&node, ActionabilityCheck::ReceivesEvents, covered_by))
            }
            None => Ok(SweepProgress::Waiting(AimStage::Resolving)),
        }
    }

    /// Compare what the band selects with the things; ready when they agree.
    fn check(
        &mut self,
        world: &mut World,
        ticket: ProbeTicket,
        from: Vec2,
        to: Vec2,
    ) -> Result<SweepProgress, PursuitError> {
        let answer = world
            .get_resource_mut::<SelectionProbes>()
            .and_then(|mut probes| probes.take_answer(ticket));
        let Some(answer) = answer else {
            self.stage = Stage::Check(ticket, from, to);
            return Ok(SweepProgress::Waiting(AimStage::Probing));
        };
        let SelectionAnswer::Sweep(swept) = answer else {
            return Ok(SweepProgress::Waiting(AimStage::Probing));
        };
        let got: BTreeSet<Uuid> = swept
            .iter()
            .filter_map(|scoped| object_full_id(world, scoped))
            .collect();
        let wanted: BTreeSet<Uuid> = self.nodes.iter().map(|node| node.full_id).collect();
        if got == wanted {
            return Ok(SweepProgress::Ready(SweepTarget {
                nodes: self.nodes.clone(),
                from,
                to,
            }));
        }
        Err(AutomationError::SweepInexact {
            locator: self.locator.clone(),
            missing: wanted.difference(&got).copied().collect(),
            extra: got.difference(&wanted).copied().collect(),
        }
        .into())
    }

    /// The not-actionable error for `node`.
    fn not_actionable(
        &self,
        node: &WorldNode,
        check: ActionabilityCheck,
        covered_by: Option<Uuid>,
    ) -> PursuitError {
        AutomationError::WorldNotActionable {
            locator: self.locator.clone(),
            check,
            node: Box::new(node.clone()),
            covered_by,
        }
        .into()
    }

    /// Give up the questions in flight.
    fn abandon(&mut self, world: &mut World) {
        match &mut self.stage {
            Stage::Corners(pending, _covered) => abandon_corners(world, pending),
            Stage::Check(ticket, _from, _to) => {
                if let Some(mut probes) = world.get_resource_mut::<SelectionProbes>() {
                    probes.abandon(*ticket);
                }
            }
            Stage::Resolve | Stage::Settle => {}
        }
    }
}

/// Give up every corner question still waiting.
fn abandon_corners(world: &mut World, pending: &mut VecDeque<(ProbeTicket, Vec2, Vec2)>) {
    if let Some(mut probes) = world.get_resource_mut::<SelectionProbes>() {
        for (ticket, _corner, _opposite) in pending.drain(..) {
            probes.abandon(ticket);
        }
    }
}
