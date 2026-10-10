//! End-to-end tests for the double-click teleport, on the fake grid, through
//! the automation driver alone, on both backends
//! ([[viewer-automation-ground-aim]]): the gesture is switched on with its
//! **Ctrl+Shift+D** chord, and a double-click is aimed at the surfaces it is
//! used on —
//!
//! - the ground within the region: the agent lands where the double-click
//!   did, and a parked flycam stays exactly where it was;
//! - an object across the border, and the neighbour's ground beyond it: both
//!   are near teleports, so the parked camera stays there too;
//! - another avatar: the agent lands on it.

#[cfg(test)]
mod test {
    use core::time::Duration;
    use std::time::Instant;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{
        AgentReadout, CameraView, LogStream, Probe, WorldKind, WorldLocator,
    };
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::RegionConfig;
    use sl_fake_grid::fixtures::border::{MARKER_OBJECT, MARKER_SIZE, MARKER_X, MARKER_Y, border};
    use sl_fake_grid::fixtures::{NpcAppearance, NpcFixture, RegionFixture};
    use sl_fake_grid::scenario::{STOCK_TERRAIN_HEIGHT_M, default_world};
    use sl_fake_grid::world::AvatarIdentity;
    use sl_proto::{AgentKey, RegionCoordinates, RegionLocalObjectId, Uuid, Vector};
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long a teleport may take, handover included.
    const TELEPORT: Duration = Duration::from_secs(120);

    /// The region east of home, across the border.
    const NEXT_DOOR: &str = "Next Door";

    /// How many frames at a time the flycam flies backwards — back and up,
    /// since it looks down at the avatar, so everything ahead stays in view
    /// (flown forward it would sink into the ground). Few, and repeated until
    /// it has flown [`FLOWN`]: the flycam covers 10 m a second, so how far a
    /// number of frames takes it is how long they took, and thirty of them
    /// were five metres on an idle machine and sixty-one on a busy one.
    const FLY_FRAMES: u32 = 2;

    /// How far a parked flycam must have flown from where it started.
    const FLOWN: f64 = 3.0;

    /// How many times the flycam is flown [`FLY_FRAMES`] before the test
    /// gives up on it moving: at 60 Hz one flight is a third of a metre.
    const FLIGHTS: u32 = 60;

    /// How far ahead of the avatar the ground double-click lands, metres.
    const AHEAD: f32 = 14.0;

    /// How far to the avatar's left of its line of sight, metres, so the
    /// avatar itself is not in the way.
    const ASIDE: f32 = 4.0;

    /// How close, horizontally, the agent must land to where the
    /// double-click did, metres.
    const LANDING_SLOP: f32 = 1.0;

    /// How far a kept camera may have moved, metres.
    const KEPT: f64 = 0.05;

    /// The other avatar the avatar test lands on.
    const NPC_AGENT: u128 = 0x0d0b_1ec1_1c4e;

    /// Its region-local id, clear of the stock scene's.
    const NPC_LOCAL_ID: u32 = 0x7E58;

    /// How far east of the region centre it stands, metres: ahead of an agent
    /// that logs in at the centre facing east.
    const NPC_AHEAD: f32 = 12.0;

    /// How far north of the centre line it stands, so the agent's own avatar
    /// is not in the way.
    const NPC_ASIDE: f32 = 4.0;

    /// A stage for `name` with the one viewer `Alpha`, on `home` and
    /// `others`.
    fn stage(
        name: &str,
        home: RegionConfig,
        others: impl IntoIterator<Item = RegionConfig>,
    ) -> StageBuilder {
        others.into_iter().fold(
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .region(home),
            StageBuilder::region,
        )
    }

    /// The region `east` regions east of home, called `name`.
    fn east(name: &str, east: u32) -> RegionConfig {
        let home = RegionConfig::default();
        RegionConfig {
            name: name.to_owned(),
            grid_x: home.grid_x.saturating_add(east),
            ..home
        }
    }

    /// The horizontal distance between two points.
    fn across(a: [f32; 3], b: [f32; 3]) -> f32 {
        let [ax, ay, _az] = a;
        let [bx, by, _bz] = b;
        (ax - bx).hypot(ay - by)
    }

    /// `point`, region-local in the region with `handle`, in global metres —
    /// what stays comparable when the agent changes region under it.
    fn global(handle: u64, point: [f32; 3]) -> Result<[f64; 3], BodyError> {
        let [x, y, z] = point;
        let east = u32::try_from(handle >> 32)?;
        let north = u32::try_from(handle & 0xffff_ffff)?;
        Ok([
            f64::from(east) + f64::from(x),
            f64::from(north) + f64::from(y),
            f64::from(z),
        ])
    }

    /// The straight-line distance between two global points.
    fn moved(a: [f64; 3], b: [f64; 3]) -> f64 {
        let [ax, ay, az] = a;
        let [bx, by, bz] = b;
        ((ax - bx).powi(2) + (ay - by).powi(2) + (az - bz).powi(2)).sqrt()
    }

    /// The camera eye of `agent` in global metres.
    fn global_eye(agent: &AgentReadout) -> Result<[f64; 3], BodyError> {
        let handle = agent
            .region
            .as_ref()
            .ok_or("the viewer reports no region")?
            .handle;
        let eye = agent
            .camera_eye
            .ok_or_else(|| format!("the viewer reports no camera eye: {agent:?}"))?;
        global(handle, eye)
    }

    /// Read the agent until `holds` accepts it, for as long as a teleport
    /// may take; the last readout either way.
    async fn agent_until(
        alpha: &Viewer,
        holds: impl Fn(&AgentReadout) -> bool,
    ) -> Result<AgentReadout, BodyError> {
        let started = Instant::now();
        let mut last = alpha.agent().await?;
        while !holds(&last) && started.elapsed() < TELEPORT {
            last = alpha.agent().await?;
        }
        Ok(last)
    }

    /// Switch the double-click teleport on with its chord, then switch to
    /// the flycam and fly it back; where it parked, in global metres.
    async fn park_flycam(alpha: &Viewer) -> Result<[f64; 3], BodyError> {
        alpha.press("Ctrl+Shift+D").await?;
        let started = global_eye(&alpha.agent().await?)?;
        alpha.press("Alt+Shift+F").await?;
        let _flying = alpha
            .expect_state(Probe::Agent)
            .at("/camera")
            .to_equal(json!(CameraView::Flycam))
            .await?;
        // By distance, not by frames: a short flight at a time until the eye
        // is far enough, so a slow frame costs one flight's overshoot and not
        // thirty frames' worth.
        let mut eye = started;
        for _flight in 0..FLIGHTS {
            if moved(eye, started) >= FLOWN {
                break;
            }
            alpha.hold("s", FLY_FRAMES).await?;
            eye = global_eye(&alpha.agent().await?)?;
        }
        assert!(
            moved(eye, started) >= FLOWN,
            "the flycam flew from {started:?} to {eye:?}"
        );
        Ok(eye)
    }

    /// Every event and command the viewer logged that names a teleport, one
    /// per line — what a failed landing reports.
    async fn teleport_entries(alpha: &Viewer) -> Result<String, BodyError> {
        let page = alpha
            .read_log(0, &[LogStream::Event, LogStream::Command])
            .await?;
        let mut lines = String::new();
        for entry in page
            .entries
            .iter()
            .filter(|entry| entry.kind.contains("Teleport"))
        {
            lines.push_str("\n  ");
            lines.push_str(&entry.kind);
            lines.push(' ');
            lines.push_str(&entry.detail);
        }
        Ok(lines)
    }

    /// Wait until the teleport's progress display is gone: until then it
    /// covers the middle of the window, and a click there lands on it.
    async fn display_cleared(alpha: &Viewer) -> Result<(), BodyError> {
        let _idle = alpha
            .expect_state(Probe::Agent)
            .at("/teleport/state")
            .timeout(TELEPORT)
            .to_equal(json!("idle"))
            .await?;
        Ok(())
    }

    /// Wait until the agent stands in `region` within [`LANDING_SLOP`] of
    /// `landing` (horizontally), and check the parked flycam at `parked`
    /// (global metres) did not move.
    async fn landed(
        alpha: &Viewer,
        region: &str,
        landing: [f32; 3],
        parked: [f64; 3],
    ) -> Result<(), BodyError> {
        let _there = alpha
            .expect_state(Probe::Agent)
            .at("/region/name")
            .timeout(TELEPORT)
            .to_equal(json!(region))
            .await?;
        let arrived = agent_until(alpha, |agent| {
            agent
                .position
                .is_some_and(|at| across(at, landing) < LANDING_SLOP)
        })
        .await?;
        let at = arrived
            .position
            .ok_or("the viewer reports no agent position")?;
        let journey = if across(at, landing) < LANDING_SLOP {
            String::new()
        } else {
            teleport_entries(alpha).await?
        };
        assert!(
            across(at, landing) < LANDING_SLOP,
            "the agent landed at {at:?}, not where the double-click did: {landing:?} \
             ({arrived:?}); the teleport's log: {journey}"
        );
        assert_eq!(
            arrived.camera,
            Some(CameraView::Flycam),
            "a near teleport keeps the flycam"
        );
        let kept = global_eye(&arrived)?;
        assert!(
            moved(kept, parked) < KEPT,
            "a near teleport keeps the camera where it was: {parked:?} → {kept:?}"
        );
        Ok(())
    }

    /// **A double-click on the ground** within the region: the agent lands
    /// where it did, and the parked flycam that aimed it stays exactly where
    /// it was.
    #[test]
    fn a_ground_double_click_lands_there_and_keeps_the_camera() -> Result<(), TestError> {
        stage("double_click_ground", RegionConfig::default(), []).run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            let parked = park_flycam(alpha).await?;
            let agent = alpha.agent().await?;
            let [x, y, _z] = agent.position.ok_or("no agent position")?;
            let heading = agent.heading.ok_or("no agent heading")?;
            let (sin, cos) = heading.sin_cos();
            let target_x = (x + AHEAD * cos - ASIDE * sin).clamp(1.0, 254.0);
            let target_y = (y + AHEAD * sin + ASIDE * cos).clamp(1.0, 254.0);
            let hit = alpha
                .world()
                .ground(stage.home_region(), target_x, target_y)
                .without_reveal()
                .double_click()
                .await?;
            landed(alpha, stage.home_region(), hit, parked).await
        })?;
        Ok(())
    }

    /// **Over the border**: from the home region's east edge, a double-click
    /// on the marker pillar standing just inside the region next door lands
    /// the agent on it, and one on that region's ground beyond lands it
    /// there. Both are near teleports: the parked flycam stays put.
    #[test]
    fn a_double_click_reaches_an_object_and_the_ground_over_the_border() -> Result<(), TestError> {
        let start_z = f32::from(STOCK_TERRAIN_HEIGHT_M);
        stage(
            "double_click_border",
            RegionConfig::default(),
            [border().into_region(east(NEXT_DOOR, 1))],
        )
        .start_position(RegionCoordinates::new(236.0, 128.0, start_z))
        .run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            let parked = park_flycam(alpha).await?;

            let marker = WorldLocator::full_id(MARKER_OBJECT.uuid());
            let _pillar = alpha
                .world()
                .locator(marker)
                .without_reveal()
                .double_click()
                .await?;
            // The pillar's west face: it is seen from the west.
            let west_face = [MARKER_X - MARKER_SIZE / 2.0, MARKER_Y, 0.0];
            let _there = alpha
                .expect_state(Probe::Agent)
                .at("/region/name")
                .timeout(TELEPORT)
                .to_equal(json!(NEXT_DOOR))
                .await?;
            let on_pillar = agent_until(alpha, |agent| {
                agent
                    .position
                    .is_some_and(|at| across(at, west_face) < MARKER_SIZE)
            })
            .await?;
            let at = on_pillar
                .position
                .ok_or("the viewer reports no agent position")?;
            assert!(
                across(at, west_face) < MARKER_SIZE,
                "the agent landed at {at:?}, not on the pillar's west face"
            );
            display_cleared(alpha).await?;
            let kept = global_eye(&on_pillar)?;
            assert!(
                moved(kept, parked) < KEPT,
                "a teleport onto the pillar keeps the camera: {parked:?} → {kept:?}"
            );

            let hit = alpha
                .world()
                .ground(NEXT_DOOR, 20.0, 120.0)
                .without_reveal()
                .double_click()
                .await?;
            landed(alpha, NEXT_DOOR, hit, parked).await
        })?;
        Ok(())
    }

    /// **A double-click on another avatar** lands the agent on it.
    #[test]
    fn a_double_click_on_another_avatar_lands_on_it() -> Result<(), TestError> {
        let agent = AgentKey::from(Uuid::from_u128(NPC_AGENT));
        let ground = f32::from(STOCK_TERRAIN_HEIGHT_M);
        let npc = NpcFixture::new(
            RegionLocalObjectId(NPC_LOCAL_ID),
            AvatarIdentity::new(agent, "Double", "Target"),
            Vector {
                x: 128.0 + NPC_AHEAD,
                y: 128.0 + NPC_ASIDE,
                z: ground + 0.9,
            },
        )
        .looking(NpcAppearance::default_avatar());
        let mut world = default_world();
        world.npcs.push(npc);
        let home = RegionFixture {
            world,
            ..RegionFixture::new()
        }
        .into_region(RegionConfig::default());
        stage("double_click_avatar", home, []).run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            let parked = park_flycam(alpha).await?;
            let _avatar = alpha
                .world()
                .locator(WorldLocator::kind(WorldKind::Avatar).own(false))
                .without_reveal()
                .double_click()
                .await?;
            let target = [128.0 + NPC_AHEAD, 128.0 + NPC_ASIDE, 0.0];
            landed(alpha, stage.home_region(), target, parked).await
        })?;
        Ok(())
    }
}
