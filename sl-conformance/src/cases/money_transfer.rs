//! Send a small L$ gift from the primary to the secondary and confirm the
//! payer's `MoneyBalanceReply` echoes the transfer.

use sl_client_tokio::{Command, Event, LindenAmount, MoneyBalance, MoneyTransactionType};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, is_opensim};

/// The token L$ amount the primary gifts the secondary. Kept to the smallest
/// possible transfer: on a grid with a real money backend (Second Life) this
/// moves one unit of (non-cashable beta) currency between the operator's own two
/// test avatars, and the case gifts it straight back for neutrality.
const GIFT_AMOUNT: LindenAmount = LindenAmount(1);

/// Sends a minimal L$ gift from the primary to the secondary and confirms the
/// payer's `MoneyBalanceReply` echoes the transfer.
///
/// Paying an avatar is a `MoneyTransferRequest` (`TransactionType` `5001`, a
/// gift). On a grid with a real money backend the simulator debits the payer,
/// credits the payee, and pushes each side an updated `MoneyBalanceReply`; the
/// payer's carries a `TransactionInfo` block naming the transfer (type, source,
/// dest, amount). The case issues the gift from the primary, awaits that echo —
/// surfaced as an [`Event::MoneyBalance`] whose [`MoneyTransaction`] block is
/// present — and asserts the block: it is a gift, from the primary to the
/// secondary, for the amount sent. It then gifts the same amount back from the
/// secondary so re-runs net to zero.
///
/// The two grids diverge here by design, so the case is grid-aware:
///
/// - **Second Life** (aditi): a real backend routes the transfer, so the echo
///   arrives and the block is asserted. The primary aditi avatar carries a live
///   L$ balance, so the 1 L$ gift succeeds.
/// - **OpenSim**: the stock `BetaGridLikeMoneyModule`'s `MoneyTransferAction`
///   handler is an empty method — the transfer is silently dropped and no
///   `MoneyBalanceReply` is sent. The case waits briefly, observes the absence,
///   and [marks the run partial](TestContext::mark_partial) rather than failing:
///   the request path is exercised, but the round-trip cannot complete without a
///   money backend.
///
/// `2av`, `[both]`. On OpenSim the money module must be enabled (see the money
/// setup notes) for the request to be accepted at all.
///
/// [`MoneyTransaction`]: sl_client_tokio::MoneyTransaction
#[derive(Debug)]
pub struct MoneyTransfer;

impl GridTest for MoneyTransfer {
    fn name(&self) -> &'static str {
        "money-transfer"
    }

    fn description(&self) -> &'static str {
        "gift L$; observe the payer's transfer echo (partial without a backend)"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Both avatars must be logged in before a transfer can be routed
            // between them.
            ctx.primary().wait_for_region(REGION_TIMEOUT).await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("two-account test ran without a secondary".to_owned())
            })?;
            secondary.wait_for_region(REGION_TIMEOUT).await?;

            // Capture both agent ids: the secondary's while it is borrowed, then
            // release the borrow before reborrowing the primary.
            let secondary_id = secondary.agent_id().ok_or_else(|| {
                TestFailure::Assertion("secondary login did not report an agent id".to_owned())
            })?;
            let primary_id = ctx.primary().agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login did not report an agent id".to_owned())
            })?;

            let grid = ctx.grid();

            // The primary gifts the secondary the token amount.
            ctx.primary()
                .send(Command::SendMoneyTransfer {
                    dest: secondary_id.uuid(),
                    amount: GIFT_AMOUNT,
                    kind: MoneyTransactionType::Gift,
                    description: "sl-conformance money-transfer".to_owned(),
                })
                .await?;

            // Await the payer's echo: a `MoneyBalanceReply` carrying a
            // `TransactionInfo` block (surfaced only for a real, non-zero
            // transaction). A plain unsolicited balance poll has no such block, so
            // this predicate matches the transfer echo specifically.
            let echo: Result<MoneyBalance, TestFailure> = ctx
                .primary()
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::MoneyBalance(balance) if balance.transaction.is_some() => {
                        Some(balance.clone())
                    }
                    _ => None,
                })
                .await;

            match echo {
                Ok(balance) => {
                    // A grid with a real backend routed the transfer; assert the
                    // echoed transaction block describes the gift we sent.
                    let transaction = balance.transaction.as_ref().ok_or_else(|| {
                        TestFailure::Assertion(
                            "matched a transfer echo with no transaction block".to_owned(),
                        )
                    })?;
                    check(
                        balance.success,
                        "expected the payer's transfer echo to report success",
                    )?;
                    check_eq(
                        "transfer echo transaction type",
                        &MoneyTransactionType::from_i32(transaction.transaction_type),
                        &MoneyTransactionType::Gift,
                    )?;
                    check(
                        transaction.source.uuid() == primary_id.uuid(),
                        "expected the transfer echo to name the primary as the source",
                    )?;
                    check(
                        transaction.dest.uuid() == secondary_id.uuid(),
                        "expected the transfer echo to name the secondary as the destination",
                    )?;
                    check_eq("transfer echo amount", &transaction.amount, &GIFT_AMOUNT)?;

                    let metrics = ctx.metrics();
                    metrics.set("amount", i64::try_from(GIFT_AMOUNT.0).unwrap_or(-1));
                    metrics.set(
                        "payer_balance_after",
                        i64::try_from(balance.balance.0).unwrap_or(-1),
                    );

                    // Neutrality: gift the same amount back so repeated runs do
                    // not drain one avatar into the other. Best effort — the
                    // assertions above already proved the transfer round-trip.
                    ctx.secondary()
                        .ok_or_else(|| {
                            TestFailure::Assertion(
                                "two-account test ran without a secondary".to_owned(),
                            )
                        })?
                        .send(Command::SendMoneyTransfer {
                            dest: primary_id.uuid(),
                            amount: GIFT_AMOUNT,
                            kind: MoneyTransactionType::Gift,
                            description: "sl-conformance money-transfer (return)".to_owned(),
                        })
                        .await?;
                    Ok(())
                }
                Err(TestFailure::Timeout(_)) if is_opensim(grid) => {
                    // OpenSim's `MoneyTransferAction` is an empty method: the
                    // request is accepted but no transfer is routed and no reply
                    // is sent. Record the request path was exercised and mark the
                    // run partial rather than failing.
                    ctx.metrics()
                        .set("amount", i64::try_from(GIFT_AMOUNT.0).unwrap_or(-1));
                    ctx.mark_partial(
                        "OpenSim's BetaGridLikeMoneyModule drops transfers (empty \
                         MoneyTransferAction); no MoneyBalanceReply echo without a money backend",
                    );
                    Ok(())
                }
                Err(other) => Err(other),
            }
        })
    }
}
