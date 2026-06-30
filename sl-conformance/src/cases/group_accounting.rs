//! Query a group's financial accounting: its summary, itemised details, and
//! transaction log.
//!
//! These are the three requests a viewer issues for the group "Land & L$"
//! floater, one per tab:
//!
//! - [`Command::RequestGroupAccountSummary`] → [`Event::GroupAccountSummary`]:
//!   the planning summary — current balance, interval credits/debits, and the
//!   per-category tax estimates.
//! - [`Command::RequestGroupAccountDetails`] → [`Event::GroupAccountDetails`]:
//!   the itemised charge lines for an interval.
//! - [`Command::RequestGroupAccountTransactions`] →
//!   [`Event::GroupAccountTransactions`]: the dated transaction log.
//!
//! Each is an `S32`-parameterised reliable request keyed by a client-chosen
//! `RequestID` echoed back in the reply for correlation; the case mints a fresh
//! [`GroupRequestId`] per request and pairs each reply by it. The interval
//! parameters mirror the reference viewer exactly
//! (`indra/newview/llpanelgrouplandmoney.cpp`): the summary (planning) tab asks
//! for a 7-day interval at offset 0, the details tab a 1-day interval at offset
//! 0, and the transactions (sales) tab a 7-day interval at offset 0.
//!
//! The group comes from [`support::membership_group`] (index 0): on OpenSim a
//! throwaway group created per run (free; the primary becomes founder/owner, so
//! it holds the `Accountable` power these requests need), or on Second Life a
//! reused pre-made group from [`crate::fixtures`] (avoiding the per-run L$100 and
//! a founder slot — the SL run additionally needs the primary to hold the
//! group's `Accountable` power). The case only reads the group, leaving it as
//! found.
//!
//! `1av`. Listed `[both]`, but the two grids exercise different halves:
//!
//! - **OpenSim has no group-accounting backend.** The simulator parses and
//!   acks all three reliable requests (`LLClientView` fires
//!   `OnGroupAccountSummaryRequest` and siblings) but no region module
//!   subscribes to those events, so it never sends a reply — the
//!   `SendGroupAccounting*` methods exist but are dead code. The OpenSim run
//!   therefore proves the client *encodes and transmits* all three requests in
//!   a form a real simulator accepts: it watches the circuit past the
//!   reliable-retransmit budget via keep-alive pings (an un-acked request would
//!   exhaust its retransmits and close the circuit, the same
//!   acceptance-by-absence-of-failure check [`super::throttle_set`] uses) and
//!   then marks the dataset partial, since no reply data is observable.
//! - The **reply assertions are the Second Life variant** (deferred with the
//!   Aditi batch): wait for all three replies, correlate each by its echoed
//!   request id and group id, and assert the echoed interval parameters.

use std::time::{Duration, Instant, SystemTime};

use sl_client_tokio::{Command, Event, GroupRequestId, Uuid};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    self, GroupSource, REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, count_metric, is_opensim,
    secs_metric,
};

/// Interval length the summary (planning) request asks for, in days — the
/// reference viewer's `SUMMARY_INTERVAL` (one week).
const SUMMARY_INTERVAL_DAYS: i32 = 7;

/// Interval length the details request asks for, in days — the reference
/// viewer's `DETAILS_INTERVAL` (one day).
const DETAILS_INTERVAL_DAYS: i32 = 1;

/// Interval length the transactions (sales) request asks for, in days — the
/// reference viewer's sales tab uses `SUMMARY_INTERVAL` (one week).
const TRANSACTIONS_INTERVAL_DAYS: i32 = 7;

/// Which interval to query: 0 = current. The viewer's planning tab fixes this at
/// 0, and the details/sales tabs start at 0 before the user pages backward.
const CURRENT_INTERVAL: i32 = 0;

/// Settle after creating a throwaway group so its records are persisted before
/// the accounting requests go out. Only applied on the created (OpenSim) path.
const CREATE_SETTLE: Duration = Duration::from_secs(2);

/// How long to keep observing the circuit after the requests on the OpenSim
/// path. OpenSim acks but never answers them, so acceptance is the *absence* of
/// failure: a reliable request the simulator never acked would be retransmitted
/// to exhaustion (`MAX_RESEND_ATTEMPTS` 6 × `RESEND_TIMEOUT` 1.5 s ≈ 9 s) and
/// close the root circuit. This window covers that budget with margin.
const ACCEPT_WINDOW: Duration = Duration::from_secs(15);

/// Per-ping timeout while spanning the [`ACCEPT_WINDOW`]. The keep-alive ping
/// fires every 5 s, so a healthy circuit always answers well inside this; a ping
/// that never arrives (or a `Disconnected`) fails the wait — the un-accepted
/// signal.
const PING_WAIT: Duration = Duration::from_secs(20);

/// Queries a group's accounting summary, details, and transaction log.
#[derive(Debug)]
pub struct GroupAccounting;

impl GridTest for GroupAccounting {
    fn name(&self) -> &'static str {
        "group-accounting"
    }

    fn description(&self) -> &'static str {
        "Query a group's account summary, details, and transaction log"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;

            // The group we query the accounting of: a pre-made group on grids
            // that configure one (Second Life), or a throwaway created here (the
            // OpenSim default, leaving the primary as founder/owner with the
            // Accountable power). The name carries a wall-clock suffix so
            // create-per-run does not collide on the unique-name constraint; it
            // is ignored on the pre-made path.
            let unique = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis());
            let group = support::membership_group(
                ctx,
                0,
                &format!("sl-client group-accounting {unique}"),
                "throwaway group for the group-accounting conformance case",
            )
            .await?;
            let group_id = group.group_id;

            if matches!(group.source, GroupSource::Created) {
                tokio::time::sleep(CREATE_SETTLE).await;
            }

            // A fresh correlation id per request, echoed back in the reply.
            let summary_req = GroupRequestId::from(Uuid::new_v4());
            let details_req = GroupRequestId::from(Uuid::new_v4());
            let transactions_req = GroupRequestId::from(Uuid::new_v4());

            let session = ctx.primary();
            let start = Instant::now();
            session
                .send(Command::RequestGroupAccountSummary {
                    group_id,
                    request_id: summary_req,
                    interval_days: SUMMARY_INTERVAL_DAYS,
                    current_interval: CURRENT_INTERVAL,
                })
                .await?;
            session
                .send(Command::RequestGroupAccountDetails {
                    group_id,
                    request_id: details_req,
                    interval_days: DETAILS_INTERVAL_DAYS,
                    current_interval: CURRENT_INTERVAL,
                })
                .await?;
            session
                .send(Command::RequestGroupAccountTransactions {
                    group_id,
                    request_id: transactions_req,
                    interval_days: TRANSACTIONS_INTERVAL_DAYS,
                    current_interval: CURRENT_INTERVAL,
                })
                .await?;

            if is_opensim(grid) {
                // OpenSim never answers any of the three; confirm they were
                // accepted at the LLUDP layer by keeping the circuit observed
                // past the reliable-retransmit budget via keep-alive pings. An
                // un-acked request would tear the circuit down and surface a
                // `Disconnected` that fails the wait.
                let mut last_rtt = None;
                while start.elapsed() < ACCEPT_WINDOW {
                    let rtt = session
                        .wait_for(PING_WAIT, |event| match event {
                            Event::Ping {
                                child: false, rtt, ..
                            } => Some(*rtt),
                            _ => None,
                        })
                        .await?;
                    last_rtt = Some(rtt);
                }
                let rtt = last_rtt.ok_or_else(|| {
                    TestFailure::Assertion(
                        "no keep-alive ping observed after the accounting requests".to_owned(),
                    )
                })?;

                let metrics = ctx.metrics();
                metrics.set("group_source", group.source.label());
                if let Some(create_rtt) = group.create_rtt {
                    metrics.set_timing(&secs_metric("group_create"), create_rtt.as_secs_f64());
                }
                metrics.set(&count_metric("requests_sent"), 3_i64);
                metrics.set(&count_metric("replies_received"), 0_i64);
                metrics.set_timing(&secs_metric("ping_rtt"), rtt.as_secs_f64());
                ctx.mark_partial(
                    "OpenSim has no group-accounting backend: the summary/details/transactions \
                     requests are accepted (acked, the circuit stays healthy past the \
                     reliable-retransmit budget) but never answered; the reply assertions are \
                     the Second Life variant",
                );
                return Ok(());
            }

            // Second Life path (deferred with the Aditi batch): each request is
            // answered; correlate every reply by its echoed group and request id
            // and assert the echoed interval parameters round-tripped.
            let summary = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::GroupAccountSummary(summary)
                        if summary.group_id == group_id
                            && summary.request_id == summary_req.get() =>
                    {
                        Some(summary.clone())
                    }
                    _ => None,
                })
                .await?;
            let summary_rtt = start.elapsed();
            check_eq(
                "summary current_interval",
                &summary.current_interval,
                &CURRENT_INTERVAL,
            )?;
            check_eq(
                "summary interval_days",
                &summary.interval_days,
                &SUMMARY_INTERVAL_DAYS,
            )?;

            let details_at = Instant::now();
            let details = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::GroupAccountDetails(details)
                        if details.group_id == group_id
                            && details.request_id == details_req.get() =>
                    {
                        Some(details.clone())
                    }
                    _ => None,
                })
                .await?;
            let details_rtt = details_at.elapsed();
            check_eq(
                "details current_interval",
                &details.current_interval,
                &CURRENT_INTERVAL,
            )?;

            let transactions_at = Instant::now();
            let transactions = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::GroupAccountTransactions(transactions)
                        if transactions.group_id == group_id
                            && transactions.request_id == transactions_req.get() =>
                    {
                        Some(transactions.clone())
                    }
                    _ => None,
                })
                .await?;
            let transactions_rtt = transactions_at.elapsed();
            check_eq(
                "transactions current_interval",
                &transactions.current_interval,
                &CURRENT_INTERVAL,
            )?;

            let details_entries = i64::try_from(details.entries.len()).unwrap_or(-1);
            let transaction_entries = i64::try_from(transactions.entries.len()).unwrap_or(-1);

            let metrics = ctx.metrics();
            metrics.set("group_source", group.source.label());
            if let Some(create_rtt) = group.create_rtt {
                metrics.set_timing(&secs_metric("group_create"), create_rtt.as_secs_f64());
            }
            metrics.set(&count_metric("requests_sent"), 3_i64);
            metrics.set(&count_metric("replies_received"), 3_i64);
            metrics.set("summary_balance", summary.balance.to_string());
            metrics.set("summary_total_credits", summary.total_credits.to_string());
            metrics.set("summary_total_debits", summary.total_debits.to_string());
            metrics.set(&count_metric("details_entries"), details_entries);
            metrics.set(&count_metric("transaction_entries"), transaction_entries);
            metrics.set_timing(&secs_metric("summary_request"), summary_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("details_request"), details_rtt.as_secs_f64());
            metrics.set_timing(
                &secs_metric("transactions_request"),
                transactions_rtt.as_secs_f64(),
            );
            Ok(())
        })
    }
}
