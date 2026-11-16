//! Library tranche: pathfinding characters (`server-lsl-lib-physics-vehicles`)
//! — `llCreateCharacter` and its siblings, the goals a character goes after,
//! and `llGetStaticPath`. Second Life's alone: OpenSim compiles none of them
//! (`physics/character`).
//!
//! The fake grid keeps no navmesh: it reads all of its ground as walkable,
//! as `llGetClosestNavPoint` does ([`super::land`]), so a point is on the
//! navmesh when it stands within a metre (`NAVMESH_REACH_M`) of the ground
//! under it.
//! A character goes in a straight line over the ground, which the host's
//! physics moves it along, raising `path_update` as it goes. What the grid
//! measured is the book chapter `gridspec/lsl.md`, § Pathfinding characters.

use sl_types::lsl::Vector;

use crate::library::{CallError, Key};
use crate::value::Element;
use crate::vm::ScriptCtx;
use crate::vm::physics::{Character, CharacterCall, Goal, PhysicsCall};

/// `CHARACTER_DESIRED_SPEED`.
const CHARACTER_DESIRED_SPEED: i32 = 1;
/// `CHARACTER_RADIUS`.
const CHARACTER_RADIUS: i32 = 2;
/// `CHARACTER_LENGTH`.
const CHARACTER_LENGTH: i32 = 3;
/// `CHARACTER_ORIENTATION`.
const CHARACTER_ORIENTATION: i32 = 4;
/// `CHARACTER_TYPE`.
const CHARACTER_TYPE: i32 = 6;
/// `CHARACTER_CMD_STOP`.
const CHARACTER_CMD_STOP: i32 = 0;
/// `CHARACTER_CMD_JUMP`.
const CHARACTER_CMD_JUMP: i32 = 1;
/// `CHARACTER_CMD_SMOOTH_STOP`.
const CHARACTER_CMD_SMOOTH_STOP: i32 = 2;
/// `PU_FAILURE_INVALID_START`.
const PU_FAILURE_INVALID_START: i32 = 2;
/// `PU_FAILURE_INVALID_GOAL`.
const PU_FAILURE_INVALID_GOAL: i32 = 3;

/// The fastest a character may be asked to go, metres a second
/// (`physics/character`: `Error: CHARACTER_DESIRED_SPEED 100 larger than max
/// 40`).
const MAX_DESIRED_SPEED: f32 = 40.0;

/// How far above or below the ground a point still lies on the fake grid's
/// navmesh, in metres. Unmeasured: aditi's navmesh lay 2.2 m below the
/// probe's prim, which it read as off it (`llGetStaticPath` from there:
/// `PU_FAILURE_INVALID_GOAL`), and its nearest point as on it.
pub(crate) const NAVMESH_REACH_M: f32 = 1.0;

/// `settings` with the options of `options` applied — a speed past 40
/// refused with Second Life's line, the others kept as given.
fn configured(ctx: &mut ScriptCtx<'_>, mut settings: Character, options: &[Element]) -> Character {
    for pair in options.chunks(2) {
        let (Some(Element::Integer(code)), Some(value)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let number = match value {
            Element::Float(value) => *value,
            Element::Integer(value) => crate::num::int_to_float(*value),
            _ => continue,
        };
        match *code {
            CHARACTER_DESIRED_SPEED if number > MAX_DESIRED_SPEED => ctx.refuse(format!(
                "Error: CHARACTER_DESIRED_SPEED {number} larger than max {MAX_DESIRED_SPEED}"
            )),
            CHARACTER_DESIRED_SPEED => settings.speed = number,
            CHARACTER_RADIUS => settings.radius = number,
            CHARACTER_LENGTH => settings.length = number,
            CHARACTER_ORIENTATION => settings.orientation = crate::num::float_to_int(number),
            CHARACTER_TYPE => settings.kind = crate::num::float_to_int(number),
            _ => {}
        }
    }
    settings
}

/// Hands the host's physics a character call.
fn character(ctx: &mut ScriptCtx<'_>, call: CharacterCall) {
    let caller = ctx.caller();
    ctx.host().physics(caller, PhysicsCall::Character(call));
}

/// Whether the caller's object is a character; when it is not, Second
/// Life's complaint that `function` needs one first
/// (`physics/character`: `You must call llCreateCharacter before
/// llNavigateTo.`, the same words for each function that needs one).
fn needs_character(ctx: &mut ScriptCtx<'_>, function: &str) -> bool {
    let caller = ctx.caller();
    if ctx.host().physics_reading(caller).character {
        return true;
    }
    ctx.refuse(format!(
        "You must call llCreateCharacter before {function}."
    ));
    false
}

/// `llCreateCharacter`: the object becomes a pathfinding character — it
/// turns physical, `OBJECT_PATHFINDING_TYPE` 2, its rotation flags left set
/// (`physics/character`) — with Second Life's defaults for what `options`
/// leave out.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_create_character(
    ctx: &mut ScriptCtx<'_>,
    options: Vec<Element>,
) -> Result<(), CallError> {
    let settings = configured(ctx, Character::default(), &options);
    character(ctx, CharacterCall::Make(settings));
    Ok(())
}

/// `llUpdateCharacter`: the character's settings that `options` name.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_update_character(
    ctx: &mut ScriptCtx<'_>,
    options: Vec<Element>,
) -> Result<(), CallError> {
    if !needs_character(ctx, "llUpdateCharacter") {
        return Ok(());
    }
    let settings = configured(ctx, Character::default(), &options);
    character(ctx, CharacterCall::Make(settings));
    Ok(())
}

/// `llDeleteCharacter`: the object is no character any more — not
/// physical, pathfinding type 0; a second call is silent
/// (`physics/character`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_delete_character(ctx: &mut ScriptCtx<'_>) -> Result<(), CallError> {
    character(ctx, CharacterCall::Delete);
    Ok(())
}

/// `llNavigateTo`: the character goes to `point` and stops there, raising
/// `path_update` `PU_SLOWDOWN_DISTANCE_REACHED` and then `PU_GOAL_REACHED`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_navigate_to(
    ctx: &mut ScriptCtx<'_>,
    point: Vector,
    _options: Vec<Element>,
) -> Result<(), CallError> {
    if needs_character(ctx, "llNavigateTo") {
        character(ctx, CharacterCall::Go(Goal::Point(point)));
    }
    Ok(())
}

/// `llPatrolPoints`: the character goes round `points`, over and over.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_patrol_points(
    ctx: &mut ScriptCtx<'_>,
    points: Vec<Element>,
    _options: Vec<Element>,
) -> Result<(), CallError> {
    if !needs_character(ctx, "llPatrolPoints") {
        return Ok(());
    }
    let points: Vec<Vector> = points
        .into_iter()
        .filter_map(|point| match point {
            Element::Vector(point) => Some(point),
            _ => None,
        })
        .collect();
    character(ctx, CharacterCall::Go(Goal::Patrol(points)));
    Ok(())
}

/// `llWanderWithin`: the character wanders from point to point within
/// `range` of `origin` on each axis.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_wander_within(
    ctx: &mut ScriptCtx<'_>,
    origin: Vector,
    range: Vector,
    _options: Vec<Element>,
) -> Result<(), CallError> {
    if needs_character(ctx, "llWanderWithin") {
        character(ctx, CharacterCall::Go(Goal::Wander { origin, range }));
    }
    Ok(())
}

/// `PURSUIT_OFFSET`.
const PURSUIT_OFFSET: i32 = 1;

/// `llPursue`: the character goes after the object or avatar keyed
/// `target`, at its `PURSUIT_OFFSET`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_pursue(
    ctx: &mut ScriptCtx<'_>,
    target: Key,
    options: Vec<Element>,
) -> Result<(), CallError> {
    if !needs_character(ctx, "llPursue") {
        return Ok(());
    }
    let offset = options
        .chunks(2)
        .find_map(|pair| match (pair.first(), pair.get(1)) {
            (Some(Element::Integer(PURSUIT_OFFSET)), Some(Element::Vector(offset))) => {
                Some(offset.clone())
            }
            _ => None,
        })
        .unwrap_or(crate::value::ZERO_VECTOR);
    character(
        ctx,
        CharacterCall::Go(Goal::Pursue {
            target: target.0,
            offset,
        }),
    );
    Ok(())
}

/// How far a character runs from what it evades, in metres: `llEvade` takes
/// no distance; Second Life's runs until it is out of sight, which the fake
/// grid, keeping no sight lines, reads as 20 m.
const EVADE_DISTANCE_M: f32 = 20.0;

/// `llEvade`: the character runs from the object or avatar keyed `target`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_evade(
    ctx: &mut ScriptCtx<'_>,
    target: Key,
    _options: Vec<Element>,
) -> Result<(), CallError> {
    if !needs_character(ctx, "llEvade") {
        return Ok(());
    }
    let caller = ctx.caller();
    let Some(details) = ctx.host().object_details(caller, &target.0) else {
        return Ok(());
    };
    character(
        ctx,
        CharacterCall::Go(Goal::Flee {
            from: details.position,
            distance: EVADE_DISTANCE_M,
        }),
    );
    Ok(())
}

/// `llFleeFrom`: the character runs `distance` metres from `source`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_flee_from(
    ctx: &mut ScriptCtx<'_>,
    source: Vector,
    distance: f32,
    _options: Vec<Element>,
) -> Result<(), CallError> {
    if needs_character(ctx, "llFleeFrom") {
        character(
            ctx,
            CharacterCall::Go(Goal::Flee {
                from: source,
                distance,
            }),
        );
    }
    Ok(())
}

/// `llExecCharacterCmd`: stops the character (`CHARACTER_CMD_STOP`,
/// `CHARACTER_CMD_SMOOTH_STOP`) or makes it jump the height its options
/// give (`CHARACTER_CMD_JUMP`); any other command is refused with
/// `Unrecognized character command.` (`physics/character`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_exec_character_cmd(
    ctx: &mut ScriptCtx<'_>,
    command: i32,
    options: Vec<Element>,
) -> Result<(), CallError> {
    match command {
        CHARACTER_CMD_STOP | CHARACTER_CMD_SMOOTH_STOP => {
            character(ctx, CharacterCall::Stop);
        }
        CHARACTER_CMD_JUMP => {
            let height = match options.first() {
                Some(Element::Float(height)) => *height,
                Some(Element::Integer(height)) => crate::num::int_to_float(*height),
                _ => 0.0,
            };
            character(ctx, CharacterCall::Jump(height));
        }
        _ => ctx.refuse("Unrecognized character command.".to_owned()),
    }
    Ok(())
}

/// The point of the fake grid's navmesh under `at`, or [`None`] where `at`
/// stands farther than [`NAVMESH_REACH_M`] from the ground.
fn on_navmesh(ctx: &mut ScriptCtx<'_>, at: &Vector) -> Option<Vector> {
    let caller = ctx.caller();
    let ground = ctx.host().ground(caller, at.x, at.y);
    ((at.z - ground).abs() <= NAVMESH_REACH_M).then_some(Vector {
        x: at.x,
        y: at.y,
        z: ground,
    })
}

/// `llGetStaticPath`: the points of a path from `start` to `end` over the
/// navmesh, then a status: on the fake grid's ground, the two points
/// themselves on it and `0`, or the status alone — `PU_FAILURE_INVALID_GOAL`
/// for an end off the navmesh, checked first (`physics/character`: a path
/// between two points 2.2 m above aditi's navmesh was `[3]`), then
/// `PU_FAILURE_INVALID_START`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_static_path(
    ctx: &mut ScriptCtx<'_>,
    start: Vector,
    end: Vector,
    _radius: f32,
    _params: Vec<Element>,
) -> Result<Vec<Element>, CallError> {
    let Some(goal) = on_navmesh(ctx, &end) else {
        return Ok(vec![Element::Integer(PU_FAILURE_INVALID_GOAL)]);
    };
    let Some(from) = on_navmesh(ctx, &start) else {
        return Ok(vec![Element::Integer(PU_FAILURE_INVALID_START)]);
    };
    Ok(vec![
        Element::Vector(from),
        Element::Vector(goal),
        Element::Integer(0),
    ])
}
