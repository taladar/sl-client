//! End-to-end tests for what a viewer does with its camera and its avatar's
//! facing when it arrives somewhere, on the fake grid, through the automation
//! driver alone, on both backends ([[test-e2e-sweep-single-viewer-ui]]):
//!
//! - a flycam parked away from the avatar is brought back behind it by a
//!   distant teleport, and a teleport within the region keeps the camera
//!   where it was;
//! - a teleport arrival faces the avatar the way the destination says, and
//!   nothing turns it back; a crossing turns nothing.

#[cfg(test)]
mod test {
    use core::f32::consts::FRAC_PI_2;
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{
        AgentReadout, CameraView, Locator, Probe, ProbeReadout, Role, StateObservation,
    };
    use sl_e2e::{BodyError, Need, Stage, StageBuilder};
    use sl_fake_grid::RegionConfig;
    use sl_proto::{RegionCoordinates, Vector};
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the grid has to answer may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// How long a teleport may take, handover included.
    const TELEPORT: Duration = Duration::from_secs(120);

    /// The region ten regions east of home: a teleport there is distant.
    const FAR: &str = "Far Region";

    /// The region east of home: a crossing's other side.
    const NEXT_DOOR: &str = "Next Door";

    /// How many frames the flycam flies forward: at its 10 m/s and the
    /// headless viewer's 60 Hz, ten metres.
    const FLY_FRAMES: u32 = 60;

    /// How far a parked flycam must have flown from where it started.
    const FLOWN: f32 = 4.0;

    /// How close a reset camera sits to the avatar it frames.
    const BEHIND_WITHIN: f32 = 15.0;

    /// How far a heading may be from the one stated, in radians.
    const HEADING_SLOP: f32 = 0.05;

    /// A stage for `name` with the one viewer `Alpha`, on the home region and
    /// `others`.
    fn stage(name: &str, others: impl IntoIterator<Item = RegionConfig>) -> StageBuilder {
        others.into_iter().fold(
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .region(RegionConfig::default()),
            StageBuilder::region,
        )
    }

    /// A region `east` regions east of home, called `name`.
    fn east(name: &str, east: u32) -> RegionConfig {
        let home = RegionConfig::default();
        RegionConfig {
            name: name.to_owned(),
            grid_x: home.grid_x.saturating_add(east),
            ..home
        }
    }

    /// The straight-line distance between two region-local points.
    fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
        let [ax, ay, az] = a;
        let [bx, by, bz] = b;
        ((ax - bx).powi(2) + (ay - by).powi(2) + (az - bz).powi(2)).sqrt()
    }

    /// `angle` less `target`, wrapped to `-π..=π`.
    fn turned(angle: f32, target: f32) -> f32 {
        (angle - target + core::f32::consts::PI).rem_euclid(core::f32::consts::TAU)
            - core::f32::consts::PI
    }

    /// The agent's camera eye, or an error saying the viewer reports none.
    fn eye_of(agent: &AgentReadout) -> Result<[f32; 3], BodyError> {
        agent
            .camera_eye
            .ok_or_else(|| format!("the viewer reports no camera eye: {agent:?}").into())
    }

    /// Whether the camera of `agent` sits close behind the avatar: within
    /// [`BEHIND_WITHIN`], on the far side of the avatar from where it faces.
    fn framed_from_behind(agent: &AgentReadout) -> bool {
        let (Some(eye), Some(at), Some(heading)) =
            (agent.camera_eye, agent.position, agent.heading)
        else {
            return false;
        };
        let [ex, ey, _ez] = eye;
        let [ax, ay, _az] = at;
        let ahead = (ex - ax) * heading.cos() + (ey - ay) * heading.sin();
        distance(eye, at) <= BEHIND_WITHIN && ahead < 0.0
    }

    /// Read the agent until `holds` accepts it, a frame or two after what
    /// changed it; the last readout either way.
    async fn agent_until(
        alpha: &Viewer,
        holds: impl Fn(&AgentReadout) -> bool,
    ) -> Result<AgentReadout, BodyError> {
        let mut last = alpha.agent().await?;
        for _read in 0..60 {
            if holds(&last) {
                break;
            }
            last = alpha.agent().await?;
        }
        Ok(last)
    }

    /// Switch to the flycam and fly it forward, answering where it parked.
    async fn park_flycam(alpha: &Viewer) -> Result<[f32; 3], BodyError> {
        let started = eye_of(&alpha.agent().await?)?;
        alpha.press("Alt+Shift+F").await?;
        let _flying = alpha
            .expect_state(Probe::Agent)
            .at("/camera")
            .to_equal(json!(CameraView::Flycam))
            .await?;
        alpha.hold("w", FLY_FRAMES).await?;
        let parked = agent_until(alpha, |agent| {
            agent
                .camera_eye
                .is_some_and(|eye| distance(eye, started) >= FLOWN)
        })
        .await?;
        let eye = eye_of(&parked)?;
        assert!(
            distance(eye, started) >= FLOWN,
            "the flycam flew from {started:?} to {eye:?}"
        );
        Ok(eye)
    }

    /// Teleport to `region` through the world map's search, and wait until
    /// the agent is there; the map is closed again afterwards.
    async fn teleport_by_map(alpha: &Viewer, region: &str) -> Result<(), BodyError> {
        let _opened = alpha
            .menu_path(&["menu-bar-world", "menu-bar-world-map"])
            .await?;
        let map = alpha.ui().window("worldmap");
        let _typed = map
            .test_id("worldmap:search")
            .role(Role::Textbox)
            .fill(region)
            .await?;
        let _picked = map
            .get(Locator::role(Role::ListItem).named(region))
            .timeout(WAIT)
            .click()
            .await?;
        let _asked = map
            .test_id("worldmap-button:teleport-selected")
            .click()
            .await?;
        let _arrived = alpha
            .expect_state(Probe::Agent)
            .at("/region/name")
            .timeout(TELEPORT)
            .to_equal(json!(region))
            .await?;
        let _closed = alpha
            .menu_path(&["menu-bar-world", "menu-bar-world-map"])
            .await?;
        Ok(())
    }

    /// **The camera after a teleport**: a flycam flown away from the avatar
    /// is, after a teleport to a distant region, back in third person close
    /// behind the avatar — its old coordinates mean nothing there. Parked
    /// again and teleported within the region, it stays exactly where it was:
    /// that framing may be how the destination was chosen.
    #[test]
    fn a_distant_teleport_resets_the_camera_and_a_local_one_keeps_it() -> Result<(), TestError> {
        stage("camera_after_teleport", [east(FAR, 10)])
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let _parked = park_flycam(alpha).await?;
                teleport_by_map(alpha, FAR).await?;
                let reset = agent_until(alpha, |agent| {
                    agent.camera == Some(CameraView::ThirdPerson) && framed_from_behind(agent)
                })
                .await?;
                assert_eq!(
                    reset.camera,
                    Some(CameraView::ThirdPerson),
                    "a distant teleport leaves the flycam"
                );
                assert!(
                    framed_from_behind(&reset),
                    "the camera is back behind the avatar: {reset:?}"
                );

                let parked = park_flycam(alpha).await?;
                let agent = stage.agent("Alpha").await?;
                let landing = RegionCoordinates::new(140.0, 120.0, 25.0);
                let _here = stage
                    .grid()?
                    .teleport_agent(
                        &agent,
                        FAR,
                        landing,
                        Vector {
                            x: 1.0,
                            y: 0.0,
                            z: 0.0,
                        },
                    )
                    .await
                    .map_err(|error| format!("the local teleport: {error}"))?;
                let moved = agent_until(alpha, |agent| {
                    agent
                        .position
                        .is_some_and(|at| distance(at, [140.0, 120.0, 25.0]) < 2.0)
                })
                .await?;
                assert_eq!(
                    moved.camera,
                    Some(CameraView::Flycam),
                    "a local teleport keeps the flycam"
                );
                let kept = eye_of(&moved)?;
                assert!(
                    distance(kept, parked) < 0.05,
                    "a local teleport keeps the camera where it was: {parked:?} → {kept:?}"
                );
                Ok(())
            })?;
        Ok(())
    }

    /// **The facing on arrival**: the grid teleports the avatar to a distant
    /// region facing north; on the frame the agent is first there it faces
    /// north, and once the destination has said everything it has to say it
    /// still does. Walked over the border into the region next door facing
    /// north, with the crossing's motion pointing east, it still faces north.
    #[test]
    fn a_teleport_arrival_faces_the_stated_way_and_a_crossing_turns_nothing()
    -> Result<(), TestError> {
        stage("arrival_facing", [east(FAR, 10), east(NEXT_DOOR, 11)])
            .needs(Need::GridControl)
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let north = Vector {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                };
                let before = alpha.agent().await?.heading.unwrap_or_default();
                assert!(
                    turned(before, FRAC_PI_2).abs() > HEADING_SLOP,
                    "the avatar already faced north before the teleport: {before}"
                );
                let agent = stage.agent("Alpha").await?;
                let arrival = alpha
                    .expect_state(Probe::Agent)
                    .at("/region/name")
                    .timeout(TELEPORT)
                    .to_equal(json!(FAR));
                let (arrived, landed) = tokio::join!(
                    arrival,
                    stage.grid()?.teleport_agent(
                        &agent,
                        FAR,
                        RegionCoordinates::new(128.0, 128.0, 25.0),
                        north,
                    )
                );
                let _far = landed.map_err(|error| format!("the teleport: {error}"))?;
                let first = match arrived? {
                    StateObservation::Probe {
                        readout: ProbeReadout::Agent(agent),
                    } => agent,
                    other => return Err(format!("not an agent readout: {other:?}").into()),
                };
                let heading = first.heading.unwrap_or_default();
                assert!(
                    turned(heading, FRAC_PI_2).abs() <= HEADING_SLOP,
                    "on its first frame there the avatar faces north, not {heading}"
                );
                alpha.wait_until_quiet(WAIT).await?;
                let settled = alpha.agent().await?.heading.unwrap_or_default();
                assert!(
                    turned(settled, FRAC_PI_2).abs() <= HEADING_SLOP,
                    "nothing turned the avatar back: it faces {settled}"
                );

                let agent = stage.agent("Alpha").await?;
                let _crossed = stage
                    .grid()?
                    .cross_agent(
                        &agent,
                        NEXT_DOOR,
                        RegionCoordinates::new(2.0, 128.0, 25.0),
                        Vector {
                            x: 3.0,
                            y: 0.0,
                            z: 0.0,
                        },
                    )
                    .await
                    .map_err(|error| format!("the crossing: {error}"))?;
                let _over = alpha
                    .expect_state(Probe::Agent)
                    .at("/region/name")
                    .timeout(WAIT)
                    .to_equal(json!(NEXT_DOOR))
                    .await?;
                alpha.wait_until_quiet(WAIT).await?;
                let crossed = alpha.agent().await?.heading.unwrap_or_default();
                assert!(
                    turned(crossed, FRAC_PI_2).abs() <= HEADING_SLOP,
                    "a crossing turned the avatar to {crossed}"
                );
                Ok(())
            })?;
        Ok(())
    }
}
