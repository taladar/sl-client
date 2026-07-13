//! Read the agent's per-experience preferences over `GetExperiences`, then flip
//! one experience's preference over `ExperiencePreferences` and put it back.
//!
//! Every Second Life avatar keeps a personal *experience preference* for each
//! experience it has met: `Allow` (admitted — the experience's objects act
//! without re-prompting), `Block` (refused), or neither (`Forget` — no standing
//! preference, so the next scripted request prompts again). The preferences live
//! behind two HTTP capabilities with no UDP path: `GetExperiences` (GET → the
//! `{ allowed, blocked }` lists) reads them, and `ExperiencePreferences`
//! (PUT `{ "<id>": { permission } }` to `Allow`/`Block`, DELETE `…?<id>` to
//! `Forget`) writes them — and both writes reply with the *full* updated lists,
//! not just the touched id.
//!
//! This case exercises both directions. The **request** half issues
//! `RequestExperiencePermissions` and asserts the read capability answers with a
//! pair of lists (either may legitimately be empty). The **set** half needs a
//! concrete experience to mutate, and mutating one is a real, avatar-visible
//! side effect, so it runs only against a stable [`experience`](crate::fixtures)
//! fixture — never a discovered one. It is written to be *non-destructive*: it
//! records the fixture's current preference (`allowed` / `blocked` / `neither`),
//! flips it to a distinct one (`Block` if it was allowed, else `Allow`), asserts
//! the write's reply lists moved the id into the target list and out of the
//! other, then **restores** the original preference (`Allow` / `Block` / `Forget`
//! as recorded) and asserts the lists returned to their starting classification.
//!
//! Without a fixture the set/restore round-trip is skipped and the case records
//! `partial` — the request half still ran and its lists are recorded, but there
//! is no experience the run may safely toggle.
//!
//! `1av`. Stock OpenSim ships **no** experience module, so with no fixture the
//! OpenSim capabilities are absent: the request would hang and the set half has
//! nothing to touch, so the case records `partial` up front rather than block on
//! a reply that never comes.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ExperienceKey, ExperiencePermission};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, count_metric, is_opensim, secs_metric};

/// The standing preference an agent holds for one experience, as read back from
/// the `{ allowed, blocked }` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preference {
    /// The experience is in the `allowed` list (admitted).
    Allowed,
    /// The experience is in the `blocked` list (refused).
    Blocked,
    /// The experience is in neither list (no standing preference).
    Neither,
}

impl Preference {
    /// Classify an experience against a freshly read `(allowed, blocked)` pair.
    fn classify(id: ExperienceKey, allowed: &[ExperienceKey], blocked: &[ExperienceKey]) -> Self {
        if allowed.contains(&id) {
            Self::Allowed
        } else if blocked.contains(&id) {
            Self::Blocked
        } else {
            Self::Neither
        }
    }

    /// The metric-friendly label for this preference.
    const fn label(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Blocked => "blocked",
            Self::Neither => "neither",
        }
    }

    /// A permission distinct from this preference, used to flip it: an allowed
    /// experience is blocked, anything else is allowed.
    const fn flip_to(self) -> ExperiencePermission {
        match self {
            Self::Allowed => ExperiencePermission::Block,
            Self::Blocked | Self::Neither => ExperiencePermission::Allow,
        }
    }

    /// The permission that restores this preference: re-`Allow`/`Block` a set one,
    /// or `Forget` to clear it back to neither.
    const fn restore_with(self) -> ExperiencePermission {
        match self {
            Self::Allowed => ExperiencePermission::Allow,
            Self::Blocked => ExperiencePermission::Block,
            Self::Neither => ExperiencePermission::Forget,
        }
    }
}

/// Reads and round-trips an experience preference over the experience caps.
#[derive(Debug)]
pub struct ExperiencePermissions;

impl GridTest for ExperiencePermissions {
    fn name(&self) -> &'static str {
        "experience-permissions"
    }

    fn description(&self) -> &'static str {
        "Read experience preferences over GetExperiences and round-trip one over ExperiencePreferences"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let fixture = ctx.experience();

            // Stock OpenSim ships no experience module, so with no configured
            // experience there is nothing to read or toggle — record partial up
            // front rather than block on capabilities the region never seeds.
            if fixture.is_none() && is_opensim(grid) {
                ctx.mark_partial(
                    "stock OpenSim ships no experience module, and no `experience` \
                     fixture is configured, so there are no experience preferences \
                     to read or set",
                );
                return Ok(());
            }

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // The request half: GetExperiences returns the agent's per-experience
            // allow/block preferences. Either list may legitimately be empty; the
            // asserted effect is only that the read capability answers.
            let request_started = Instant::now();
            session.send(Command::RequestExperiencePermissions).await?;
            let (allowed, blocked) = match session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::ExperiencePermissions { allowed, blocked } => {
                        Some((allowed.clone(), blocked.clone()))
                    }
                    _ => None,
                })
                .await
            {
                Ok(lists) => lists,
                Err(TestFailure::Timeout(_)) => {
                    ctx.mark_partial(
                        "grid did not answer GetExperiences (capability absent from \
                         the region seed)",
                    );
                    return Ok(());
                }
                Err(other) => return Err(other),
            };
            let request_rtt = request_started.elapsed();

            let allowed_count = i64::try_from(allowed.len()).unwrap_or(-1);
            let blocked_count = i64::try_from(blocked.len()).unwrap_or(-1);

            // The set half mutates a real, avatar-visible preference, so it runs
            // only against a stable fixture — never a discovered experience. With
            // no fixture, record the request-half lists and mark partial.
            let Some(anchor) = fixture else {
                let metrics = ctx.metrics();
                metrics.set(&count_metric("allowed"), allowed_count);
                metrics.set(&count_metric("blocked"), blocked_count);
                metrics.set_timing(&secs_metric("request_rtt"), request_rtt.as_secs_f64());
                metrics.set("set_round_trip", false);
                ctx.mark_partial(
                    "no `experience` fixture configured, so the set/restore \
                     round-trip is skipped; only the read (GetExperiences) half ran",
                );
                return Ok(());
            };

            // Record the fixture's starting preference so the run can restore it.
            let original = Preference::classify(anchor, &allowed, &blocked);
            let target = original.flip_to();

            // Flip the preference; the PUT/DELETE reply carries the full updated
            // lists, so assert the id moved into the target list and out of the
            // other.
            let set_started = Instant::now();
            let (set_allowed, set_blocked) = set_preference(session, anchor, target).await?;
            let set_rtt = set_started.elapsed();
            let after = Preference::classify(anchor, &set_allowed, &set_blocked);
            let expected = match target {
                ExperiencePermission::Allow => Preference::Allowed,
                ExperiencePermission::Block => Preference::Blocked,
                // `flip_to` only ever yields `Allow`/`Block`, never `Forget`.
                _ => Preference::Neither,
            };
            check(
                after == expected,
                "setting the experience preference did not move the id into the \
                 target list in the reply",
            )?;

            // Restore the original preference and assert the lists returned to
            // their starting classification, leaving the avatar as we found it.
            let restore_started = Instant::now();
            let (restored_allowed, restored_blocked) =
                set_preference(session, anchor, original.restore_with()).await?;
            let restore_rtt = restore_started.elapsed();
            let restored = Preference::classify(anchor, &restored_allowed, &restored_blocked);
            check(
                restored == original,
                "restoring the experience preference did not return the id to its \
                 original list",
            )?;

            let metrics = ctx.metrics();
            metrics.set(&count_metric("allowed"), allowed_count);
            metrics.set(&count_metric("blocked"), blocked_count);
            metrics.set("original_preference", original.label());
            metrics.set("set_permission", target.as_str());
            metrics.set("set_round_trip", true);
            metrics.set_timing(&secs_metric("request_rtt"), request_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("set_rtt"), set_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("restore_rtt"), restore_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// Set `permission` on `experience` and read back the reply's full
/// `(allowed, blocked)` preference lists.
///
/// `SetExperiencePermission` is a PUT (`Allow`/`Block`) or DELETE (`Forget`) to
/// `ExperiencePreferences`; both verbs reply with the agent's complete updated
/// lists, delivered as [`Event::ExperiencePermissions`].
async fn set_preference(
    session: &mut crate::context::Session,
    experience: ExperienceKey,
    permission: ExperiencePermission,
) -> Result<(Vec<ExperienceKey>, Vec<ExperienceKey>), TestFailure> {
    session
        .send(Command::SetExperiencePermission {
            experience_id: experience,
            permission,
        })
        .await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ExperiencePermissions { allowed, blocked } => {
                Some((allowed.clone(), blocked.clone()))
            }
            _ => None,
        })
        .await
}
