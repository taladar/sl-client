//! When the region's `AvatarRenderInfo` capability is asked and told: the
//! reference viewer's cadence (Firestorm `llavatarrenderinfoaccountant.cpp`),
//! kept by the session so both runtimes keep the same one.
//!
//! The session holds the timers and the application's latest report; a driver
//! asks [`Session::next_avatar_render_info`] each turn of its loop and sends
//! what it answers to the capability. A statement the region answers arrives
//! through [`Session::handle_caps_event`] as
//! [`Event::AvatarRenderInfo`](crate::Event::AvatarRenderInfo); the answer to a
//! report is Second Life's `{}`, and nothing is read from it.
//!
//! [`Session::next_avatar_render_info`]: crate::Session::next_avatar_render_info
//! [`Session::handle_caps_event`]: crate::Session::handle_caps_event

use std::time::{Duration, Instant};

use sl_wire::AvatarRenderReport;

use crate::link::deadline;

/// How long after arriving in a region its statement is asked for again, and
/// every time after (`SECS_BETWEEN_REGION_REQUEST`).
const REQUEST_EVERY: Duration = Duration::from_secs(15);

/// How long after arriving in a region the first report is due
/// (`SECS_BETWEEN_REGION_SCANS`: the reference reports on the scan after the
/// one that found the region).
const FIRST_REPORT_AFTER: Duration = Duration::from_secs(5);

/// How long after a report the next one is due, before the per-avatar addition
/// (`SECS_BETWEEN_REGION_REPORTS`).
const REPORT_EVERY: Duration = Duration::from_secs(60);

/// How much later the next report is due for each avatar the last one named
/// (the reference's `2.f * num_avs`): a crowded region is told less often.
const REPORT_DELAY_PER_AVATAR: Duration = Duration::from_secs(2);

/// The session's `AvatarRenderInfo` timers and the report it sends next.
#[derive(Debug, Default)]
pub(crate) struct RenderInfoSchedule {
    /// When the region's statement is next asked for; [`None`] while the
    /// region grants no `AvatarRenderInfo` (or none is known yet).
    next_request: Option<Instant>,
    /// When the next report is due; [`None`] likewise.
    next_report: Option<Instant>,
    /// The application's latest reckoning of the avatars in the region, sent
    /// as the next report
    /// ([`Session::set_avatar_render_reports`](crate::Session::set_avatar_render_reports)).
    reports: Vec<AvatarRenderReport>,
}

/// What [`Session::next_avatar_render_info`](crate::Session::next_avatar_render_info)
/// says is due now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AvatarRenderInfoDue {
    /// Ask the region for its statement (a GET of `AvatarRenderInfo`).
    pub request: bool,
    /// Report these avatars (a POST of `AvatarRenderInfo`); [`None`] when no
    /// report is due or there is no avatar to report, which the reference does
    /// not send.
    pub report: Option<Vec<AvatarRenderReport>>,
}

impl RenderInfoSchedule {
    /// A schedule with nothing due and nothing to report.
    pub(crate) const fn new() -> Self {
        Self {
            next_request: None,
            next_report: None,
            reports: Vec::new(),
        }
    }

    /// The earlier of the two deadlines, for the session's wake-up.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        match (self.next_request, self.next_report) {
            (Some(request), Some(report)) => Some(request.min(report)),
            (request, report) => request.or(report),
        }
    }

    /// The agent left the region: its capability is gone, and the reckonings
    /// were of the avatars there.
    pub(crate) fn leave_region(&mut self) {
        *self = Self::default();
    }

    /// See [`Session::start_avatar_render_info`](crate::Session::start_avatar_render_info).
    pub(crate) fn start(&mut self, granted: bool, now: Instant) {
        self.next_request = granted.then_some(now);
        self.next_report = granted.then(|| deadline(now, FIRST_REPORT_AFTER));
    }

    /// See [`Session::set_avatar_render_reports`](crate::Session::set_avatar_render_reports).
    pub(crate) fn set_reports(&mut self, reports: Vec<AvatarRenderReport>) {
        self.reports = reports;
    }

    /// See [`Session::next_avatar_render_info`](crate::Session::next_avatar_render_info).
    pub(crate) fn next_due(&mut self, now: Instant) -> AvatarRenderInfoDue {
        let mut due = AvatarRenderInfoDue::default();
        if self.next_request.is_some_and(|at| at <= now) {
            self.next_request = Some(deadline(now, REQUEST_EVERY));
            due.request = true;
        }
        if self.next_report.is_some_and(|at| at <= now) {
            let avatars = u32::try_from(self.reports.len()).unwrap_or(u32::MAX);
            let wait = REPORT_EVERY.saturating_add(REPORT_DELAY_PER_AVATAR.saturating_mul(avatars));
            self.next_report = Some(deadline(now, wait));
            if !self.reports.is_empty() {
                due.report = Some(self.reports.clone());
            }
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use sl_wire::AvatarRenderReport;
    use uuid::Uuid;

    use super::{AvatarRenderInfoDue, RenderInfoSchedule};

    /// A report of `n` avatars, each at a distinct weight.
    fn reports(n: u32) -> Vec<AvatarRenderReport> {
        (1..=n)
            .map(|index| AvatarRenderReport {
                agent: AgentKey::from(Uuid::from_u128(u128::from(index))),
                weight: index.saturating_mul(1000),
                too_complex: false,
            })
            .collect()
    }

    /// Nothing is due before a region grants the capability, nor after one
    /// that does not.
    #[test]
    fn nothing_is_due_without_the_capability() {
        let start = Instant::now();
        let mut schedule = RenderInfoSchedule::new();
        assert_eq!(schedule.next_due(start), AvatarRenderInfoDue::default());
        schedule.start(false, start);
        schedule.set_reports(reports(1));
        assert_eq!(
            schedule.next_due(start + Duration::from_secs(600)),
            AvatarRenderInfoDue::default()
        );
        assert_eq!(schedule.next_deadline(), None);
    }

    /// The statement is asked for on arrival and every 15 seconds after.
    #[test]
    fn the_statement_is_asked_for_every_fifteen_seconds() {
        let start = Instant::now();
        let mut schedule = RenderInfoSchedule::new();
        schedule.start(true, start);
        assert!(schedule.next_due(start).request);
        assert!(!schedule.next_due(start + Duration::from_secs(14)).request);
        assert!(schedule.next_due(start + Duration::from_secs(15)).request);
    }

    /// The first report is due five seconds after arrival, the next 60 seconds
    /// plus two per avatar after that.
    #[test]
    fn a_report_waits_two_seconds_longer_per_avatar() {
        let start = Instant::now();
        let mut schedule = RenderInfoSchedule::new();
        schedule.start(true, start);
        schedule.set_reports(reports(3));
        assert_eq!(schedule.next_due(start).report, None);
        let first = start + Duration::from_secs(5);
        assert_eq!(schedule.next_due(first).report, Some(reports(3)));
        assert_eq!(
            schedule.next_due(first + Duration::from_secs(65)).report,
            None
        );
        assert_eq!(
            schedule.next_due(first + Duration::from_secs(66)).report,
            Some(reports(3))
        );
    }

    /// With no avatar to report nothing is sent, but the timer moves on.
    #[test]
    fn an_empty_report_is_not_sent() {
        let start = Instant::now();
        let mut schedule = RenderInfoSchedule::new();
        schedule.start(true, start);
        let first = start + Duration::from_secs(5);
        assert_eq!(schedule.next_due(first).report, None);
        schedule.set_reports(reports(1));
        assert_eq!(
            schedule.next_due(first + Duration::from_secs(59)).report,
            None
        );
        assert_eq!(
            schedule.next_due(first + Duration::from_secs(60)).report,
            Some(reports(1))
        );
    }

    /// Leaving a region stops the cadence and forgets the reckonings, which
    /// were of the avatars there.
    #[test]
    fn leaving_the_region_forgets_everything() {
        let start = Instant::now();
        let mut schedule = RenderInfoSchedule::new();
        schedule.start(true, start);
        schedule.set_reports(reports(2));
        schedule.leave_region();
        assert_eq!(schedule.next_deadline(), None);
        assert!(schedule.reports.is_empty());
    }
}
