//! Which login response fields each grid sends, and which of them the request's
//! `options` list gates.
//!
//! A login request names the optional sections it wants (`inventory-root`,
//! `buddy-list`, `max-agent-groups`, …). A grid that honours the list leaves
//! out what was not named; one that ignores it sends everything. Which grid
//! does which — and which fields come regardless — decides what a viewer has
//! to ask for, and what the fake grid has to withhold per flavour.
//!
//! The case logs the same avatar in three times and records the top-level
//! field names each response carried
//! ([`LoginAccount::response_fields`](sl_client_tokio::LoginAccount::response_fields)):
//!
//! 1. with the client's **default** list (`fields_default`) — what every other
//!    case and the viewer get;
//! 2. with an **empty** list (`fields_none`) — what a grid sends whatever was
//!    asked;
//! 3. with **every** option the reference viewer knows (`fields_all`) —
//!    [`EVERY_OPTION`].
//!
//! `fields_gated` (in the third, not the second) is what the options control.
//! On aditi each relogin waits out the per-avatar login cooldown, so the case
//! takes about five minutes there. `1av`, `[both, fake]`.
//!
//! The measured answers are in the book's *Grid Behaviour* part,
//! `book/src/gridspec/login.md`.

use std::collections::BTreeSet;
use std::time::Duration;

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check};

/// Every option the reference viewer requests at login: the list in
/// Firestorm's `LLLoginInstance::constructAuthParams`
/// (`lllogininstance.cpp`), its Second Life part and the five it adds on an
/// OpenSim grid (`currency`, `max_groups`, `search`, `destination_guide_url`,
/// `avatar_picker_url`) together, so one list surveys both grids.
pub const EVERY_OPTION: &[&str] = &[
    "inventory-root",
    "inventory-skeleton",
    "inventory-lib-root",
    "inventory-lib-owner",
    "inventory-skel-lib",
    "initial-outfit",
    "gestures",
    "display_names",
    "event_categories",
    "event_notifications",
    "classified_categories",
    "adult_compliant",
    "buddy-list",
    "newuser-config",
    "ui-config",
    "advanced-mode",
    "max-agent-groups",
    "map-server-url",
    "voice-config",
    "tutorial_setting",
    "login-flags",
    "global-textures",
    "currency",
    "max_groups",
    "search",
    "destination_guide_url",
    "avatar_picker_url",
];

/// Whether the grid honours the request's `options` list at all — whether
/// asking for nothing gets fewer fields than asking for everything.
const HONOURS_OPTIONS: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: "book/src/gridspec/login.md (login-options, 2026-10-04)",
};

/// Which of the three logins a presence row is read from.
#[derive(Debug, Clone, Copy)]
enum Asked {
    /// The login with an empty `options` list: sent whatever was asked.
    Nothing,
    /// The login with [`EVERY_OPTION`]: sent at all.
    Everything,
}

/// The source every presence row cites.
const SOURCE: &str = "book/src/gridspec/login.md (login-options, 2026-10-04)";

/// Whether a field is present, per grid, in the response to the login named.
///
/// The rows are the ones the fake grid has to get right per flavour, because
/// a viewer reads the field (`home`, `look_at`, `max-agent-groups`,
/// `map-server-url`, `inventory-lib-owner`, the region size) or because a
/// grid was believed to send it and does not (`voice-config`, `currency`).
const PRESENCE: &[(&str, Asked, Measured<bool>)] = &[
    (
        "home",
        Asked::Everything,
        Measured {
            second_life: false,
            opensim: true,
            source: SOURCE,
        },
    ),
    (
        "look_at",
        Asked::Nothing,
        Measured {
            second_life: true,
            opensim: true,
            source: SOURCE,
        },
    ),
    (
        "max-agent-groups",
        Asked::Nothing,
        Measured {
            second_life: true,
            opensim: true,
            source: SOURCE,
        },
    ),
    (
        "map-server-url",
        Asked::Nothing,
        Measured {
            second_life: true,
            opensim: true,
            source: SOURCE,
        },
    ),
    (
        "inventory-lib-owner",
        Asked::Everything,
        Measured {
            second_life: true,
            opensim: true,
            source: SOURCE,
        },
    ),
    (
        "inventory-lib-owner",
        Asked::Nothing,
        Measured {
            second_life: false,
            opensim: true,
            source: SOURCE,
        },
    ),
    (
        "region_size_x",
        Asked::Everything,
        Measured {
            second_life: false,
            opensim: true,
            source: SOURCE,
        },
    ),
    (
        "voice-config",
        Asked::Everything,
        Measured {
            second_life: false,
            opensim: false,
            source: SOURCE,
        },
    ),
    (
        "currency",
        Asked::Everything,
        Measured {
            second_life: false,
            opensim: false,
            source: SOURCE,
        },
    ),
];

/// The overall budget: three logins, two of them behind aditi's 120 s
/// per-avatar cooldown.
const CASE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The sorted top-level field names of the session's current login.
fn fields_of(session: &Session) -> Result<BTreeSet<String>, TestFailure> {
    let account = session
        .login_account()
        .ok_or_else(|| TestFailure::State("the session holds no login account".to_owned()))?;
    Ok(account.response_fields.iter().cloned().collect())
}

/// `fields` as one comma-separated metric value.
fn joined(fields: &BTreeSet<String>) -> String {
    fields.iter().cloned().collect::<Vec<_>>().join(",")
}

/// Log the primary out and back in asking for `options`, and return the
/// fields the new response carried.
async fn relogin_asking(
    session: &mut Session,
    options: Vec<String>,
) -> Result<BTreeSet<String>, TestFailure> {
    session.disconnect().await?;
    session.relogin_with_options(Some(options)).await?;
    session.wait_for_region(REGION_TIMEOUT).await?;
    fields_of(session)
}

/// Maps which login response fields each grid gates behind the `options` list.
#[derive(Debug)]
pub struct LoginOptions;

impl GridTest for LoginOptions {
    fn name(&self) -> &'static str {
        "login-options"
    }

    fn description(&self) -> &'static str {
        "Record the login response's fields with the default, no and every option"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let default = fields_of(session)?;
            let none = relogin_asking(session, Vec::new()).await?;
            let every = relogin_asking(
                session,
                EVERY_OPTION
                    .iter()
                    .map(|&option| option.to_owned())
                    .collect(),
            )
            .await?;
            // Back to the default list, so the runner's logout — and anything
            // after it — sees the session every other case gets.
            session.disconnect().await?;
            session.relogin_with_options(None).await?;
            session.wait_for_region(REGION_TIMEOUT).await?;

            let gated: BTreeSet<String> = every.difference(&none).cloned().collect();
            let lost: BTreeSet<String> = none.difference(&every).cloned().collect();
            let metrics = ctx.metrics();
            metrics.set("fields_default", joined(&default));
            metrics.set("fields_none", joined(&none));
            metrics.set("fields_all", joined(&every));
            metrics.set("fields_gated", joined(&gated));
            metrics.set("fields_lost_by_asking", joined(&lost));
            metrics.set(
                "max_agent_groups_without_option",
                none.contains("max-agent-groups"),
            );
            // Asking for more must never take a field away.
            check(
                lost.is_empty(),
                &format!(
                    "asking for every option dropped fields the empty list got: {}",
                    joined(&lost)
                ),
            )?;
            HONOURS_OPTIONS.check(
                "whether the options list gates any field",
                grid,
                &!gated.is_empty(),
            )?;
            for (field, asked, measured) in PRESENCE {
                let (fields, label) = match asked {
                    Asked::Nothing => (&none, "asked for nothing"),
                    Asked::Everything => (&every, "asked for every option"),
                };
                measured.check(
                    &format!("`{field}` in the response {label}"),
                    grid,
                    &fields.contains(*field),
                )?;
            }
            Ok(())
        })
    }
}
