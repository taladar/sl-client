//! Another avatar's appearance arrives, and names the bakes it is painted with.
//!
//! When an avatar comes into range the simulator pushes an `AvatarAppearance`
//! for it: the visual parameters that shape the body and a `TextureEntry` whose
//! baked slots name the composited textures every other client fetches and
//! wraps it in. Without it a viewer has an avatar-shaped nothing — the reference
//! client leaves it a cloud, and this workspace's leaves it a placeholder.
//!
//! The catalogue's NPC is the subject: a scripted avatar standing at the west
//! end of the prim row, painted one known colour so a render test can classify
//! it. This is the protocol half of that — the ids, not the pixels:
//!
//! - the appearance arrives for the NPC's own agent id,
//! - it carries visual parameters (a body with no shape at all is not an
//!   avatar),
//! - every baked slot the fixture declares names the fixture's texture id, and
//! - each of those ids is one the grid actually serves, proved by fetching it
//!   over the `GetTexture` capability the appearance expects a client to use.
//!
//! Fake grid only. The NPC's bakes are ids this workspace mints and pixels it
//! encodes, so "the appearance names the fixture's textures" only has a meaning
//! against the fixture; on a live grid the nearby avatars are whoever happened
//! to be standing there.

use std::time::Duration;

use sl_client_tokio::{Command, DiscardLevel, Event, TextureKey, avatar_texture};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq, count_metric, secs_metric};

/// How long to wait for the NPC's appearance. It is pushed in the arrival
/// burst, so this is a backstop rather than a budget.
const APPEARANCE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for one baked texture to arrive over `GetTexture`.
const TEXTURE_TIMEOUT: Duration = Duration::from_secs(30);

/// Reads the catalogue NPC's `AvatarAppearance` and checks its bakes are the
/// fixture's, and fetchable.
#[derive(Debug)]
pub struct AvatarAppearanceNpc;

impl GridTest for AvatarAppearanceNpc {
    fn name(&self) -> &'static str {
        "avatar-appearance-npc"
    }

    fn description(&self) -> &'static str {
        "Another avatar's AvatarAppearance names the baked textures the fixture declares"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Fake]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let npc = sl_fake_grid::fixtures::catalogue::npc();
            let agent = npc.identity.agent_id;
            let started = std::time::Instant::now();

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let appearance = session
                .wait_for(APPEARANCE_TIMEOUT, |event| match event {
                    Event::AvatarAppearance(appearance) if appearance.avatar_id == agent => {
                        Some((**appearance).clone())
                    }
                    _other => None,
                })
                .await?;
            let elapsed = started.elapsed().as_secs_f64();

            check_eq("the appearance's avatar", &appearance.avatar_id, &agent)?;
            check(
                !appearance.visual_params.is_empty(),
                "the NPC's appearance carried no visual parameters, so it describes no body",
            )?;
            check_eq(
                "visual parameters",
                &appearance.visual_params,
                &npc.appearance.visual_params,
            )?;

            // Every slot the fixture bakes, named in the appearance's texture
            // entry.
            check(
                !npc.appearance.bakes.is_empty(),
                "the catalogue NPC declares no bakes, so this case would assert nothing",
            )?;
            for bake in &npc.appearance.bakes {
                let named = appearance.texture_entry.texture_id(bake.slot);
                check_eq(
                    &format!("baked slot {}", bake.slot),
                    &named,
                    &Some(bake.texture),
                )?;
            }
            // The skirt is the one body region the fixture leaves unbaked (it is
            // also the one the reference viewer skips unless a skirt is worn), so
            // whatever stands in that slot must not be one of the NPC's bakes —
            // a bake landing there would mean the slots were laid out by
            // position rather than by name.
            let skirt = appearance
                .texture_entry
                .texture_id(avatar_texture::SKIRT_BAKED);
            check(
                !skirt.is_some_and(|id| bakes_include(&npc.appearance, id)),
                &format!(
                    "the NPC's skirt slot names {skirt:?}, which is one of the bakes the \
                     fixture declares for another region"
                ),
            )?;

            // A named bake nobody serves is a cloud with extra steps: fetch the
            // first one over the same capability a viewer would.
            let first =
                npc.appearance.bakes.first().ok_or_else(|| {
                    TestFailure::Assertion("the NPC declares no bakes".to_owned())
                })?;
            let fetch_started = std::time::Instant::now();
            let bytes = fetch_texture(session, first.texture).await?;
            let fetch_secs = fetch_started.elapsed().as_secs_f64();
            check(
                bytes > 0,
                "the NPC's first baked texture fetched no bytes, so the appearance names an \
                 id the grid does not serve",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("appearance"), elapsed);
            metrics.set_timing(&secs_metric("bake_fetch"), fetch_secs);
            metrics.set(
                &count_metric("bakes"),
                i64::try_from(npc.appearance.bakes.len()).unwrap_or(-1),
            );
            metrics.set(
                &count_metric("visual_params"),
                i64::try_from(appearance.visual_params.len()).unwrap_or(-1),
            );
            metrics.set(
                &count_metric("bake_bytes"),
                i64::try_from(bytes).unwrap_or(-1),
            );
            Ok(())
        })
    }
}

/// Whether `id` is one of the textures the fixture bakes.
fn bakes_include(appearance: &sl_fake_grid::NpcAppearance, id: TextureKey) -> bool {
    appearance.bakes.iter().any(|bake| bake.texture == id)
}

/// Fetches `texture` the way a viewer fetches a bake, and returns its size in
/// bytes.
async fn fetch_texture(session: &mut Session, texture: TextureKey) -> Result<usize, TestFailure> {
    session
        .send(Command::FetchTexture {
            texture_id: texture,
            discard_level: DiscardLevel::FULL,
        })
        .await?;
    session
        .wait_for(TEXTURE_TIMEOUT, |event| match event {
            Event::TextureReceived(received) if received.id == texture => {
                Some(Ok(received.data.len()))
            }
            Event::TextureNotFound(missing) if *missing == texture => Some(Err(())),
            _other => None,
        })
        .await?
        .map_err(|()| {
            TestFailure::Assertion(format!(
                "the grid answered `not found` for the baked texture {texture} the appearance \
                 names"
            ))
        })
}
