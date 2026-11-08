//! End-to-end test for the viewer's own avatar being moved by the grid,
//! through the automation driver alone, on both backends
//! ([[gridspec-agent-movement]]).
//!
//! The simulator moves the avatar; the viewer states which keys are held and,
//! on Second Life, says when the avatar's landing and pre-jump animations have
//! played out (`FINISH_ANIM`) — until it does, the simulator holds the avatar
//! where it stands (`book/src/gridspec/movement.md`). So on that grid each
//! step here is also a check of that handshake:
//!
//! - the forward key walks the avatar from where the login put it, which a
//!   Second Life simulator allows only once the arrival's landing animation
//!   has been reported finished;
//! - a tap of the jump key takes it off the ground, which needs the pre-jump
//!   reported finished;
//! - and the forward key walks it again afterwards, past the jump's landing.
//!
//! After each walk the viewer must hold the collision (foot) plane the grid
//! reports under the avatar — a floor beneath it and not under the land —
//! since that plane is what the viewer stands the avatar and its feet on
//! ([[viewer-avatar-ground-from-collision-plane]]). Once it stands, both feet
//! must be planted: the foot IK fully applied and each ankle a sole's height
//! over its ground. An uploaded stand, as an animation overrider plays one on
//! a live grid, keeps its own legs, so the feet go unchecked there. A second
//! stage, on the fake grid only, stands the avatar on a slope and checks that
//! one foot rests higher than the other.
//!
//! It runs on the fake grid, whose avatar controller moves the avatar to each
//! flavour's measurements (`sl_fake_grid::movement`) — the Second Life flavour
//! holding the pre-jump for the viewer's `FINISH_ANIM` as Second Life does —
//! and on a live one: `SL_E2E_GRID=opensim` or `SL_E2E_GRID=aditi`, under
//! `--profile live`. On the local OpenSim it needs a start clear of the test
//! content the default spot stands among, which lets an avatar walk 1.2 m:
//! `SL_E2E_START='uri:Northeast Region&70&180&26'`.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use sl_automation_proto::{AgentReadout, FeetReadout, FootReadout};
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::{Heightfield, RegionConfig, TerrainFixture};
    use sl_proto::RegionCoordinates;
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How many frames the forward key is held at a time. Frames are not
    /// time — a headless viewer in an empty region runs hundreds a second, and
    /// one loading a busy mainland region a handful — so the hold is short
    /// enough to be answered at any frame rate, and repeated until the avatar
    /// has gone [`WALKED`] or [`WALK_WITHIN`] has passed.
    const WALK_FRAMES: u32 = 30;

    /// How long a walk may take.
    const WALK_WITHIN: Duration = Duration::from_secs(120);

    /// How far a walk must have taken the avatar: more than the metre an
    /// avatar coasts after one step, and well short of anything that stands
    /// in the way on a live grid.
    const WALKED: f32 = 3.0;

    /// How many frames the jump key is held: a tap, let go long before the
    /// half second that would make it a take-off.
    const JUMP_FRAMES: u32 = 6;

    /// How high a jump must have taken the avatar: Second Life's reaches four
    /// metres and OpenSim's five and a half.
    const JUMPED: f32 = 1.5;

    /// How long the grid is given to show a movement it was asked for.
    const SHOWN_WITHIN: Duration = Duration::from_secs(10);

    /// How long the viewer may take to learn where its avatar is.
    const POSITION_WITHIN: Duration = Duration::from_secs(60);

    /// How often the avatar's position is read while waiting.
    const READ_EVERY: Duration = Duration::from_millis(50);

    /// Where the grid says the avatar of `agent` is.
    fn position_of(agent: &AgentReadout) -> Result<[f32; 3], BodyError> {
        agent
            .position
            .ok_or_else(|| format!("the viewer reports no position: {agent:?}").into())
    }

    /// How high a position is.
    const fn height(at: [f32; 3]) -> f32 {
        let [_x, _y, z] = at;
        z
    }

    /// The distance along the ground between two positions.
    fn ground_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
        let [ax, ay, _az] = a;
        let [bx, by, _bz] = b;
        (ax - bx).hypot(ay - by)
    }

    /// Read the avatar's position until `holds` accepts one or
    /// [`SHOWN_WITHIN`] has passed; the last position either way, and the
    /// highest seen.
    async fn position_until(
        alpha: &Viewer,
        holds: impl Fn([f32; 3]) -> bool,
    ) -> Result<([f32; 3], f32), BodyError> {
        let started = std::time::Instant::now();
        let mut last = position_of(&alpha.agent().await?)?;
        let mut highest = height(last);
        while !holds(last) && started.elapsed() < SHOWN_WITHIN {
            tokio::time::sleep(READ_EVERY).await;
            last = position_of(&alpha.agent().await?)?;
            highest = highest.max(height(last));
        }
        Ok((last, highest))
    }

    /// The smallest up component a foot plane may have and still be a floor:
    /// 60° from level. Second Life lets an avatar stand on steeper ground than
    /// one might think (46° measured on a Mauve bank, `nz` 0.69), and a plane
    /// with no up component at all, `[0, 0, 0, 1]`, is the "nothing under me"
    /// both grids send while the avatar is in the air.
    const FLOOR_UP: f32 = 0.5;

    /// How long an avatar may take to come to stand on a foot plane: Second
    /// Life drops a fresh login in from metres above the ground, and a walk
    /// can end before the fall does.
    const PLANE_WITHIN: Duration = Duration::from_secs(30);

    /// How far below the avatar's centre its foot plane may lie: an avatar's
    /// whole height, with room to spare.
    const PLANE_BELOW: f32 = 2.5;

    /// How far a foot plane may lie under the land height: it is the surface
    /// the avatar rests on, so never metres beneath the ground.
    const PLANE_UNDER_LAND: f32 = 0.5;

    /// Wait until the viewer holds a foot plane the avatar stands on (see
    /// [`foot_plane_holds`]), or [`PLANE_WITHIN`] has passed; the last
    /// reading's fault then.
    async fn stands_on_a_foot_plane(alpha: &Viewer) -> Result<(), BodyError> {
        let started = std::time::Instant::now();
        loop {
            let held = foot_plane_holds(&alpha.agent().await?);
            if held.is_ok() || started.elapsed() >= PLANE_WITHIN {
                return held;
            }
            tokio::time::sleep(READ_EVERY).await;
        }
    }

    /// Whether the viewer holds the foot plane the grid reports under a
    /// standing avatar: a floor (up-facing), under the avatar's centre and
    /// within its height, and not beneath the land. This is the surface the
    /// viewer stands the avatar and its feet on.
    fn foot_plane_holds(agent: &AgentReadout) -> Result<(), BodyError> {
        let [x, y, z] = position_of(agent)?;
        let [nx, ny, nz, w] = agent
            .foot_plane
            .ok_or_else(|| format!("the viewer holds no foot plane: {agent:?}"))?;
        if nz < FLOOR_UP {
            return Err(format!("the foot plane is no floor: {agent:?}").into());
        }
        let plane_z = (w - nx * x - ny * y) / nz;
        let below = z - plane_z;
        if !(0.0..=PLANE_BELOW).contains(&below) {
            return Err(format!(
                "the foot plane lies {below} m below the avatar's centre: {agent:?}"
            )
            .into());
        }
        if let Some(land) = agent.ground_height
            && plane_z < land - PLANE_UNDER_LAND
        {
            return Err(format!(
                "the foot plane lies {} m under the land: {agent:?}",
                land - plane_z
            )
            .into());
        }
        Ok(())
    }

    /// How long a stopped avatar may take to stand with its feet planted:
    /// the walk's coast, the stand animation and the foot IK's ease-in.
    const FEET_WITHIN: Duration = Duration::from_secs(30);

    /// The band a planted ankle sits above the ground under it, metres: the
    /// ankle joint is a sole's height up, never below the ground (a foot sunk
    /// into it) and never well above it (a foot floating). Measured at 5 to
    /// 12 cm on the fake grid's flat ground and slope and 8 cm on OpenSim.
    const ANKLE_CLEARANCE: core::ops::RangeInclusive<f32> = 0.02..=0.2;

    /// How far the two ankles' clearances may differ, metres: a planted pair
    /// stands the same height over its ground, slope or not.
    const CLEARANCE_AGREES: f32 = 0.05;

    /// Wait until the viewer poses the standing avatar with both feet
    /// planted (see [`feet_hold`]), or [`FEET_WITHIN`] has passed; the feet,
    /// or the last reading's fault. [`None`] when the avatar stands in an
    /// uploaded stand rather than a built-in one, as an animation overrider
    /// plays it: such a stand keeps its own legs, in the reference viewer as
    /// here, so there are no planted feet to check.
    async fn feet_planted(alpha: &Viewer) -> Result<Option<FeetReadout>, BodyError> {
        let started = std::time::Instant::now();
        loop {
            let agent = alpha.agent().await?;
            let held = feet_hold(&agent);
            if held.is_ok() || started.elapsed() >= FEET_WITHIN {
                if let Some(feet) = agent.feet
                    && !feet.standing
                    && held.is_err()
                {
                    tracing::warn!(
                        ?feet,
                        "the avatar plays no built-in stand (an animation overrider's \
                         stand keeps its own legs), so its planted feet go unchecked"
                    );
                    return Ok(None);
                }
                return held.map(Some);
            }
            tokio::time::sleep(READ_EVERY).await;
        }
    }

    /// Whether the viewer poses the avatar standing with both feet planted:
    /// a built-in stand playing with the foot IK fully applied, ground found
    /// under each ankle, each ankle a sole's height above its ground, and the
    /// two at the same clearance.
    fn feet_hold(agent: &AgentReadout) -> Result<FeetReadout, BodyError> {
        let feet = agent
            .feet
            .ok_or_else(|| format!("the viewer poses no feet: {agent:?}"))?;
        if !feet.standing {
            return Err(format!("the avatar plays no built-in stand: {feet:?}").into());
        }
        if feet.foot_ik_weight < 1.0 {
            return Err(format!("the foot IK is not fully applied: {feet:?}").into());
        }
        let clearance = |foot: FootReadout| -> Result<f32, BodyError> {
            let ground = foot
                .ground
                .ok_or_else(|| format!("no ground under a foot: {feet:?}"))?;
            let [_x, _y, z] = foot.ankle;
            let clearance = z - ground;
            if ANKLE_CLEARANCE.contains(&clearance) {
                Ok(clearance)
            } else {
                Err(format!("an ankle stands {clearance} m over its ground: {feet:?}").into())
            }
        };
        let (left, right) = (clearance(feet.left)?, clearance(feet.right)?);
        if (left - right).abs() > CLEARANCE_AGREES {
            return Err(format!(
                "the ankles stand {left} m and {right} m over their ground: {feet:?}"
            )
            .into());
        }
        Ok(feet)
    }

    /// Where the avatar is, once the viewer knows: a settled scene on a live
    /// grid can come before the first update that places the own avatar.
    async fn position_known(alpha: &Viewer) -> Result<[f32; 3], BodyError> {
        let started = std::time::Instant::now();
        loop {
            let agent = alpha.agent().await?;
            if let Some(at) = agent.position {
                return Ok(at);
            }
            if started.elapsed() >= POSITION_WITHIN {
                return position_of(&agent);
            }
            tokio::time::sleep(READ_EVERY).await;
        }
    }

    /// Hold the forward key, again and again, until the avatar has gone
    /// [`WALKED`]; how far it went.
    async fn walk(alpha: &Viewer) -> Result<f32, BodyError> {
        let from = position_known(alpha).await?;
        let started = std::time::Instant::now();
        let mut gone = 0.0;
        while started.elapsed() < WALK_WITHIN {
            alpha.hold("ArrowUp", WALK_FRAMES).await?;
            gone = ground_distance(position_of(&alpha.agent().await?)?, from);
            if gone >= WALKED {
                return Ok(gone);
            }
        }
        // The last hold's steps may still be on their way back from the grid.
        let (to, _highest) =
            position_until(alpha, |at| ground_distance(at, from) >= WALKED).await?;
        Ok(gone.max(ground_distance(to, from)))
    }

    /// **Walking and jumping**: the forward key walks the
    /// avatar from where the login put it, a tap of the jump key takes it off
    /// the ground, and the forward key walks it again once it is down.
    #[test]
    fn the_own_avatar_walks_and_jumps_on_a_grid_that_moves_it() -> Result<(), TestError> {
        StageBuilder::new("movement")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;

                let walked = walk(alpha).await?;
                assert!(
                    walked >= WALKED,
                    "the forward key moved the avatar {walked} m from where it logged in"
                );
                stands_on_a_foot_plane(alpha).await?;
                let _feet = feet_planted(alpha).await?;

                let ground = height(position_of(&alpha.agent().await?)?);
                alpha.hold("PageUp", JUMP_FRAMES).await?;
                let (_at, highest) =
                    position_until(alpha, |at| height(at) - ground >= JUMPED).await?;
                assert!(
                    highest - ground >= JUMPED,
                    "a tap of the jump key took the avatar {} m off the ground",
                    highest - ground
                );
                // Back down before the next walk: at rest within a step of
                // the height it jumped from.
                let (_down, _highest) =
                    position_until(alpha, |at| (height(at) - ground).abs() < 0.5).await?;

                let walked = walk(alpha).await?;
                assert!(
                    walked >= WALKED,
                    "the forward key moved the avatar {walked} m after its jump"
                );
                stands_on_a_foot_plane(alpha).await?;
                let _feet = feet_planted(alpha).await?;
                Ok(())
            })?;
        Ok(())
    }

    /// The ridge the slope stage stands on: level along west to east, rising
    /// from the south edge to the centre line at about 21°, so an avatar
    /// facing along it has one foot uphill of the other.
    const RIDGE: Heightfield = Heightfield::Ridge {
        base: 20.0,
        peak: 70.0,
    };

    /// Where the slope stage's avatar starts: halfway up the ridge's south
    /// flank, well clear of its crest.
    const ON_THE_FLANK: (f32, f32) = (128.0, 64.0);

    /// How much higher the ground under one foot must be than under the
    /// other for the stage to be standing on a slope at all, metres.
    const UPHILL: f32 = 0.02;

    /// **Feet on a slope**: an avatar that walks along a hillside and stops
    /// stands with both feet planted on it — one higher than the other, each
    /// a sole's height over its own ground — rather than one buried in the
    /// hill and the other in the air.
    #[test]
    fn the_own_avatar_stands_with_both_feet_on_a_slope() -> Result<(), TestError> {
        let (east, north) = ON_THE_FLANK;
        StageBuilder::new("movement_slope")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .region(RegionConfig {
                terrain: TerrainFixture::default().with_heights(RIDGE),
                ..RegionConfig::default()
            })
            .start_position(RegionCoordinates::new(
                east,
                north,
                RIDGE.height_at(east, north) + 1.0,
            ))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let walked = walk(alpha).await?;
                assert!(
                    walked >= WALKED,
                    "the forward key moved the avatar {walked} m along the slope"
                );
                stands_on_a_foot_plane(alpha).await?;
                // The fake grid plays the built-in stand, so the feet are
                // always there to check.
                let feet = feet_planted(alpha)
                    .await?
                    .ok_or("the avatar plays no built-in stand on the fake grid")?;
                let (Some(left), Some(right)) = (feet.left.ground, feet.right.ground) else {
                    return Err(format!("no ground under a foot: {feet:?}").into());
                };
                assert!(
                    (left - right).abs() >= UPHILL,
                    "the feet stand on level ground, not the slope: {feet:?}"
                );
                Ok(())
            })?;
        Ok(())
    }
}
