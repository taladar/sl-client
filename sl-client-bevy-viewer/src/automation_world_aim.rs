//! **World aim** over the fixture world: world actions aimed through the
//! viewer's own pick resolver (the CPU double here), so a click lands on the
//! thing the locator names or not at all — and the camera frames a thing no
//! click can reach yet.

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_automation_proto::{ActionabilityCheck, AutomationError, WorldKind, WorldLocator};
    use sl_client_bevy::{Command, ScopedObjectId, Uuid, Vector};
    use sl_viewer_automation::{
        AimProgress, AimStage, PursuitError, WorldAim, WorldIntent, WorldModelPlugin, WorldTarget,
        screen_projection,
    };
    use sl_viewer_testkit::interact;

    use crate::world_test::{
        drain_commands, fixture_prim, install_camera, install_camera_rig, scene_position_of,
        seed_object, settle, world_app_with_edit, world_app_with_input,
    };

    /// A failed setup step or an aim that went wrong.
    type TestError = Box<dyn core::error::Error>;

    /// The prim the tests aim at.
    const TARGET: u32 = 1;
    /// The wall in front of it.
    const WALL: u32 = 2;
    /// The prim a place rezzes.
    const REZZED: u32 = 3;

    /// A region-local position.
    const fn at(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// `a + (b − a)·t`, component-wise.
    fn along(a: Vec3, b: Vec3, t: f32) -> Vec3 {
        Vec3::new(
            a.x + (b.x - a.x) * t,
            a.y + (b.y - a.y) * t,
            a.z + (b.z - a.z) * t,
        )
    }

    /// The locator of fixture prim `local_id`.
    fn prim(local_id: u32) -> WorldLocator {
        WorldLocator::kind(WorldKind::Object).local_id(local_id)
    }

    /// The fixture with the camera group: the target prim at the region's
    /// middle and, 5 m south of it, a wall wide and tall enough to hide it
    /// from the south. Returns the app and the two prims' scene positions.
    fn walled() -> Result<(App, Vec3, Vec3), TestError> {
        let mut app = world_app_with_input();
        app.add_plugins(WorldModelPlugin);
        seed_object(&mut app, fixture_prim(TARGET, at(128.0, 128.0, 30.0), 0));
        let mut wall = fixture_prim(WALL, at(128.0, 123.0, 30.0), 0);
        wall.scale = at(12.0, 0.5, 12.0);
        seed_object(&mut app, wall);
        settle(&mut app, 5);
        let target =
            scene_position_of(&mut app, scoped(TARGET)).ok_or("the target prim never spawned")?;
        let wall = scene_position_of(&mut app, scoped(WALL)).ok_or("the wall never spawned")?;
        Ok((app, target, wall))
    }

    /// The scoped id of fixture prim `local_id`.
    fn scoped(local_id: u32) -> ScopedObjectId {
        ScopedObjectId::new(
            sl_client_bevy::CircuitId::new(1),
            sl_client_bevy::RegionLocalObjectId(local_id),
        )
    }

    /// What an aim went through and where it ended.
    struct Aimed {
        /// The target, when the aim got one.
        target: Result<WorldTarget, PursuitError>,
        /// Every stage it waited in, in order, repeats folded.
        stages: Vec<AimStage>,
    }

    /// Poll `aim` a frame at a time until it answers.
    fn run(app: &mut App, mut aim: WorldAim) -> Aimed {
        let mut stages = Vec::new();
        loop {
            match aim.poll(app.world_mut()) {
                Ok(AimProgress::Ready(target)) => {
                    return Aimed {
                        target: Ok(target),
                        stages,
                    };
                }
                Ok(AimProgress::Waiting(stage)) => {
                    if stages.last() != Some(&stage) {
                        stages.push(stage);
                    }
                    app.update();
                }
                Err(error) => {
                    return Aimed {
                        target: Err(error),
                        stages,
                    };
                }
            }
        }
    }

    /// The region-local ids every `TouchObject` since the last drain named.
    fn touches(app: &mut App) -> Vec<u32> {
        drain_commands(app)
            .into_iter()
            .filter_map(|command| match command {
                Command::TouchObject { local_id, .. } => Some(local_id.id.0),
                _other => None,
            })
            .collect()
    }

    /// **The acceptance**: the prim behind the wall is reported covered, the
    /// camera frames it, and the click then lands on *it*; a right-click the
    /// same way opens the object pie on it.
    #[test]
    fn a_covered_prim_is_revealed_then_clicked_and_right_clicked() -> Result<(), TestError> {
        let (mut app, target, wall) = walled()?;
        // From beyond the wall, looking at the target through it.
        install_camera_rig(&mut app, along(target, wall, 3.0), target);
        settle(&mut app, 2);
        let projection = {
            let nodes = sl_viewer_automation::world_snapshot(app.world_mut())?;
            let node = nodes
                .iter()
                .find(|node| node.local_id == Some(TARGET))
                .ok_or("the target is not in the world model")?;
            screen_projection(app.world_mut(), node).ok_or("the target does not project")?
        };
        assert!(
            projection.on_screen,
            "the target projects into the view — only the wall hides it"
        );

        let clicked = run(&mut app, WorldAim::new(prim(TARGET), WorldIntent::Click));
        let target_aim = clicked.target?;
        assert_eq!(
            clicked.stages.first(),
            Some(&AimStage::Settling),
            "{:?}",
            clicked.stages
        );
        assert!(
            clicked
                .stages
                .contains(&AimStage::Revealing(ActionabilityCheck::ReceivesEvents)),
            "the wall covers the target, so the aim reveals it: {:?}",
            clicked.stages
        );
        assert_eq!(target_aim.node.local_id, Some(TARGET));
        let _before = drain_commands(&mut app);
        interact::perform(&mut app, target_aim.input());
        settle(&mut app, 3);
        assert_eq!(
            touches(&mut app),
            vec![TARGET],
            "the click touches the target, not the wall"
        );

        // The camera stays where the reveal left it: the right-click aims
        // straight away.
        let right = run(
            &mut app,
            WorldAim::new(prim(TARGET), WorldIntent::RightClick),
        );
        let right_aim = right.target?;
        assert!(
            !right
                .stages
                .iter()
                .any(|stage| matches!(stage, AimStage::Revealing(_))),
            "the target is already framed: {:?}",
            right.stages
        );
        interact::perform(&mut app, right_aim.input());
        settle(&mut app, 3);
        let picked = app
            .world()
            .resource::<crate::object_menu::ObjectMenuTarget>()
            .hit
            .as_ref()
            .map(|hit| hit.summary.picked_scoped);
        assert_eq!(picked, Some(scoped(TARGET)), "the pie's target is the prim");
        Ok(())
    }

    /// Without a reveal the covered prim fails, naming the wall.
    #[test]
    fn a_covered_prim_without_reveal_fails_naming_what_covers_it() -> Result<(), TestError> {
        let (mut app, target, wall) = walled()?;
        install_camera_rig(&mut app, along(target, wall, 3.0), target);
        settle(&mut app, 2);
        let aimed = run(
            &mut app,
            WorldAim::new(prim(TARGET), WorldIntent::Click).without_reveal(),
        );
        match aimed.target {
            Err(PursuitError::Automation(error)) => match *error {
                AutomationError::WorldNotActionable {
                    check, covered_by, ..
                } => {
                    assert_eq!(check, ActionabilityCheck::ReceivesEvents);
                    assert_eq!(covered_by, Some(Uuid::from_u128(u128::from(WALL))));
                }
                other => return Err(format!("not a covered failure: {other}").into()),
            },
            Err(other) => return Err(format!("not a covered failure: {other}").into()),
            Ok(target) => return Err(format!("aimed through the wall at {target:?}").into()),
        }
        assert!(touches(&mut app).is_empty(), "nothing was clicked");
        Ok(())
    }

    /// A hover leaves the pointer where the pick resolver finds the prim —
    /// what the hover tip and every other pointer consumer then read.
    #[test]
    fn a_hover_rests_the_pointer_on_the_prim() -> Result<(), TestError> {
        let (mut app, target, wall) = walled()?;
        // From the north, where nothing stands in front of the target.
        install_camera_rig(&mut app, along(wall, target, 3.0), target);
        settle(&mut app, 2);
        let hovered = run(&mut app, WorldAim::new(prim(TARGET), WorldIntent::Hover));
        let aimed = hovered.target?;
        interact::perform(&mut app, aimed.input());
        let cursor = interact::cursor(&mut app).ok_or("the pointer is not in the window")?;
        assert_eq!(cursor, aimed.aim, "the pointer rests on the aim point");
        let probe = app
            .world_mut()
            .resource_mut::<crate::world_api::PickProbes>()
            .request(cursor);
        let mut answer = None;
        for _frame in 0..10 {
            app.update();
            answer = app
                .world_mut()
                .resource_mut::<crate::world_api::PickProbes>()
                .take_answer(probe);
            if answer.is_some() {
                break;
            }
        }
        let hit = answer
            .ok_or("the probe at the pointer was never answered")?
            .ok_or("the probe at the pointer hit nothing")?;
        assert!(
            matches!(
                hit.target,
                crate::world_api::ProbeTarget::Object { scoped: over, .. } if over == scoped(TARGET)
            ),
            "the pointer is over {:?}, not the target",
            hit.target
        );
        assert!(touches(&mut app).is_empty(), "a hover touches nothing");
        Ok(())
    }

    /// A prim behind the camera is off screen; the reveal turns the camera
    /// onto it.
    #[test]
    fn a_prim_behind_the_camera_is_revealed() -> Result<(), TestError> {
        let (mut app, target, wall) = walled()?;
        // North of the target, looking further north: the target is behind.
        let north = along(wall, target, 3.0);
        install_camera_rig(&mut app, north, along(target, north, 2.0));
        settle(&mut app, 2);
        let aimed = run(&mut app, WorldAim::new(prim(TARGET), WorldIntent::Click));
        aimed.target?;
        assert!(
            aimed
                .stages
                .contains(&AimStage::Revealing(ActionabilityCheck::InViewport)),
            "{:?}",
            aimed.stages
        );
        Ok(())
    }

    /// A select waits for build mode, and then selects the prim through the
    /// real selection gesture. This app has no camera group, so there is no
    /// reveal to fall back on: the aim must find the prim as the camera shows
    /// it.
    #[test]
    fn a_select_waits_for_build_mode_then_selects() -> Result<(), TestError> {
        let mut app = world_app_with_edit();
        app.add_plugins(WorldModelPlugin);
        seed_object(&mut app, fixture_prim(TARGET, at(128.0, 128.0, 30.0), 0));
        settle(&mut app, 5);
        let target = scene_position_of(&mut app, scoped(TARGET)).ok_or("no target prim")?;
        install_camera(
            &mut app,
            Vec3::new(target.x, target.y + 2.0, target.z + 12.0),
            target,
        );
        settle(&mut app, 2);

        let mut aim = WorldAim::new(prim(TARGET), WorldIntent::Select);
        let mut waited_for_build_mode = false;
        for _frame in 0..10 {
            if aim.poll(app.world_mut())? == AimProgress::Waiting(AimStage::BuildMode) {
                waited_for_build_mode = true;
            }
            app.update();
        }
        assert!(waited_for_build_mode, "a select waits for the build tool");
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .active = true;
        let mut stages = Vec::new();
        let selected = loop {
            match aim.poll(app.world_mut())? {
                AimProgress::Ready(target) => break target,
                AimProgress::Waiting(stage) => {
                    stages.push(stage);
                    app.update();
                }
            }
        };
        assert!(
            !stages
                .iter()
                .any(|stage| matches!(stage, AimStage::Revealing(_))),
            "{stages:?}"
        );
        interact::perform(&mut app, selected.input());
        settle(&mut app, 3);
        assert!(
            app.world()
                .resource::<crate::world_api::SelectionSet>()
                .is_selected(scoped(TARGET)),
            "the select click selected the prim"
        );
        assert!(
            touches(&mut app).is_empty(),
            "in build mode a click does not touch"
        );
        Ok(())
    }

    // ---- Build mode: handle drags and the rubber band. ----------------------

    use sl_client_bevy::{
        AgentKey, LindenAmount, ObjectKey, ObjectPropertiesFamily, OwnerKey, Permissions5, SlEvent,
        SlSessionEvent,
    };
    use sl_viewer_automation::{
        DragProgress, HeldKeys, ManipulatorDrag, SweepProgress, WorldSweep,
    };

    use crate::world_api::{
        ManipulatorAmount, ManipulatorAxis, ManipulatorHandle, ManipulatorQuery, SnapRegime,
    };
    use crate::world_test::{entity_of, open_build_floater, world_app_with_build_tools};

    /// The build-tools fixture with one prim at the region's middle, the
    /// camera `back` metres back and looking left of it so it stands clear of
    /// the Build window (parked over the left half of the screen), the window
    /// open on the move tool and the prim selected by an aimed select.
    fn building(back: f32) -> Result<(App, Entity), TestError> {
        let mut app = world_app_with_build_tools()?;
        app.add_plugins(WorldModelPlugin);
        seed_object(&mut app, fixture_prim(TARGET, at(128.0, 128.0, 30.0), 0));
        settle(&mut app, 5);
        let target =
            scene_position_of(&mut app, scoped(TARGET)).ok_or("the target prim never spawned")?;
        let look = Vec3::new(target.x - 3.5, target.y, target.z);
        install_camera(
            &mut app,
            Vec3::new(look.x, look.y + 2.0, look.z + back),
            look,
        );
        open_build_floater(&mut app);
        let select = run(&mut app, WorldAim::new(prim(TARGET), WorldIntent::Select));
        interact::perform(&mut app, select.target?.input());
        settle(&mut app, 3);
        assert!(
            app.world()
                .resource::<crate::world_api::SelectionSet>()
                .is_selected(scoped(TARGET)),
            "the aimed select selected the prim"
        );
        let entity = entity_of(&mut app, scoped(TARGET)).ok_or("the prim has no entity")?;
        let _before = drain_commands(&mut app);
        Ok((app, entity))
    }

    /// The second prim the shift-select test selects beside [`TARGET`].
    const BESIDE: u32 = 4;

    /// A shift-select adds a prim to the selection and keeps what was
    /// selected, and a second one on a selected prim takes it out again — the
    /// selection gesture's extend / toggle, read off `Shift` held at the
    /// press.
    #[test]
    fn a_shift_select_toggles_a_prim_and_keeps_the_rest() -> Result<(), TestError> {
        let mut app = world_app_with_build_tools()?;
        app.add_plugins(WorldModelPlugin);
        seed_object(&mut app, fixture_prim(TARGET, at(128.0, 128.0, 30.0), 0));
        seed_object(&mut app, fixture_prim(BESIDE, at(128.0, 132.0, 30.0), 0));
        settle(&mut app, 5);
        let target =
            scene_position_of(&mut app, scoped(TARGET)).ok_or("the target prim never spawned")?;
        let beside =
            scene_position_of(&mut app, scoped(BESIDE)).ok_or("the second prim never spawned")?;
        let middle = along(target, beside, 0.5);
        let look = Vec3::new(middle.x - 3.5, middle.y, middle.z);
        install_camera(
            &mut app,
            Vec3::new(look.x, look.y + 2.0, look.z + 16.0),
            look,
        );
        open_build_floater(&mut app);
        let selected = |app: &App| {
            let selection = app.world().resource::<crate::world_api::SelectionSet>();
            [TARGET, BESIDE].map(|local_id| selection.is_selected(scoped(local_id)))
        };

        let select = run(&mut app, WorldAim::new(prim(TARGET), WorldIntent::Select));
        interact::perform(&mut app, select.target?.input());
        settle(&mut app, 3);
        assert_eq!(selected(&app), [true, false], "a plain select");

        let add = run(
            &mut app,
            WorldAim::new(prim(BESIDE), WorldIntent::ShiftSelect),
        );
        interact::perform(&mut app, add.target?.input());
        settle(&mut app, 3);
        assert_eq!(
            selected(&app),
            [true, true],
            "a shift-select adds the second prim and keeps the first"
        );

        let remove = run(
            &mut app,
            WorldAim::new(prim(TARGET), WorldIntent::ShiftSelect),
        );
        interact::perform(&mut app, remove.target?.input());
        settle(&mut app, 3);
        assert_eq!(
            selected(&app),
            [false, true],
            "a shift-select on a selected prim takes it out"
        );
        Ok(())
    }

    /// A place waits for the Create tool — the Build window open on the Move
    /// tool is not enough — and then rezzes on the prim: one `ObjectAdd`, on
    /// the prim's top face, and no touch.
    #[test]
    fn a_place_waits_for_the_create_tool_then_rezzes_on_the_prim() -> Result<(), TestError> {
        let mut app = world_app_with_build_tools()?;
        app.add_plugins(WorldModelPlugin);
        seed_object(&mut app, fixture_prim(TARGET, at(128.0, 128.0, 30.0), 0));
        settle(&mut app, 5);
        let target =
            scene_position_of(&mut app, scoped(TARGET)).ok_or("the target prim never spawned")?;
        let look = Vec3::new(target.x - 3.5, target.y, target.z);
        install_camera(
            &mut app,
            Vec3::new(look.x, look.y + 2.0, look.z + 12.0),
            look,
        );
        open_build_floater(&mut app);

        let mut aim = WorldAim::new(prim(TARGET), WorldIntent::Place);
        let mut waited_for_create = false;
        for _frame in 0..10 {
            if aim.poll(app.world_mut())? == AimProgress::Waiting(AimStage::CreateTool) {
                waited_for_create = true;
            }
            app.update();
        }
        assert!(waited_for_create, "a place waits for the Create tool");
        app.world_mut()
            .resource_mut::<crate::world_api::EditToolState>()
            .tool = crate::world_api::EditTool::Create;
        let placed = loop {
            match aim.poll(app.world_mut())? {
                AimProgress::Ready(target) => break target,
                AimProgress::Waiting(_stage) => app.update(),
            }
        };
        let _before = drain_commands(&mut app);
        interact::perform(&mut app, placed.input());
        settle(&mut app, 3);
        let commands = drain_commands(&mut app);
        let rezzes: Vec<&Vector> = commands
            .iter()
            .filter_map(|command| match command {
                Command::RezObject { shape, .. } => Some(&shape.position),
                _other => None,
            })
            .collect();
        let [on_prim] = rezzes.as_slice() else {
            return Err(format!("not one rez: {commands:?}").into());
        };
        // Where the aim's pick said the click lands, which is on the prim (a
        // 2 × 3 × 4 m fixture box).
        let [hit_x, hit_y, hit_z] = placed.hit_point.ok_or("the aim has no hit point")?;
        assert!(
            (on_prim.x - hit_x).abs() < 0.05
                && (on_prim.y - hit_y).abs() < 0.05
                && (on_prim.z - hit_z).abs() < 0.05,
            "the rez lands at the aim's hit point {:?}: {on_prim:?}",
            placed.hit_point
        );
        assert!(
            (on_prim.x - 128.0).abs() <= 1.05
                && (on_prim.y - 128.0).abs() <= 1.55
                && (on_prim.z - 30.0).abs() <= 2.05,
            "the rez lands on the prim: {on_prim:?}"
        );
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, Command::TouchObject { .. })),
            "a place touches nothing"
        );
        assert!(
            app.world()
                .resource::<crate::world_api::SelectionSet>()
                .primary()
                .is_none(),
            "a plain click with the Create tool selects nothing, not even what it landed on"
        );

        // The grid's answer: the new prim, where the rez asked for it — within
        // the match slop of the prim it landed on, which must not be taken for
        // it.
        let mut rezzed = fixture_prim(REZZED, (*on_prim).clone(), 0);
        rezzed.scale = at(0.5, 0.5, 0.5);
        seed_object(&mut app, rezzed);
        settle(&mut app, 3);
        let selection = app.world().resource::<crate::world_api::SelectionSet>();
        assert!(
            selection.is_selected(scoped(REZZED)) && !selection.is_selected(scoped(TARGET)),
            "the rez drops into edit on the new prim, not on the one under it"
        );
        Ok(())
    }

    /// Drag `handle` by `amount` in `regime` holding `keys`, polling to the
    /// end; the plan's prediction.
    fn drag(
        app: &mut App,
        handle: ManipulatorHandle,
        amount: ManipulatorAmount,
        regime: SnapRegime,
        keys: HeldKeys,
    ) -> Result<ManipulatorAmount, TestError> {
        let mut dragging = ManipulatorDrag::new(
            ManipulatorQuery {
                handle,
                amount,
                regime,
            },
            keys,
        );
        loop {
            match dragging.poll(app.world_mut())? {
                DragProgress::Done(predicted) => {
                    settle(app, 3);
                    return Ok(predicted);
                }
                DragProgress::Waiting(_stage) => app.update(),
            }
        }
    }

    /// The prim's live wire-frame motion.
    fn motion(app: &App, entity: Entity) -> Result<crate::objects::ObjectSlMotion, TestError> {
        app.world()
            .get::<crate::objects::ObjectSlMotion>(entity)
            .cloned()
            .ok_or_else(|| "the prim has no motion".into())
    }

    /// How many `UpdateObject`s and `DuplicateObjects` went out since the
    /// last drain.
    fn edits(app: &mut App) -> (usize, usize) {
        let commands = drain_commands(app);
        let updates = commands
            .iter()
            .filter(|command| matches!(command, Command::UpdateObject { .. }))
            .count();
        let copies = commands
            .iter()
            .filter(|command| matches!(command, Command::DuplicateObjects { .. }))
            .count();
        (updates, copies)
    }

    /// The distance a prediction says, or an error.
    fn distance(predicted: ManipulatorAmount) -> Result<f32, TestError> {
        match predicted {
            ManipulatorAmount::Distance(distance) => Ok(distance),
            other => Err(format!("not a distance: {other:?}").into()),
        }
    }

    /// The angle a prediction says, or an error.
    fn angle(predicted: ManipulatorAmount) -> Result<f32, TestError> {
        match predicted {
            ManipulatorAmount::Angle(angle) => Ok(angle),
            other => Err(format!("not an angle: {other:?}").into()),
        }
    }

    /// The turn about the region's Y axis from `before` to `after`, radians.
    fn turn_about_y(before: &sl_client_bevy::Rotation, after: &sl_client_bevy::Rotation) -> f32 {
        let before = Quat::from_xyzw(before.x, before.y, before.z, before.s);
        let after = Quat::from_xyzw(after.x, after.y, after.z, after.s);
        let delta = after.mul_quat(before.inverse());
        // Into (−π, π]: `q` and `−q` are one rotation, and the half-angle form
        // reads either as a turn or as that turn less a full circle.
        let turn = 2.0 * delta.y.atan2(delta.w);
        (turn + core::f32::consts::PI).rem_euclid(core::f32::consts::TAU) - core::f32::consts::PI
    }

    /// **Move**: along X by a metre on the axis — exactly a metre, although
    /// snapping is on, because the drag stays on the free side of the guide —
    /// then 0.7 m past the guide, which lands on the half-metre grid instead;
    /// and a Shift drag leaves one copy behind.
    #[test]
    fn a_move_drag_moves_by_the_amount_or_to_the_grid_and_shift_copies() -> Result<(), TestError> {
        let (mut app, entity) = building(10.0)?;
        let x = ManipulatorHandle::Translate(ManipulatorAxis::X);
        let start = motion(&app, entity)?.position;

        let free = drag(
            &mut app,
            x,
            ManipulatorAmount::Distance(1.0),
            SnapRegime::Free,
            HeldKeys::None,
        )?;
        assert!((distance(free)? - 1.0).abs() < 1e-6, "{free:?}");
        let moved = motion(&app, entity)?.position;
        assert!(
            (moved.x - start.x - 1.0).abs() < 1e-2,
            "{start:?} → {moved:?}"
        );
        assert!((moved.y - start.y).abs() < 1e-3 && (moved.z - start.z).abs() < 1e-3);
        assert_eq!(edits(&mut app), (1, 0), "one update on release, no copy");

        let snapped = distance(drag(
            &mut app,
            x,
            ManipulatorAmount::Distance(0.7),
            SnapRegime::Grid,
            HeldKeys::None,
        )?)?;
        assert!(
            (snapped - 0.5).abs() < 1e-4,
            "129.0 + 0.7 lands on the 129.5 grid mark: {snapped}"
        );
        let on_grid = motion(&app, entity)?.position;
        assert!((on_grid.x - 129.5).abs() < 1e-3, "{on_grid:?}");

        let copied = drag(
            &mut app,
            x,
            ManipulatorAmount::Distance(1.0),
            SnapRegime::Free,
            HeldKeys::Shift,
        )?;
        assert!((distance(copied)? - 1.0).abs() < 1e-6);
        let (updates, copies) = edits(&mut app);
        assert_eq!(copies, 1, "a Shift move leaves exactly one copy behind");
        assert!(updates >= 1);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::ShiftLeft),
            "the drag let go of Shift"
        );
        Ok(())
    }

    /// **Rotate under `Ctrl`**: the move tool is on, the held key brings up
    /// the rings; 30° inside the tick circle turns the prim 30°, and 20° more
    /// outside it lands the prim on the nearest 5.625° detent (50.625°).
    #[test]
    fn a_ctrl_ring_drag_turns_by_the_angle_or_to_a_detent() -> Result<(), TestError> {
        let (mut app, entity) = building(10.0)?;
        let ring = ManipulatorHandle::Rotate(ManipulatorAxis::Y);
        let start = motion(&app, entity)?.rotation;

        let free = angle(drag(
            &mut app,
            ring,
            ManipulatorAmount::Angle(30_f32.to_radians()),
            SnapRegime::Free,
            HeldKeys::Ctrl,
        )?)?;
        let turned = motion(&app, entity)?.rotation;
        let free_turn = turn_about_y(&start, &turned);
        assert!((free - 30_f32.to_radians()).abs() < 1e-6);
        assert!(
            (free_turn - free).abs() < 2e-3,
            "turned {} degrees",
            free_turn.to_degrees()
        );
        assert_eq!(edits(&mut app).0, 1);

        let detent = angle(drag(
            &mut app,
            ring,
            ManipulatorAmount::Angle(20_f32.to_radians()),
            SnapRegime::Grid,
            HeldKeys::Ctrl,
        )?)?;
        let snapped = motion(&app, entity)?.rotation;
        let total = turn_about_y(&start, &snapped);
        assert!(
            (total.to_degrees() - 50.625).abs() < 0.05,
            "30° + 20° lands on the nearest detent, 9 × 5.625° = 50.625°, turned {} in all \
             (predicted {})",
            total.to_degrees(),
            detent.to_degrees()
        );
        assert!((turn_about_y(&turned, &snapped) - detent).abs() < 2e-3);
        Ok(())
    }

    /// **Stretch under `Ctrl+Shift`**: the X face by half a metre on the line,
    /// then 0.3 m past the guide, which lands the size on the grid (3.0 m);
    /// and a corner by ×1.2 scales every axis. (×1.5 is refused from this
    /// camera: without stretch-both-sides the corner travels twice as far as
    /// the factor grows, which leaves the window.)
    #[test]
    fn a_ctrl_shift_stretch_resizes_by_the_amount_or_to_the_grid() -> Result<(), TestError> {
        let (mut app, entity) = building(16.0)?;
        let face = ManipulatorHandle::StretchFace(ManipulatorAxis::X, true);
        let start = motion(&app, entity)?.scale;

        let free = distance(drag(
            &mut app,
            face,
            ManipulatorAmount::Distance(0.5),
            SnapRegime::Free,
            HeldKeys::CtrlShift,
        )?)?;
        let grown = motion(&app, entity)?.scale;
        assert!((free - 0.5).abs() < 1e-6);
        assert!(
            (grown.x - start.x - 0.5).abs() < 1e-2,
            "{start:?} → {grown:?}"
        );
        assert!((grown.y - start.y).abs() < 1e-4 && (grown.z - start.z).abs() < 1e-4);
        assert!(edits(&mut app).0 >= 1, "a stretch streams its updates");

        let snapped = distance(drag(
            &mut app,
            face,
            ManipulatorAmount::Distance(0.3),
            SnapRegime::Grid,
            HeldKeys::CtrlShift,
        )?)?;
        let on_grid = motion(&app, entity)?.scale;
        assert!(
            (on_grid.x - 3.0).abs() < 1e-3,
            "2.5 + 0.3 lands on the 3.0 mark (predicted +{snapped}): {on_grid:?}"
        );

        let before = motion(&app, entity)?.scale;
        let corner = ManipulatorHandle::StretchCorner([true, false, true]);
        let factor = drag(
            &mut app,
            corner,
            ManipulatorAmount::Factor(1.2),
            SnapRegime::Free,
            HeldKeys::CtrlShift,
        )?;
        assert_eq!(factor, ManipulatorAmount::Factor(1.2));
        let scaled = motion(&app, entity)?.scale;
        for (was, is) in [
            (before.x, scaled.x),
            (before.y, scaled.y),
            (before.z, scaled.z),
        ] {
            assert!((is / was - 1.2).abs() < 1e-2, "{before:?} → {scaled:?}");
        }
        Ok(())
    }

    /// The name a simulator's family reply gives fixture prim `local_id`.
    fn name_prim(app: &mut App, local_id: u32, name: &str) {
        app.world_mut()
            .write_message(SlEvent(SlSessionEvent::ObjectPropertiesFamily {
                properties: ObjectPropertiesFamily {
                    request_flags: 0,
                    object_id: ObjectKey::from(Uuid::from_u128(u128::from(local_id))),
                    owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0xA))),
                    group: None,
                    permissions: Permissions5::default(),
                    ownership_cost: LindenAmount(0),
                    sale_type: 0,
                    sale_price: None,
                    category: 0,
                    last_owner_id: Uuid::nil(),
                    name: name.to_owned(),
                    description: String::new(),
                },
            }));
    }

    /// Poll `sweep` to its answer.
    fn sweep(app: &mut App, mut sweep: WorldSweep) -> Result<SweepProgress, PursuitError> {
        loop {
            match sweep.poll(app.world_mut())? {
                ready @ SweepProgress::Ready(_) => return Ok(ready),
                SweepProgress::Waiting(_stage) => app.update(),
            }
        }
    }

    /// Three prims in a row, 4 m apart, named Left, Middle and Right, all
    /// right of the Build window; the camera 20 m back. Far enough apart that
    /// their projected boxes do not overlap from this slanted view — closer
    /// than that, no band over two of them misses the third, and the sweep
    /// rightly says so.
    fn row_of_three() -> Result<App, TestError> {
        let mut app = world_app_with_build_tools()?;
        app.add_plugins(WorldModelPlugin);
        for (local_id, x) in [(1, 124.0), (2, 128.0), (3, 132.0)] {
            seed_object(&mut app, fixture_prim(local_id, at(x, 128.0, 30.0), 0));
        }
        settle(&mut app, 5);
        name_prim(&mut app, 1, "Left");
        name_prim(&mut app, 2, "Middle");
        name_prim(&mut app, 3, "Right");
        settle(&mut app, 2);
        let middle =
            scene_position_of(&mut app, scoped(2)).ok_or("the middle prim never spawned")?;
        let look = Vec3::new(middle.x - 7.5, middle.y, middle.z);
        install_camera(
            &mut app,
            Vec3::new(look.x, look.y + 2.0, look.z + 20.0),
            look,
        );
        open_build_floater(&mut app);
        Ok(app)
    }

    /// **The rubber band** over Left and Middle selects exactly those two;
    /// one over the two named `End`s cannot be drawn without catching the
    /// prim between them, and says so.
    #[test]
    fn a_sweep_selects_exactly_its_targets_or_names_what_it_would_catch() -> Result<(), TestError> {
        let mut app = row_of_three()?;
        let pair = WorldLocator::kind(WorldKind::Object).near(
            sl_automation_proto::Anchor::Point([126.0, 128.0, 30.0]),
            Some(2.5),
        );
        let SweepProgress::Ready(band) = sweep(&mut app, WorldSweep::new(pair))? else {
            return Err("the sweep never became ready".into());
        };
        interact::perform(&mut app, band.input());
        settle(&mut app, 3);
        let selection = app.world().resource::<crate::world_api::SelectionSet>();
        let mut selected: Vec<u32> = selection.iter().map(|node| node.scoped.id.0).collect();
        selected.sort_unstable();
        assert_eq!(
            selected,
            vec![1, 2],
            "the band selected Left and Middle only"
        );

        name_prim(&mut app, 1, "End");
        name_prim(&mut app, 3, "End");
        settle(&mut app, 2);
        let ends = WorldLocator::kind(WorldKind::Object).named("End");
        match sweep(&mut app, WorldSweep::new(ends)) {
            Err(PursuitError::Automation(error)) => match *error {
                AutomationError::SweepInexact { missing, extra, .. } => {
                    assert!(missing.is_empty(), "{missing:?}");
                    assert_eq!(extra, vec![Uuid::from_u128(2)], "it would catch Middle");
                }
                other => return Err(format!("not an inexact sweep: {other}").into()),
            },
            other => return Err(format!("not an inexact sweep: {other:?}").into()),
        }
        Ok(())
    }

    // ---- The same, through the executor's queue. ----------------------------

    use sl_automation_proto::{
        Deadline, DragAmount, DragModifiers, Request, RequestBody, RequestId, Response,
        ResponseBody, SnapSide, WorldAction, WorldWaitCondition,
    };
    use sl_viewer_automation::{AutomationPlugin, AutomationQueue};

    /// Submit `body` to the executor and step frames until it is answered.
    fn ask(app: &mut App, body: RequestBody) -> Result<Response, TestError> {
        let id = RequestId(1);
        app.world_mut()
            .resource_mut::<AutomationQueue>()
            .submit(Request { id, body });
        for _frame in 0..3000 {
            app.update();
            if let Some(response) = app
                .world_mut()
                .resource_mut::<AutomationQueue>()
                .take_response(id)
            {
                return Ok(response);
            }
        }
        Err("the executor never answered".into())
    }

    /// The body of a response that succeeded.
    fn answered(response: Response) -> Result<ResponseBody, TestError> {
        response
            .result
            .map_err(|error| format!("the request failed: {error}").into())
    }

    /// **World requests through the executor**: a find, a wait, an action
    /// that fails without a reveal (naming the wall, and saying what happened
    /// around it), and the same action with the reveal, which touches the
    /// target and nothing else.
    #[test]
    fn world_requests_find_wait_and_act_through_the_executor() -> Result<(), TestError> {
        let (mut app, target, wall) = walled()?;
        app.add_plugins(AutomationPlugin);
        install_camera_rig(&mut app, along(target, wall, 3.0), target);
        settle(&mut app, 2);

        let found = answered(ask(
            &mut app,
            RequestBody::FindWorld {
                locator: prim(TARGET),
                deadline: Deadline::default(),
            },
        )?)?;
        assert!(
            matches!(&found, ResponseBody::FoundWorld { nodes } if nodes.len() == 1),
            "{found:?}"
        );
        let gone = answered(ask(
            &mut app,
            RequestBody::WaitForWorld {
                locator: prim(99),
                condition: WorldWaitCondition::Detached,
                deadline: Deadline::default(),
            },
        )?)?;
        assert_eq!(gone, ResponseBody::WorldSatisfied { nodes: Vec::new() });

        let covered = ask(
            &mut app,
            RequestBody::WorldAction {
                locator: prim(TARGET),
                action: WorldAction::Click,
                reveal: false,
                deadline: Deadline::default(),
            },
        )?;
        let report = covered.report.clone().ok_or("an error with no report")?;
        match covered.result {
            Err(AutomationError::WorldNotActionable {
                check, covered_by, ..
            }) => {
                assert_eq!(check, ActionabilityCheck::ReceivesEvents);
                assert_eq!(covered_by, Some(Uuid::from_u128(u128::from(WALL))));
            }
            other => return Err(format!("not a covered failure: {other:?}").into()),
        }
        assert!(report.tree.is_empty(), "a world failure has no UI excerpt");
        assert!(touches(&mut app).is_empty(), "nothing was clicked");

        let done = answered(ask(
            &mut app,
            RequestBody::WorldAction {
                locator: prim(TARGET),
                action: WorldAction::Click,
                reveal: true,
                deadline: Deadline::default(),
            },
        )?)?;
        let ResponseBody::WorldDone { node, hit_point } = done else {
            return Err(format!("not a world action: {done:?}").into());
        };
        assert_eq!(node.local_id, Some(TARGET));
        assert!(hit_point.is_some(), "a click's pick says where it lands");
        settle(&mut app, 3);
        assert_eq!(
            touches(&mut app),
            vec![TARGET],
            "the click touches the target, not the wall"
        );
        Ok(())
    }

    /// **A handle drag and a rubber band through the executor**: the drag
    /// moves the selection by the metre asked, and the band selects exactly
    /// the two prims it names.
    #[test]
    fn a_drag_and_a_sweep_through_the_executor() -> Result<(), TestError> {
        let (mut app, entity) = building(10.0)?;
        app.add_plugins(AutomationPlugin);
        let start = motion(&app, entity)?.position;
        let dragged = answered(ask(
            &mut app,
            RequestBody::DragHandle {
                handle: "translate-x".to_owned(),
                amount: DragAmount::Distance(1.0),
                snap: SnapSide::Free,
                modifiers: DragModifiers::None,
                deadline: Deadline::default(),
            },
        )?)?;
        assert_eq!(
            dragged,
            ResponseBody::Dragged {
                predicted: DragAmount::Distance(1.0)
            }
        );
        settle(&mut app, 3);
        let moved = motion(&app, entity)?.position;
        assert!(
            (moved.x - start.x - 1.0).abs() < 1e-2,
            "{start:?} → {moved:?}"
        );

        let mut app = row_of_three()?;
        app.add_plugins(AutomationPlugin);
        let pair = WorldLocator::kind(WorldKind::Object).near(
            sl_automation_proto::Anchor::Point([126.0, 128.0, 30.0]),
            Some(2.5),
        );
        let swept = answered(ask(
            &mut app,
            RequestBody::Sweep {
                locator: pair,
                deadline: Deadline::default(),
            },
        )?)?;
        let ResponseBody::Swept { nodes } = swept else {
            return Err(format!("not a sweep: {swept:?}").into());
        };
        let mut ids: Vec<Option<u32>> = nodes.iter().map(|node| node.local_id).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![Some(1), Some(2)]);
        let selection = app.world().resource::<crate::world_api::SelectionSet>();
        let mut selected: Vec<u32> = selection.iter().map(|node| node.scoped.id.0).collect();
        selected.sort_unstable();
        assert_eq!(
            selected,
            vec![1, 2],
            "the answer came with the selection made"
        );
        Ok(())
    }
}
