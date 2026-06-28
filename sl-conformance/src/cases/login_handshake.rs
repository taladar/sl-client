//! Login and region handshake: the most basic liveness check on a grid.

use std::time::{Duration, Instant};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};

/// How long to wait for the first region to become active.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

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
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let start = Instant::now();
            ctx.primary().wait_for_region(HANDSHAKE_TIMEOUT).await?;
            let elapsed = start.elapsed().as_secs_f64();
            ctx.metrics().set_timing("handshake_secs", elapsed);
            Ok(())
        })
    }
}
