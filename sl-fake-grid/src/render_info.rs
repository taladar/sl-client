//! The region's `AvatarRenderInfo`: what its viewers report of each other's
//! complexity, and what the region states back.
//!
//! Second Life's behaviour, as measured (`avatar-render-info`, 2026-10-21,
//! `book/src/gridspec/avatars.md` § Render info): before any report the
//! statement is `{agents: {}, overlimit: 0, reportinglimit: 0}`; a lone report
//! of an avatar is stated verbatim, to every reader in the region; the
//! `reportinglimit` a reader is told counts the reports of it — reports, not
//! reporters, and its own report of itself too. `overlimit` stayed `0` though
//! one of those reports found the reader too complex; what makes Second Life
//! count one was not found, so the fake grid never does.
//!
//! What Second Life states once several reports stand behind an avatar was not
//! reducible to them, and how long it keeps a report was not measured. The
//! fake grid's rules for both are its own: **the latest report of an avatar is
//! its stated weight**, and **a report is held for five minutes**
//! ([`REPORT_HOLD`]) — long enough for a census to read back every report it
//! sent, short enough that the counts of a long session stay those of the
//! viewers still reporting (each reports every minute or so).

use std::time::{Duration, Instant};

use sl_proto::{AgentKey, AvatarRenderInfo, AvatarRenderReport, RenderLimitCounts};

/// How long the region holds a report: the fake grid's rule, not a
/// measurement.
pub(crate) const REPORT_HOLD: Duration = Duration::from_secs(300);

/// One report the region holds.
#[derive(Debug, Clone, Copy)]
struct HeldReport {
    /// What was reported.
    report: AvatarRenderReport,
    /// When it arrived.
    at: Instant,
}

/// The reports a region holds.
#[derive(Debug, Clone, Default)]
pub(crate) struct RegionRenderInfo {
    /// Every report still held, oldest first.
    reports: Vec<HeldReport>,
}

impl RegionRenderInfo {
    /// Files a viewer's `reports`, arrived at `now`. Who sent them does not
    /// matter: Second Life counts reports, not reporters.
    pub(crate) fn file(&mut self, reports: &[AvatarRenderReport], now: Instant) {
        self.forget_expired(now);
        self.reports.extend(reports.iter().map(|report| HeldReport {
            report: *report,
            at: now,
        }));
    }

    /// The region's statement to `reader` at `now`.
    pub(crate) fn statement(&mut self, reader: AgentKey, now: Instant) -> AvatarRenderInfo {
        self.forget_expired(now);
        let mut weights: Vec<(AgentKey, u32)> = Vec::new();
        // Oldest first, so a later report of an avatar replaces an earlier one.
        for held in &self.reports {
            match weights
                .iter_mut()
                .find(|(agent, _weight)| *agent == held.report.agent)
            {
                Some(entry) => entry.1 = held.report.weight,
                None => weights.push((held.report.agent, held.report.weight)),
            }
        }
        weights.sort_by_key(|(agent, _weight)| agent.uuid());
        let of_reader = self
            .reports
            .iter()
            .filter(|held| held.report.agent == reader);
        AvatarRenderInfo {
            weights,
            limits: Some(RenderLimitCounts {
                reporting: u32::try_from(of_reader.count()).unwrap_or(u32::MAX),
                // As measured: not raised by a too-complex report.
                over_limit: 0,
            }),
        }
    }

    /// Drops the reports held longer than [`REPORT_HOLD`].
    fn forget_expired(&mut self, now: Instant) {
        self.reports
            .retain(|held| now.saturating_duration_since(held.at) < REPORT_HOLD);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;
    use sl_proto::{AgentKey, AvatarRenderInfo, AvatarRenderReport, RenderLimitCounts, Uuid};

    use super::{REPORT_HOLD, RegionRenderInfo};

    /// The two avatars of the census.
    fn a() -> AgentKey {
        AgentKey::from(Uuid::from_u128(0xa))
    }

    /// The census's second avatar.
    fn b() -> AgentKey {
        AgentKey::from(Uuid::from_u128(0xb))
    }

    /// A report of `agent` at `weight`.
    const fn report(agent: AgentKey, weight: u32, too_complex: bool) -> AvatarRenderReport {
        AvatarRenderReport {
            agent,
            weight,
            too_complex,
        }
    }

    /// `secs` seconds after `at`.
    fn after(at: Instant, secs: u64) -> Instant {
        at.checked_add(Duration::from_secs(secs)).unwrap_or(at)
    }

    /// Before any report the statement names nobody and counts nothing.
    #[test]
    fn the_first_statement_is_empty() {
        let mut region = RegionRenderInfo::default();
        assert_eq!(
            region.statement(a(), Instant::now()),
            AvatarRenderInfo {
                weights: Vec::new(),
                limits: Some(RenderLimitCounts::default()),
            }
        );
    }

    /// A lone report is stated verbatim to every reader, and counted only for
    /// the avatar it is about.
    #[test]
    fn a_lone_report_is_stated_to_everyone() {
        let now = Instant::now();
        let mut region = RegionRenderInfo::default();
        region.file(&[report(a(), 123_457, false)], now);
        let to_a = region.statement(a(), now);
        let to_b = region.statement(b(), now);
        assert_eq!(to_a.weights, vec![(a(), 123_457)]);
        assert_eq!(to_b.weights, to_a.weights);
        assert_eq!(
            to_a.limits,
            Some(RenderLimitCounts {
                reporting: 1,
                over_limit: 0
            })
        );
        assert_eq!(to_b.limits, Some(RenderLimitCounts::default()));
    }

    /// Reports are counted, not reporters, and the latest report of an avatar
    /// is its stated weight; a too-complex one does not count as over the
    /// limit, as it did not on Second Life.
    #[test]
    fn every_report_counts_and_the_latest_is_stated() {
        let now = Instant::now();
        let mut region = RegionRenderInfo::default();
        region.file(&[report(a(), 123_457, false)], now);
        region.file(&[report(a(), 345_679, false)], after(now, 1));
        region.file(&[report(a(), 234_571, true)], after(now, 2));
        let statement = region.statement(a(), after(now, 3));
        assert_eq!(statement.weights, vec![(a(), 234_571)]);
        assert_eq!(
            statement.limits,
            Some(RenderLimitCounts {
                reporting: 3,
                over_limit: 0
            })
        );
    }

    /// A report is forgotten once the region has held it for five minutes.
    #[test]
    fn a_report_is_held_for_five_minutes() {
        let now = Instant::now();
        let mut region = RegionRenderInfo::default();
        region.file(&[report(b(), 25_054, false)], now);
        let held = REPORT_HOLD.as_secs();
        assert_eq!(
            region.statement(a(), after(now, held - 1)).weights,
            vec![(b(), 25_054)]
        );
        assert_eq!(region.statement(a(), after(now, held)).weights, Vec::new());
    }
}
