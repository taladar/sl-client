//! How often the interest camera is reported to the simulator, and what the
//! simulator streams back at that cadence.
//!
//! [`report_camera_interest`](crate::session::report_camera_interest) sends the
//! fly-camera's viewpoint as an `AgentUpdate` whenever it has moved enough and
//! the rate limit allows. Which limit and which thresholds is an
//! [`InterestCadence`]: one of three [`CadencePreset`]s, picked at start-up by
//! `SL_VIEWER_INTEREST_CADENCE` (`2`, `45`, `125`, or `cycle`).
//!
//! - `45` (the default) is what the viewer has sent since
//!   `viewer-physical-object-motion-not-smooth`: up to ~45 Hz, after 2 cm or ~2.6°.
//! - `2` is the 0.5 s cadence it sent before that.
//! - `125` is the reference viewer's `send_agent_update`
//!   (`llviewermessage.cpp`): up to `MAX_AGENT_UPDATES_PER_SECOND` (125), after
//!   `TRANSLATE_THRESHOLD` (1 cm) or `CAMERA_AT_THRESHOLD` (a 0.1° change of the
//!   look axis).
//! - `cycle` steps through all three, [`CYCLE_PERIOD_SECS`] each, so one session
//!   compares them on the same scene. Comparing separate logins mixes the
//!   cadence up with whatever the region happened to be doing in each.
//!
//! `SL_VIEWER_LOG_INTEREST_CADENCE` adds a once-a-second `interest_cadence` log
//! line: the active preset, the interest reports sent, the object-update stream
//! by message form, the busiest objects' terse-update rates, and the region's own
//! `AgentUpdatesPerSecond` and frame rate from `SimStats`. That is the
//! measurement `viewer-agentupdate-cadence-effects` asks for. Whether the
//! interest list resolves avatars faster is the existing
//! `SL_VIEWER_LOG_AVATAR_INTEREST` census.

use std::collections::HashMap;

use bevy::prelude::*;
use sl_client_bevy::{
    ObjectStreamBatch, ObjectUpdateForm, RegionHandle, RegionLocalObjectId, SimStatId, SlEvent,
    SlSessionEvent,
};

/// How long `cycle` keeps each preset before moving on to the next.
pub const CYCLE_PERIOD_SECS: f32 = 20.0;

/// How many of the busiest objects the readout names.
const TOP_OBJECTS: usize = 3;

/// The rate limit and change thresholds of one way to report the camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CadencePolicy {
    /// The shortest interval, in seconds, between two reports.
    pub min_period_secs: f32,
    /// How far, in metres, the camera must move before it is reported again.
    pub move_eps_m: f32,
    /// How far the camera's unit look axis must move (the length of the
    /// difference of the two vectors, which is the angle in radians for a small
    /// turn) before it is reported again. The reference compares the look axis
    /// the same way.
    pub look_eps: f32,
}

impl CadencePolicy {
    /// Whether a camera at `eye` looking along `forward` is reported now,
    /// `since_last` seconds after the last report, which was of `last_view`
    /// (`None` before the first one).
    #[must_use]
    pub fn should_report(
        &self,
        since_last: f32,
        last_view: Option<(Vec3, Vec3)>,
        eye: Vec3,
        forward: Vec3,
    ) -> bool {
        if since_last < self.min_period_secs {
            return false;
        }
        last_view.is_none_or(|(last_eye, last_forward)| {
            eye.distance(last_eye) > self.move_eps_m
                || forward.distance(last_forward) > self.look_eps
        })
    }
}

/// A named [`CadencePolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CadencePreset {
    /// The 0.5 s cadence the viewer sent before
    /// `viewer-physical-object-motion-not-smooth`.
    Legacy2Hz,
    /// Up to ~45 Hz, after 2 cm or ~2.6°: the viewer's cadence since then.
    #[default]
    Current45Hz,
    /// The reference viewer's: up to 125 Hz, after 1 cm or 0.1°.
    Reference125Hz,
}

/// The look-axis threshold of the 2 Hz and 45 Hz presets: the vector-difference
/// form of the `1 - dot < 1e-3` test the viewer used, `sqrt(2e-3)` (~2.6°).
const COARSE_LOOK_EPS: f32 = 0.044_721_36;

/// The reference's `CAMERA_AT_THRESHOLD`, `0.2 * F_PI / 360` (a 0.1° turn).
const REFERENCE_LOOK_EPS: f32 = 0.2 * core::f32::consts::PI / 360.0;

impl CadencePreset {
    /// The rate limit and thresholds this preset reports with.
    #[must_use]
    pub const fn policy(self) -> CadencePolicy {
        match self {
            Self::Legacy2Hz => CadencePolicy {
                min_period_secs: 0.5,
                move_eps_m: 0.02,
                look_eps: COARSE_LOOK_EPS,
            },
            Self::Current45Hz => CadencePolicy {
                min_period_secs: 1.0 / 45.0,
                move_eps_m: 0.02,
                look_eps: COARSE_LOOK_EPS,
            },
            Self::Reference125Hz => CadencePolicy {
                min_period_secs: 1.0 / 125.0,
                move_eps_m: 0.01,
                look_eps: REFERENCE_LOOK_EPS,
            },
        }
    }

    /// The preset `cycle` moves to after this one.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Legacy2Hz => Self::Current45Hz,
            Self::Current45Hz => Self::Reference125Hz,
            Self::Reference125Hz => Self::Legacy2Hz,
        }
    }

    /// The short name the log line and `SL_VIEWER_INTEREST_CADENCE` use.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Legacy2Hz => "2",
            Self::Current45Hz => "45",
            Self::Reference125Hz => "125",
        }
    }
}

/// Why an `SL_VIEWER_INTEREST_CADENCE` value was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SL_VIEWER_INTEREST_CADENCE={0:?} is not one of 2, 45, 125 or cycle")]
pub struct CadenceParseError(String);

/// The interest camera's reporting cadence, and the count of reports it let
/// through.
#[derive(Debug, Clone, Resource, Default)]
pub struct InterestCadence {
    /// The preset reports go out under now.
    active: CadencePreset,
    /// Whether to step through the presets ([`CYCLE_PERIOD_SECS`] each).
    cycle: bool,
    /// Seconds the active preset has been in force (counted only when cycling).
    since_switch: f32,
    /// Interest reports sent since the readout last took the count.
    reports: u32,
}

impl InterestCadence {
    /// The cadence an `SL_VIEWER_INTEREST_CADENCE` value asks for.
    ///
    /// # Errors
    ///
    /// When the value is none of `2`, `45`, `125` and `cycle`.
    pub fn parse(value: &str) -> Result<Self, CadenceParseError> {
        let (active, cycle) = match value.trim() {
            "2" => (CadencePreset::Legacy2Hz, false),
            "45" => (CadencePreset::Current45Hz, false),
            "125" => (CadencePreset::Reference125Hz, false),
            // Start from the slowest, so the first window is the baseline.
            "cycle" => (CadencePreset::Legacy2Hz, true),
            other => return Err(CadenceParseError(other.to_owned())),
        };
        Ok(Self {
            active,
            cycle,
            ..Self::default()
        })
    }

    /// The cadence from the environment: the default when
    /// `SL_VIEWER_INTEREST_CADENCE` is unset, and the default with an error
    /// logged when it holds something else.
    #[must_use]
    pub fn from_env() -> Self {
        let Ok(value) = std::env::var("SL_VIEWER_INTEREST_CADENCE") else {
            return Self::default();
        };
        match Self::parse(&value) {
            Ok(cadence) => {
                info!(
                    target: "interest_cadence",
                    "interest camera cadence {} Hz{}",
                    cadence.active.label(),
                    if cadence.cycle { ", cycling" } else { "" },
                );
                cadence
            }
            Err(error) => {
                error!(target: "interest_cadence", "{error}; keeping the default 45 Hz");
                Self::default()
            }
        }
    }

    /// The policy reports go out under now.
    #[must_use]
    pub const fn policy(&self) -> CadencePolicy {
        self.active.policy()
    }

    /// The preset in force now.
    #[must_use]
    pub const fn active(&self) -> CadencePreset {
        self.active
    }

    /// Count one interest report sent.
    pub const fn record_report(&mut self) {
        self.reports = self.reports.saturating_add(1);
    }

    /// Take the reports counted since the last call.
    const fn take_reports(&mut self) -> u32 {
        core::mem::replace(&mut self.reports, 0)
    }

    /// Let `delta_secs` pass; when cycling and the active preset's time is up,
    /// move to the next one and return it.
    pub fn advance(&mut self, delta_secs: f32) -> Option<CadencePreset> {
        if !self.cycle {
            return None;
        }
        self.since_switch += delta_secs;
        if self.since_switch < CYCLE_PERIOD_SECS {
            return None;
        }
        self.since_switch = 0.0;
        self.active = self.active.next();
        Some(self.active)
    }
}

/// Step a cycling [`InterestCadence`] on to its next preset when its time is up.
pub fn advance_interest_cadence(time: Res<Time>, mut cadence: ResMut<InterestCadence>) {
    if let Some(preset) = cadence.advance(time.delta_secs()) {
        info!(target: "interest_cadence", "interest camera cadence now {} Hz", preset.label());
    }
}

/// Messages and the objects they named, for one message form.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FormCount {
    /// Messages of this form.
    pub messages: u32,
    /// Objects those messages named, summed.
    pub entries: u32,
}

impl FormCount {
    /// Count one message naming `entries` objects.
    fn add(&mut self, entries: usize) {
        self.messages = self.messages.saturating_add(1);
        self.entries = self
            .entries
            .saturating_add(u32::try_from(entries).unwrap_or(u32::MAX));
    }
}

/// What the object-update stream carried over one readout window.
#[derive(Debug, Clone, Default)]
pub struct StreamWindow {
    /// `ObjectUpdate`.
    pub full: FormCount,
    /// `ObjectUpdateCompressed`.
    pub compressed: FormCount,
    /// `ObjectUpdateCached`.
    pub cached: FormCount,
    /// `ImprovedTerseObjectUpdate`.
    pub terse: FormCount,
    /// `KillObject`.
    pub kill: FormCount,
    /// Terse updates per object, the motion stream a moving object rides.
    terse_per_object: HashMap<(RegionHandle, RegionLocalObjectId), u32>,
}

impl StreamWindow {
    /// Count one message of the stream.
    pub fn record(&mut self, batch: &ObjectStreamBatch) {
        let count = match batch.form {
            ObjectUpdateForm::Full => &mut self.full,
            ObjectUpdateForm::Compressed => &mut self.compressed,
            ObjectUpdateForm::Cached => &mut self.cached,
            ObjectUpdateForm::Terse => &mut self.terse,
            ObjectUpdateForm::Kill => &mut self.kill,
        };
        count.add(batch.entries.len());
        if batch.form == ObjectUpdateForm::Terse {
            for entry in &batch.entries {
                let seen = self
                    .terse_per_object
                    .entry((batch.region_handle, entry.local_id))
                    .or_insert(0);
                *seen = seen.saturating_add(1);
            }
        }
    }

    /// How many distinct objects got a terse update.
    #[must_use]
    pub fn moving_objects(&self) -> usize {
        self.terse_per_object.len()
    }

    /// The `n` objects with the most terse updates, most first (ties by id, so
    /// the order is stable).
    #[must_use]
    pub fn busiest(&self, n: usize) -> Vec<(RegionLocalObjectId, u32)> {
        let mut objects: Vec<_> = self
            .terse_per_object
            .iter()
            .map(|(&(_, id), &count)| (id, count))
            .collect();
        objects.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        objects.truncate(n);
        objects
    }
}

/// The readout's state: the window being counted and the region's last
/// reported statistics.
#[derive(Debug, Resource, Default)]
pub struct InterestStreamDiag {
    /// The stream in the current window.
    window: StreamWindow,
    /// Seconds into the current window.
    elapsed: f32,
    /// The region's last `AgentUpdatesPerSecond`.
    sim_agent_updates: Option<f32>,
    /// The region's last `SimFps`.
    sim_fps: Option<f32>,
}

/// Count the object-update stream and the region's statistics, and once a
/// second log them with the reports sent under the active preset.
pub fn log_interest_cadence(
    time: Res<Time>,
    mut events: MessageReader<SlEvent>,
    mut diag: ResMut<InterestStreamDiag>,
    mut cadence: ResMut<InterestCadence>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ObjectStreamBatch(batch) => diag.window.record(batch),
            SlSessionEvent::SimStats(stats) => {
                for &(id, value) in &stats.stats {
                    match id {
                        SimStatId::AgentUpdatesPerSecond => diag.sim_agent_updates = Some(value),
                        SimStatId::SimFps => diag.sim_fps = Some(value),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    diag.elapsed += time.delta_secs();
    if diag.elapsed < 1.0 {
        return;
    }
    let window = core::mem::take(&mut diag.window);
    diag.elapsed = 0.0;
    let busiest = window
        .busiest(TOP_OBJECTS)
        .iter()
        .map(|(id, count)| format!("{id}:{count}"))
        .collect::<Vec<_>>()
        .join(",");
    info!(
        target: "interest_cadence",
        "cadence={}Hz reports={} terse={}/{} full={}/{} compressed={}/{} cached={}/{} \
         kill={}/{} moving={} busiest=[{busiest}] sim_agent_updates={:?} sim_fps={:?}",
        cadence.active().label(),
        cadence.take_reports(),
        window.terse.messages,
        window.terse.entries,
        window.full.messages,
        window.full.entries,
        window.compressed.messages,
        window.compressed.entries,
        window.cached.messages,
        window.cached.entries,
        window.kill.messages,
        window.kill.entries,
        window.moving_objects(),
        diag.sim_agent_updates,
        diag.sim_fps,
    );
}

/// Whether the once-a-second readout is on (`SL_VIEWER_LOG_INTEREST_CADENCE`).
#[must_use]
pub fn log_enabled() -> bool {
    std::env::var_os("SL_VIEWER_LOG_INTEREST_CADENCE").is_some()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::ObjectStreamEntry;

    use super::*;

    /// A camera at the origin looking down `-Z`, reported once already.
    fn last_view() -> Option<(Vec3, Vec3)> {
        Some((Vec3::ZERO, Vec3::NEG_Z))
    }

    /// A unit look axis turned `degrees` about `Y` from `-Z`.
    fn turned(degrees: f32) -> Vec3 {
        Quat::from_rotation_y(degrees.to_radians()).mul_vec3(Vec3::NEG_Z)
    }

    #[test]
    fn the_first_report_waits_only_for_the_rate_limit() {
        let policy = CadencePreset::Current45Hz.policy();
        assert!(!policy.should_report(0.01, None, Vec3::ZERO, Vec3::NEG_Z));
        assert!(policy.should_report(0.05, None, Vec3::ZERO, Vec3::NEG_Z));
    }

    #[test]
    fn a_still_camera_is_not_reported_again() {
        for preset in [
            CadencePreset::Legacy2Hz,
            CadencePreset::Current45Hz,
            CadencePreset::Reference125Hz,
        ] {
            assert!(
                !preset
                    .policy()
                    .should_report(10.0, last_view(), Vec3::ZERO, Vec3::NEG_Z),
                "{preset:?}"
            );
        }
    }

    /// The reference reports a 0.5° turn and a 1.5 cm step; the 45 Hz preset
    /// waits for ~2.6° and 2 cm.
    #[test]
    fn the_reference_preset_reports_finer_changes() {
        let reference = CadencePreset::Reference125Hz.policy();
        let current = CadencePreset::Current45Hz.policy();
        let small_turn = turned(0.5);
        assert!(reference.should_report(0.1, last_view(), Vec3::ZERO, small_turn));
        assert!(!current.should_report(0.1, last_view(), Vec3::ZERO, small_turn));
        let small_step = Vec3::new(0.015, 0.0, 0.0);
        assert!(reference.should_report(0.1, last_view(), small_step, Vec3::NEG_Z));
        assert!(!current.should_report(0.1, last_view(), small_step, Vec3::NEG_Z));
        assert!(current.should_report(0.1, last_view(), Vec3::ZERO, turned(3.0)));
    }

    /// The coarse look threshold is the viewer's old `1 - dot < 1e-3` test in
    /// the reference's vector-difference form: a turn just inside the old
    /// threshold stays quiet and one just past it reports.
    #[test]
    fn the_coarse_look_threshold_matches_the_dot_product_test() {
        let policy = CadencePreset::Current45Hz.policy();
        let threshold_degrees = (1.0_f32 - 1.0e-3).acos().to_degrees();
        let inside = turned(threshold_degrees - 0.05);
        let outside = turned(threshold_degrees + 0.05);
        assert!(Vec3::NEG_Z.dot(inside) > 1.0 - 1.0e-3);
        assert!(!policy.should_report(1.0, last_view(), Vec3::ZERO, inside));
        assert!(Vec3::NEG_Z.dot(outside) < 1.0 - 1.0e-3);
        assert!(policy.should_report(1.0, last_view(), Vec3::ZERO, outside));
    }

    #[test]
    fn the_legacy_preset_holds_half_a_second() {
        let policy = CadencePreset::Legacy2Hz.policy();
        let far = Vec3::new(5.0, 0.0, 0.0);
        assert!(!policy.should_report(0.4, last_view(), far, Vec3::NEG_Z));
        assert!(policy.should_report(0.5, last_view(), far, Vec3::NEG_Z));
    }

    #[test]
    fn the_environment_value_picks_the_preset() -> Result<(), CadenceParseError> {
        assert_eq!(
            InterestCadence::parse("2")?.active(),
            CadencePreset::Legacy2Hz
        );
        assert_eq!(
            InterestCadence::parse(" 125 ")?.active(),
            CadencePreset::Reference125Hz
        );
        assert_eq!(
            InterestCadence::parse("knob").map(|cadence| cadence.active()),
            Err(CadenceParseError("knob".to_owned()))
        );
        Ok(())
    }

    #[test]
    fn a_fixed_preset_never_changes() -> Result<(), CadenceParseError> {
        let mut cadence = InterestCadence::parse("45")?;
        assert_eq!(cadence.advance(CYCLE_PERIOD_SECS * 3.0), None);
        assert_eq!(cadence.active(), CadencePreset::Current45Hz);
        Ok(())
    }

    #[test]
    fn cycling_visits_every_preset_in_turn() -> Result<(), CadenceParseError> {
        let mut cadence = InterestCadence::parse("cycle")?;
        assert_eq!(cadence.active(), CadencePreset::Legacy2Hz);
        assert_eq!(cadence.advance(CYCLE_PERIOD_SECS - 1.0), None);
        assert_eq!(cadence.advance(1.0), Some(CadencePreset::Current45Hz));
        assert_eq!(
            cadence.advance(CYCLE_PERIOD_SECS),
            Some(CadencePreset::Reference125Hz)
        );
        assert_eq!(
            cadence.advance(CYCLE_PERIOD_SECS),
            Some(CadencePreset::Legacy2Hz)
        );
        Ok(())
    }

    #[test]
    fn taking_the_report_count_resets_it() {
        let mut cadence = InterestCadence::default();
        cadence.record_report();
        cadence.record_report();
        assert_eq!(cadence.take_reports(), 2);
        assert_eq!(cadence.take_reports(), 0);
    }

    /// One stream message of `form` naming the objects `ids` in region `1`.
    fn batch(form: ObjectUpdateForm, ids: &[u32]) -> ObjectStreamBatch {
        ObjectStreamBatch {
            region_handle: RegionHandle::new(1),
            child: false,
            form,
            entries: ids
                .iter()
                .map(|&id| ObjectStreamEntry {
                    local_id: RegionLocalObjectId::new(id),
                    known: true,
                    crc: None,
                    cache_hit: None,
                })
                .collect(),
        }
    }

    #[test]
    fn the_window_counts_messages_and_objects_per_form() {
        let mut window = StreamWindow::default();
        window.record(&batch(ObjectUpdateForm::Terse, &[7, 8]));
        window.record(&batch(ObjectUpdateForm::Terse, &[7]));
        window.record(&batch(ObjectUpdateForm::Terse, &[7, 9]));
        window.record(&batch(ObjectUpdateForm::Full, &[3]));
        window.record(&batch(ObjectUpdateForm::Kill, &[4, 5, 6]));
        assert_eq!(
            window.terse,
            FormCount {
                messages: 3,
                entries: 5
            }
        );
        assert_eq!(
            window.full,
            FormCount {
                messages: 1,
                entries: 1
            }
        );
        assert_eq!(
            window.kill,
            FormCount {
                messages: 1,
                entries: 3
            }
        );
        assert_eq!(window.cached, FormCount::default());
        assert_eq!(window.moving_objects(), 3);
        assert_eq!(
            window.busiest(2),
            vec![
                (RegionLocalObjectId::new(7), 3),
                (RegionLocalObjectId::new(8), 1)
            ]
        );
    }
}
