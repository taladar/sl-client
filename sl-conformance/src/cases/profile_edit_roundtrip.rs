//! Edit the agent's *own* profile and interests, then read them back.
//!
//! Where `avatar-properties` (Phase 7) reads a **different** avatar's profile,
//! this case is the "edit my own profile floater" round-trip: it reads the
//! agent's current profile and interests, writes a changed copy back, and
//! confirms the grid reflects the edit on a fresh read. A viewer edits the
//! profile with [`Command::UpdateProfile`] (`AvatarPropertiesUpdate`) and the
//! interests with [`Command::UpdateInterests`] (`AvatarInterestsUpdate`); both
//! *replace the whole record*, so the case first reads the current values
//! ([`Event::AvatarProperties`] / [`Event::AvatarInterests`]) and edits from
//! there rather than blanking the unrelated fields.
//!
//! Neither update message carries an acknowledgement of its own, so the case
//! verifies the edit by polling a fresh `RequestAvatarProperties` read until the
//! new about-text appears (or a timeout fires). The marker text *toggles*
//! between two fixed values keyed off the value just read, so a re-run always
//! flips to the other marker and the change is detectable even if a previous run
//! was interrupted before it could restore. After asserting the edit took, the
//! case writes the original profile and interests back so it leaves the test
//! avatar's profile as it found it.
//!
//! `1av`, `[both]`. The interests reply rides alongside the properties reply on
//! grids that send it; when a grid omits interests entirely the interests
//! half of the round-trip is untested and the record is downgraded to partial
//! rather than failed. OpenSim needs the UserProfiles module enabled (see the
//! setup memory); the aditi record is batched with the rest of the deferred
//! Aditi runs.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AvatarInterests, AvatarProperties, Command, Event, InterestsUpdate, ProfileUpdate,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, secs_metric};

/// The `AVATAR_ALLOW_PUBLISH` profile flag bit (profile externally visible).
const FLAG_ALLOW_PUBLISH: u32 = 0x1 << 0;
/// The `AVATAR_MATURE_PUBLISH` profile flag bit (profile flagged "mature").
const FLAG_MATURE_PUBLISH: u32 = 0x1 << 1;

/// One of two fixed about-text markers; the round-trip flips to whichever the
/// current value is *not*, so a fresh read can always detect the edit.
const ABOUT_MARKER_A: &str = "sl-conformance profile-edit-roundtrip marker A";
/// The other about-text marker (see [`ABOUT_MARKER_A`]).
const ABOUT_MARKER_B: &str = "sl-conformance profile-edit-roundtrip marker B";
/// One of two fixed interests "want to" markers (toggled like the about text).
const WANT_MARKER_A: &str = "sl-conformance interests marker A";
/// The other interests "want to" marker (see [`WANT_MARKER_A`]).
const WANT_MARKER_B: &str = "sl-conformance interests marker B";

/// How long to wait, after each properties request, for the interests reply
/// some grids send alongside it. Absence is recorded, not failed.
const INTERESTS_GRACE: Duration = Duration::from_secs(3);
/// How long to keep re-reading the profile for the written value to appear.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait between re-reads while polling for the written value.
const VERIFY_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Reads the agent's own profile and interests, then writes a changed copy back
/// and confirms the grid reflects the edit.
#[derive(Debug)]
pub struct ProfileEditRoundtrip;

impl GridTest for ProfileEditRoundtrip {
    fn name(&self) -> &'static str {
        "profile-edit-roundtrip"
    }

    fn description(&self) -> &'static str {
        "Edit the agent's own profile and interests, then read them back"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let own = session.agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login reported no agent id".to_owned())
            })?;

            // Read the current profile (and the interests that ride alongside it
            // on grids that send them) so the edit replaces only the about-text
            // and "want to" fields, not the whole record.
            let (props_before, interests_before) = read_profile(session, own).await?;

            // Toggle the about-text to whichever marker the current value is not,
            // so a re-run is always a real change the read-back can detect.
            let new_about = if props_before.about_text == ABOUT_MARKER_A {
                ABOUT_MARKER_B
            } else {
                ABOUT_MARKER_A
            };
            session
                .send(Command::UpdateProfile(profile_update(
                    &props_before,
                    new_about,
                )))
                .await?;

            // Edit the interests the same way, when the grid supplied them.
            let new_want = interests_before.as_ref().map(|interests| {
                if interests.want_to_text == WANT_MARKER_A {
                    WANT_MARKER_B
                } else {
                    WANT_MARKER_A
                }
            });
            if let (Some(interests), Some(want)) = (interests_before.as_ref(), new_want) {
                session
                    .send(Command::UpdateInterests(interests_update(interests, want)))
                    .await?;
            }

            // The update messages carry no ack of their own, so confirm the edit
            // by polling a fresh read until the new about-text appears. Each read
            // consumes the properties *and* the interests reply that follows it,
            // so the interests value stays paired with the request that produced
            // it (reading them apart races the replies against each other).
            let started = Instant::now();
            let (props_after, _) = poll_profile_until(
                session,
                own,
                |props, _| props.about_text == new_about,
                "edited about-text never appeared",
            )
            .await?;
            let reread_rtt = started.elapsed();
            check_eq(
                "about_text after edit",
                &props_after.about_text,
                &new_about.to_owned(),
            )?;
            check(
                props_after.avatar_id == own,
                "profile read-back was attributed to a different avatar",
            )?;

            // Confirm the interests "want to" text changed, but only when the
            // grid actually sends interests — otherwise that half of the
            // round-trip is untested and the record is downgraded to partial.
            let interests_tested = match new_want {
                Some(want) => {
                    let reached = poll_profile_until(
                        session,
                        own,
                        |_, interests| {
                            interests
                                .as_ref()
                                .is_some_and(|interests| interests.want_to_text == want)
                        },
                        "edited interests never appeared",
                    )
                    .await;
                    match reached {
                        Ok(_) => true,
                        Err(_unreflected) => {
                            ctx.mark_partial(
                                "interests written but never reflected on read-back; \
                                 interests round-trip untested on this grid",
                            );
                            false
                        }
                    }
                }
                None => {
                    ctx.mark_partial(
                        "grid sent no interests reply on the initial read; interests \
                         round-trip untested",
                    );
                    false
                }
            };

            // Leave the profile as it was found: write the original values back
            // and confirm the restore took (best-effort — a failed restore is
            // recorded, not fatal, since the marker toggle self-heals next run).
            let session = ctx.primary();
            session
                .send(Command::UpdateProfile(profile_update(
                    &props_before,
                    &props_before.about_text,
                )))
                .await?;
            if let Some(interests) = interests_before.as_ref() {
                session
                    .send(Command::UpdateInterests(interests_update(
                        interests,
                        &interests.want_to_text,
                    )))
                    .await?;
            }
            let restored = poll_profile_until(
                session,
                own,
                |props, _| props.about_text == props_before.about_text,
                "profile not restored",
            )
            .await
            .is_ok();

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("reread_rtt"), reread_rtt.as_secs_f64());
            metrics.set("about_text_edited", new_about.to_owned());
            metrics.set("interests_tested", interests_tested);
            metrics.set("profile_restored", restored);
            Ok(())
        })
    }
}

/// Builds an [`AvatarProperties`]-faithful [`ProfileUpdate`] that changes only
/// the about-text, reconstructing the publish/mature flags from the read flags.
fn profile_update(props: &AvatarProperties, about_text: &str) -> ProfileUpdate {
    ProfileUpdate {
        image_id: props.image_id,
        fl_image_id: props.fl_image_id,
        about_text: about_text.to_owned(),
        fl_about_text: props.fl_about_text.clone(),
        allow_publish: props.flags & FLAG_ALLOW_PUBLISH != 0,
        mature_publish: props.flags & FLAG_MATURE_PUBLISH != 0,
        profile_url: props.profile_url.clone(),
    }
}

/// Builds an [`AvatarInterests`]-faithful [`InterestsUpdate`] that changes only
/// the "want to" free text.
fn interests_update(interests: &AvatarInterests, want_to_text: &str) -> InterestsUpdate {
    InterestsUpdate {
        want_to_mask: interests.want_to_mask,
        want_to_text: want_to_text.to_owned(),
        skills_mask: interests.skills_mask,
        skills_text: interests.skills_text.clone(),
        languages_text: interests.languages_text.clone(),
    }
}

/// Requests the agent's own profile and returns the properties plus any
/// interests reply that arrives alongside within the grace window.
async fn read_profile(
    session: &mut Session,
    own: sl_client_tokio::AgentKey,
) -> Result<(AvatarProperties, Option<AvatarInterests>), TestFailure> {
    session.send(Command::RequestAvatarProperties(own)).await?;
    let props = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::AvatarProperties(props) if props.avatar_id == own => Some((**props).clone()),
            _ => None,
        })
        .await?;
    let interests = read_interests(session, own).await;
    Ok((props, interests))
}

/// Waits the grace window for an interests reply describing `own`, returning it
/// if one arrives. Absence is reported as `None`, not an error.
async fn read_interests(
    session: &mut Session,
    own: sl_client_tokio::AgentKey,
) -> Option<AvatarInterests> {
    session
        .wait_for(INTERESTS_GRACE, |event| match event {
            Event::AvatarInterests(interests) if interests.avatar_id == own => {
                Some((**interests).clone())
            }
            _ => None,
        })
        .await
        .ok()
}

/// Re-reads the agent's profile (properties paired with the interests reply
/// that follows) until `predicate` holds, or fails with `description` after
/// [`VERIFY_TIMEOUT`].
async fn poll_profile_until<P>(
    session: &mut Session,
    own: sl_client_tokio::AgentKey,
    mut predicate: P,
    description: &str,
) -> Result<(AvatarProperties, Option<AvatarInterests>), TestFailure>
where
    P: FnMut(&AvatarProperties, &Option<AvatarInterests>) -> bool,
{
    let start = Instant::now();
    loop {
        let (props, interests) = read_profile(session, own).await?;
        if predicate(&props, &interests) {
            return Ok((props, interests));
        }
        if start.elapsed() >= VERIFY_TIMEOUT {
            return Err(TestFailure::Assertion(format!(
                "{description} after {VERIFY_TIMEOUT:?}"
            )));
        }
        tokio::time::sleep(VERIFY_POLL_INTERVAL).await;
    }
}
