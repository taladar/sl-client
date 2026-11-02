//! Sitting and attaching as a script reads them (`server-world-sit-and-attach`):
//! a sit target set and cleared as each grid clears it, the sitter after the
//! prims in the link numbers and the count, the camera a seat gives its
//! sitter, wearing the object only with the owner's grant, and where a worn
//! prim stands — each held to the rows of `book/src/gridspec/lsl.md` the
//! `avatar/sit`, `events/attach`, `prim/identity` and `prim/link` probes
//! measured.

use pretty_assertions::assert_eq;

use super::*;
use crate::value::NULL_KEY;
use crate::vm::seating::{Placement, Seat, SitTarget};

/// The key of the prim the scripts run in.
const PRIM: &str = "fab01e52-3517-4545-be1b-4cbf6d8ace3b";

/// An avatar who is not the owner.
const OTHER: &str = "0b0b0b0b-0000-4000-8000-000000000002";

/// Runs `source` once in a fresh `flavour` engine against `host`, for the
/// lines it prints.
fn printed(
    source: &str,
    flavour: EngineFlavour,
    host: &mut Recorder,
) -> Result<Vec<String>, String> {
    let mut engine = engine_as(flavour);
    let _none = engine.add(A, program_for(source, flavour)?);
    let _reports = run(&mut engine, host, 2);
    Ok(host.texts(A).into_iter().map(str::to_owned).collect())
}

/// The three flavours.
const FLAVOURS: [EngineFlavour; 3] = [
    EngineFlavour::SecondLife,
    EngineFlavour::YEngine,
    EngineFlavour::XEngine,
];

/// `llSitTarget` with an offset sets the prim's target, as written; a zero
/// offset clears it on Second Life whatever the turn, and on OpenSim only
/// with the identity turn.
#[test]
fn a_sit_target_is_set_and_cleared_as_each_grid_clears_it() -> Result<(), String> {
    let set = "default { state_entry() {
        llSitTarget(<0.0, 0.0, 0.5>, llEuler2Rot(<0.0, 0.0, PI_BY_TWO>));
    } }";
    let turned_only = "default { state_entry() {
        llSitTarget(ZERO_VECTOR, <0.0, 0.0, 1.0, 0.0>);
    } }";
    let cleared = "default { state_entry() {
        llSitTarget(<0.0, 0.0, 0.5>, ZERO_ROTATION);
        llSitTarget(ZERO_VECTOR, ZERO_ROTATION);
    } }";
    for flavour in FLAVOURS {
        let mut host = Recorder::default();
        let _lines = printed(set, flavour, &mut host)?;
        let target = host.seats.get(&A.0).and_then(|seat| seat.target.clone());
        assert_eq!(
            target.map(|target| target.offset),
            Some(sl_types::lsl::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.5
            }),
            "{flavour:?}"
        );

        let mut host = Recorder::default();
        let _lines = printed(cleared, flavour, &mut host)?;
        assert_eq!(
            host.seats.get(&A.0).and_then(|seat| seat.target.clone()),
            None,
            "{flavour:?}"
        );

        let mut host = Recorder::default();
        let _lines = printed(turned_only, flavour, &mut host)?;
        assert_eq!(
            host.seats
                .get(&A.0)
                .is_some_and(|seat| seat.target.is_some()),
            flavour.is_opensim(),
            "{flavour:?}"
        );
    }
    Ok(())
}

/// The prim's counts and link keys with nobody on it and with an avatar on
/// its target (`avatar/sit`, `prim/identity`): one prim and link 0 alone, two
/// and the sitter the last link once seated; `llGetLinkKey(1)` of a lone prim
/// is `NULL_KEY` on Second Life and the prim on OpenSim.
#[test]
fn a_sitter_is_counted_and_takes_the_link_after_the_prims() -> Result<(), String> {
    let source = "default { state_entry() {
        print((string)llGetNumberOfPrims() + \" \" + (string)llGetLinkNumber()
            + \" \" + (string)llGetLinkKey(0) + \" \" + (string)llGetLinkKey(1)
            + \" \" + (string)llGetLinkKey(llGetNumberOfPrims())
            + \" \" + (string)llAvatarOnSitTarget());
    } }";
    for flavour in FLAVOURS {
        let mut host = Recorder::default();
        let alone = printed(source, flavour, &mut host)?;
        let link_one = if flavour.is_opensim() { PRIM } else { NULL_KEY };
        let last = if flavour.is_opensim() { PRIM } else { NULL_KEY };
        assert_eq!(
            alone,
            vec![format!("1 0 {PRIM} {link_one} {last} {NULL_KEY}")],
            "{flavour:?}"
        );

        let mut host = Recorder {
            sitters: vec![OTHER.to_owned()],
            on_target: [(A.0, OTHER.to_owned())].into_iter().collect(),
            ..Recorder::default()
        };
        let seated = printed(source, flavour, &mut host)?;
        let link_zero = if flavour.is_opensim() { PRIM } else { NULL_KEY };
        assert_eq!(
            seated,
            vec![format!("2 1 {link_zero} {PRIM} {OTHER} {OTHER}")],
            "{flavour:?}"
        );
    }
    Ok(())
}

/// The camera a seat gives its sitter and the pie's texts: the offsets as
/// set, mouselook on, and the texts cut to nine characters.
#[test]
fn a_seat_keeps_its_camera_and_its_pie_texts() -> Result<(), String> {
    let source = "default { state_entry() {
        llSetCameraEyeOffset(<-2.0, 0.0, 1.0>);
        llSetCameraAtOffset(<0.0, 0.0, 0.5>);
        llForceMouselook(TRUE);
        llSetSitText(\"Probe sit here\");
        llSetTouchText(\"Touch me please\");
    } }";
    let mut host = Recorder::default();
    let _lines = printed(source, EngineFlavour::SecondLife, &mut host)?;
    assert_eq!(
        host.seats.get(&A.0),
        Some(&Seat {
            target: None,
            camera_eye: sl_types::lsl::Vector {
                x: -2.0,
                y: 0.0,
                z: 1.0
            },
            camera_at: sl_types::lsl::Vector {
                x: 0.0,
                y: 0.0,
                z: 0.5
            },
            force_mouselook: true,
        })
    );
    assert_eq!(
        host.pie_texts,
        vec![
            (A, true, "Probe sit".to_owned()),
            (A, false, "Touch me ".to_owned()),
        ]
    );
    Ok(())
}

/// `llAttachToAvatar` wears the object only once its owner granted
/// `PERMISSION_ATTACH`, and not again while it is worn; `llDetachFromAvatar`
/// takes off only a worn object, with the same grant.
#[test]
fn wearing_and_taking_off_need_the_owners_grant() -> Result<(), String> {
    let source = "default {
        state_entry() { llAttachToAvatar(ATTACH_LHAND); llRequestPermissions(llGetOwner(), PERMISSION_ATTACH); }
        run_time_permissions(integer granted) { llAttachToAvatar(ATTACH_LHAND); llDetachFromAvatar(); }
        touch_start(integer count) { llDetachFromAvatar(); }
    }";
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(A, program(source)?);
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.attached_to, Vec::new(), "attached without a grant");
    assert_eq!(host.questions.len(), 1);
    let _posted = engine
        .answer(A, OWNER, crate::vm::permissions::PERMISSION_ATTACH)
        .map_err(|error| error.to_string())?;
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.attached_to, vec![(A, 5)]);
    assert_eq!(host.detached, Vec::new(), "took off an object not worn");

    let _worn = host.points.insert(A, 5);
    let _posted = engine
        .post_detected(A, named("touch_start")?, vec![super::toucher("Owner")])
        .map_err(|error| error.to_string())?;
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.detached, vec![A]);
    Ok(())
}

/// A grant from somebody other than the owner wears nothing.
#[test]
fn another_avatars_grant_wears_nothing() -> Result<(), String> {
    let source = "default {
        state_entry() { llRequestPermissions(\"0b0b0b0b-0000-4000-8000-000000000002\", PERMISSION_ATTACH); }
        run_time_permissions(integer granted) { llAttachToAvatar(ATTACH_LHAND); }
    }";
    let mut engine = engine();
    let mut host = Recorder::default();
    let _none = engine.add(A, program(source)?);
    let _reports = run(&mut engine, &mut host, 1);
    let _posted = engine
        .answer(A, OTHER, crate::vm::permissions::PERMISSION_ATTACH)
        .map_err(|error| error.to_string())?;
    let _reports = run(&mut engine, &mut host, 1);
    assert_eq!(host.attached_to, Vec::new());
    Ok(())
}

/// The placement functions read the host's three positions, and
/// `llGetAttached` its point.
#[test]
fn the_placement_functions_read_where_the_host_has_the_prim() -> Result<(), String> {
    let source = "default { state_entry() {
        print((string)llGetPos() + (string)llGetLocalPos() + (string)llGetRootPosition()
            + (string)llGetAttached());
    } }";
    let at = |x: f32| sl_types::lsl::Vector { x, y: 0.0, z: 0.0 };
    let mut host = Recorder {
        placements: [(
            A,
            Placement {
                position: at(1.0),
                local: at(2.0),
                root: at(3.0),
            },
        )]
        .into_iter()
        .collect(),
        points: [(A, 5)].into_iter().collect(),
        ..Recorder::default()
    };
    let lines = printed(source, EngineFlavour::SecondLife, &mut host)?;
    assert_eq!(
        lines,
        vec!["<1.00000, 0.00000, 0.00000><2.00000, 0.00000, 0.00000><3.00000, 0.00000, 0.00000>5"]
    );
    Ok(())
}

/// `llLinkSitTarget(LINK_THIS, …)` sets the caller's own prim, and a link
/// number naming no prim sets nothing.
#[test]
fn a_link_sit_target_names_one_prim() -> Result<(), String> {
    let source = "default { state_entry() {
        llLinkSitTarget(5, <0.0, 0.0, 2.0>, ZERO_ROTATION);
        llLinkSitTarget(LINK_THIS, <0.0, 0.0, 1.0>, ZERO_ROTATION);
    } }";
    let mut host = Recorder::default();
    let _lines = printed(source, EngineFlavour::SecondLife, &mut host)?;
    assert_eq!(
        host.seats.get(&A.0).and_then(|seat| seat.target.clone()),
        Some(SitTarget {
            offset: sl_types::lsl::Vector {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
            rotation: sl_types::lsl::Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0
            },
        })
    );
    Ok(())
}
