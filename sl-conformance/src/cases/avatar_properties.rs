//! Request *another* avatar's profile properties and decode the reply.
//!
//! Where `profile-edit-roundtrip` (Phase 7) edits the agent's *own* profile, this
//! reads a **different** avatar's profile — the everyday "open someone's profile
//! floater" lookup. A viewer issues [`Command::RequestAvatarProperties`] with the
//! target avatar's id; the grid's profile service answers with an
//! `AvatarPropertiesReply` ([`Event::AvatarProperties`]) carrying that avatar's
//! account-level facts (account creation date, partner, about text, flags), and —
//! on grids that send them — an `AvatarInterestsReply` ([`Event::AvatarInterests`])
//! alongside.
//!
//! The target need not be online: profile data is account/profile-service state,
//! not presence, so a single logged-in avatar can read any account's profile —
//! hence `1av`. The point of the case (vs `profile-edit-roundtrip`) is that the
//! reply describes *that other avatar*, so it asserts the reply's `avatar_id`
//! equals the requested target and differs from the logged-in primary, and that
//! the grid returned real account data (a non-empty `born_on`) rather than the
//! "profile not available" placeholder.
//!
//! `1av`, `[both]`. The "other avatar" id is resolved per grid: on OpenSim it
//! falls back to the local secondary test avatar (`Friend Tester`, a fixed-UUID
//! account on this workspace's grid), so OpenSim needs no configuration; Second
//! Life has no built-in second avatar, so the aditi run reads the `other_avatar`
//! configured in `fixtures.aditi.toml`. When that fixture is absent the aditi run
//! records `partial` rather than failing. OpenSim needs the UserProfiles module
//! enabled (see the setup memory); the aditi record is batched with the rest of
//! the deferred Aditi runs.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, fixtures, is_opensim, secs_metric,
};

/// How long to wait, after the properties reply, for the interests reply that
/// some grids send alongside it. Absence is recorded, not failed.
const INTERESTS_GRACE: Duration = Duration::from_secs(3);

/// Requests another avatar's profile properties and asserts the reply describes
/// that avatar with real account data.
#[derive(Debug)]
pub struct AvatarProperties;

impl GridTest for AvatarProperties {
    fn name(&self) -> &'static str {
        "avatar-properties"
    }

    fn description(&self) -> &'static str {
        "Request another avatar's profile properties and decode the reply"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Resolve the "other avatar" to look up. A configured fixture wins
            // (the Second Life path); otherwise OpenSim falls back to the local
            // secondary test avatar. With neither, the dataset is legitimately
            // incomplete (an aditi run with no fixture), so record partial.
            let grid = ctx.grid();
            let target = match ctx.other_avatar() {
                Some(other) => other,
                None if is_opensim(grid) => fixtures::opensim_secondary_avatar()?,
                None => {
                    ctx.mark_partial(
                        "no other-avatar fixture configured for this grid \
                         (set `other_avatar` in fixtures.<grid>.toml)",
                    );
                    return Ok(());
                }
            };

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let primary = session.agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login reported no agent id".to_owned())
            })?;

            // This case reads *another* avatar's profile, so the target must not
            // be the logged-in avatar itself (that would be the `profile-edit`
            // read path, not this one).
            check(
                target != primary,
                "target avatar must differ from the logged-in primary",
            )?;

            // Request the target's profile and await the matching reply.
            let started = Instant::now();
            session
                .send(Command::RequestAvatarProperties(target))
                .await?;
            let props = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::AvatarProperties(props) if props.avatar_id == target => {
                        Some((**props).clone())
                    }
                    _ => None,
                })
                .await?;
            let properties_rtt = started.elapsed();

            // The reply describes the requested avatar — not us — and carries
            // real account data (the grid's profile service answered, rather than
            // returning the "profile not available" placeholder with no born-on).
            check_eq("avatar_properties target", &props.avatar_id, &target)?;
            check(
                props.avatar_id != primary,
                "profile reply was attributed to the logged-in avatar, not the requested target",
            )?;
            check(
                !props.born_on.trim().is_empty(),
                "born_on was empty — the profile service returned no account data",
            )?;

            // The interests reply rides alongside the properties on grids that
            // send it (OpenSim does); record whether it arrived without failing
            // when it does not.
            let interests_received = session
                .wait_for(INTERESTS_GRACE, |event| match event {
                    Event::AvatarInterests(interests) if interests.avatar_id == target => Some(()),
                    _ => None,
                })
                .await
                .is_ok();

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("properties_rtt"), properties_rtt.as_secs_f64());
            metrics.set("target_avatar", target.to_string());
            metrics.set("born_on", props.born_on.clone());
            metrics.set("about_text_present", !props.about_text.trim().is_empty());
            metrics.set("interests_received", interests_received);
            metrics.set("partner_present", props.partner_id.is_some());
            metrics.set("profile_flags_hex", format!("{:#010x}", props.flags));
            Ok(())
        })
    }
}
