//! A peer's instant message, sent while the recipient is logged out, is stored
//! by the grid and replayed as an *offline* message when the recipient logs back
//! in and explicitly fetches it.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, ImDialog};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, is_aditi, secs_metric,
};

/// A short settle after the recipient disconnects, giving the grid time to drop
/// the presence so the sender's instant message is routed to offline storage
/// rather than to a session that is on its way down.
const OFFLINE_SETTLE: Duration = Duration::from_secs(3);

/// How long to keep collecting replayed offline IMs after a fetch before
/// concluding the store is drained. Short, because loopback replay is sub-second
/// — this only bounds the "no more messages" case.
const DRAIN_WINDOW: Duration = Duration::from_secs(3);

/// The text fragment the V2 offline-IM module returns to the *sender* when the
/// recipient is offline and the message is stored (see
/// `OfflineIMRegionModule.UndeliveredMessage`). Used as the synchronisation
/// point that proves the message reached offline storage before the recipient
/// returns.
const STORED_REPLY_FRAGMENT: &str = "Message saved";

/// Second Life's wording of the same stored-for-later confirmation — the
/// simulator sends the sender exactly "User not online - message will be
/// stored and delivered later." (the reference viewer matches this string
/// verbatim as `NOT_ONLINE_MSG`).
const STORED_REPLY_FRAGMENT_SL: &str = "not online";

/// How long to wait for the `ReadOfflineMsgs` capability to appear after the
/// relogin before falling back to the legacy UDP trigger. The capability map
/// fills asynchronously shortly after the region handshake.
const CAP_WAIT: Duration = Duration::from_secs(15);

/// The capability that serves the stored offline-IM history on Second Life
/// (cap names are used as string literals throughout the cases).
const READ_OFFLINE_MSGS_CAP: &str = "ReadOfflineMsgs";

/// A peer's IM, sent while the recipient is offline, is stored by the grid and
/// replayed as an offline message when the recipient returns and fetches it.
///
/// This is the offline-delivery counterpart of [`super::im_1to1`]: where that
/// case proves a live 1:1 IM is delivered to a logged-in recipient, this one
/// proves the store-and-forward path. The flow needs the recipient *absent* when
/// the message is sent, so the case drives a mid-run logout/login on the primary:
///
/// 1. Both avatars log in; the case captures their agent ids and, while the
///    primary is still online, drains any offline messages an earlier
///    interrupted run may have left stored for it (the store deletes on fetch,
///    so a clean run leaves nothing) — establishing an empty baseline.
/// 2. The primary (recipient) [disconnects](crate::context::Session::disconnect)
///    — logs out but keeps the identity needed to log back in.
/// 3. The secondary (sender) [`Command::InstantMessage`]s the now-offline
///    primary. The grid cannot deliver it, so it stores it and replies to the
///    sender with a "<…> Message saved" system IM — observed here as the proof
///    the message reached offline storage.
/// 4. The primary [relogs in](crate::context::Session::relogin) (on OpenSim this
///    inherits the "already logged in" retry that evicts the stale presence the
///    disconnect left behind).
/// 5. The primary issues [`Command::RetrieveInstantMessages`] (the legacy UDP
///    `RetrieveInstantMessages` trigger; the modern Second Life path is the
///    `ReadOfflineMsgs` capability) and observes the stored IM replayed as an
///    [`Event::InstantMessageReceived`] with [`InstantMessage::offline`] set.
///
/// [`InstantMessage::offline`]: sl_client_tokio::InstantMessage::offline
///
/// The replayed IM is matched on the sender's agent id and the exact marker text
/// so an unrelated background IM cannot satisfy it, and `offline` must be `true`
/// — distinguishing a *replayed stored* message from a live one. The fetch is
/// explicit (the client never auto-requests offline IMs), so this also confirms
/// the fetch command round-trips.
///
/// `2av`, `[both]`, per-grid routing in two places. The *stored* confirmation
/// wording differs: OpenSim's V2 module says "Message saved.", Second Life
/// says "User not online - message will be stored and delivered later." — the
/// wait accepts either. The *fetch* differs too: OpenSim answers the legacy
/// UDP `RetrieveInstantMessages` trigger (and offers no capability; needs the
/// "Offline Message Module V2" on the test grid), while Second Life serves
/// the store over the `ReadOfflineMsgs` capability and ignores the UDP
/// trigger — on aditi the case waits (bounded) for the capability after the
/// relogin and GETs it ([`Command::RequestOfflineMessages`]). Second Life may
/// also replay the store on its own right after login, so the post-relogin
/// handshake drain captures an early marker replay instead of discarding it,
/// and the explicit fetch is skipped when the replay already arrived
/// (recorded via the `auto_delivered_at_login` metric).
/// [`Session::relogin`](crate::context::Session::relogin) waits out the aditi
/// login cooldown rather than bypassing it, so the mid-run relogin slows an
/// aditi run but does not block it.
#[derive(Debug)]
pub struct OfflineMsgFetch;

impl GridTest for OfflineMsgFetch {
    fn name(&self) -> &'static str {
        "offline-msg-fetch"
    }

    fn description(&self) -> &'static str {
        "A peer's IM sent while the recipient is offline is stored and replayed as an offline IM"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            // Both avatars must be logged in and active before anything is routed
            // between them.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // Capture both ids up front: the recipient's account id is stable
            // across its logout/login, so the sender can address the offline
            // message and the recipient can recognise the replayed one.
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            // Tag the marker with a per-run nonce as well as the sender's id, so
            // a stale offline message left by an earlier *failed* run (the store
            // deletes on fetch, so a clean run leaves nothing behind) can never
            // satisfy this run's predicate.
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos());
            let marker = format!("sl-conformance offline-msg-fetch {secondary_id} {nonce}");

            // Start from a clean slate: drain any offline messages already stored
            // for the recipient (a leftover from an earlier interrupted run)
            // while it is still online. A fetch makes the grid replay *and
            // delete* them, so after this the store is empty and the later
            // fetch can only return the message this run creates. The baseline
            // count is recorded, not asserted: a contaminated store is worth
            // seeing but is not this run's failure. Same per-grid routing as
            // the main fetch below (SL ignores the UDP trigger).
            if is_aditi(grid) && ctx.primary().cap(READ_OFFLINE_MSGS_CAP).is_some() {
                ctx.primary().send(Command::RequestOfflineMessages).await?;
            } else {
                ctx.primary().send(Command::RetrieveInstantMessages).await?;
            }
            let mut baseline = 0_i64;
            loop {
                match ctx
                    .primary()
                    .wait_for(DRAIN_WINDOW, |event| match event {
                        Event::InstantMessageReceived(im) if im.offline => Some(()),
                        _ => None,
                    })
                    .await
                {
                    Ok(()) => baseline = baseline.saturating_add(1),
                    Err(TestFailure::Timeout(_message)) => break,
                    Err(other) => return Err(other),
                }
            }

            // The recipient goes offline (but stays reusable for the relogin).
            ctx.primary().disconnect().await?;
            check(
                !ctx.primary().is_connected(),
                "primary should be disconnected after disconnect()",
            )?;
            // Let the grid finish dropping the presence so the next message is
            // routed to offline storage, not to the closing session.
            tokio::time::sleep(OFFLINE_SETTLE).await;

            // The sender messages the now-offline recipient; the grid stores it
            // and replies "User is not logged in. Message saved." to the sender.
            let sent_at = Instant::now();
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::InstantMessage {
                    to_agent_id: primary_id,
                    message: marker.clone(),
                })
                .await?;

            // The "Message saved" confirmation comes back from the recipient's
            // id (the offline module synthesises it as `From: <recipient>`), so
            // it proves the message reached storage before the recipient returns.
            let stored_reply = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == primary_id
                            && (im.message.contains(STORED_REPLY_FRAGMENT)
                                || im.message.contains(STORED_REPLY_FRAGMENT_SL)) =>
                    {
                        Some((**im).clone())
                    }
                    _ => None,
                })
                .await?;
            let store_confirm = sent_at.elapsed();
            check_eq(
                "stored-reply to_agent_id",
                &stored_reply.to_agent_id,
                &secondary_id,
            )?;

            // The recipient returns. On OpenSim the disconnect left a stale
            // presence; the relogin's first attempt evicts it and retries.
            ctx.primary().relogin().await?;
            check(
                ctx.primary().is_connected(),
                "primary should be connected after relogin()",
            )?;
            // Second Life replays the stored IM on its own shortly after the
            // login (no explicit fetch needed), so the region-handshake wait
            // must not discard it: one combined drain waits for the handshake
            // while capturing an early marker replay.
            let mut auto_replayed = None;
            ctx.primary()
                .wait_for(REGION_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == secondary_id && im.message == marker =>
                    {
                        auto_replayed = Some((**im).clone());
                        None
                    }
                    Event::RegionHandshakeComplete | Event::RegionChanged { .. } => Some(()),
                    _ => None,
                })
                .await?;

            // Fetch stored offline messages and observe the marker replayed as
            // an offline IM — unless the grid already auto-delivered it above.
            // OpenSim answers the legacy UDP `RetrieveInstantMessages` trigger
            // (and offers no capability); Second Life serves the store over
            // the `ReadOfflineMsgs` capability (the UDP trigger goes
            // unanswered), so on aditi wait (bounded) for the cap the
            // asynchronous caps fetch delivers, then GET it.
            let fetch_at = Instant::now();
            let mut fetched_via_cap = false;
            let auto_delivered = auto_replayed.is_some();
            let replayed = if let Some(replayed) = auto_replayed {
                replayed
            } else {
                if is_aditi(grid) {
                    let cap_wait_started = Instant::now();
                    while ctx.primary().cap(READ_OFFLINE_MSGS_CAP).is_none()
                        && cap_wait_started.elapsed() < CAP_WAIT
                    {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    fetched_via_cap = ctx.primary().cap(READ_OFFLINE_MSGS_CAP).is_some();
                }
                if fetched_via_cap {
                    ctx.primary().send(Command::RequestOfflineMessages).await?;
                } else {
                    ctx.primary().send(Command::RetrieveInstantMessages).await?;
                }
                ctx.primary()
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::InstantMessageReceived(im)
                            if im.from_agent_id == secondary_id && im.message == marker =>
                        {
                            Some((**im).clone())
                        }
                        _ => None,
                    })
                    .await?
            };
            let fetch_rtt = fetch_at.elapsed();

            // It must be the stored message replayed as *offline*, an ordinary
            // 1:1 IM addressed to the recipient. The `offline` flag is what
            // separates a replayed stored message from a live one.
            check(replayed.offline, "replayed IM should be flagged offline")?;
            check_eq("replayed dialog", &replayed.dialog, &ImDialog::Message)?;
            check_eq(
                "replayed from_agent_id",
                &replayed.from_agent_id,
                &secondary_id,
            )?;
            check_eq("replayed to_agent_id", &replayed.to_agent_id, &primary_id)?;
            check_eq("replayed message", &replayed.message, &marker)?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("store_confirm"), store_confirm.as_secs_f64());
            metrics.set_timing(&secs_metric("fetch_rtt"), fetch_rtt.as_secs_f64());
            metrics.set(&count_metric("offline_messages"), 1_i64);
            metrics.set("fetched_via_cap", fetched_via_cap);
            metrics.set("auto_delivered_at_login", auto_delivered);
            metrics.set(&count_metric("baseline_offline"), baseline);
            Ok(())
        })
    }
}
