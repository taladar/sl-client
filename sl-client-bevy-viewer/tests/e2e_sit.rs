//! End-to-end test for sitting on an object and standing up again, through
//! the automation driver alone, against each flavour of the fake grid
//! ([[gridspec-sit-stand]]).
//!
//! The two live grids answer a sit alike where it matters to a viewer — an
//! `AvatarSitResponse` with its `AutoPilot` flag set, and the avatar's own
//! object re-sent as a child of the seat — and differ in three things a
//! viewer has to take as they come (`book/src/gridspec/movement.md`
//! § Sitting):
//!
//! - where the avatar is put when it stands up: a third of a metre in front
//!   of where it sat on Second Life, two thirds of a metre in front and more
//!   than half a metre up on OpenSim;
//! - what the response says of a scripted seat's position, which the viewer
//!   must not place the avatar by;
//! - and a sit on an object the region does not have, which Second Life
//!   refuses with a named alert and OpenSim does not answer at all.
//!
//! Each flavour is run through the same steps: sit by the object's pie, be
//! shown seated, stand by the same pie, be shown standing where that grid
//! puts an avatar; then sit on an object the grid has dropped without a word,
//! and be told so on the one grid that says it.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{Locator, Probe};
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::ImitatedGrid;
    use sl_proto::{AgentKey, ObjectKey, RegionLocalObjectId, Uuid, Vector};

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long the viewer is given to show what the grid sent.
    const WAIT: Duration = Duration::from_secs(60);

    /// How long a sit the grid does not answer is left before the viewer is
    /// read: long enough for an answer to have come had there been one.
    const UNANSWERED: Duration = Duration::from_secs(3);

    /// The box the test sits on.
    const SEAT: &str = "Seat";

    /// Its region-local id, clear of the stock scene's.
    const SEAT_LOCAL_ID: u32 = 0x5EA7;

    /// Where it stands: beside the stock box, in view of an avatar at the
    /// region's centre.
    const SEAT_AT: Vector = Vector {
        x: 132.0,
        y: 124.0,
        z: 25.5,
    };

    /// The object pie's slice that stands a seated avatar up.
    const STAND_UP: &str = "pie-object-stand-up";

    /// The notification Second Life's refusal of a sit raises.
    const REFUSAL: &str = "SitFailNotSameRegion";

    /// How far from where a grid puts a standing avatar the viewer may report
    /// it, in metres.
    const PLACED_WITHIN: f32 = 0.1;

    /// The seat's full id.
    fn seat_key() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(0x5EA7_0B1E))
    }

    /// Put a box named [`SEAT`] on the grid and show it to the viewer `label`.
    async fn rez_seat(stage: &Stage, label: &str) -> Result<(), BodyError> {
        let agent = stage.agent(label).await?;
        let now = agent.now();
        let mut seat = sl_fake_grid::world::box_prim(
            RegionLocalObjectId(SEAT_LOCAL_ID),
            seat_key(),
            AgentKey::from(Uuid::from_u128(0x5EA7_0A11)),
            SEAT_AT,
            Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        );
        let mut properties = sl_fake_grid::world::default_object_properties(&seat);
        properties.name = SEAT.to_owned();
        seat.properties = Some(properties);
        agent
            .with_world(|world, sim| {
                world.objects.push(seat.clone());
                sl_fake_grid::world::send_objects(sim, &[seat], now)
            })
            .await
            .map_err(|error| format!("showing the seat: {error}"))?;
        Ok(())
    }

    /// Where `flavour` puts an avatar that stands up from [`SEAT`].
    fn stands_at(flavour: ImitatedGrid) -> [f32; 3] {
        let policy = flavour.sit_policy();
        let sat = sl_fake_grid::world::SIT_TARGET_OFFSET;
        [
            SEAT_AT.x + sat.x + policy.stand_forward_m,
            SEAT_AT.y + sat.y,
            SEAT_AT.z + sat.z + policy.stand_up_m,
        ]
    }

    /// The largest difference between two positions along any axis.
    fn apart(a: [f32; 3], b: [f32; 3]) -> f32 {
        a.into_iter()
            .zip(b)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max)
    }

    /// **Sitting and standing on each flavour**: the pie's *Sit Here* seats
    /// the avatar, its *Stand Up* puts it where that grid puts a standing
    /// avatar, and a sit on an object the grid no longer has is refused aloud
    /// on Second Life and met with silence on OpenSim.
    #[test]
    fn the_avatar_sits_stands_and_is_refused_as_each_grid_does() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "sit_second_life"),
            (ImitatedGrid::OpenSim, "sit_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async |stage: &Stage| {
                    let alpha = &stage.viewer("Alpha")?;
                    rez_seat(stage, "Alpha").await?;
                    let seat = alpha.world().object_named(SEAT).timeout(WAIT);

                    let _sat = seat.sit().await?;
                    let _seated = alpha
                        .expect_state(Probe::Agent)
                        .at("/seated_on")
                        .timeout(WAIT)
                        .to_equal(json!(seat_key().uuid().to_string()))
                        .await?;
                    assert_eq!(
                        stage
                            .agent("Alpha")
                            .await?
                            .with_sim(|sim| sim.seated_on())
                            .await,
                        Some(seat_key()),
                        "the grid has the avatar on the seat"
                    );

                    let _pie = seat.open_pie().await?;
                    let _stood = alpha
                        .pie_slice(Locator::default().name_key(STAND_UP))
                        .await?;
                    let _standing = alpha
                        .expect_state(Probe::Agent)
                        .at("/seated_on")
                        .timeout(WAIT)
                        .to_be_absent()
                        .await?;
                    let expected = stands_at(flavour);
                    let started = std::time::Instant::now();
                    let mut placed = alpha.agent().await?.position;
                    while placed.is_none_or(|at| apart(at, expected) > PLACED_WITHIN)
                        && started.elapsed() < WAIT
                    {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        placed = alpha.agent().await?.position;
                    }
                    let placed = placed.ok_or("the viewer reports no position")?;
                    assert!(
                        apart(placed, expected) <= PLACED_WITHIN,
                        "{flavour:?} stands an avatar at {expected:?}; the viewer has it at \
                         {placed:?}"
                    );

                    // The grid drops the seat and tells nobody, so the viewer
                    // asks to sit on an object the region does not have.
                    stage
                        .agent("Alpha")
                        .await?
                        .with_world(|world, _sim| {
                            world.objects.retain(|object| object.full_id != seat_key());
                        })
                        .await;
                    let _asked = seat.sit().await?;
                    match flavour {
                        ImitatedGrid::SecondLife => {
                            let _refused = alpha
                                .expect_notification()
                                .timeout(WAIT)
                                .to_show(REFUSAL)
                                .await?;
                        }
                        ImitatedGrid::OpenSim => {
                            tokio::time::sleep(UNANSWERED).await;
                            let raised = alpha.notifications().await?;
                            assert!(
                                raised
                                    .iter()
                                    .all(|notification| notification.template != REFUSAL),
                                "OpenSim refuses nothing: {raised:?}"
                            );
                        }
                    }
                    assert_eq!(
                        alpha.agent().await?.seated_on,
                        None,
                        "a sit on nothing seats nobody"
                    );
                    Ok(())
                })?;
        }
        Ok(())
    }
}
