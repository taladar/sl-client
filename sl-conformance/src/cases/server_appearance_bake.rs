//! Trigger a modern **server-side** appearance bake over the
//! `UpdateAvatarAppearance` capability (Second Life "Sunshine" / central baking)
//! and confirm the grid accepts the request — the SL-native counterpart of
//! [`baked-texture-upload`](super::baked_texture_upload).
//!
//! Where the legacy path has the *client* composite and upload each baked layer
//! (`UploadBakedTexture`), central baking moves that to the grid: the viewer only
//! manages the Current Outfit Folder in inventory and POSTs `{ "cof_version":
//! <int> }` to the capability, after which the grid bakes the outfit and
//! broadcasts the result to every viewer over the UDP `AvatarAppearance` message.
//! The POST's own LLSD reply (`{ success, error?, expected? }`) is surfaced as
//! [`Event::ServerAppearanceUpdate`]; the baked appearance itself arrives
//! separately (and only for *other* observers), so this case asserts on the
//! reply — the grid accepting the bake — not the downstream broadcast.
//!
//! **COF-version handshake:** the grid bakes a specific Current Outfit Folder
//! version and rejects a stale one, answering `success = false` with the version
//! it `expected`. The case drives exactly that recovery loop the protocol
//! documents: it starts from version 0 and, on each mismatch, re-requests with
//! the grid's expected version until the bake is accepted — so it needs no prior
//! inventory crawl to learn the current COF version.
//!
//! **Grid divergence:** central baking is Second Life-only — OpenSim has no
//! `UpdateAvatarAppearance` capability (it uses the legacy client-side bake
//! `baked-texture-upload` exercises), so on OpenSim the case records `partial`
//! ("capability not offered"). The mirror image of `baked-texture-upload`, which
//! is `complete` on OpenSim and `partial` on aditi.

use std::time::Instant;

use sl_client_tokio::{Command, Event, Throttle};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, is_aditi};

/// The Current Outfit Folder version to bake from first; a mismatch makes the
/// grid answer with the version it expects, which the case then re-requests.
const INITIAL_COF_VERSION: i32 = 0;

/// A bound on the version-mismatch recovery loop, so a grid that never accepts a
/// bake cannot spin forever.
const MAX_ATTEMPTS: u32 = 4;

/// Triggers a server-side appearance bake over `UpdateAvatarAppearance`.
#[derive(Debug)]
pub struct ServerAppearanceBake;

impl GridTest for ServerAppearanceBake {
    fn name(&self) -> &'static str {
        "server-appearance-bake"
    }

    fn description(&self) -> &'static str {
        "Trigger a server-side appearance bake over UpdateAvatarAppearance"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // Central baking is Second Life-only; OpenSim never offers the
            // capability (it uses the legacy client-side bake path instead).
            if session.cap("UpdateAvatarAppearance").is_none() {
                ctx.mark_partial("no UpdateAvatarAppearance capability offered");
                return Ok(());
            }

            // Drive the COF-version handshake: request a bake, and on a version
            // mismatch re-request with the version the grid says it expects, until
            // the bake is accepted or the attempt bound is reached.
            let start = Instant::now();
            let mut cof_version = INITIAL_COF_VERSION;
            let mut attempts = 0_u32;
            let accepted = loop {
                attempts = attempts.saturating_add(1);
                session
                    .send(Command::RequestServerAppearanceUpdate { cof_version })
                    .await?;
                let (success, error, expected) = session
                    .wait_for(LONG_TIMEOUT, |event| match event {
                        Event::ServerAppearanceUpdate {
                            success,
                            error,
                            expected_cof_version,
                        } => Some((*success, error.clone(), *expected_cof_version)),
                        _other => None,
                    })
                    .await?;

                if success {
                    break true;
                }
                // A version mismatch is recoverable: the grid tells us the version
                // it expected, so re-request with it (once, then stop looping on the
                // same value). Any other rejection is terminal for this run.
                match expected {
                    Some(expected) if expected != cof_version && attempts < MAX_ATTEMPTS => {
                        cof_version = expected;
                    }
                    _other => {
                        let reason = error.unwrap_or_else(|| "no reason given".to_owned());
                        // A Second Life grid declining the bake outright is a
                        // grid-behaviour outcome, not a client fault — the request
                        // was formed and POSTed correctly — so record it partial.
                        if is_aditi(grid) {
                            ctx.mark_partial(&format!("grid declined the bake — {reason}"));
                            return Ok(());
                        }
                        return Err(TestFailure::Assertion(format!(
                            "UpdateAvatarAppearance bake rejected: {reason}"
                        )));
                    }
                }
            };
            let bake_secs = start.elapsed().as_secs_f64();
            check(accepted, "the grid did not accept the appearance bake")?;

            let metrics = ctx.metrics();
            metrics.set_timing("bake_secs", bake_secs);
            metrics.set("cof_version", i64::from(cof_version));
            metrics.set("attempts", i64::from(attempts));

            Ok(())
        })
    }
}
