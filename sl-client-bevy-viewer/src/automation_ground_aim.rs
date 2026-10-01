//! **Ground aim and the double-click teleport** over the fixture world: a
//! ground action aimed by region and position through the viewer's own pick
//! resolver (the CPU double here), and the double-click teleport's rule — on
//! which surfaces a double-click teleports, and where to — driven through the
//! automation executor and the real gesture.

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_automation_proto::{
        ActionabilityCheck, AutomationError, Deadline, GroundPoint, Request, RequestBody,
        RequestId, Response, ResponseBody, WorldAction, WorldKind, WorldLocator,
    };
    use sl_client_bevy::{AgentKey, Command, RegionHandle, SlIdentity, Uuid, Vector};
    use sl_settings::{Scope, SettingValue};
    use sl_viewer_automation::{AutomationPlugin, AutomationQueue};

    use crate::coords::sl_to_bevy_vec;
    use crate::settings::ViewerSettings;
    use crate::world_api::{
        FLAGS_HANDLE_TOUCH, SETTING_DOUBLE_CLICK_ACTION, SETTING_DOUBLE_CLICK_SCRIPTED_OBJECTS,
    };
    use crate::world_test::{
        drain_commands, fixture_prim, install_camera_rig, seed_avatar, seed_object,
        seed_region_name, seed_terrain, settle, world_app_with_input,
    };

    /// A failed setup step or a request that went wrong.
    type TestError = Box<dyn core::error::Error>;

    /// The fixture region's name.
    const REGION: &str = "Fixture Region";

    /// The height of the fixture's flat ground, metres.
    const GROUND: f32 = 25.0;

    /// The ground point the tests aim at: the middle of the one land patch.
    const SPOT: [f32; 2] = [8.0, 8.0];

    /// The agent.
    const OWN: u128 = 0x0_5e1f;

    /// Somebody else.
    const OTHER: u128 = 0x0_07e4;

    /// How close a teleport's position must be to the point aimed at, metres.
    const LANDING_SLOP: f32 = 0.3;

    /// The own avatar's pelvis-to-foot height at the rest shape, which the
    /// teleport raises its arrival by.
    fn pelvis_to_foot(app: &App) -> Result<f32, TestError> {
        let library = app
            .world()
            .get_resource::<crate::avatar_assets::AvatarAssetLibrary>()
            .ok_or("no avatar library")?;
        Ok(library
            .skeleton()
            .body_size_metrics(
                &sl_client_bevy::SkeletalDeformations::default(),
                &sl_client_bevy::JointOverrides::default(),
            )
            .ok_or("the skeleton has no body size")?
            .pelvis_to_foot)
    }

    /// The fixture: flat ground in a named region with the agent in it, the
    /// avatar library (for the pelvis-to-foot lift), the double-click teleport
    /// switched on, the automation executor, and a camera looking down at
    /// [`SPOT`] from the south-west.
    fn grounded() -> Result<App, TestError> {
        let mut app = world_app_with_input();
        app.add_plugins((
            crate::double_click_teleport::DoubleClickTeleportPlugin,
            AutomationPlugin,
        ));
        let vendored =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../viewer-assets/character");
        app.insert_resource(crate::avatar_assets::AvatarAssetLibrary::load(&vendored)?);
        {
            let mut identity = app.world_mut().resource_mut::<SlIdentity>();
            identity.agent_id = Some(AgentKey::from(Uuid::from_u128(OWN)));
            identity.region_handle = Some(RegionHandle(0));
        }
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_DOUBLE_CLICK_ACTION,
            SettingValue::I32(1),
        );
        seed_terrain(&mut app, GROUND);
        seed_region_name(&mut app, RegionHandle(0), REGION);
        let [x, y] = SPOT;
        let spot = sl_to_bevy_vec(&Vector { x, y, z: GROUND });
        let eye = sl_to_bevy_vec(&Vector {
            x: x - 6.0,
            y: y - 6.0,
            z: GROUND + 7.0,
        });
        install_camera_rig(&mut app, eye, spot);
        settle(&mut app, 5);
        Ok(app)
    }

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

    /// A ground action on [`SPOT`] in `region`.
    fn on_ground(region: &str, action: WorldAction, reveal: bool) -> RequestBody {
        let [x, y] = SPOT;
        RequestBody::GroundAction {
            ground: GroundPoint::new(region, x, y),
            action,
            reveal,
            deadline: Deadline {
                frames: Some(240),
                millis: None,
            },
        }
    }

    /// A world action on what `locator` names, with the reveal.
    fn on_thing(locator: WorldLocator, action: WorldAction) -> RequestBody {
        RequestBody::WorldAction {
            locator,
            action,
            reveal: true,
            deadline: Deadline::default(),
        }
    }

    /// Every teleport asked for since the last drain: where to.
    fn teleports(app: &mut App) -> Vec<(RegionHandle, [f32; 3])> {
        settle(app, 3);
        drain_commands(app)
            .into_iter()
            .filter_map(|command| match command {
                Command::Teleport {
                    region_handle,
                    position,
                    ..
                } => Some((region_handle, [position.x(), position.y(), position.z()])),
                _other => None,
            })
            .collect()
    }

    /// The straight-line distance between two points.
    fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
        let [ax, ay, az] = a;
        let [bx, by, bz] = b;
        ((ax - bx).powi(2) + (ay - by).powi(2) + (az - bz).powi(2)).sqrt()
    }

    /// **A double-click on the ground** teleports there: the aim finds the
    /// spot by region name and position, the pick confirms the ground, and
    /// the teleport goes to that point raised by the pelvis-to-foot height.
    /// The region name is matched without regard to case.
    #[test]
    fn a_ground_double_click_teleports_onto_the_spot() -> Result<(), TestError> {
        let mut app = grounded()?;
        let lift = pelvis_to_foot(&app)?;
        let _before = drain_commands(&mut app);
        let done = ask(
            &mut app,
            on_ground(&REGION.to_uppercase(), WorldAction::DoubleClick, false),
        )?
        .result?;
        let ResponseBody::GroundDone { hit_point } = done else {
            return Err(format!("not a ground action: {done:?}").into());
        };
        let [x, y] = SPOT;
        assert!(
            distance(hit_point, [x, y, GROUND]) < LANDING_SLOP,
            "the pick lands on the spot: {hit_point:?}"
        );
        let asked = teleports(&mut app);
        let [(region, at)] = asked.as_slice() else {
            return Err(format!("one teleport, not {asked:?}").into());
        };
        assert_eq!(*region, RegionHandle(0));
        assert!(
            distance(*at, [x, y, GROUND + lift]) < LANDING_SLOP,
            "the teleport goes to the spot, pelvis-to-foot up: {at:?}"
        );
        Ok(())
    }

    /// **What a ground action refuses**: a spot under a prim fails naming the
    /// prim (no reveal); a region the viewer does not know times out on the
    /// `attached` check; the ground has nothing to select; and a point past a
    /// region's edge is no point of it.
    #[test]
    fn a_ground_action_names_what_is_in_the_way() -> Result<(), TestError> {
        let mut app = grounded()?;
        let [x, y] = SPOT;
        seed_object(&mut app, fixture_prim(7, Vector { x, y, z: GROUND }, 0));
        settle(&mut app, 3);
        let covered = ask(&mut app, on_ground(REGION, WorldAction::DoubleClick, false))?;
        match covered.result {
            Err(AutomationError::GroundNotActionable {
                check, covered_by, ..
            }) => {
                assert_eq!(check, ActionabilityCheck::ReceivesEvents);
                assert_eq!(covered_by, Some(Uuid::from_u128(7)));
            }
            other => return Err(format!("not a covered failure: {other:?}").into()),
        }
        let unknown = ask(&mut app, on_ground("Nowhere", WorldAction::Click, false))?;
        assert!(
            matches!(
                unknown.result,
                Err(AutomationError::GroundTimedOut {
                    failed_check: ActionabilityCheck::Attached,
                    ..
                })
            ),
            "{:?}",
            unknown.result
        );
        let select = ask(&mut app, on_ground(REGION, WorldAction::Select, false))?;
        assert!(
            matches!(select.result, Err(AutomationError::InvalidRequest { .. })),
            "{:?}",
            select.result
        );
        let past = ask(
            &mut app,
            RequestBody::GroundAction {
                ground: GroundPoint::new(REGION, 256.0, 8.0),
                action: WorldAction::Click,
                reveal: false,
                deadline: Deadline::default(),
            },
        )?;
        assert!(
            matches!(past.result, Err(AutomationError::InvalidRequest { .. })),
            "{:?}",
            past.result
        );
        assert!(teleports(&mut app).is_empty(), "nothing was double-clicked");
        Ok(())
    }

    /// **A double-click on objects and avatars**: a prim that takes no click
    /// of its own and another avatar are landed on; a sit prim and the own
    /// avatar never; a touch-scripted prim only while
    /// `FSAllowDoubleClickOnScriptedObjects` is on.
    #[test]
    fn a_double_click_lands_on_what_takes_no_click_and_on_others() -> Result<(), TestError> {
        let mut app = grounded()?;
        let [x, y] = SPOT;
        let small = |local: u32, dx: f32, flags: u32, click_action: u8| {
            let mut prim = fixture_prim(
                local,
                Vector {
                    x: x + dx,
                    y,
                    z: GROUND + 0.5,
                },
                flags,
            );
            prim.scale = Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            };
            prim.click_action = click_action;
            prim
        };
        seed_object(&mut app, small(1, -3.0, 0, 0));
        seed_object(&mut app, small(2, 0.0, FLAGS_HANDLE_TOUCH, 0));
        seed_object(&mut app, small(3, 3.0, 0, 1));
        let other = AgentKey::from(Uuid::from_u128(OTHER));
        let own = AgentKey::from(Uuid::from_u128(OWN));
        let _other = seed_avatar(
            &mut app,
            other,
            10,
            Vector {
                x,
                y: y + 4.0,
                z: GROUND + 1.0,
            },
        );
        let _own = seed_avatar(
            &mut app,
            own,
            11,
            Vector {
                x: x + 4.0,
                y: y + 4.0,
                z: GROUND + 1.0,
            },
        );
        settle(&mut app, 5);
        let _before = drain_commands(&mut app);
        let prim = |local: u32| WorldLocator::kind(WorldKind::Object).local_id(local);
        let lands = |app: &mut App, locator: WorldLocator| -> Result<bool, TestError> {
            let done = ask(app, on_thing(locator, WorldAction::DoubleClick))?.result?;
            let ResponseBody::WorldDone {
                hit_point: Some(hit),
                ..
            } = done
            else {
                return Err(format!("no hit point: {done:?}").into());
            };
            let asked = teleports(app);
            match asked.as_slice() {
                [] => Ok(false),
                [(_region, at)] => {
                    let [hx, hy, _hz] = hit;
                    let [tx, ty, _tz] = *at;
                    assert!(
                        (hx - tx).hypot(hy - ty) < LANDING_SLOP,
                        "the teleport goes where the double-click landed: {at:?} for {hit:?}"
                    );
                    Ok(true)
                }
                more => Err(format!("several teleports: {more:?}").into()),
            }
        };
        assert!(lands(&mut app, prim(1))?, "a plain prim");
        assert!(
            lands(&mut app, prim(2))?,
            "a touch-scripted prim, with scripted objects allowed"
        );
        assert!(!lands(&mut app, prim(3))?, "a sit prim");
        assert!(
            lands(&mut app, WorldLocator::full_id(other.uuid()))?,
            "another avatar"
        );
        assert!(
            !lands(&mut app, WorldLocator::own_avatar())?,
            "one's own avatar"
        );
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_DOUBLE_CLICK_SCRIPTED_OBJECTS,
            SettingValue::Bool(false),
        );
        assert!(
            !lands(&mut app, prim(2))?,
            "a touch-scripted prim, with scripted objects not allowed"
        );
        assert!(lands(&mut app, prim(1))?, "a plain prim still");
        Ok(())
    }
}
