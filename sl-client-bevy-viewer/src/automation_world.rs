//! The **world model** over the fixture world: world locators resolved
//! against what the viewer's own world layers track (`sl_viewer_automation::
//! WorldModel`), with object names that only arrive once asked for.

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_automation_proto::{Anchor, AutomationError, WorldKind, WorldLocator, WorldNode};
    use sl_client_bevy::{
        AgentKey, Command, LindenAmount, ObjectKey, ObjectPropertiesFamily, OwnerKey, Permissions5,
        Rotation, SlEvent, SlIdentity, SlSessionEvent as SessionEvent, Uuid, Vector, pcode,
    };
    use sl_viewer_automation::{
        PursuitError, WorldModelPlugin, WorldProgress, WorldQuery, WorldWant, world_snapshot,
    };

    use crate::world_test::{
        drain_commands, entity_of, fixture_prim, seed_attachment, seed_child_prim, seed_object,
        settle, world_app,
    };

    /// A failed setup step or a query that went wrong.
    type TestError = Box<dyn core::error::Error>;

    /// The own agent.
    const OWN: u128 = 0xA;
    /// The other resident.
    const OTHER: u128 = 0xB;
    /// The own avatar's region-local id.
    const OWN_LOCAL: u32 = 10;
    /// The other avatar's region-local id.
    const OTHER_LOCAL: u32 = 11;
    /// The door prim, a linkset root.
    const DOOR: u32 = 1;
    /// The window prim, with floating text.
    const WINDOW: u32 = 2;
    /// The door's hinge, a child prim of the door.
    const HINGE: u32 = 3;
    /// A hat the own avatar wears on its chest point.
    const HAT: u32 = 4;
    /// The chest attachment point.
    const CHEST: u8 = 1;

    /// A region-local position.
    const fn at(x: f32, y: f32, z: f32) -> Vector {
        Vector { x, y, z }
    }

    /// No rotation.
    const UNROTATED: Rotation = Rotation {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        s: 1.0,
    };

    /// A quarter turn about the region's up axis: facing north.
    const QUARTER_TURN: Rotation = Rotation {
        x: 0.0,
        y: 0.0,
        z: core::f32::consts::FRAC_1_SQRT_2,
        s: core::f32::consts::FRAC_1_SQRT_2,
    };

    /// Whether a readout rotation is `expected`, to a tolerance.
    fn rotated(rotation: Option<[f32; 4]>, expected: Rotation) -> bool {
        rotation.is_some_and(|rotation| {
            rotation
                .iter()
                .zip([expected.x, expected.y, expected.z, expected.s])
                .all(|(got, want)| (got - want).abs() < 1e-3)
        })
    }

    /// A (placeholder-sphere) avatar for `agent` under `local_id`, named by the
    /// legacy name-values a simulator streams with it.
    fn seed_named_avatar(
        app: &mut App,
        agent: u128,
        local_id: u32,
        first: &str,
        at: Vector,
        rotation: Rotation,
    ) {
        let agent = AgentKey::from(Uuid::from_u128(agent));
        let mut object = crate::objects::fixture_object(pcode::AVATAR);
        object.local_id = sl_client_bevy::RegionLocalObjectId(local_id);
        object.full_id = ObjectKey::from(agent.uuid());
        object.motion.position = at;
        object.motion.rotation = rotation;
        object.name_value = format!(
            "FirstName STRING RW SV {first}\nLastName STRING RW SV Resident\nTitle STRING RW SV "
        );
        seed_object(app, object);
    }

    /// The fixture world: the own avatar and another, a two-prim linkset, a
    /// prim with floating text, and a hat the own avatar wears — nothing
    /// named but the avatars.
    fn fixture() -> App {
        let mut app = world_app();
        app.add_plugins(WorldModelPlugin);
        app.world_mut().resource_mut::<SlIdentity>().agent_id =
            Some(AgentKey::from(Uuid::from_u128(OWN)));
        seed_named_avatar(
            &mut app,
            OWN,
            OWN_LOCAL,
            "Own",
            at(100.0, 100.0, 25.0),
            UNROTATED,
        );
        seed_named_avatar(
            &mut app,
            OTHER,
            OTHER_LOCAL,
            "Two",
            at(110.0, 100.0, 25.0),
            QUARTER_TURN,
        );
        seed_object(&mut app, fixture_prim(DOOR, at(105.0, 100.0, 25.0), 0));
        let mut window = fixture_prim(WINDOW, at(120.0, 100.0, 25.0), 0);
        window.text = "For sale".to_owned();
        window.motion.rotation = QUARTER_TURN;
        seed_object(&mut app, window);
        settle(&mut app, 2);
        seed_child_prim(&mut app, DOOR, HINGE, at(0.0, 0.0, 1.0));
        seed_attachment(&mut app, OWN_LOCAL, HAT, CHEST, at(0.0, 0.0, 0.5));
        settle(&mut app, 3);
        app
    }

    /// Poll `query` a frame at a time until it answers, settling between polls.
    fn resolve(app: &mut App, query: &mut WorldQuery) -> Result<Vec<WorldNode>, PursuitError> {
        loop {
            match query.poll(app.world_mut())? {
                WorldProgress::Ready(nodes) => return Ok(nodes),
                WorldProgress::Waiting { .. } => settle(app, 1),
            }
        }
    }

    /// The family reply a simulator sends for fixture prim `local_id`.
    fn family_reply(local_id: u32, name: &str, description: &str, owner: u128) -> SlEvent {
        SlEvent(SessionEvent::ObjectPropertiesFamily {
            properties: ObjectPropertiesFamily {
                request_flags: 0,
                object_id: ObjectKey::from(Uuid::from_u128(u128::from(local_id))),
                owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(owner))),
                group: None,
                permissions: Permissions5::default(),
                ownership_cost: LindenAmount(0),
                sale_type: 0,
                sale_price: None,
                category: 0,
                last_owner_id: Uuid::nil(),
                name: name.to_owned(),
                description: description.to_owned(),
            },
        })
    }

    /// The objects a drain of the outbound commands asked properties for.
    fn property_requests(app: &mut App) -> Vec<u128> {
        let mut asked: Vec<u128> = drain_commands(app)
            .into_iter()
            .filter_map(|command| match command {
                Command::RequestObjectPropertiesFamily { object_id, .. } => {
                    Some(object_id.uuid().as_u128())
                }
                _ => None,
            })
            .collect();
        asked.sort_unstable();
        asked
    }

    /// Whether two positions agree to a millimetre.
    fn near(left: Option<[f32; 3]>, right: [f32; 3]) -> bool {
        left.is_some_and(|left| {
            left.iter()
                .zip(right)
                .all(|(left, right)| (left - right).abs() < 1e-3)
        })
    }

    #[test]
    fn an_avatar_is_found_by_name_and_reads_back_as_seeded() -> Result<(), TestError> {
        let mut app = fixture();
        let two = WorldLocator::kind(WorldKind::Avatar).named("Two");
        let found = resolve(&mut app, &mut WorldQuery::new(two, WorldWant::One))?;
        let [avatar] = found.as_slice() else {
            return Err(format!("one avatar named Two, not {found:?}").into());
        };
        assert_eq!(avatar.full_id, Uuid::from_u128(OTHER));
        assert_eq!(avatar.local_id, Some(OTHER_LOCAL));
        assert_eq!(avatar.pcode, pcode::AVATAR);
        assert!(!avatar.own, "not the own avatar");
        assert!(
            near(avatar.position, [110.0, 100.0, 25.0]),
            "{:?}",
            avatar.position
        );
        assert!(
            avatar
                .name_tag
                .as_deref()
                .is_some_and(|tag| tag.lines().any(|line| line == "Two")),
            "the tag names the avatar: {:?}",
            avatar.name_tag
        );
        assert!(
            rotated(avatar.rotation, QUARTER_TURN),
            "facing north: {:?}",
            avatar.rotation
        );
        assert_eq!(avatar.sitting_on, None);
        assert!(
            property_requests(&mut app).is_empty(),
            "an avatar's name is not an object property"
        );

        let own = resolve(
            &mut app,
            &mut WorldQuery::new(WorldLocator::own_avatar(), WorldWant::One),
        )?;
        let [own] = own.as_slice() else {
            return Err(format!("one own avatar, not {own:?}").into());
        };
        assert_eq!(own.full_id, Uuid::from_u128(OWN));
        assert_eq!(own.name.as_deref(), Some("Own"));
        assert_eq!(own.children, vec![HAT], "the hat hangs off it");
        Ok(())
    }

    #[test]
    fn a_prim_is_found_by_name_once_its_properties_arrive() -> Result<(), TestError> {
        let mut app = fixture();
        let _setup = drain_commands(&mut app);
        let mut door = WorldQuery::new(
            WorldLocator::kind(WorldKind::Object).named("Door"),
            WorldWant::One,
        );

        // Nothing is named yet: the query waits, and asks once for each prim
        // it cannot judge — not for the worn hat, which is no `object`.
        let first = door.poll(app.world_mut())?;
        assert_eq!(first, WorldProgress::Waiting { unresolved: 3 });
        settle(&mut app, 1);
        assert_eq!(
            property_requests(&mut app),
            vec![DOOR, WINDOW, HINGE]
                .into_iter()
                .map(u128::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            door.poll(app.world_mut())?,
            WorldProgress::Waiting { unresolved: 3 }
        );
        settle(&mut app, 1);
        assert!(property_requests(&mut app).is_empty(), "asked only once");

        // The replies land; the next poll answers with the door alone.
        for reply in [
            family_reply(DOOR, "Door", "The front door", OWN),
            family_reply(WINDOW, "Window", "", OTHER),
            family_reply(HINGE, "Hinge", "", OWN),
        ] {
            app.world_mut().write_message(reply);
        }
        let scoped = sl_client_bevy::ScopedObjectId::new(
            sl_client_bevy::CircuitId::new(1),
            sl_client_bevy::RegionLocalObjectId(DOOR),
        );
        let entity = entity_of(&mut app, scoped).ok_or("the door has an entity")?;
        app.world_mut()
            .resource_mut::<crate::world_api::SelectionSet>()
            .insert(
                scoped,
                ObjectKey::from(Uuid::from_u128(u128::from(DOOR))),
                entity,
            );
        settle(&mut app, 1);
        let found = resolve(&mut app, &mut door)?;
        let expected = WorldNode {
            kind: WorldKind::Object,
            own: false,
            full_id: Uuid::from_u128(u128::from(DOOR)),
            local_id: Some(DOOR),
            pcode: pcode::PRIMITIVE,
            name: Some("Door".to_owned()),
            description: Some("The front door".to_owned()),
            owner: Some(Uuid::from_u128(OWN)),
            // Placement is compared to a tolerance below: it went through the
            // drawn transform and back.
            position: found.first().and_then(|node| node.position),
            rotation: found.first().and_then(|node| node.rotation),
            scale: Some([2.0, 3.0, 4.0]),
            parent: None,
            children: vec![HINGE],
            attachment_point: None,
            worn_by: None,
            sitting_on: None,
            selected: true,
            hover_text: None,
            name_tag: None,
        };
        assert_eq!(found, vec![expected]);
        assert!(
            near(
                found.first().and_then(|node| node.position),
                [105.0, 100.0, 25.0]
            ),
            "{found:?}"
        );
        let rotation = found.first().and_then(|node| node.rotation);
        assert!(rotated(rotation, UNROTATED), "unrotated: {rotation:?}");
        Ok(())
    }

    #[test]
    fn every_readout_matches_the_fixture() -> Result<(), TestError> {
        let mut app = fixture();
        for reply in [
            family_reply(DOOR, "Door", "", OWN),
            family_reply(WINDOW, "Window", "", OTHER),
            family_reply(HINGE, "Hinge", "", OWN),
            family_reply(HAT, "Hat", "Red", OWN),
        ] {
            app.world_mut().write_message(reply);
        }
        settle(&mut app, 1);
        let nodes = world_snapshot(app.world_mut())?;
        let order: Vec<(WorldKind, Option<u32>)> = nodes
            .iter()
            .map(|node| (node.kind, node.local_id))
            .collect();
        assert_eq!(
            order,
            vec![
                (WorldKind::Avatar, Some(OWN_LOCAL)),
                (WorldKind::Avatar, Some(OTHER_LOCAL)),
                (WorldKind::Object, Some(DOOR)),
                (WorldKind::Object, Some(WINDOW)),
                (WorldKind::Object, Some(HINGE)),
                (WorldKind::Attachment, Some(HAT)),
            ],
            "avatars first, the own one leading; objects by local id"
        );
        let find = |locator: WorldLocator| -> Result<WorldNode, TestError> {
            match sl_viewer_automation::find_world(&nodes, &locator).as_slice() {
                [only] => Ok((*only).clone()),
                other => Err(format!("{locator} matched {other:?}").into()),
            }
        };

        let hinge = find(WorldLocator::default().named("Hinge"))?;
        assert_eq!(hinge.parent, Some(DOOR), "a child prim hangs off its root");
        assert!(
            near(hinge.position, [105.0, 100.0, 26.0]),
            "its position is the root's plus its offset: {:?}",
            hinge.position
        );

        let window = find(WorldLocator::default().hover_text_containing("sale"))?;
        assert_eq!(window.local_id, Some(WINDOW));
        assert_eq!(window.hover_text.as_deref(), Some("For sale"));
        assert!(
            rotated(window.rotation, QUARTER_TURN),
            "facing north: {:?}",
            window.rotation
        );
        assert_eq!(window.owner, Some(Uuid::from_u128(OTHER)));

        let hat = find(WorldLocator::kind(WorldKind::Attachment).own(true))?;
        assert_eq!(hat.name.as_deref(), Some("Hat"));
        assert_eq!(hat.description.as_deref(), Some("Red"));
        assert_eq!(hat.attachment_point, Some(CHEST));
        assert_eq!(hat.worn_by, Some(Uuid::from_u128(OWN)));
        assert_eq!(hat.parent, Some(OWN_LOCAL));

        let nearest = find(WorldLocator::kind(WorldKind::Object).nearest_to(Anchor::OwnAvatar))?;
        assert_eq!(nearest.local_id, Some(DOOR), "5 m away, the hinge 5.1 m");
        let owned = sl_viewer_automation::find_world(
            &nodes,
            &WorldLocator::kind(WorldKind::Object).owned_by(Uuid::from_u128(OWN)),
        );
        assert_eq!(
            owned.iter().map(|node| node.local_id).collect::<Vec<_>>(),
            vec![Some(DOOR), Some(HINGE)]
        );
        Ok(())
    }

    #[test]
    fn an_ambiguous_name_fails_with_every_candidate() -> Result<(), TestError> {
        let mut app = fixture();
        for reply in [
            family_reply(DOOR, "Box", "", OWN),
            family_reply(WINDOW, "Box", "", OWN),
            family_reply(HINGE, "Hinge", "", OWN),
        ] {
            app.world_mut().write_message(reply);
        }
        settle(&mut app, 1);
        let mut query = WorldQuery::new(
            WorldLocator::kind(WorldKind::Object).named("Box"),
            WorldWant::One,
        );
        match resolve(&mut app, &mut query) {
            Err(PursuitError::Automation(error)) => match *error {
                AutomationError::WorldAmbiguous { candidates, .. } => {
                    assert_eq!(
                        candidates
                            .iter()
                            .map(|node| node.local_id)
                            .collect::<Vec<_>>(),
                        vec![Some(DOOR), Some(WINDOW)]
                    );
                    Ok(())
                }
                other => Err(format!("an ambiguity, not {other}").into()),
            },
            other => Err(format!("an ambiguity, not {other:?}").into()),
        }
    }

    #[test]
    fn a_name_that_never_arrives_times_out_naming_the_unresolved() -> Result<(), TestError> {
        let mut app = fixture();
        let mut query = WorldQuery::new(
            WorldLocator::kind(WorldKind::Object).named("Door"),
            WorldWant::All,
        )
        .with_deadline(sl_automation_proto::Deadline {
            frames: Some(5),
            millis: None,
        });
        match resolve(&mut app, &mut query) {
            Err(PursuitError::Automation(error)) => match *error {
                AutomationError::WorldTimedOut { unresolved, .. } => {
                    assert_eq!(
                        unresolved
                            .iter()
                            .map(|node| node.local_id)
                            .collect::<Vec<_>>(),
                        vec![Some(DOOR), Some(WINDOW), Some(HINGE)]
                    );
                    Ok(())
                }
                other => Err(format!("a timeout, not {other}").into()),
            },
            other => Err(format!("a timeout, not {other:?}").into()),
        }
    }
}
