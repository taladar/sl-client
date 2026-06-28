//! Request the current region's info and record its limits.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};

/// How long to wait for the region to become active.
const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait for the region info reply.
const INFO_TIMEOUT: Duration = Duration::from_secs(30);

/// Requests region info and records the response time and the max-agents limit.
#[derive(Debug)]
pub struct RegionInfo;

impl GridTest for RegionInfo {
    fn name(&self) -> &'static str {
        "region-info"
    }

    fn description(&self) -> &'static str {
        "Request the current region's info and limits"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let start = Instant::now();
            session.send(Command::RequestRegionInfo).await?;
            let max_agents = session
                .wait_for(INFO_TIMEOUT, |event| match event {
                    Event::RegionLimits(limits) => Some(limits.max_agents),
                    _ => None,
                })
                .await?;
            let elapsed = start.elapsed().as_secs_f64();

            let metrics = ctx.metrics();
            metrics.set_timing("region_info_secs", elapsed);
            metrics.set("max_agents", max_agents);
            Ok(())
        })
    }
}
