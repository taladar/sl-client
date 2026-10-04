//! Whether the login response carries `buddy-list` for an account that has a
//! friend.
//!
//! `login-options` found aditi sending no `buddy-list` even when the request
//! asked for it, for accounts that had no friends at the time, and OpenSim
//! sending it as an empty array. That leaves Second Life's rule open: does it
//! leave out an *empty* list, or never send one? This case makes the two
//! accounts friends, logs the primary in again asking for `buddy-list`, records
//! what came back, and ends the friendship. `2av`, live grids only — the fake
//! grid's answer is held by `login-options` from the result.

use std::time::Duration;

use sl_client_tokio::{Command, Event, ImDialog, InventoryFolderKey, TransactionId, Uuid};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT};

/// How long to settle after a terminate so the grid's friends cache reflects
/// it before the next step.
const SETTLE: Duration = Duration::from_secs(3);

/// The overall budget: two logins of the primary behind aditi's cooldown.
const CASE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Whether a response for an account with a friend carries `buddy-list`.
const SENDS_BUDDY_LIST_WITH_A_FRIEND: Measured<bool> = Measured {
    second_life: true,
    opensim: true,
    source: "book/src/gridspec/login.md (login-buddy-list, 2026-10-04)",
};

/// Makes the two accounts friends and records the primary's next login's
/// `buddy-list`.
#[derive(Debug)]
pub struct LoginBuddyList;

impl GridTest for LoginBuddyList {
    fn name(&self) -> &'static str {
        "login-buddy-list"
    }

    fn description(&self) -> &'static str {
        "Whether the login's buddy-list arrives for an account with a friend"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            // Pre-clean, then befriend: the primary offers, the secondary
            // accepts, and the primary sees the acceptance.
            ctx.primary()
                .send(Command::TerminateFriendship(secondary_id.uuid().into()))
                .await?;
            tokio::time::sleep(SETTLE).await;
            ctx.primary()
                .send(Command::OfferFriendship {
                    to_agent_id: secondary_id,
                    message: "sl-conformance login-buddy-list".to_owned(),
                })
                .await?;
            let offer = ctx
                .secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == primary_id
                            && im.dialog == ImDialog::FriendshipOffered =>
                    {
                        Some(im.id)
                    }
                    _ => None,
                })
                .await?;
            ctx.secondary()
                .ok_or_else(|| {
                    TestFailure::Assertion("two-account test ran without a secondary".to_owned())
                })?
                .send(Command::AcceptFriendship {
                    transaction_id: TransactionId::from(offer),
                    friend_id: primary_id.uuid().into(),
                    calling_card_folder: InventoryFolderKey::from(Uuid::nil()),
                })
                .await?;
            ctx.primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InstantMessageReceived(im)
                        if im.from_agent_id == secondary_id
                            && im.dialog == ImDialog::FriendshipAccepted =>
                    {
                        Some(())
                    }
                    _ => None,
                })
                .await?;

            // The primary logs in again, asking for its buddy list.
            let primary = ctx.primary();
            primary.disconnect().await?;
            let relogged = primary
                .relogin_with_options(Some(vec!["buddy-list".to_owned()]))
                .await;
            let observed = match relogged {
                Ok(()) => primary
                    .wait_for_region(REGION_TIMEOUT)
                    .await
                    .and_then(|()| {
                        let success = primary.login_success().ok_or_else(|| {
                            TestFailure::State("the relogin left no response".to_owned())
                        })?;
                        Ok((
                            success
                                .response_fields
                                .iter()
                                .any(|field| field == "buddy-list"),
                            success.buddy_list.len(),
                            success
                                .buddy_list
                                .iter()
                                .any(|buddy| buddy.buddy_id == secondary_id.uuid()),
                        ))
                    }),
                Err(failure) => Err(failure),
            };

            // End the friendship whatever the relogin did, from whichever side
            // is still connected.
            let terminate = Command::TerminateFriendship(primary_id.uuid().into());
            if let Some(secondary) = ctx.secondary() {
                secondary.send(terminate).await?;
            }
            tokio::time::sleep(SETTLE).await;

            let (present, count, names_secondary) = observed?;
            let metrics = ctx.metrics();
            metrics.set("buddy_list_present", present);
            metrics.set("buddy_list_count", i64::try_from(count).unwrap_or(-1));
            metrics.set("buddy_list_names_the_friend", names_secondary);
            SENDS_BUDDY_LIST_WITH_A_FRIEND.check(
                "`buddy-list` for an account with a friend",
                grid,
                &present,
            )?;
            Ok(())
        })
    }
}
