//! The primary offers a teleport lure to the secondary; the secondary accepts
//! it and teleports to the offerer, arriving in the offerer's region.
//!
//! Where [`super::teleport_local_phases`] and [`super::teleport_cross_region`]
//! prove a *self-initiated* teleport walks its phase sequence, this two-avatar
//! case proves the *invited* teleport: one avatar offers a lure and another
//! accepts it, driving the same teleport handover but provoked by the offer
//! rather than a `TeleportLocationRequest` the accepter chose itself.
//!
//! A lure offer is a `StartLure` from the offerer, which the grid delivers to
//! the target as an `ImprovedInstantMessage` with the
//! [`ImDialog::LureUser`] dialog. The offer IM's
//! [`id`](sl_client_tokio::InstantMessage::id) is the
//! lure id (on OpenSim a *fake parcel id* encoding the offerer's region handle
//! and position — `LureModule.OnStartLure` builds it from the offerer's
//! `AbsolutePosition`), which the target quotes back in a `TeleportLureRequest`
//! to accept. OpenSim's `LureModule.OnTeleportLureRequest` parses that fake
//! parcel id back into a region handle + position and calls
//! `RequestTeleportLocation`, so the accepter teleports to the offerer's
//! location — an intra-region `TeleportLocal` when the two avatars share a
//! region (the OpenSim default, both logging in to the same region) or a
//! cross-region handover when they do not.
//!
//! Sequence (primary = offerer, secondary = accepter):
//!
//! 1. Both avatars log in and become active.
//! 2. The primary [`Command::OfferTeleport`]s the secondary with a distinct
//!    per-run message.
//! 3. The secondary — a separate session — observes the matching
//!    [`Event::InstantMessageReceived`] with [`ImDialog::LureUser`], attributed
//!    to the primary and carrying the exact message, and takes the lure id from
//!    its [`id`](sl_client_tokio::InstantMessage::id).
//! 4. The secondary [`Command::AcceptTeleportLure`]s that lure id, collects the
//!    teleport phases until arrival, and asserts the sequence opens with
//!    *Starting* and ends at a terminal arrival phase (`TeleportLocal` for the
//!    shared-region case, or a `RegionChanged` handover otherwise).
//! 5. The case confirms the secondary's current region handle is now the
//!    primary's region — it teleported *to the offerer*, the point the lure id
//!    encodes.
//!
//! OpenSim's `OnTeleportLureRequest` sends the offerer nothing back (no
//! `IM_LURE_ACCEPTED`), so the acceptance is observable only on the accepter
//! side, as the completed teleport. Records the offer-delivery latency, the
//! observed phase sequence and progress-update count, the arrival kind
//! (`local` / `region-changed`), and the request-to-arrival time.
//!
//! `2av`. `[opensim]` only; the Aditi variant is deferred to Phase Z pending
//! its Aditi run. The flow is plain LLUDP `StartLure` /
//! `ImprovedInstantMessage` / `TeleportLureRequest`, and no new client code —
//! the [`Command::OfferTeleport`] / [`Command::AcceptTeleportLure`] surface and
//! the lure-accept teleport handover already existed from earlier IM and
//! teleport work.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ImDialog, LureId, RegionHandle};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, secs_metric};

/// One teleport phase observed on the accepter's circuit between the accepted
/// lure and arrival — mirroring the phases [`super::teleport_local_phases`]
/// models. The arrival is `TeleportLocal` when the two avatars share a region
/// or a `RegionChanged` handover when the lure crosses a boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The simulator acknowledged the request and began the teleport
    /// (`TeleportStart` → [`Event::TeleportStarted`]).
    Started,
    /// A progress update arrived mid-teleport (`TeleportProgress`).
    Progress,
    /// The intra-region teleport completed without a circuit change
    /// (`TeleportLocal`).
    Local,
    /// The destination region's handshake completed after a border crossing
    /// ([`Event::RegionChanged`]).
    RegionChanged,
}

impl Phase {
    /// The short label recorded in the `phase_sequence` metric.
    const fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Progress => "progress",
            Self::Local => "local",
            Self::RegionChanged => "region-changed",
        }
    }

    /// Whether this phase terminates the teleport (arrival), ending observation.
    const fn is_terminal(self) -> bool {
        matches!(self, Self::Local | Self::RegionChanged)
    }
}

/// Offers a teleport lure from the primary to the secondary, accepts it, and
/// asserts the accepter teleports to the offerer's region.
#[derive(Debug)]
pub struct TeleportOfferAccept;

impl GridTest for TeleportOfferAccept {
    fn name(&self) -> &'static str {
        "teleport-offer-accept"
    }

    fn description(&self) -> &'static str {
        "Primary offers a teleport lure; secondary accepts and teleports to the offerer"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in and active before a lure can be
            // routed between them: the primary to offer, the secondary to accept.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // Capture the secondary's agent id while it is borrowed, then release
            // the borrow before reborrowing the primary. The offer is addressed to
            // the secondary and attributed to the primary; the accepter ends up in
            // the primary's region, so capture that handle too.
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary = ctx.primary();
            let primary_id = primary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;
            let primary_handle = primary.region_handle().ok_or_else(|| {
                TestFailure::Assertion("primary login reported no region handle".to_owned())
            })?;

            // Distinct per-run message so a leftover lure from an aborted run
            // cannot be mistaken for this run's, and concurrent runs do not
            // collide. OpenSim forwards the message verbatim in the delivered IM.
            let message = format!("sl-conformance teleport-offer-accept {primary_id}");

            // --- Primary offers the teleport lure to the secondary; time the
            // delivery of the resulting IM.
            let offered_at = Instant::now();
            primary
                .send(Command::OfferTeleport {
                    targets: vec![secondary_id],
                    message: message.clone(),
                })
                .await?;

            // --- Secondary observes the lure offer from the primary and takes the
            // lure id from the offer IM. Filtering on the offering agent, the
            // `LureUser` dialog, and the exact message ignores any unrelated
            // background IM.
            let match_message = message.clone();
            let (lure_id, offer_message) = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, move |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == primary_id
                            && im.dialog == ImDialog::LureUser
                            && im.message == match_message =>
                    {
                        Some((LureId::from(im.id), im.message.clone()))
                    }
                    _ => None,
                })
                .await?;
            let offer_rtt = offered_at.elapsed();

            // The offer arrived attributed to the primary (matched by the
            // predicate) and carried our exact message verbatim.
            check_eq("offer message", &offer_message, &message)?;

            // --- Secondary accepts the lure, driving the teleport handover, and
            // collects the phases until it arrives (a terminal phase) or a
            // TeleportFailed fails the case.
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            let accepted_at = Instant::now();
            secondary
                .send(Command::AcceptTeleportLure { lure_id })
                .await?;

            let mut phases: Vec<Phase> = Vec::new();
            loop {
                let phase = secondary
                    .wait_for(REGION_TIMEOUT, |event| match event {
                        Event::TeleportStarted => Some(Ok(Phase::Started)),
                        Event::TeleportProgress { .. } => Some(Ok(Phase::Progress)),
                        Event::TeleportLocal => Some(Ok(Phase::Local)),
                        Event::RegionChanged { .. } => Some(Ok(Phase::RegionChanged)),
                        Event::TeleportFailed { reason, .. } => Some(Err(reason.clone())),
                        _ => None,
                    })
                    .await?;
                match phase {
                    Ok(phase) => {
                        let terminal = phase.is_terminal();
                        phases.push(phase);
                        if terminal {
                            break;
                        }
                    }
                    Err(reason) => {
                        return Err(TestFailure::Assertion(format!(
                            "accepted lure teleport failed: {reason}"
                        )));
                    }
                }
            }
            let teleport_rtt = accepted_at.elapsed();

            // The teleport must have opened with the Starting phase and ended at an
            // arrival (TeleportLocal for the shared-region case, or a RegionChanged
            // handover otherwise).
            check(
                phases.first() == Some(&Phase::Started),
                "expected the accepted teleport to begin with a Starting (TeleportStart) phase",
            )?;
            let arrival = phases
                .last()
                .copied()
                .ok_or_else(|| TestFailure::Assertion("no teleport phases observed".to_owned()))?;
            check(
                arrival.is_terminal(),
                "expected the accepted teleport to end at an arrival phase \
                 (TeleportLocal / RegionChanged)",
            )?;

            // The accepter teleported *to the offerer*: the lure id encodes the
            // offerer's region handle, so the accepter's current region must now be
            // the primary's.
            let current: RegionHandle = secondary.region_handle().ok_or_else(|| {
                TestFailure::Assertion("no region handle after the accepted teleport".to_owned())
            })?;
            check_eq("arrival_region_handle", &current, &primary_handle)?;

            let progress_updates = phases.iter().filter(|p| **p == Phase::Progress).count();
            let sequence = phases
                .iter()
                .map(|p| p.label())
                .collect::<Vec<_>>()
                .join(",");

            let metrics = ctx.metrics();
            metrics.set("phase_sequence", sequence);
            metrics.set("arrival", arrival.label());
            metrics.set(
                &count_metric("progress_updates"),
                i64::try_from(progress_updates).unwrap_or(-1),
            );
            metrics.set_timing(&secs_metric("offer_rtt"), offer_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("teleport"), teleport_rtt.as_secs_f64());
            Ok(())
        })
    }
}
