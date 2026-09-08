//! Login and region handshake: the most basic liveness check on a grid.
//!
//! `1av`, `[both, fake]`. Offline it is also the first thing to break when the
//! login response or the handshake changes shape, and the cheapest test in the
//! suite to run.

use std::time::{Duration, Instant};

use sl_client_tokio::{Event, LoginAccount};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};

/// How long to wait for the first region to become active.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the account event, which the session pushes as soon as
/// the login response is parsed.
const ACCOUNT_TIMEOUT: Duration = Duration::from_secs(30);

/// Records what the login response said the account is entitled to.
///
/// This is the only measurement in the suite of the benefits package and the
/// maturity trio, and both are grid divergences worth pinning: Second Life
/// sends a benefits package and a maturity *preference*, a stock OpenSim grid
/// sends neither and hard-codes the other two maturity fields to `M`/`A` for
/// every account.
///
/// The upload costs are the point of it. On Second Life the reference viewer
/// charges from here rather than from `EconomyData`, and the tiered
/// `large_texture_upload_cost` cannot be expressed in that older reply at all —
/// so `economy-data`'s `price_upload` is a real measurement of a field nobody
/// spends, and these are the numbers an upload is actually billed.
fn record_account(metrics: &mut crate::metrics::Metrics, account: &LoginAccount) {
    metrics.set("agent_access", format!("{:?}", account.agent_access));
    metrics.set(
        "agent_access_max",
        format!("{:?}", account.agent_access_max),
    );
    metrics.set(
        "preferred_maturity",
        account
            .preferred_maturity
            .map_or_else(|| "absent".to_owned(), |maturity| format!("{maturity:?}")),
    );
    metrics.set(
        "account_type",
        account.account_type.clone().unwrap_or_else(|| {
            // Not "Base": a grid that names no package and a grid that says you
            // are on the free one are different answers, and flattening them
            // would read as the latter.
            "absent".to_owned()
        }),
    );
    metrics.set(
        "premium_packages",
        if account.packages.is_empty() {
            "absent".to_owned()
        } else {
            account
                .packages
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        },
    );
    let Some(benefits) = account.benefits.as_ref() else {
        metrics.set("benefits", "absent");
        return;
    };
    metrics.set("benefits", "present");
    for (name, amount) in [
        ("texture_upload_cost", benefits.texture_upload_cost.clone()),
        (
            "large_texture_upload_cost",
            benefits.large_texture_upload_cost(),
        ),
        ("sound_upload_cost", benefits.sound_upload_cost.clone()),
        (
            "animation_upload_cost",
            benefits.animation_upload_cost.clone(),
        ),
        ("create_group_cost", benefits.create_group_cost.clone()),
    ] {
        metrics.set(name, i64::try_from(amount.0).unwrap_or(-1));
    }
    for (name, limit) in [
        ("attachment_limit", benefits.attachment_limit),
        ("group_membership_limit", benefits.group_membership_limit),
        ("animated_object_limit", benefits.animated_object_limit),
        ("picks_limit", benefits.picks_limit),
    ] {
        metrics.set(name, i64::from(limit));
    }
    // How many rungs the large-texture ladder actually has. The reference
    // viewer sorts the array and reads only its first entry, so a grid sending
    // more than one tier would be describing pricing no viewer yet charges —
    // worth knowing before anyone builds on the assumption of a single rung.
    metrics.set(
        "large_texture_tiers",
        i64::try_from(benefits.large_texture_upload_costs.len()).unwrap_or(-1),
    );

    // The other packages, which are the only way to see what a subscription
    // this account does not hold would grant. The test avatar is on the free
    // tier and buying a Premium one to read four numbers would be absurd, so
    // `premium_packages` is the measurement — the grid describes every package
    // to every account precisely so a viewer can render the comparison.
    for (package, other) in &account.packages {
        for (field, value) in [
            (
                "texture_upload_cost",
                i64::try_from(other.texture_upload_cost.0).unwrap_or(-1),
            ),
            (
                "large_texture_upload_cost",
                i64::try_from(other.large_texture_upload_cost().0).unwrap_or(-1),
            ),
            (
                "sound_upload_cost",
                i64::try_from(other.sound_upload_cost.0).unwrap_or(-1),
            ),
            (
                "animation_upload_cost",
                i64::try_from(other.animation_upload_cost.0).unwrap_or(-1),
            ),
            (
                "create_group_cost",
                i64::try_from(other.create_group_cost.0).unwrap_or(-1),
            ),
            ("attachment_limit", i64::from(other.attachment_limit)),
            (
                "group_membership_limit",
                i64::from(other.group_membership_limit),
            ),
            (
                "animated_object_limit",
                i64::from(other.animated_object_limit),
            ),
            ("picks_limit", i64::from(other.picks_limit)),
        ] {
            metrics.set(&format!("package_{package}_{field}"), value);
        }
    }
}

/// Logs in and waits for the region handshake, recording the time taken.
#[derive(Debug)]
pub struct LoginHandshake;

impl GridTest for LoginHandshake {
    fn name(&self) -> &'static str {
        "login-handshake"
    }

    fn description(&self) -> &'static str {
        "Log in and reach the first region handshake"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let start = Instant::now();
            // Before the region wait, not after. The session pushes
            // `Event::Account` as soon as it parses the login response — well
            // before the handshake — and `wait_for` discards every event its
            // predicate rejects, so waiting for the region first would eat this
            // one on the way past and record the account as absent.
            let account: LoginAccount = ctx
                .primary()
                .wait_for(ACCOUNT_TIMEOUT, |event| match event {
                    Event::Account(account) => Some((**account).clone()),
                    _other => None,
                })
                .await?;
            // The start region's own rating, captured before the region wait for
            // the same reason as the account event. It is recorded to settle a
            // question the account fields alone cannot: aditi sends
            // `agent_access = "M"` while that account's ceiling *and* preference
            // are both `"A"`, so `agent_access` is neither of those — and the
            // most testable remaining reading is that it describes the region
            // being entered rather than the agent. If it tracks this figure
            // across differently-rated start regions, that is the answer; if it
            // stays `"M"` while this varies, it is the legacy constant OpenSim's
            // hard-coded `"M"` suggests it has become.
            let region_maturity = ctx
                .primary()
                .wait_for(ACCOUNT_TIMEOUT, |event| match event {
                    Event::RegionInfoHandshake(identity) => Some(identity.maturity),
                    _other => None,
                })
                .await
                .ok();
            ctx.primary().wait_for_region(HANDSHAKE_TIMEOUT).await?;
            let elapsed = start.elapsed().as_secs_f64();
            ctx.metrics().set_timing("handshake_secs", elapsed);
            record_account(ctx.metrics(), &account);
            ctx.metrics().set(
                "start_region_maturity",
                region_maturity
                    .map_or_else(|| "absent".to_owned(), |maturity| format!("{maturity:?}")),
            );
            Ok(())
        })
    }
}
