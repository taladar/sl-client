//! The offline grid: an in-process [`sl_fake_grid`] a conformance case runs
//! against with no network, no credentials file and no cooldown.
//!
//! Every other grid in [`Grid`] is something an operator prepares and the
//! harness merely logs into. This one the harness *starts*: a
//! [`FakeGridHarness`] binds a pair of ephemeral ports, dresses its regions
//! from the shared fixture catalogue, registers three accounts and hands out
//! the login URI it bound — as synthesised [`Credentials`], so the login path
//! below it is the same one a live grid takes, down to the XML-RPC round trip.
//!
//! Two things follow from that, and they are the whole point of the module:
//!
//! - **The cases in [`OFFLINE_CASES`] are asserted on every `cargo test`.** They
//!   assert protocol *shape* — a handshake, a ping, a throttle, a parcel record
//!   — against fixtures that are all present offline, so nothing about them
//!   needs a grid session someone has to remember to run. See
//!   [`run_offline_case`].
//! - **A case can speak as the simulator.** A border crossing and a
//!   grid-initiated teleport are decisions a region makes, and a fake grid
//!   simulates no movement to make them with; [`FakeControl`] is the handle a
//!   case reaches for through [`TestContext::fake`] to make them itself. Only a
//!   [`Grid::Fake`]-only case may: on a live grid there is nothing to hold.
//!
//! [`Credentials`]: sl_repl::Credentials

use std::sync::Arc;

use sl_repl::{Avatar, Credentials};

use crate::context::{Session, TestContext, TestFailure};
use crate::fixtures::Fixtures;
use crate::grid::Grid;
use crate::record::Completeness;
use crate::registry::GridTest;

/// The seed every fake conformance grid is built with, so the ids a failing
/// run reports are the same ids the next run mints.
const SEED: u64 = 0x5C_C0_4F_A6;

/// The last name every fake account shares.
const LAST_NAME: &str = "Tester";

/// The password every fake account shares. The login endpoint checks it, so it
/// cannot be empty; nothing else cares what it is.
const PASSWORD: &str = "conformance";

/// The credentials-file labels and first names of the three accounts a fake
/// grid registers, in the order the runner resolves them (primary, secondary,
/// tertiary).
///
/// Three because that is the most any registered case asks for
/// ([`GridTest::accounts`]); registering them all costs one login-endpoint
/// entry each and nothing at run time, since an account nobody logs in as is
/// never minted a session.
const ACCOUNTS: [(&str, &str); 3] = [
    ("primary", "Conformance"),
    ("secondary", "Bystander"),
    ("tertiary", "Onlooker"),
];

/// The credentials-file label of the account the fake grid registers as an
/// **estate manager**.
///
/// The primary, matching what a live OpenSim run has to do by hand (`--avatar
/// estate-owner`): an estate command from an agent without the power is
/// silently dropped, so a case that provokes one has nothing to observe unless
/// the avatar it runs as holds it. The other two stay ordinary residents, so
/// the check is a check.
const ESTATE_MANAGER: &str = "primary";

/// The name of the region a fake grid's accounts start in: the shared prim
/// catalogue, the same scene the full-stack viewer harness photographs.
pub const START_REGION: &str = "Fake Region";

/// The name of the region east of [`START_REGION`], announced as its
/// neighbour: the border scene, whose marker pillar stands just past the
/// shared edge.
///
/// It exists so the two handover cases have somewhere to go. Neither live grid
/// reliably offers an adjacent region an avatar may walk between, which is why
/// `region-crossing` and `neighbour-child-circuits` live here and nowhere else.
pub const EAST_REGION: &str = "Fake Region East";

/// The cases that run against the fake grid on every `cargo test`.
///
/// Two rules decide membership. Each case's fixtures must all be **offline** —
/// it asserts protocol *shape* (a handshake, a ping, a throttle, a parcel
/// record, the world map), not grid semantics — and each must **bite**: a case
/// that passes here only by recording `partial` costs suite time to assert
/// nothing, so a case joins the list when the grid can answer it, not when it
/// stops erroring. [`run_offline_case`] enforces the second rule rather than
/// trusting it: a partial run is a failure here.
///
/// The list is asserted against the registry in both directions (each name
/// resolves and declares [`Grid::Fake`]; no case declares [`Grid::Fake`]
/// without being listed), so it cannot drift into naming a case that does not
/// exist or missing one that opted in.
pub const OFFLINE_CASES: &[&str] = &[
    "login-handshake",
    "keepalive-ping",
    "throttle-set",
    "simulator-features",
    "object-update-decode",
    "parcel-properties",
    "terrain-raw-transfer-download",
    "terrain-layerdata",
    "map-blocks-items",
    "teleport-local-phases",
    "teleport-cross-region",
    "region-crossing",
    "neighbour-child-circuits",
    "avatar-appearance-npc",
    "texture-fetch-http",
    "asset-fetch-http",
    "economy-data",
    "parcel-info-dwell",
    "agent-alert",
    "server-error",
    "logout-clean",
];

/// The grid-side half of a fake-grid run: the simulator a case may talk to as
/// well as through.
///
/// Handed to the case body on [`Grid::Fake`] only (see
/// [`TestContext::fake`]). It carries the grid itself — region handles, region
/// names, the neighbour graph — and the live session handle for the primary
/// avatar, which is what the handover calls
/// ([`FakeGrid::cross_agent`](sl_fake_grid::FakeGrid::cross_agent),
/// [`FakeGrid::teleport_agent`](sl_fake_grid::FakeGrid::teleport_agent)) take.
#[expect(
    clippy::module_name_repetitions,
    reason = "the name is read at its use sites (`TestContext::fake`, a case's \
              `ctx.fake()`), where a bare `Control` would say nothing about which \
              grid it controls"
)]
#[derive(Debug, Clone)]
pub struct FakeControl {
    /// The running grid, shared with the harness that started it.
    grid: Arc<sl_fake_grid::FakeGrid>,
    /// The grid-side session handle for the primary avatar.
    agent: sl_fake_grid::FakeAgent,
}

impl FakeControl {
    /// The running grid.
    #[must_use]
    pub fn grid(&self) -> &sl_fake_grid::FakeGrid {
        &self.grid
    }

    /// The grid-side session handle for the primary avatar, as it was when the
    /// case started.
    ///
    /// A handover retires this session and opens another, so a case that
    /// crosses or teleports keeps the handle the call returns rather than
    /// asking again.
    #[must_use]
    pub const fn agent(&self) -> &sl_fake_grid::FakeAgent {
        &self.agent
    }
}

/// A started fake grid plus the credentials that reach it.
///
/// Owns the grid, so dropping the harness shuts every session and socket down.
/// The tokio runtime it was started on must outlive it, as it must for any
/// [`sl_fake_grid::FakeGrid`].
#[expect(
    clippy::module_name_repetitions,
    reason = "the runner and the offline tests both name it unqualified; `fake::Harness` \
              would not say what it is a harness for"
)]
#[derive(Debug)]
pub struct FakeGridHarness {
    /// The running grid.
    grid: Arc<sl_fake_grid::FakeGrid>,
    /// The synthesised accounts, each naming the URI the grid bound.
    credentials: Credentials,
    /// Login notices, subscribed before any login, so the grid-side session
    /// handle for an avatar can be picked up after it logs in.
    logins: tokio::sync::Mutex<tokio::sync::broadcast::Receiver<sl_fake_grid::LoginNotice>>,
}

impl FakeGridHarness {
    /// Start a grid serving the catalogue region and its eastern neighbour,
    /// with the three accounts registered.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::State`] if the grid cannot bind its sockets, and
    /// [`TestFailure::Auth`] if the synthesised credentials do not parse (which
    /// would be a bug in this module, not in the caller's input).
    pub async fn start() -> Result<Self, TestFailure> {
        let mut builder = sl_fake_grid::FakeGridBuilder::new().deterministic(SEED);
        for (label, first_name) in ACCOUNTS {
            let account = sl_fake_grid::AccountConfig::new(first_name, LAST_NAME, PASSWORD);
            builder = builder.account(if label == ESTATE_MANAGER {
                account.estate_manager()
            } else {
                account
            });
        }
        let start = sl_fake_grid::RegionConfig {
            name: START_REGION.to_owned(),
            ..sl_fake_grid::RegionConfig::default()
        };
        let east = sl_fake_grid::RegionConfig {
            name: EAST_REGION.to_owned(),
            grid_x: sl_fake_grid::RegionConfig::default()
                .grid_x
                .saturating_add(1),
            ..sl_fake_grid::RegionConfig::default()
        };
        let grid = builder
            .region(sl_fake_grid::catalogue().into_region(start))
            .region(sl_fake_grid::fixtures::border::border().into_region(east))
            .start()
            .await
            .map_err(|error| TestFailure::State(format!("the fake grid did not start: {error}")))?;
        let logins = grid.logins();
        let credentials = credentials_for(&grid.login_uri())?;
        Ok(Self {
            grid: Arc::new(grid),
            credentials,
            logins: tokio::sync::Mutex::new(logins),
        })
    }

    /// The synthesised credentials: three avatars, each naming this grid's
    /// login URI.
    #[must_use]
    pub const fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// The running grid.
    #[must_use]
    pub fn grid(&self) -> &sl_fake_grid::FakeGrid {
        &self.grid
    }

    /// The avatar registered under credentials label `label` (`"primary"`,
    /// `"secondary"`, `"tertiary"`).
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Auth`] if there is no such account.
    pub fn avatar(&self, label: &str) -> Result<&Avatar, TestFailure> {
        self.credentials
            .select(Some(label))
            .map_err(|error| TestFailure::Auth(error.to_string()))
    }

    /// The grid-side handle onto the session `avatar` just logged in with.
    ///
    /// Drains the login broadcast until it names this avatar, so a run that
    /// logged two accounts in resolves each to its own session rather than to
    /// whichever notice arrived first.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Timeout`] if no login notice for `avatar`
    /// arrives, and [`TestFailure::State`] if the notice names a session the
    /// grid no longer holds.
    pub async fn control_for(&self, avatar: &Avatar) -> Result<FakeControl, TestFailure> {
        // The receiver is borrowed only for the drain: holding it across the
        // session lookup below would make a second `control_for` wait on a lock
        // it does not need.
        let notice = {
            let mut logins = self.logins.lock().await;
            let found = loop {
                let notice = logins.recv().await.map_err(|error| {
                    TestFailure::Timeout(format!("the fake grid announced no login: {error}"))
                })?;
                if notice.first_name == avatar.first() && notice.last_name == avatar.last() {
                    break notice;
                }
            };
            drop(logins);
            found
        };
        let agent = self.grid.agent(&notice).await.ok_or_else(|| {
            TestFailure::State("the fake grid forgot the session it just opened".to_owned())
        })?;
        Ok(FakeControl {
            grid: Arc::clone(&self.grid),
            agent,
        })
    }

    /// Log in as many avatars as `test` needs and assemble the context its body
    /// runs against.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Assertion`] if the case asks for more avatars than
    /// the grid registers, and otherwise propagates the login failures of
    /// [`crate::context::login`] and the lookup failures of
    /// [`avatar`](Self::avatar) / [`control_for`](Self::control_for).
    pub async fn context(&self, test: &dyn GridTest) -> Result<TestContext, TestFailure> {
        let wanted = usize::from(test.accounts());
        if wanted == 0 || wanted > ACCOUNTS.len() {
            return Err(TestFailure::Assertion(format!(
                "{} asks for {wanted} avatars; the fake grid registers {}",
                test.name(),
                ACCOUNTS.len()
            )));
        }
        let state_dir = std::env::temp_dir();
        let mut sessions: Vec<Session> = Vec::new();
        let mut control: Option<FakeControl> = None;
        for (label, _first_name) in ACCOUNTS.iter().take(wanted) {
            let avatar = self.avatar(label)?;
            sessions.push(
                crate::context::login(
                    Grid::Fake,
                    avatar,
                    CHANNEL,
                    clap::crate_version!(),
                    test.start_location(Grid::Fake),
                    &state_dir,
                    // Nothing to force: the fake grid rate-limits nothing.
                    false,
                    None,
                )
                .await?,
            );
            let resolved = self.control_for(avatar).await?;
            control.get_or_insert(resolved);
        }
        let mut sessions = sessions.into_iter();
        let primary = sessions
            .next()
            .ok_or_else(|| TestFailure::Assertion("a case needs at least one avatar".to_owned()))?;
        let context = TestContext::new(
            Grid::Fake,
            primary,
            sessions.next(),
            sessions.next(),
            Fixtures::default(),
        );
        Ok(match control {
            Some(control) => context.with_fake(control),
            None => context,
        })
    }
}

/// The viewer channel a fake-grid run reports at login.
const CHANNEL: &str = "sl-conformance-fake";

/// The synthesised credentials for a grid bound at `login_uri`.
///
/// Built as TOML and parsed, rather than assembled field by field, because
/// [`Avatar`] deliberately has no public constructor: a credential is something
/// read from a file the operator owns. This is the one place that is not true,
/// and going the long way round keeps it the one place.
fn credentials_for(login_uri: &url::Url) -> Result<Credentials, TestFailure> {
    use core::fmt::Write as _;

    let mut text = String::from("default_avatar = \"primary\"\n");
    for (label, first_name) in ACCOUNTS {
        // Writing into a `String` cannot fail, so the result carries nothing to
        // report; the alternative would be an error path no input can reach.
        let _infallible = write!(
            text,
            "\n[avatars.{label}]\nfirst = \"{first_name}\"\nlast = \"{LAST_NAME}\"\n\
             password = \"{PASSWORD}\"\nlogin_uri = \"{login_uri}\"\n"
        );
    }
    Credentials::from_toml_str(&text).map_err(|error| TestFailure::Auth(error.to_string()))
}

/// Start a grid, run `test` against it, log every avatar out and shut it down.
///
/// The whole of what an offline case needs, and what the `cargo test` harness
/// calls once per name in [`OFFLINE_CASES`]. Unlike the runner this writes no
/// record: the assertion *is* the record, and it is re-made on every test run
/// rather than committed and left to go stale.
///
/// A case that finishes [`Completeness::Partial`] **fails** here, which is the
/// second membership rule enforced rather than documented: a partial run is a
/// case telling you it could not provoke what it came to assert, and the fake
/// grid is the one grid where that is always fixable — every fixture and every
/// policy it meets is one this workspace wrote. On a live grid the same
/// outcome is honest reporting; on this one it is a to-do.
///
/// # Errors
///
/// Returns the case's own [`TestFailure`], the failure that stopped it from
/// starting (the grid, the login, the account resolution), or
/// [`TestFailure::Assertion`] naming the reason it recorded partial.
pub async fn run_offline_case(test: &dyn GridTest) -> Result<(), TestFailure> {
    let harness = FakeGridHarness::start().await?;
    let mut context = harness.context(test).await?;
    let outcome =
        crate::isolate::run_isolated(test.run(&mut context), crate::isolate::DEFAULT_CASE_TIMEOUT)
            .await;
    let (_metrics, completeness, note, primary, secondary, tertiary) = context.into_parts();
    for session in [Some(primary), secondary, tertiary].into_iter().flatten() {
        if let Err(error) = session.logout().await {
            tracing::warn!("logout error after the offline case: {error}");
        }
    }
    outcome?;
    if completeness == Completeness::Partial {
        return Err(TestFailure::Assertion(format!(
            "the case recorded partial offline, so it asserted less than it came to: {}",
            note.as_deref().unwrap_or("no reason given")
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ACCOUNTS, OFFLINE_CASES, credentials_for};
    use crate::grid::Grid;
    use crate::registry::find;
    use pretty_assertions::assert_eq;

    /// Every synthesised account resolves under its own label, names the grid's
    /// URI, and is distinct from the others — which is what the runner's
    /// secondary/tertiary resolution walks the file for.
    #[test]
    fn the_synthesised_credentials_name_three_distinct_accounts() -> Result<(), String> {
        let uri: url::Url = "http://127.0.0.1:12345/"
            .parse()
            .map_err(|error| format!("{error}"))?;
        let credentials = credentials_for(&uri).map_err(|error| format!("{error}"))?;
        let mut names = Vec::new();
        for (label, first_name) in ACCOUNTS {
            let avatar = credentials
                .select(Some(label))
                .map_err(|error| format!("{label}: {error}"))?;
            assert_eq!(avatar.first(), first_name);
            assert_eq!(avatar.login_uri(), Some("http://127.0.0.1:12345/"));
            names.push(avatar.first().to_owned());
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ACCOUNTS.len(), "the accounts are not distinct");
        Ok(())
    }

    /// The offline list and the registry agree: every name resolves, every one
    /// of them declares the fake grid, and no case declares the fake grid
    /// without being listed. A list that drifted either way would silently stop
    /// running a case the pre-commit suite believes it runs.
    #[test]
    fn the_offline_list_matches_the_registry() {
        for name in OFFLINE_CASES {
            let test = find(name);
            assert!(test.is_some(), "the offline case {name} is not registered");
            assert!(
                test.is_some_and(|test| test.grids().contains(&Grid::Fake)),
                "the offline case {name} does not declare the fake grid"
            );
        }
        for test in crate::registry::registry() {
            assert_eq!(
                test.grids().contains(&Grid::Fake),
                OFFLINE_CASES.contains(&test.name()),
                "{} declares the fake grid but is not in OFFLINE_CASES (or the reverse)",
                test.name()
            );
        }
    }
}
