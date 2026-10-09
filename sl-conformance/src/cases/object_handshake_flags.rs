//! Log in once for each thing a viewer can say about its object cache in its
//! `RegionHandshakeReply`, and count what the object-update stream does about
//! it.
//!
//! The reply's `Flags` hold three bits
//! ([`RegionHandshakeReplyFlags`]): "send all cacheable objects", "my cache is
//! empty, do not probe it", and "I understand an appearance message about my
//! own avatar". Each leg is a fresh login with one combination and a watch of
//! the arrival.

use std::time::Duration;

use sl_client_tokio::{ObjectUpdateForm, RegionHandshakeReplyFlags};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::measured::Measured;
use crate::object_stream::{World, record, watch};
use crate::registry::{GridTest, TestFuture};
use crate::support::{check, check_eq, is_opensim};

/// Where the measured answers are written down.
const SOURCE: &str = "book/src/gridspec/objects.md (object-handshake-flags, 2026-10-09)";

/// Whether an arriving agent is sent an `AvatarAppearance` about itself
/// though its handshake reply did not say it understands one. OpenSim sends
/// one whatever the flags; Second Life only when asked.
const OWN_APPEARANCE_UNASKED: Measured<bool> = Measured {
    second_life: false,
    opensim: true,
    source: SOURCE,
};

/// The OpenSim start location: the middle of the region this workspace's test
/// objects are in.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long each arrival is watched on a live grid.
const ARRIVAL_WATCH: Duration = Duration::from_secs(25);

/// How long each arrival is watched on the fake grid, which sends everything
/// at once.
const FAKE_ARRIVAL_WATCH: Duration = Duration::from_secs(5);

/// The case's budget: on a grid that rate-limits logins each leg waits out
/// two minutes first.
const CASE_TIMEOUT: Duration = Duration::from_secs(1_200);

/// The handshake-flag combinations a fresh login is made with, and the name
/// each leg goes by.
const FLAG_LEGS: [(&str, u32); 5] = [
    ("none", 0),
    ("cache_all", RegionHandshakeReplyFlags::CACHE_ALL),
    ("cache_empty", RegionHandshakeReplyFlags::CACHE_EMPTY),
    (
        "self_appearance",
        RegionHandshakeReplyFlags::SUPPORTS_SELF_APPEARANCE,
    ),
    (
        "reference",
        RegionHandshakeReplyFlags::CACHE_ALL
            | RegionHandshakeReplyFlags::CACHE_EMPTY
            | RegionHandshakeReplyFlags::SUPPORTS_SELF_APPEARANCE,
    ),
];

/// Logs in with each combination of handshake flags and records the arrival
/// each one gets.
#[derive(Debug)]
pub struct ObjectHandshakeFlags;

impl GridTest for ObjectHandshakeFlags {
    fn name(&self) -> &'static str {
        "object-handshake-flags"
    }

    fn description(&self) -> &'static str {
        "Log in with each combination of RegionHandshakeReply flags and count the object stream"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let duration = if grid.is_fake() {
                FAKE_ARRIVAL_WATCH
            } else {
                ARRIVAL_WATCH
            };
            for (name, bits) in FLAG_LEGS {
                let flags = RegionHandshakeReplyFlags(bits);
                ctx.primary().disconnect().await?;
                ctx.primary().relogin_with_handshake_flags(flags).await?;
                let mut world = World::new();
                let leg = watch(ctx.primary(), &mut world, duration).await?;
                record(ctx.metrics(), name, &leg);

                if flags.contains(RegionHandshakeReplyFlags::SUPPORTS_SELF_APPEARANCE) {
                    check(
                        leg.own_appearance,
                        &format!(
                            "{name}: no AvatarAppearance about the agent itself came, though \
                             the handshake reply said the viewer understands one ({SOURCE})"
                        ),
                    )?;
                } else {
                    OWN_APPEARANCE_UNASKED.check(
                        &format!("{name}: whether the agent's own appearance comes unasked"),
                        grid,
                        &leg.own_appearance,
                    )?;
                }
                let probed = leg.named(ObjectUpdateForm::Cached);
                if flags.contains(RegionHandshakeReplyFlags::CACHE_EMPTY) {
                    check_eq(
                        &format!("{name}: cache probes sent to a viewer whose cache is empty"),
                        &probed,
                        &0,
                    )?;
                } else if !grid.is_fake() {
                    // The fake grid sends no cache probes at all yet
                    // (`server-fake-grid-object-update-forms`).
                    check(
                        probed > 0,
                        &format!(
                            "{name}: no cache probe came, though the viewer did not say its \
                             cache is empty ({SOURCE})"
                        ),
                    )?;
                }
            }
            Ok(())
        })
    }
}
