//! The region's **render info** (`protocol-avatar-render-info`): what this
//! viewer tells the region about each avatar's complexity, what the region
//! states back from every viewer's reports, and the notice that some of them
//! render ours as a jellydoll.
//!
//! Second Life only — OpenSim grants no `AvatarRenderInfo`, so there the
//! session never asks or tells, and nothing here ever has anything to show.
//!
//! - **Telling.** Each avatar in our region that [`AvatarComplexityModel`] has
//!   scored is reported with that score and whether it is past our own limit
//!   (`report_avatar_render_info`), as the reference's
//!   `LLAvatarRenderInfoAccountant::avatarRenderInfoReportCoro` builds its
//!   POST. The session sends the report on the reference cadence; this module
//!   only keeps it current.
//! - **Hearing.** Each statement the region answers is folded into
//!   [`ReportedComplexity`] (`fold_avatar_render_info`): each avatar's weight as
//!   the region reckons it, which the radar's Reported column and the name
//!   tag's complexity line show beside our own reckoning, and the two counts
//!   about our avatar.
//! - **The notice.** When the counts say some viewers render us as a jellydoll,
//!   or no longer do, the reference raises "Your avatar complexity is N. You
//!   may not be rendered by …" (`AgentComplexityWithVisibility`), under the
//!   rules of its `LLAvatarRenderNotifier::updateNotificationRegion`, ported in
//!   `OverLimitNotifier`.
//!
//! Reference (Firestorm, read-only): `llavatarrenderinfoaccountant.cpp`,
//! `llavatarrendernotifier.cpp`, `LLVOAvatar::isTooComplex`.

use std::collections::HashMap;

use bevy::prelude::*;

use sl_client_bevy::{
    AgentKey, AvatarRenderReport, Command, RenderLimitCounts, SlCommand, SlEvent, SlIdentity,
    SlSessionEvent,
};
use sl_viewer_notifications::ShowNotification;
use sl_viewer_ui_core::i18n::Translator;
use sl_viewer_world_api::{AvatarState, CameraMode};

use crate::avatar_complexity::{AvatarComplexityModel, JellyReason};

/// The catalogue name of the notice that some viewers render us as a
/// jellydoll.
pub const AGENT_COMPLEXITY_WITH_VISIBILITY: &str = "AgentComplexityWithVisibility";

/// The `unique` context the reference raises the notice under
/// (`AgentComplexityNotice`), so a newer one replaces the one still showing.
const NOTICE_CONTEXT: &str = "AgentComplexityNotice";

/// How long after the viewer starts the first notice may be raised
/// (`OVER_LIMIT_UPDATE_DELAY`): the counts of the first minute are of an
/// avatar still loading.
const FIRST_NOTICE_AFTER_SECS: f64 = 70.0;

/// How long after a notice the next may be raised, unless the last is still
/// showing (`ComplexityChangesPopUpDelay`, default 300).
const NOTICE_EVERY_SECS: f64 = 300.0;

/// How long a notice shows (`ShowMyComplexityChanges`, default 20): while it
/// does, a further change replaces it at once.
const NOTICE_SHOWS_SECS: f64 = 20.0;

/// How much the share of viewers not rendering us must move for a new notice:
/// more than a tenth of the new share (`RENDER_ALLOWED_CHANGE_PCT`).
const NOTICE_CHANGE_FRACTION: f32 = 0.1;

/// What the region states of each avatar's complexity, folded from every
/// `AvatarRenderInfo` statement: the weight it reckons from the viewers'
/// reports, kept per avatar until a later statement names it again (as the
/// reference keeps `mReportedVisualComplexity` on the avatar), and the two
/// counts about our own avatar from the latest statement.
#[derive(Resource, Debug, Default)]
pub struct ReportedComplexity {
    /// The stated weight of each avatar a statement has named.
    weights: HashMap<AgentKey, u32>,
    /// The latest statement's counts about our own avatar.
    limits: Option<RenderLimitCounts>,
    /// Bumped whenever a stated weight changes, so a view over them rebuilds.
    revision: u64,
}

impl ReportedComplexity {
    /// The weight the region states for `agent`, if a statement named it.
    #[must_use]
    pub fn reported(&self, agent: AgentKey) -> Option<u32> {
        self.weights.get(&agent).copied()
    }

    /// The latest statement's counts about our own avatar: how many reports
    /// of it the region holds, and how many found it too complex.
    #[must_use]
    pub const fn limits(&self) -> Option<RenderLimitCounts> {
        self.limits
    }

    /// The revision of the stated weights — a view stores the value it last
    /// built at and rebuilds when it advances.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Folds one statement in; whether a stated weight changed.
    fn fold(&mut self, weights: &[(AgentKey, u32)], limits: Option<RenderLimitCounts>) -> bool {
        let mut changed = false;
        for (agent, weight) in weights {
            if self.weights.insert(*agent, *weight) != Some(*weight) {
                changed = true;
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        if limits.is_some() {
            self.limits = limits;
        }
        changed
    }
}

/// Which of the reference's five sentences a notice ends with, by the share
/// of the viewers reporting on us that render us as a jellydoll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverLimitShare {
    /// None any more (`av_render_everyone_now`) — only ever after some did.
    EveryoneNow,
    /// More than a tenth (`av_render_not_everyone`).
    NotEveryone,
    /// Half or more (`av_render_over_half`).
    OverHalf,
    /// Three quarters or more (`av_render_most_of`).
    MostOf,
    /// All of them (`av_render_anyone`, from 99 %).
    Anyone,
}

impl OverLimitShare {
    /// The sentence for `percent` of the reporting viewers, by the reference's
    /// thresholds.
    fn of(percent: f32) -> Self {
        if percent >= 99.0 {
            Self::Anyone
        } else if percent >= 75.0 {
            Self::MostOf
        } else if percent >= 50.0 {
            Self::OverHalf
        } else if percent > 10.0 {
            Self::NotEveryone
        } else {
            Self::EveryoneNow
        }
    }

    /// The Fluent key of the sentence.
    const fn key(self) -> &'static str {
        match self {
            Self::EveryoneNow => "avatar-render-everyone-now",
            Self::NotEveryone => "avatar-render-not-everyone",
            Self::OverHalf => "avatar-render-over-half",
            Self::MostOf => "avatar-render-most-of",
            Self::Anyone => "avatar-render-anyone",
        }
    }
}

/// When the jellydoll notice is raised: a port of the reference's
/// `LLAvatarRenderNotifier::updateNotificationRegion`. Times are the app's
/// elapsed seconds.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub(crate) struct OverLimitNotifier {
    /// The other viewers reporting on us when the last notice was raised.
    shown_reporting: u32,
    /// How many of them rendered us as a jellydoll then.
    shown_over_limit: u32,
    /// That as a percentage.
    shown_percent: f32,
    /// When a notice may next be raised, unless the last is still showing.
    quiet_until: f64,
    /// When the last notice was raised.
    shown_at: Option<f64>,
}

impl Default for OverLimitNotifier {
    fn default() -> Self {
        Self {
            shown_reporting: 0,
            shown_over_limit: 0,
            shown_percent: 0.0,
            quiet_until: FIRST_NOTICE_AFTER_SECS,
            shown_at: None,
        }
    }
}

impl OverLimitNotifier {
    /// Takes the latest counts at `now`; the sentence a notice raised now ends
    /// with, or [`None`] when no notice is due. A notice due is noted as shown
    /// whether or not it can be (the reference records it before deciding
    /// mouselook keeps it off the screen).
    pub(crate) fn update(&mut self, counts: RenderLimitCounts, now: f64) -> Option<OverLimitShare> {
        if counts.reporting == 0 {
            // Nobody has reported on us yet: the data is not ready.
            return None;
        }
        // Our own report of ourselves is counted too; the reference takes one
        // off for it unless that would leave fewer viewers than jellydoll us.
        let reporting = if counts.reporting > counts.over_limit {
            counts.reporting.saturating_sub(1)
        } else {
            counts.reporting
        };
        let percent = if reporting == 0 {
            0.0
        } else {
            share_percent(counts.over_limit, reporting)
        };
        if reporting == self.shown_reporting && counts.over_limit == self.shown_over_limit {
            return None;
        }
        let showing = self.shown_at.is_some_and(|at| now - at < NOTICE_SHOWS_SECS);
        let due = (now >= self.quiet_until || showing)
            && (self.shown_percent > 0.0 || percent > 0.0)
            && (self.shown_percent - percent).abs() > percent * NOTICE_CHANGE_FRACTION;
        if !due {
            return None;
        }
        self.shown_reporting = reporting;
        self.shown_over_limit = counts.over_limit;
        self.shown_percent = percent;
        self.shown_at = Some(now);
        self.quiet_until = now + NOTICE_EVERY_SECS;
        Some(OverLimitShare::of(percent))
    }
}

/// `part` of `whole` as a percentage.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a count of viewers is far below f32's exact-integer range; the share only picks \
              a sentence"
)]
fn share_percent(part: u32, whole: u32) -> f32 {
    part as f32 / whole as f32 * 100.0
}

/// Whether a jellydoll reason is the reference's "too complex" — past the
/// budget or the surface-area trigger — rather than a choice about the person
/// (`LLVOAvatar::isTooComplex` leaves those out).
const fn is_too_complex(reason: Option<JellyReason>) -> bool {
    matches!(
        reason,
        Some(JellyReason::TooComplex | JellyReason::TooMuchArea)
    )
}

/// Keep the session's report current: every avatar in our region the model
/// has scored, with that score and whether it is past our limit. Sent to the
/// session only when it changes; the session reports it on the reference
/// cadence, and never where the region grants no `AvatarRenderInfo`.
pub(crate) fn report_avatar_render_info(
    model: Res<AvatarComplexityModel>,
    avatars: Res<AvatarState>,
    identity: Res<SlIdentity>,
    mut sent: Local<Option<Vec<AvatarRenderReport>>>,
    mut writer: MessageWriter<SlCommand>,
) {
    if !model.is_changed() && !avatars.is_changed() && !identity.is_changed() {
        return;
    }
    let Some(circuit) = identity.circuit_id else {
        return;
    };
    let mut reports: Vec<AvatarRenderReport> = avatars
        .by_scoped
        .iter()
        .filter(|(scoped, _agent)| scoped.circuit() == circuit)
        .filter_map(|(_scoped, agent)| {
            let score = model.complexity(*agent)?.score;
            // The reference reports only an avatar it has a complexity for.
            (score > 0).then(|| AvatarRenderReport {
                agent: *agent,
                weight: score,
                too_complex: is_too_complex(model.jelly_reason_for(*agent)),
            })
        })
        .collect();
    reports.sort_by_key(|report| report.agent.uuid());
    reports.dedup_by_key(|report| report.agent);
    if sent.as_ref() == Some(&reports) {
        return;
    }
    *sent = Some(reports.clone());
    writer.write(SlCommand(Command::SetAvatarRenderReports(reports)));
}

/// The inputs of the jellydoll notice beside the statements themselves,
/// bundled as one [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct NoticeFacts<'w> {
    /// Our own complexity, which the notice states.
    model: Res<'w, AvatarComplexityModel>,
    /// Our own agent.
    identity: Res<'w, SlIdentity>,
    /// The camera mode: the reference raises no notice in mouselook.
    camera: Option<Res<'w, CameraMode>>,
    /// The clock the notice's delays run on.
    time: Res<'w, Time>,
}

/// Fold each `AvatarRenderInfo` statement into [`ReportedComplexity`], and
/// raise the jellydoll notice when the counts about us call for one.
pub(crate) fn fold_avatar_render_info(
    mut events: MessageReader<SlEvent>,
    mut reported: ResMut<ReportedComplexity>,
    mut notifier: ResMut<OverLimitNotifier>,
    facts: NoticeFacts,
    translator: Translator,
    mut notices: MessageWriter<ShowNotification>,
) {
    for event in events.read() {
        let SlSessionEvent::AvatarRenderInfo(info) = &event.0 else {
            continue;
        };
        let _changed = reported.fold(&info.weights, info.limits);
        let Some(counts) = info.limits else {
            continue;
        };
        let Some(share) = notifier.update(counts, facts.time.elapsed_secs_f64()) else {
            continue;
        };
        let in_mouselook = facts
            .camera
            .as_deref()
            .is_some_and(|mode| *mode == CameraMode::Mouselook);
        let own = facts
            .identity
            .agent_id
            .and_then(|agent| facts.model.complexity(agent));
        // A notice of a complexity not measured yet would state a number
        // that means nothing.
        let (false, Some(own)) = (in_mouselook, own) else {
            continue;
        };
        notices.write(
            ShowNotification::new(AGENT_COMPLEXITY_WITH_VISIBILITY)
                .arg("AGENT_COMPLEXITY", translator.integer(i64::from(own.score)))
                .arg("OVERLIMIT_MSG", translator.get(share.key()))
                .with_context(NOTICE_CONTEXT),
        );
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, RenderLimitCounts, Uuid};

    use super::{
        FIRST_NOTICE_AFTER_SECS, NOTICE_EVERY_SECS, OverLimitNotifier, OverLimitShare,
        ReportedComplexity,
    };

    /// Counts of `reporting` reports, `over_limit` of them too complex.
    const fn counts(reporting: u32, over_limit: u32) -> RenderLimitCounts {
        RenderLimitCounts {
            reporting,
            over_limit,
        }
    }

    /// The statement's weights are kept per avatar, and only a change bumps the
    /// revision.
    #[test]
    fn statements_fold_per_avatar() {
        let a = AgentKey::from(Uuid::from_u128(1));
        let b = AgentKey::from(Uuid::from_u128(2));
        let mut reported = ReportedComplexity::default();
        assert!(reported.fold(&[(a, 100)], Some(counts(1, 0))));
        assert!(!reported.fold(&[(a, 100)], None));
        assert_eq!(reported.revision(), 1);
        assert!(reported.fold(&[(b, 200)], None));
        assert_eq!(reported.reported(a), Some(100));
        assert_eq!(reported.reported(b), Some(200));
        assert_eq!(reported.limits(), Some(counts(1, 0)));
    }

    /// Nothing is raised before anyone reports, nor in the first 70 seconds.
    #[test]
    fn no_notice_before_the_data_or_the_first_delay() {
        let mut notifier = OverLimitNotifier::default();
        assert_eq!(notifier.update(counts(0, 0), 100.0), None);
        assert_eq!(notifier.update(counts(5, 2), 10.0), None);
        assert_eq!(
            notifier.update(counts(5, 2), FIRST_NOTICE_AFTER_SECS),
            Some(OverLimitShare::OverHalf),
            "two of the four other viewers"
        );
    }

    /// A count that never went over the limit raises nothing.
    #[test]
    fn nobody_over_the_limit_is_no_notice() {
        let mut notifier = OverLimitNotifier::default();
        assert_eq!(notifier.update(counts(3, 0), 100.0), None);
    }

    /// After a notice the next waits five minutes, unless the last is still
    /// showing; dropping back to nobody says everyone can see us now.
    #[test]
    fn notices_wait_unless_the_last_is_showing() {
        let mut notifier = OverLimitNotifier::default();
        let first = FIRST_NOTICE_AFTER_SECS;
        assert_eq!(
            notifier.update(counts(11, 10), first),
            Some(OverLimitShare::Anyone)
        );
        // Still showing: a change replaces it at once.
        assert_eq!(
            notifier.update(counts(5, 3), first + 5.0),
            Some(OverLimitShare::MostOf)
        );
        // No longer showing, and the five minutes have not passed.
        assert_eq!(notifier.update(counts(5, 0), first + 60.0), None);
        assert_eq!(
            notifier.update(counts(5, 0), first + 5.0 + NOTICE_EVERY_SECS),
            Some(OverLimitShare::EveryoneNow)
        );
    }

    /// The five sentences, by the reference's thresholds.
    #[test]
    fn the_sentence_follows_the_share() {
        assert_eq!(OverLimitShare::of(0.0), OverLimitShare::EveryoneNow);
        assert_eq!(OverLimitShare::of(10.0), OverLimitShare::EveryoneNow);
        assert_eq!(OverLimitShare::of(10.5), OverLimitShare::NotEveryone);
        assert_eq!(OverLimitShare::of(50.0), OverLimitShare::OverHalf);
        assert_eq!(OverLimitShare::of(75.0), OverLimitShare::MostOf);
        assert_eq!(OverLimitShare::of(99.0), OverLimitShare::Anyone);
    }
}
