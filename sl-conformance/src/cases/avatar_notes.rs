//! Write and read back the agent's *private notes* about another avatar.
//!
//! Every viewer keeps a per-account, private free-text note about each other
//! avatar (the "My Notes" box on a profile floater). It is profile-service
//! state keyed on the pair (viewing agent, target avatar); the target is never
//! told and need not be online, so a single logged-in avatar exercises the
//! whole round-trip — hence `1av`.
//!
//! A viewer reads the current note with [`Command::RequestAvatarNotes`] (the
//! `avatarnotesrequest` `GenericMessage`), which a grid answers with an
//! `AvatarNotesReply` ([`Event::AvatarNotes`]); it writes a new note with
//! [`Command::UpdateAvatarNotes`] (`AvatarNotesUpdate`). The update message
//! carries no acknowledgement of its own, so the case verifies the edit by
//! polling a fresh read until the new note text appears. The note *toggles*
//! between two fixed markers keyed off what was just read, so every re-run is a
//! real, detectable change and an interrupted run self-heals; after asserting
//! the edit, the case writes the original note back so it leaves the profile as
//! it found it.
//!
//! **Live OpenSim finding (worked around, not fixed):** stock OpenSim leaves the
//! `avatarnotesrequest` `GenericMessage` *unanswered* — the same class of
//! unresponsive profile query the `picks-classifieds` case documented — and,
//! unlike picks, `AvatarNotesUpdate` volunteers no reply either, so the note can
//! never be read back on OpenSim. The case detects the silence (the initial read
//! times out), still exercises the write on the wire best-effort, and records
//! `partial` — the read-back round-trip is only assertable on a grid that
//! answers the query (Second Life). On grids that *do* answer, the full
//! toggle → write → re-read → assert → restore round-trip runs and is green.
//!
//! `1av`, `[both]`. The "other avatar" the note is about is resolved per grid:
//! OpenSim falls back to the local secondary test avatar (`Friend Tester`, a
//! fixed-UUID account on this workspace's grid), so no configuration is needed;
//! Second Life has no built-in second avatar, so the aditi run reads the
//! `other_avatar` configured in `fixtures.aditi.toml`, recording `partial` when
//! that fixture is absent. OpenSim needs the UserProfiles module enabled (see
//! the setup memory); the aditi record is batched with the rest of the deferred
//! Aditi runs.

use std::time::{Duration, Instant};

use sl_client_tokio::{AgentKey, Command, Event};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, fixtures, is_opensim, secs_metric,
};

/// The note text written on grids that do not answer the read query (stock
/// OpenSim), so the write is still exercised on the wire even though it can
/// never be read back there.
const NOTE_UNVERIFIABLE: &str = "sl-conformance avatar-notes (write-only, unverifiable read)";

/// One of two fixed note markers; the round-trip flips to whichever the current
/// value is *not*, so a fresh read can always detect the edit.
const NOTE_MARKER_A: &str = "sl-conformance avatar-notes marker A";
/// The other note marker (see [`NOTE_MARKER_A`]).
const NOTE_MARKER_B: &str = "sl-conformance avatar-notes marker B";

/// How long to keep re-reading the notes for the written value to appear.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait between re-reads while polling for the written value.
const VERIFY_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Writes the agent's private note about another avatar and reads it back.
#[derive(Debug)]
pub struct AvatarNotes;

impl GridTest for AvatarNotes {
    fn name(&self) -> &'static str {
        "avatar-notes"
    }

    fn description(&self) -> &'static str {
        "Write and read back the agent's private notes about another avatar"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Resolve the avatar the note is *about*. A configured fixture wins
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
            let own = session.agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login reported no agent id".to_owned())
            })?;

            // A note is *about another* avatar, so the target must not be the
            // logged-in avatar itself.
            check(
                target != own,
                "target avatar must differ from the logged-in primary",
            )?;

            // Read the current note. On a grid that answers the query the reply
            // seeds the toggle; stock OpenSim never answers (see the module docs),
            // so a timeout here is the "unanswered" signal, not a failure.
            let note_before = match read_notes(session, target).await {
                Ok(note) => note,
                Err(TestFailure::Timeout(_)) => {
                    // The grid does not answer notes reads. Still push a write so
                    // the `AvatarNotesUpdate` encoding is exercised on the wire,
                    // then record partial — the read-back cannot be verified here.
                    session
                        .send(Command::UpdateAvatarNotes {
                            target_id: target,
                            notes: NOTE_UNVERIFIABLE.to_owned(),
                        })
                        .await?;
                    let metrics = ctx.metrics();
                    metrics.set("target_avatar", target.to_string());
                    metrics.set("notes_read_answered", false);
                    metrics.set("note_written", NOTE_UNVERIFIABLE.to_owned());
                    ctx.mark_partial(
                        "grid left the avatarnotesrequest query unanswered (stock \
                         OpenSim); the note write was exercised but the read-back \
                         round-trip is unverifiable on this grid",
                    );
                    return Ok(());
                }
                Err(other) => return Err(other),
            };

            // The grid answers notes reads — run the full round-trip. Flip to the
            // other marker so every re-run is a real, detectable change.
            let new_note = if note_before == NOTE_MARKER_A {
                NOTE_MARKER_B
            } else {
                NOTE_MARKER_A
            };
            session
                .send(Command::UpdateAvatarNotes {
                    target_id: target,
                    notes: new_note.to_owned(),
                })
                .await?;

            // The update carries no ack, so confirm the edit by polling a fresh
            // read until the new note appears (or the timeout fires).
            let started = Instant::now();
            let read_back = poll_notes_until(
                session,
                target,
                |notes| notes == new_note,
                "edited note never appeared",
            )
            .await?;
            let reread_rtt = started.elapsed();
            check_eq("note after edit", &read_back, &new_note.to_owned())?;

            // Leave the note as it was found: write the original value back and
            // confirm the restore took (best-effort — a failed restore is
            // recorded, not fatal, since the marker toggle self-heals next run).
            session
                .send(Command::UpdateAvatarNotes {
                    target_id: target,
                    notes: note_before.clone(),
                })
                .await?;
            let restored = poll_notes_until(
                session,
                target,
                |notes| notes == note_before,
                "note not restored",
            )
            .await
            .is_ok();

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("reread_rtt"), reread_rtt.as_secs_f64());
            metrics.set("target_avatar", target.to_string());
            metrics.set("notes_read_answered", true);
            metrics.set("note_edited", new_note.to_owned());
            metrics.set("note_restored", restored);
            Ok(())
        })
    }
}

/// Requests the agent's note about `target` and returns the reply text.
async fn read_notes(session: &mut Session, target: AgentKey) -> Result<String, TestFailure> {
    session.send(Command::RequestAvatarNotes(target)).await?;
    session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::AvatarNotes { target_id, notes } if *target_id == target.uuid() => {
                Some(notes.clone())
            }
            _ => None,
        })
        .await
}

/// Re-reads the note about `target` until `predicate` holds, or fails with
/// `description` after [`VERIFY_TIMEOUT`].
async fn poll_notes_until<P>(
    session: &mut Session,
    target: AgentKey,
    mut predicate: P,
    description: &str,
) -> Result<String, TestFailure>
where
    P: FnMut(&str) -> bool,
{
    let start = Instant::now();
    loop {
        let notes = read_notes(session, target).await?;
        if predicate(&notes) {
            return Ok(notes);
        }
        if start.elapsed() >= VERIFY_TIMEOUT {
            return Err(TestFailure::Assertion(format!(
                "{description} after {VERIFY_TIMEOUT:?}"
            )));
        }
        tokio::time::sleep(VERIFY_POLL_INTERVAL).await;
    }
}
