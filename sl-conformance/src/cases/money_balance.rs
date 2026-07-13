//! Request the agent's L$ balance and confirm the `MoneyBalanceReply` flow.

use sl_client_tokio::{Command, Event, MoneyBalance};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, send_then_wait};

/// Polls the agent's current L$ balance and confirms the reply.
///
/// A viewer asks the simulator for the agent's balance with a
/// `MoneyBalanceRequest`; the simulator answers with a `MoneyBalanceReply`,
/// surfaced here as [`Event::MoneyBalance`]. This case issues the request,
/// awaits that reply, and asserts the *shape* of an unsolicited balance poll
/// rather than any amount: the reply is for our own agent, it reports success,
/// and it carries no triggering transaction (nil `TransactionID`, no
/// `TransactionInfo` block) — i.e. it is a plain balance poll, not the echo of
/// a pay/buy.
///
/// The balance amount itself is deliberately not asserted: OpenSim's
/// `BetaGridLikeMoneyModule` hardcodes the balance to `0`, while Second Life
/// reports the real account balance, so the amount is grid- and account-policy
/// and only recorded as a metric. This runs on both grids (`1av`); on OpenSim
/// the money module must be enabled for a `MoneyBalanceReply` to arrive.
///
/// Named `…Case` rather than `MoneyBalance` to avoid clashing with the
/// [`MoneyBalance`] reply type this case decodes.
#[expect(
    clippy::module_name_repetitions,
    reason = "the bare `MoneyBalance` name is the reply type; the case struct needs a distinct name"
)]
#[derive(Debug)]
pub struct MoneyBalanceCase;

impl GridTest for MoneyBalanceCase {
    fn name(&self) -> &'static str {
        "money-balance"
    }

    fn description(&self) -> &'static str {
        "request balance; observe reply"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Capture our own agent id before the request so we can confirm the
            // reply is addressed to us.
            let own_agent = session.agent_id();

            // Issue the `MoneyBalanceRequest` and await the `MoneyBalanceReply`.
            let balance: MoneyBalance = send_then_wait(
                session,
                Command::RequestMoneyBalance,
                REPLY_TIMEOUT,
                |event| match event {
                    Event::MoneyBalance(balance) => Some(balance.clone()),
                    _ => None,
                },
            )
            .await?;

            // A plain balance poll reports success, is addressed to our own
            // agent, and carries no triggering transaction — a nil
            // `TransactionID` and no `TransactionInfo` block. Assert this flow
            // rather than the amount (grid/account policy, recorded below).
            if let Some(own_agent) = own_agent {
                check(
                    balance.agent_id == own_agent,
                    "expected the balance reply to be addressed to our own agent",
                )?;
            }
            check(
                balance.success,
                "expected a plain balance poll to report success",
            )?;
            check(
                balance.transaction_id.is_nil(),
                "expected a plain balance poll to carry a nil transaction id",
            )?;
            check(
                balance.transaction.is_none(),
                "expected a plain balance poll to carry no transaction info block",
            )?;

            let metrics = ctx.metrics();
            metrics.set("balance", i64::try_from(balance.balance.0).unwrap_or(-1));
            metrics.set(
                "square_meters_credit",
                i64::from(balance.square_meters_credit.0),
            );
            metrics.set(
                "square_meters_committed",
                i64::from(balance.square_meters_committed.0),
            );
            Ok(())
        })
    }
}
