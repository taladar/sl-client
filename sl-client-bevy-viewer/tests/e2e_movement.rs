//! End-to-end test for the viewer's own avatar being moved by a grid that
//! moves avatars — a live one, through the automation driver alone, on both
//! backends ([[gridspec-agent-movement]]).
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
//! The fake grid decodes an `AgentUpdate` and moves nothing
//! ([[server-world-agent-movement]]), so this needs a live grid:
//! `SL_E2E_GRID=opensim` or `SL_E2E_GRID=aditi`, under `--profile live`.
//! On the local OpenSim it also needs a start clear of the test content the
//! default spot stands among, which lets an avatar walk 1.2 m:
//! `SL_E2E_START='uri:Northeast Region&70&180&26'`.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use sl_automation_proto::AgentReadout;
    use sl_e2e::{BodyError, Need, Stage, StageBuilder};
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

    /// Hold the forward key, again and again, until the avatar has gone
    /// [`WALKED`]; how far it went.
    async fn walk(alpha: &Viewer) -> Result<f32, BodyError> {
        let from = position_of(&alpha.agent().await?)?;
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

    /// **Walking and jumping on a live grid**: the forward key walks the
    /// avatar from where the login put it, a tap of the jump key takes it off
    /// the ground, and the forward key walks it again once it is down.
    #[test]
    fn the_own_avatar_walks_and_jumps_on_a_grid_that_moves_it() -> Result<(), TestError> {
        StageBuilder::new("movement")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::LiveGrid(
                "a grid that moves an avatar: the fake grid ignores AgentUpdate",
            ))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;

                let walked = walk(alpha).await?;
                assert!(
                    walked >= WALKED,
                    "the forward key moved the avatar {walked} m from where it logged in"
                );

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
                Ok(())
            })?;
        Ok(())
    }
}
