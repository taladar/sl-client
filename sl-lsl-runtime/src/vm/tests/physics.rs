//! The physics tranche's library rules held to the grids: what each flavour
//! refuses and in which words, which frame it keeps a local force in, the
//! time constant a look-at turns with, how a keyframe list is read, and
//! what the pathfinding and OpenSim's own physics functions hand the host.
//! What the physics then does with a call is the host's, held to the
//! `physics/*` probes by the fake grid's runs of them (`results/fake-*`).

use pretty_assertions::assert_eq;
use sl_types::lsl::{Rotation, Vector};

use super::*;
use crate::vm::physics::{
    CharacterCall, Goal, Inertia, KeyframeMode, KeyframedCall, KeyframedState, PhysicsCall,
    PhysicsReading, VehicleCall,
};

/// The key the Recorder lends a script's own prim under.
const OWN: &str = "fab01e52-3517-4545-be1b-4cbf6d8ace3b";

/// A Recorder whose script `A` is in the root of its object, so the root
/// the library reads is the prim the script changes.
fn rooted() -> Recorder {
    let mut host = Recorder::default();
    let _previous = host.objects.insert(A, OWN.to_owned());
    host
}

/// Runs `source` as script `A` under `flavour` for `ticks`, the host made
/// by `host`.
fn ran(
    flavour: EngineFlavour,
    source: &str,
    host: Recorder,
    ticks: u64,
) -> Result<Recorder, String> {
    let mut engine = engine_as(flavour);
    let mut host = host;
    let _none = engine.add(A, program_for(source, flavour)?);
    let _reports = run(&mut engine, &mut host, ticks);
    Ok(host)
}

/// The physics calls `host` was handed, in order.
fn calls(host: &Recorder) -> Vec<PhysicsCall> {
    host.physics_calls
        .iter()
        .map(|(_caller, call)| call.clone())
        .collect()
}

/// The zero vector.
const ZERO: Vector = Vector {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

#[test]
fn a_local_force_is_kept_in_the_prims_frame_on_second_life_and_turned_on_opensim()
-> Result<(), String> {
    let source = "default { state_entry() {
        llSetRot(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
        llSetForce(<1.0, 0.0, 0.0>, TRUE);
        llSetTorque(<1.0, 2.0, 3.0>, FALSE);
    } }";
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
        let host = ran(flavour, source, rooted(), 40)?;
        let calls = calls(&host);
        let Some(PhysicsCall::Force { force, local }) = calls.first() else {
            return Err(format!("{flavour:?}: no force in {calls:?}"));
        };
        if flavour.is_opensim() {
            assert!(!local, "{flavour:?}");
            assert!(
                force.x.abs() < 1e-5 && (force.y - 1.0).abs() < 1e-5,
                "{force:?}"
            );
        } else {
            assert!(local, "{flavour:?}");
            assert_eq!(force.clone(), Vector { x: 1.0, ..ZERO });
        }
        assert_eq!(
            calls.get(1),
            Some(&PhysicsCall::Torque {
                torque: Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0
                },
                local: false,
            }),
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn impulses_and_velocities_on_a_prim_that_is_not_physical() -> Result<(), String> {
    // Second Life drops an impulse and both drop a velocity on a prim that
    // is not physical; OpenSim keeps the impulse for when it turns physical,
    // capped at 20 000.
    let source = "default { state_entry() {
        llApplyImpulse(<0.0, 0.0, 30000.0>, FALSE);
        llSetVelocity(<1.0, 0.0, 0.0>, FALSE);
        llSetAngularVelocity(<0.0, 0.0, 1.0>, FALSE);
    } }";
    let second_life = ran(EngineFlavour::SecondLife, source, rooted(), 2)?;
    assert_eq!(calls(&second_life), Vec::new());
    let opensim = ran(EngineFlavour::YEngine, source, rooted(), 2)?;
    assert_eq!(
        calls(&opensim),
        vec![PhysicsCall::Impulse {
            impulse: Vector {
                z: 20_000.0,
                ..ZERO
            },
            local: false,
        }]
    );
    Ok(())
}

#[test]
fn a_material_out_of_range_is_refused_whole() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetPhysicsMaterial(DENSITY | FRICTION, 0.0, 0.0, 300.0, 500.0);
        llSetPhysicsMaterial(DENSITY | GRAVITY_MULTIPLIER, 0.5, 9.0, 9.0, 500.0);
        print(llList2CSV(llGetPhysicsMaterial()));
    } }";
    for (flavour, heard) in [
        (
            EngineFlavour::SecondLife,
            vec!["ERROR: Failed to set material properties."],
        ),
        (EngineFlavour::YEngine, Vec::new()),
    ] {
        let host = ran(flavour, source, rooted(), 40)?;
        assert_eq!(host.heard(A), heard, "{flavour:?}");
        assert_eq!(
            host.texts(A),
            vec!["0.500000, 0.500000, 0.600000, 500.000000"],
            "{flavour:?}"
        );
    }
    Ok(())
}

#[test]
fn vehicle_codes_no_setter_takes_are_named_on_second_life_alone() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetVehicleType(VEHICLE_TYPE_CAR);
        llSetVehicleFloatParam(999, 1.0);
        llSetVehicleVectorParam(VEHICLE_HOVER_HEIGHT, <1.0, 0.0, 0.0>);
        llSetVehicleRotationParam(999, ZERO_ROTATION);
        llSetVehicleFloatParam(VEHICLE_LINEAR_FRICTION_TIMESCALE, 2.0);
        llSetVehicleFlags(0x40000000);
    } }";
    let second_life = ran(EngineFlavour::SecondLife, source, rooted(), 2)?;
    assert_eq!(
        second_life.heard(A),
        vec![
            "Unknown vehicle float parameter 999",
            "Unknown vehicle vector parameter 24",
            "Unknown vehicle rotation parameter 999",
        ]
    );
    let expected = vec![
        PhysicsCall::Vehicle(VehicleCall::Type(2)),
        PhysicsCall::Vehicle(VehicleCall::Float(16, 2.0)),
        PhysicsCall::Vehicle(VehicleCall::SetFlags(0x4000_0000)),
    ];
    assert_eq!(calls(&second_life), expected);
    let opensim = ran(EngineFlavour::YEngine, source, rooted(), 12)?;
    assert_eq!(opensim.heard(A), Vec::<&str>::new());
    assert_eq!(calls(&opensim), expected);
    Ok(())
}

#[test]
fn a_look_at_turns_a_physical_prim_with_each_grids_time_constant() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetStatus(STATUS_PHYSICS, TRUE);
        llRotLookAt(ZERO_ROTATION, 1.0, 0.5);
        llRotLookAt(ZERO_ROTATION, 4.0, 0.5);
        llStopLookAt();
    } }";
    let taus = |host: &Recorder| -> Vec<Option<f32>> {
        calls(host)
            .into_iter()
            .filter_map(|call| match call {
                PhysicsCall::LookAt(look) => Some(look.map(|look| look.tau)),
                _ => None,
            })
            .collect()
    };
    let second_life = ran(EngineFlavour::SecondLife, source, rooted(), 2)?;
    assert_eq!(taus(&second_life), vec![Some(0.5), Some(0.5), None]);
    // OpenSim spins at the angle left times π × min(1, 1 / strength).
    let opensim = ran(EngineFlavour::YEngine, source, rooted(), 2)?;
    let got = taus(&opensim);
    let pi = core::f32::consts::PI;
    let expected = [1.0 / pi, 4.0 / pi];
    assert_eq!(got.len(), 3);
    for (got, expected) in got.iter().zip(expected) {
        let got = got.ok_or("no time constant")?;
        assert!((got - expected).abs() < 1e-5, "{got} against {expected}");
    }
    Ok(())
}

#[test]
fn a_look_at_turns_a_prim_that_is_not_physical_at_once() -> Result<(), String> {
    let source = "default { state_entry() {
        llRotLookAt(llEuler2Rot(<0.0, 0.0, PI_BY_TWO>), 1.0, 0.5);
        print((string)(llAngleBetween(llGetRot(), ZERO_ROTATION) > 1.0));
        llLookAt(llGetPos() + <5.0, 0.0, 0.0>, 0.0, 0.5);
        print((string)(llAngleBetween(llGetRot(), ZERO_ROTATION) > 1.0));
    } }";
    for flavour in [EngineFlavour::SecondLife, EngineFlavour::YEngine] {
        let host = ran(flavour, source, rooted(), 40)?;
        // The rotation look-at turned it; a look-at of no strength did not
        // turn it back.
        assert_eq!(host.texts(A), vec!["1", "1"], "{flavour:?}");
        assert_eq!(calls(&host), Vec::new(), "{flavour:?}");
    }
    Ok(())
}

#[test]
fn second_life_refuses_a_keyframed_motion_on_the_default_physics_shape() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION, 1.0], []);
        llSetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE, PRIM_PHYSICS_SHAPE_CONVEX]);
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION, 1.0], [KFM_MODE, KFM_REVERSE]);
    } }";
    let host = ran(EngineFlavour::SecondLife, source, rooted(), 40)?;
    assert_eq!(
        host.heard(A),
        vec![
            "Error, failed to play animation. Does your linkset exceed the complexity limit for \
             keyframing?"
        ]
    );
    let played: Vec<KeyframeMode> = calls(&host)
        .into_iter()
        .filter_map(|call| match call {
            PhysicsCall::Keyframed(KeyframedCall::Play(motion)) => Some(motion.mode),
            _ => None,
        })
        .collect();
    assert_eq!(played, vec![KeyframeMode::Reverse]);
    Ok(())
}

#[test]
fn second_life_names_what_is_wrong_with_a_keyframe_list() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetPrimitiveParams([PRIM_PHYSICS_SHAPE_TYPE, PRIM_PHYSICS_SHAPE_CONVEX]);
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION, 0.05], []);
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION], []);
        llSetKeyframedMotion([ZERO_ROTATION, <0.0, 0.0, 0.5>, 1.0], []);
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION, 1.0], [KFM_DATA, KFM_TRANSLATION]);
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION, 1.0], [KFM_MODE, 7]);
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION, 1.0], [99, 1]);
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION, 1], []);
    } }";
    let host = ran(EngineFlavour::SecondLife, source, rooted(), 40)?;
    assert_eq!(
        host.heard(A),
        vec![
            "Delta times must be >0.1s",
            "Expected delta time in keyframe list.",
            "Error, expected translation vector in keyframe list.",
            "Expected delta time in keyframe list.",
            "Unknown playback mode.",
            "Unknown parameter type.",
        ]
    );
    // The integer time is a time.
    let seconds: Vec<f32> = calls(&host)
        .into_iter()
        .filter_map(|call| match call {
            PhysicsCall::Keyframed(KeyframedCall::Play(motion)) => {
                motion.frames.first().map(|frame| frame.seconds)
            }
            _ => None,
        })
        .collect();
    assert_eq!(seconds, vec![1.0]);
    Ok(())
}

#[test]
fn opensim_plays_what_second_life_refuses_and_throws_at_a_wrong_type() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION, 0.05], [KFM_MODE, 7, 99, 1]);
        llSetKeyframedMotion([<0.0, 0.0, 0.5>, ZERO_ROTATION], []);
        llSetKeyframedMotion([ZERO_ROTATION, <0.0, 0.0, 0.5>, 1.0], []);
        print(\"after\");
    } }";
    let mut engine = engine_as(EngineFlavour::YEngine);
    let mut host = rooted();
    let _none = engine.add(A, program_for(source, EngineFlavour::YEngine)?);
    let reports = run(&mut engine, &mut host, 4);
    let played: Vec<(usize, KeyframeMode)> = calls(&host)
        .into_iter()
        .filter_map(|call| match call {
            PhysicsCall::Keyframed(KeyframedCall::Play(motion)) => {
                Some((motion.frames.len(), motion.mode))
            }
            _ => None,
        })
        .collect();
    // The short frame and the unknown mode play forward; the incomplete
    // frame is dropped, leaving none.
    assert_eq!(
        played,
        vec![(1, KeyframeMode::Forward), (0, KeyframeMode::Forward)]
    );
    let faulted = reports.iter().any(|report| {
        outcomes(report, A).iter().any(|outcome| {
            matches!(
                outcome,
                Outcome::Faulted(Fault {
                    error: RuntimeError::Exception {
                        exception: crate::library::Exception::ItemNot {
                            expected: "Vector3",
                            given: "Quaternion",
                        },
                        ..
                    },
                    ..
                })
            )
        })
    });
    assert!(faulted, "the swapped frame threw");
    assert_eq!(host.texts(A), Vec::<&str>::new());
    Ok(())
}

#[test]
fn second_life_will_not_move_or_make_physical_an_object_in_a_motion() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetPos(llGetPos() + <1.0, 0.0, 0.0>);
        llSetStatus(STATUS_PHYSICS, TRUE);
        print((string)llGetStatus(STATUS_PHYSICS));
    } }";
    let playing = |host: &mut Recorder| {
        let mut reading = PhysicsReading::at_rest(ZERO);
        reading.keyframed = KeyframedState::Playing;
        let _previous = host.physics_readings.insert(A, reading);
    };
    let mut host = rooted();
    playing(&mut host);
    let second_life = ran(EngineFlavour::SecondLife, source, host, 40)?;
    assert_eq!(
        second_life.heard(A),
        vec![
            "Cannot use a script to move a linkset while it is playing an animation. Stop the \
             animation first.",
            "Can't enable physics for keyframed objects.",
        ]
    );
    assert_eq!(second_life.texts(A), vec!["0"]);
    let mut host = rooted();
    playing(&mut host);
    let opensim = ran(EngineFlavour::YEngine, source, host, 40)?;
    assert_eq!(opensim.heard(A), Vec::<&str>::new());
    assert_eq!(opensim.texts(A), vec!["1"]);
    Ok(())
}

#[test]
fn a_character_is_made_steered_and_refused_as_second_life_does() -> Result<(), String> {
    // The Recorder's physics never makes the object a character, so the
    // update after the creation is refused as one before it would be.
    let source = "default { state_entry() {
        llNavigateTo(<10.0, 10.0, 20.0>, []);
        llCreateCharacter([CHARACTER_DESIRED_SPEED, 100.0, CHARACTER_RADIUS, 0.25]);
        llUpdateCharacter([CHARACTER_DESIRED_SPEED, 2.0]);
        llExecCharacterCmd(999, []);
        llExecCharacterCmd(CHARACTER_CMD_STOP, []);
        llDeleteCharacter();
    } }";
    let host = ran(EngineFlavour::SecondLife, source, rooted(), 2)?;
    assert_eq!(
        host.heard(A),
        vec![
            "You must call llCreateCharacter before llNavigateTo.",
            "Error: CHARACTER_DESIRED_SPEED 100 larger than max 40",
            "You must call llCreateCharacter before llUpdateCharacter.",
            "Unrecognized character command.",
        ]
    );
    let made: Vec<CharacterCall> = calls(&host)
        .into_iter()
        .filter_map(|call| match call {
            PhysicsCall::Character(call) => Some(call),
            _ => None,
        })
        .collect();
    let Some(CharacterCall::Make(settings)) = made.first() else {
        return Err(format!("no character made: {made:?}"));
    };
    // The refused speed leaves the default.
    assert_eq!(settings.speed.to_bits(), 3.0_f32.to_bits());
    assert_eq!(settings.radius.to_bits(), 0.25_f32.to_bits());
    assert_eq!(
        made.get(1..),
        Some(&[CharacterCall::Stop, CharacterCall::Delete][..])
    );
    Ok(())
}

#[test]
fn a_character_goes_after_its_goal() -> Result<(), String> {
    let source = "default { state_entry() {
        llNavigateTo(<10.0, 10.0, 20.0>, []);
        llPatrolPoints([<1.0, 1.0, 20.0>, \"skipped\", <2.0, 2.0, 20.0>], []);
        llFleeFrom(<5.0, 5.0, 20.0>, 8.0, []);
    } }";
    let mut host = rooted();
    let mut reading = PhysicsReading::at_rest(ZERO);
    reading.character = true;
    let _previous = host.physics_readings.insert(A, reading);
    let host = ran(EngineFlavour::SecondLife, source, host, 2)?;
    let at = |x: f32, y: f32| Vector { x, y, z: 20.0 };
    assert_eq!(
        calls(&host),
        vec![
            PhysicsCall::Character(CharacterCall::Go(Goal::Point(at(10.0, 10.0)))),
            PhysicsCall::Character(CharacterCall::Go(Goal::Patrol(vec![
                at(1.0, 1.0),
                at(2.0, 2.0)
            ]))),
            PhysicsCall::Character(CharacterCall::Go(Goal::Flee {
                from: at(5.0, 5.0),
                distance: 8.0,
            })),
        ]
    );
    Ok(())
}

#[test]
fn a_static_path_needs_its_goal_then_its_start_on_the_navmesh() -> Result<(), String> {
    // The Recorder's ground is flat at its `ground`.
    let source = "default { state_entry() {
        print(llList2CSV(llGetStaticPath(<10.0, 10.0, 21.0>, <12.0, 10.0, 25.0>, 0.5, [])));
        print(llList2CSV(llGetStaticPath(<10.0, 10.0, 25.0>, <12.0, 10.0, 21.0>, 0.5, [])));
        print(llList2CSV(llGetStaticPath(<10.0, 10.0, 21.0>, <12.0, 10.0, 20.5>, 0.5, [])));
    } }";
    let mut host = rooted();
    host.ground = 20.0;
    let host = ran(EngineFlavour::SecondLife, source, host, 2)?;
    assert_eq!(
        host.texts(A),
        vec![
            "3",
            "2",
            "<10.000000, 10.000000, 20.000000>, <12.000000, 10.000000, 20.000000>, 0",
        ]
    );
    Ok(())
}

#[test]
fn opensims_inertia_reads_the_rotations_y_twice_and_a_teleport_needs_a_key() -> Result<(), String> {
    let source = "default { state_entry() {
        osSetInertia(2.0, <0.0, 0.0, 0.1>, <1.0, 2.0, 3.0>, <0.0, 0.6, 0.8, 0.0>);
        osSetInertia(-1.0, ZERO_VECTOR, <1.0, 1.0, 1.0>, ZERO_ROTATION);
        osSetInertiaAsSphere(5.0, 2.0, ZERO_VECTOR);
        osClearInertia();
        print((string)osTeleportObject(\"nothing\", <1.0, 1.0, 1.0>, ZERO_ROTATION, 0));
    } }";
    // `osTeleportObject` is a Severe function the stock settings allow the
    // estate's owner.
    let mut owner = rooted();
    owner.standing.roles.push(crate::vm::Allowed::EstateOwner);
    let host = ran(EngineFlavour::YEngine, source, owner, 40)?;
    let set: Vec<Option<Inertia>> = calls(&host)
        .into_iter()
        .filter_map(|call| match call {
            PhysicsCall::Inertia(inertia) => Some(inertia),
            _ => None,
        })
        .collect();
    let half = core::f32::consts::FRAC_1_SQRT_2;
    let Some(Some(first)) = set.first() else {
        return Err(format!("no inertia set: {set:?}"));
    };
    assert_eq!(
        first.inertia,
        Vector {
            x: 2.0,
            y: 4.0,
            z: 6.0
        }
    );
    // `<0, 0.6, 0.8, 0>` read as `<0, 0.6, 0.6, 0>`, normalised.
    assert!((first.rotation.y - half).abs() < 1e-5 && (first.rotation.z - half).abs() < 1e-5);
    assert_eq!(first.rotation.x.to_bits(), 0.0_f32.to_bits());
    let Some(Some(sphere)) = set.get(1) else {
        return Err(format!("no sphere: {set:?}"));
    };
    assert_eq!(
        sphere.inertia,
        Vector {
            x: 8.0,
            y: 8.0,
            z: 8.0
        }
    );
    assert_eq!(
        sphere.rotation,
        Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0
        }
    );
    assert_eq!(set.get(2), Some(&None));
    assert_eq!(set.len(), 3);
    assert_eq!(host.texts(A), vec!["-1"]);
    assert_eq!(host.heard(A), vec!["osTeleportObject() invalid object Key"]);
    Ok(())
}

#[test]
fn a_collision_filter_of_nothing_is_no_filter() -> Result<(), String> {
    let source = "default { state_entry() {
        llCollisionFilter(\"Ball\", NULL_KEY, TRUE);
        llCollisionFilter(\"\", NULL_KEY, FALSE);
        llCollisionFilter(\"\", NULL_KEY, TRUE);
    } }";
    let host = ran(EngineFlavour::SecondLife, source, rooted(), 2)?;
    let filters: Vec<Option<(String, bool)>> = calls(&host)
        .into_iter()
        .filter_map(|call| match call {
            PhysicsCall::CollisionFilter(filter) => {
                Some(filter.map(|filter| (filter.name, filter.accept)))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        filters,
        vec![
            Some(("Ball".to_owned(), true)),
            Some((String::new(), false)),
            None
        ]
    );
    Ok(())
}
